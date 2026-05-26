//Librerias externas instaladas via Cargo
use esp_idf_svc::{
    nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault},
    sys::EspError,
};

//Definicion de tipo de variable RobotEvent para representar eventos relacionados a los robots
pub enum RobotEvent {
    DeltaCompleted { color: String, id_cap: String },
    AmrArrived     { location: String },
    CobotCompleted { id_pallet: String },
}

//Implementacion de interfaces para Mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]

//Definicion de tipo de variable para representar los modos de operacion del sistema
pub enum Mode {
    Manual,
    Auto,
}

#[derive(Debug)]
pub struct ControlState {
    pub mode: Mode,                                         //Modo de operación actual
    pub auto_target: u32,                                   //Cantidad total de tapas solicitadas en modo Auto
    pub auto_spawned: u32,                                  //Tapas ya generadas en modo Auto
    pub auto_validated: u32,                                //Tapas clasificadas en modo Auto
    pub id_lote: Option<String>,                            //Identificador del lote activo
    pub manual_remaining: u32,                              //Tapas pendientes en modo Manual
    pub manual_color: String,                               //Color de tapa seleccionado en modo Manual
    pub manual_spawn_pending: bool,                         //Indica si hay una generación de tapa pendiente en modo Manual
    pub total_processed: u64,                               //Contador total de tapas procesadas en el lote activo
    pub tolva_counts: [u64; 6],                             //Cantidad de tapas por tolva (red=0..blue=5)
    pub amr_pending_tolva: Option<usize>,                   //Tolva asignada al AMR para recoger
    pub amr_dispatched_at: Option<std::time::Instant>,      //Marca de tiempo de cuando se despachó el AMR a la tolva
    pub amr_arrived_tolva: Option<usize>,                   //Tolva a la que llegó el AMR
    pub amr_arrived_at: Option<std::time::Instant>,         //Marca de tiempo de cuando el AMR llegó a la tolva
    pub amr_caja: Option<(usize, String)>,                  //Tolva e identificador de la caja que el AMR está transportando
    pub cobot_ready: bool,                                  //Indica si el cobot está listo para iniciar una tarea de paletizado
    pub cobot_in_progress: bool,                            //Indica si el cobot está ejecutando una tarea de paletizado
    pub cobot_started_at: Option<std::time::Instant>,       //Marca de tiempo de cuando el cobot inició su tarea actual
    pub cobot_pending: Option<(String, String)>,            //Color e identificador de la caja que el cobot tiene pendiente por paletizar
    pub cobot_completed_event: Option<String>,              //Identificador del último evento de paletizado completado por el cobot
    pub pallets: [(u32, u64); 6],                           //Identificador y cantidad de cajas del pallet activo por color (red=0..blue=5)
    pub status_requested: bool,                             //Indica si el SCADA solicitó el estado del sistema
    pub batch_complete_pending: bool,                       //Indica si el lote de producción fue completado y hay que notificar al SCADA
    pub reset_db_pending: bool,                             //Indica si hay un reset de BD pendiente por enviar al bridge
    pub tapas_clasificadas_pending: u32,                    //Tapas clasificadas pendientes de notificar al bridge
}

//Implementacion de valores por defecto para ControlState
impl Default for ControlState {
    fn default() -> Self {
        Self {
            mode: Mode::Manual,
            auto_target: 0,
            auto_spawned: 0,
            auto_validated: 0,
            id_lote: None,
            manual_remaining: 0,
            manual_color: "red".to_string(),
            manual_spawn_pending: false,
            total_processed: 0,
            tolva_counts: [0; 6],
            amr_pending_tolva: None,
            amr_dispatched_at: None,
            amr_arrived_tolva: None,
            amr_arrived_at: None,
            amr_caja: None,
            cobot_ready: false,
            cobot_in_progress: false,
            cobot_started_at: None,
            cobot_pending: None,
            cobot_completed_event: None,
            pallets: [(1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0)],
            status_requested: false,
            batch_complete_pending: false,
            reset_db_pending: false,
            tapas_clasificadas_pending: 0,
        }
    }
}

//Implementación de funciones para ControlState
impl ControlState {
    //Función pública para guardar contradores de tolva en NVS
    pub fn save_tolva_counts(&self, nvs: &EspDefaultNvsPartition) -> Result<(), EspError> {
        let nvs = EspNvs::<NvsDefault>::new(nvs.clone(), "tolva_counts", true)?;
        for (index, count) in self.tolva_counts.iter().enumerate() {
            let key = format!("tolva_{}", index + 1);
            nvs.set_u64(&key, *count)?;
        }
        Ok(())
    }
    //Función pública para resetear contadores de tolva a cero
    pub fn reset_tolva_counts(&mut self) {
        for count in self.tolva_counts.iter_mut() {
            *count = 0;
        }
    }
    //Función pública para cargar contadores de tolva almacenados en la NVS
    pub fn load_tolva_counts(nvs: &EspDefaultNvsPartition) -> Result<[u64; 6], EspError> {
        let nvs = EspNvs::<NvsDefault>::new(nvs.clone(), "tolva_counts", true)?;
        let mut counts = [0u64; 6];
        for (index, count) in counts.iter_mut().enumerate() {
            let key = format!("tolva_{}", index + 1);
            if let Ok(value) = nvs.get_u64(&key) {
                if let Some(v) = value {
                    *count = v;
                }
            }
        }
        Ok(counts)
    }
}
