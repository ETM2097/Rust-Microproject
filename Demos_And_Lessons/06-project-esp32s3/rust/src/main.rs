//Librerias externas instaladas via Cargo
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::EspDefaultNvsPartition,
};

//Libreria estándar de Rust
use std::{
    sync::{
        atomic::AtomicBool,
        mpsc,
        Arc,
        Mutex,
    },
};

//Modulos internos del proyecto
mod config;
mod control_state;
mod emergency_task;
mod logic_task;
mod mqtt_manager;
mod wifi_manager;

//Función prinicipal del programa, inicializa periféricos, recursos compartidos y tareas necesarias para el funcionamiento del sistema.
fn main() -> anyhow::Result<()> {
    //Inicialización de logger y parches necesarios para funcionamiento de las bibliotecas utilizadas.
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    //Inicialización de periféricos y recursos compartidos entre tareas.
    let peripherals = Peripherals::take().unwrap();
    //Loop de eventos del sistema.
    let sys_loop = EspSystemEventLoop::take().unwrap();
    //Partición de NVS por defecto para conifuración Wi-Fi.
    let nvs = EspDefaultNvsPartition::take().unwrap();
    let nvs_wifi = nvs.clone();
    let nvs_logic = nvs.clone();

    //Inicialización de recursos compartidos de conexión wifi y pines para emergencia.
    let wifi_ready = Arc::new(AtomicBool::new(false));
    let modem = peripherals.modem;
    let pins = peripherals.pins;
    let emergency_button_pin = pins.gpio38;
    let resume_button_pin = pins.gpio39;
    let emergency_led_pin = pins.gpio10;
    let emergency_buzzer_pin = pins.gpio48;

    //Inicio de tareas, conexión WIFI
    wifi_manager::spawn_wifi_manager(modem, sys_loop, nvs_wifi, Arc::clone(&wifi_ready))?;
    wifi_manager::wait_until_ready(&wifi_ready);

    //Conexión MQTT y recursos compartidos
    let mut state = control_state::ControlState::default();
    
    // Cargar tolva_counts desde NVS si existen
    match control_state::ControlState::load_tolva_counts(&nvs) {
        Ok(counts) => {
            state.tolva_counts = counts;
            log::info!("Tolva counts cargados desde NVS: {:?}", counts);
        }
        Err(e) => {
            log::warn!("No se pudieron cargar tolva counts desde NVS: {:?}", e);
        }
    }
    
    //Inicialización de recursos compartidos par estado de control y emergencia.
    let control_state = Arc::new(Mutex::new(state));
    let emergency_stop = Arc::new(AtomicBool::new(false));
    
    //Inicialización de espacio vacío para consultas a la base de datos.
    let pull_slot = Arc::new(Mutex::new(None::<std::sync::mpsc::SyncSender<String>>));
    //Inicialización de canal de transferencia de eventos del robot de core 0 a core 1
    let (event_tx, event_rx) = mpsc::sync_channel::<control_state::RobotEvent>(64);

    //Conexión a MQTT y registro de callback compartiendo recursos del sistema,,
    let mqtt = Arc::new(Mutex::new(mqtt_manager::MqttManager::connect_and_subscribe_with_state(
        Arc::clone(&control_state),
        Arc::clone(&emergency_stop),
        Arc::clone(&pull_slot),
        nvs,
        event_tx,
    )?));

    //Lanza logic_task en el core 1
    let _logic_handle = logic_task::spawn_logic_task(
        Arc::clone(&mqtt),
        Arc::clone(&emergency_stop),
        Arc::clone(&control_state),
        pull_slot,
        nvs_logic,
        event_rx,
    )?;

    //Ejecua la tarea de emergencia en el hilo principal y sin retorno
    emergency_task::run_emergency_task(
        Arc::clone(&mqtt),
        emergency_button_pin,
        resume_button_pin,
        emergency_led_pin,
        emergency_buzzer_pin,
        emergency_stop,
    )
}
