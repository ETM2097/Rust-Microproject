//Librerias externas instaladas via Cargo
use anyhow::Result;
use embedded_svc::mqtt::client::QoS;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::mqtt::client::{EspMqttClient, EventPayload, MqttClientConfiguration};
use log::{error, info};
use serde_json::Value;

//Libreria estándar de Rust
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

//Modulos internos del proyecto
use crate::{
    config,
    control_state::{ControlState, Mode, RobotEvent},
};

pub type PullSlot = Arc<Mutex<Option<SyncSender<String>>>>;

//Estructura que almacena al cliente MQTT
pub struct MqttManager<'a> {
    client: EspMqttClient<'a>,
}

//Definicion de métodos y funciones para la gestión del cliente MQTT
impl<'a> MqttManager<'a> {
    //Función de conexión y suscripción a tópicos MQTT, con manejo de eventos para procesar los mensajes recibidos y actualizar el estado de control o activar la parada de emergencia según corresponda.
    pub fn connect_and_subscribe_with_state(
        control_state: Arc<Mutex<ControlState>>,
        emergency_stop: Arc<AtomicBool>,
        pull_slot: PullSlot,
        nvs: EspDefaultNvsPartition,
        event_tx: SyncSender<RobotEvent>,
    ) -> Result<Self> {

        //Configuración de creedenciales MQTT.
        let mqtt_user = if config::MQTT_USER.is_empty() {
            None
        } else {
            Some(config::MQTT_USER)
        };
        let mqtt_password = if config::MQTT_PASSWORD.is_empty() {
            None
        } else {
            Some(config::MQTT_PASSWORD)
        };

        //Configuración de cliente MQTT.
        let cfg = MqttClientConfiguration {
            client_id: Some(config::MQTT_CLIENT_ID),
            username: mqtt_user,
            password: mqtt_password,
            ..Default::default()
        };
        
        //Creación de cliente MQTT con callback para manejo de eventos.
        let nvs = Arc::new(nvs);
        let mut client = EspMqttClient::new_cb(config::MQTT_URL, &cfg, move |event| match event.payload() {
            //Log de eventos relevantes.
            EventPayload::Connected(_) => info!("MQTT conectado al servidor"),
            EventPayload::Subscribed(id) => info!("Suscrito con ID: {}", id),
            
            //Log de mensajes recibidos con procesamiento de mensaje según topico y mensaje
            EventPayload::Received { topic, data, .. } => {
                let mensaje = std::str::from_utf8(data).unwrap_or("");
                let topic_recibido = topic.unwrap_or("");
                //Este match es para procesar los mensajes según el topic recibido
                match topic_recibido {
                    //Si el mensaje es del topic SCADA/ACTION
                    config::MQTT_TOPIC_SCADA_ACTION => {
                        //Si existe una emergencia activa, se ignoran los mensajes recibidos por el scada
                        if emergency_stop.load(Ordering::SeqCst) {
                            info!("Sistema en emergencia, ignorando comando SCADA");
                            return;
                        }
                        //Se muestra en Log el mensaje recibido
                        info!("SCADA ordena: {}", mensaje);
                        //Se deserializa el mensaje JSON para procesar el contenido del mismo.
                        if let Ok(value) = serde_json::from_str::<Value>(mensaje) {
                            //Extrae el campo "cmd" del mensaje, que es comando. Si no existe o no  es texto, devuelve cadena vacía.
                            let cmd = value.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
                            //Si es un comando status, se marca en el estado de control que se ha solicitado un status
                            if cmd.eq_ignore_ascii_case("status") {
                                if let Ok(mut state) = control_state.try_lock() {
                                    state.status_requested = true;
                                } else {
                                    error!("No se pudo lockear control_state para status");
                                }
                                return;
                            }
                            //Si es un comando set_mode, se extrae el modo deseado y se actualiza el estado de control con el nuevo modo.
                            if cmd.eq_ignore_ascii_case("set_mode") {
                                let mode_str = value.get("mode")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");

                                if let Ok(mut state) = control_state.try_lock() {
                                    if mode_str.eq_ignore_ascii_case("AUTO") {
                                        info!("Activando modo AUTO");
                                        state.mode = Mode::Auto;
                                        state.id_lote = None;
                                    } else if mode_str.eq_ignore_ascii_case("MANUAL") {
                                        info!("Activando modo MANUAL");
                                        state.mode = Mode::Manual;
                                        state.id_lote = None;
                                    }
                                } else {
                                    error!("No se pudo lockear control_state para set_mode");
                                }
                                return;
                            }

                            //Si es un comando gen, extrae la cantidad de tapas a generar, el id_lote y el color(en modo manual) y se actualiza el estado de control
                            if cmd.eq_ignore_ascii_case("gen") {
                                let quantity = value.get("quantity")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32;
                                let id_lote = value
                                    .get("id_lote")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");

                                
                                if let Ok(mut state) = control_state.try_lock() {
                                    //Si el id_lote es una cadena vacía, se asigna None, sino se asigna el valor del id_lote a lote_value
                                    let lote_value = if id_lote.is_empty() {
                                        None
                                    } else {
                                        Some(id_lote.to_string())
                                    };

                                    //Si el modo es Auto, se actualiza el estado de control con la cantidad de tapas a generar
                                    if state.mode == Mode::Auto {
                                        info!("Generando lote AUTO de {} tapas", quantity);
                                        state.auto_target = quantity;
                                        state.auto_spawned = 0;
                                        state.auto_validated = 0;
                                        state.id_lote = lote_value;
                                    //Si el modo es manual se extrae el color del mensaje, se valida y luego se actualiza el estado del control
                                    } else if state.mode == Mode::Manual {
                                        let color_str = value.get("color")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("red");

                                        if !config::VALID_COLORS.contains(&color_str) {
                                            error!("Color inválido recibido: {}", color_str);
                                            return;
                                        }
                                        
                                        info!("Generando MANUAL: color={}, cantidad=1", color_str);
                                        state.manual_remaining = 1;
                                        state.manual_color = color_str.to_string();
                                        state.manual_spawn_pending = true;
                                        if state.id_lote.is_none() {
                                            state.id_lote = lote_value;
                                        }
                                    }
                                } else {
                                    error!("No se pudo lockear control_state para gen");
                                }
                                return;
                            }
                            //Si es un comando reset, se resetean las tolvas y parámetros de estado de control.
                            if cmd.eq_ignore_ascii_case("reset") {
                                info!("SCADA ordenó reset de tolvas");
                                if let Ok(mut state) = control_state.try_lock() {
                                    state.reset_tolva_counts();
                                    state.pallets           = [(1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0)];
                                    state.amr_pending_tolva = None;
                                    state.amr_dispatched_at = None;
                                    state.amr_arrived_tolva = None;
                                    state.amr_arrived_at    = None;
                                    state.amr_caja          = None;
                                    state.cobot_ready       = false;
                                    state.cobot_in_progress = false;
                                    state.cobot_pending     = None;
                                    state.cobot_completed_event = None;
                                    state.total_processed = 0;
                                    state.tapas_clasificadas_pending = 0;
                                    state.auto_target = 0;
                                    state.auto_spawned = 0;
                                    state.auto_validated = 0;
                                    state.id_lote = None;
                                    state.reset_db_pending = true;

                                    if let Err(err) = state.save_tolva_counts(&nvs) {
                                        error!("No se pudo guardar tolvas en NVS tras reset: {:?}", err);
                                    } else {
                                        info!("Tolvas reseteadas y guardadas en NVS");
                                    }
                                } else {
                                    error!("No se pudo lockear control_state para reset tolvas");
                                }
                                return;
                            }
                        }
                    }
                    //Si es un mensaje del topic delta/status
                    config::MQTT_TOPIC_DELTA_STATUS => {
                         //Se deserializa el mensaje JSON para procesar el contenido del mismo.
                        if let Ok(value) = serde_json::from_str::<Value>(mensaje) {
                            //Extrae el campo "status" del mensaje, que es comando. Si no existe o no  es texto, devuelve cadena vacía.
                            let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            //Si el status es completed, se extrae el color, el id_cap y se envia un evento DeltaCompleted a la cola de eventos
                            if status.eq_ignore_ascii_case("completed") {
                                let color  = value.get("color").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let id_cap = value.get("id_cap").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                                //Se envia un evento Deltacompleted a la cola de eventos, si la cola esta llena se muestra un error y se descarta
                                if let Err(e) = event_tx.try_send(RobotEvent::DeltaCompleted { color, id_cap }) {
                                    error!("Cola llena — delta/status descartado: {:?}", e);
                                }
                            }
                        } else {
                            error!("delta/status sin JSON válido: {}", mensaje);
                        }
                    }

                    //Si es un mensaje del topic emergency/action
                    config::MQTT_TOPIC_EMERGENCY_ACTION => {
                        error!("EMERGENCIA RECIBIDA POR MQTT");
                        //Se deserializa el mensaje JSON para procesar el contenido del mismo.
                        if let Ok(value) = serde_json::from_str::<Value>(mensaje) {
                            //Se extrae el campo "cmd" del mensaje, que es comando. Si no existe o no  es texto, devuelve cadena vacía.
                            let cmd = value.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
                            //Si el comando es estop, se activa la parada de emergencia, si es resume se desactiva la paradad e emergencia.
                            if cmd.eq_ignore_ascii_case("estop") {
                                emergency_stop.store(true, Ordering::SeqCst);
                            } else if cmd.eq_ignore_ascii_case("resume") {
                                emergency_stop.store(false, Ordering::SeqCst);
                            }
                        } else {
                            error!("EMERGENCIA action sin JSON valido: {}", mensaje);
                        }
                    }
                    //Si un mensaje es del topic db/pull/response  
                    config::MQTT_SUB_TOPIC_DB_PULL_RESPONSE => {
                        info!("db/pull/response: {}", mensaje);
                        //Se intenta enviar el mensaje recibido al slot de pull.
                        if let Ok(slot) = pull_slot.try_lock() {
                            if let Some(tx) = slot.as_ref() {
                                let _ = tx.try_send(mensaje.to_string());
                            }
                        }
                    }
                    //Si un mensaje es del topic amr/status
                    config::MQTT_TOPIC_AMR_STATUS => {
                        //Se deserializa el mensaje JSON para procesar el contenido del mismo.
                        if let Ok(value) = serde_json::from_str::<Value>(mensaje) {
                            //Se extrae el campo "status" del mensaje, que es comando. Si no existe o no  es texto, devuelve cadena vacía.
                            let status   = value.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            //Se extrae el campo "location" del mensaje, que es la ubicación del amr. Si no existe o no es texto, devuelve cadena vacía.
                            let location = value.get("location").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            //Si el status es arrived, se envia un evento AmrArrived a la cola de eventos
                            if status.eq_ignore_ascii_case("ARRIVED") {
                                if let Err(e) = event_tx.try_send(RobotEvent::AmrArrived { location }) {
                                    error!("Cola llena — amr/status descartado: {:?}", e);
                                }
                            }
                        } else {
                            error!("amr/status sin JSON válido: {}", mensaje);
                        }
                    }

                    //Si un mensaje es del topic cobot/status
                    config::MQTT_TOPIC_COBOT_STATUS => {
                        //Se deserializa el mensaje JSON para procesar el contenido del mismo.
                        if let Ok(value) = serde_json::from_str::<Value>(mensaje) {
                            //Se extrae el campo "status" del mensaje, que es comando. Si no existe o no  es texto, devuelve cadena vacía.
                            let status    = value.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            //Se extrae el campo "id_pallet" del mensaje, que es el id del pallet que esta procesando el cobot. Si no existe o no es texto, devuelve "?".
                            let id_pallet = value.get("id_pallet").and_then(|v| v.as_str()).unwrap_or("?").to_string();
                            //Si el status es completed, se envia un evento CobotCompleted a la cola de eventos
                            if status.eq_ignore_ascii_case("completed") {
                                if let Err(e) = event_tx.try_send(RobotEvent::CobotCompleted { id_pallet }) {
                                    error!("Cola llena — cobot/status descartado: {:?}", e);
                                }
                            }
                        } else {
                            error!("cobot/status sin JSON válido: {}", mensaje);
                        }
                    }
                    
                    _ => info!("Mensaje en topic no gestionado: {}", topic_recibido),
                }
            }
            _ => {}
        })?;

        info!("Esperando conexion al broker...");
        thread::sleep(Duration::from_secs(5));

        subscribe_all_topics(&mut client, config::MQTT_SUB_TOPICS);

        Ok(Self { client })
    }
    //Función pública para publicar mensajes en un tópico MQTT específico, con manejo de errores y log de publicaciones.
    pub fn publish_text(&mut self, topic: &str, payload: &str) {
        if let Err(e) = self.client.publish(topic, QoS::AtLeastOnce, false, payload.as_bytes()) {
            error!("Error al publicar en {}: {:?}", topic, e);
        } else {
            info!("Publicado en {}: {}", topic, payload);
        }
    }
}

//Función para suscribirse a una lista de tópicos MQTT, con reintentos en caso de error y log de suscripciones.
fn subscribe_all_topics(client: &mut EspMqttClient<'_>, topics: &[&str]) {
    for &topic in topics {
        loop {
            match client.subscribe(topic, QoS::AtLeastOnce) {
                Ok(_) => {
                    info!("Suscrito a {}", topic);
                    break;
                }
                Err(e) => {
                    error!("Error al suscribirse a {}: {:?}. Reintentando...", topic, e);
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    }
}

