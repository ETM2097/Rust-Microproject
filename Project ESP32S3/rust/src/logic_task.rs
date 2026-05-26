//Librerias externas instaladas via Cargo
use anyhow::Result;
use log::{error, info};
use serde_json::json;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

//Libreria estándar de Rust
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
        Arc,
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

//Modulos internos del proyecto
use crate::{
    config,
    mqtt_manager::{MqttManager, PullSlot},
    control_state::{ControlState, Mode, RobotEvent},
};

//Función publica para generar un nuevo ID de tapa
pub fn next_id_cap() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("C{:04}", id)
}

//Función pública para iniciar el hilo de la lógica principal
pub fn spawn_logic_task<'a: 'static>(
    mqtt: Arc<Mutex<MqttManager<'a>>>,
    emergency_stop: Arc<AtomicBool>,
    control_state: Arc<Mutex<ControlState>>,
    pull_slot: PullSlot,
    nvs: EspDefaultNvsPartition,
    event_rx: Receiver<RobotEvent>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::Builder::new()
        .name("logic-task".to_string())
        .spawn(move || {
            let cycle_period = Duration::from_millis(500);
            loop {
                let cycle_start = Instant::now();

                //Si hay emergencia activa, no hace nada
                if emergency_stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                //Drena los eventos del robot
                while let Ok(event) = event_rx.try_recv() {
                    process_robot_event(event, &control_state, &nvs);
                }
                //Intenta generar tapas
                try_spawn_caps(&mqtt, &control_state, &emergency_stop);
                //Verifica si el cobot ha completado su tarea
                handle_cobot_completed(&mqtt, &control_state, &pull_slot);
                //Publica el estado actual
                publish_status(&mqtt, &control_state, &nvs);
                //Espera absoluta: duerme solo el tiempo restante del ciclo
                if let Some(remaining) = cycle_period.checked_sub(cycle_start.elapsed()) {
                    thread::sleep(remaining);
                }
            }
        })?;

    Ok(handle)
}



//Función para procesar eventos provenientes del robot
fn process_robot_event(
    event: RobotEvent,
    control_state: &Arc<Mutex<ControlState>>,
    nvs: &EspDefaultNvsPartition,
) {
    //Depende del tipo de evento, actualiza el estado de control
    match event {
        //Si el evento es de tipo DeltaCompleted, actualiza las tolvas y el conteo total
        RobotEvent::DeltaCompleted { color, id_cap } => {
            //Determina el índice de la tolva basado en el color de la tapa
            let tolva_index = match color.to_ascii_lowercase().as_str() {
                "red"    => 0,
                "yellow" => 1,
                "green"  => 2,
                "white"  => 3,
                "orange" => 4,
                "blue"   => 5,
                _ => { error!("Delta completed con color desconocido: {}", color); return; }
            };
            
            if let Ok(mut state) = control_state.lock() {
                //Actualiza el conteo de tapas en la tolva correspondiente y conteo total
                state.tolva_counts[tolva_index] += 1;
                state.total_processed += 1;
                if state.id_lote.is_some() {
                    state.tapas_clasificadas_pending += 1;
                }
                //En modo auto, incrementa el conteo  de tapas validadas y verifica si alcanzo el objetivo para marcar el lote completo
                if state.mode == Mode::Auto {
                    if state.auto_validated < state.auto_target {
                        state.auto_validated += 1;
                        if state.auto_validated >= state.auto_target {
                            state.batch_complete_pending = true;
                        }
                    }
                    //En modo manual, decrementa el conteo de tapas por clasificar si es que hay un lote activo
                } else {
                    if state.manual_remaining > 0 {
                        state.manual_remaining -= 1;
                    }
                }
                info!("Delta depositó {} en TOLVA_{} (total: {})", id_cap, tolva_index + 1, state.tolva_counts[tolva_index]);
                if let Err(e) = state.save_tolva_counts(nvs) {
                    error!("No se pudo guardar tolvas en NVS: {:?}", e);
                }
            }
        }

        RobotEvent::AmrArrived { location } => {
            //Si el AMR llegó al almacén, el cobot puede iniciar su tarea
            if location.eq_ignore_ascii_case(config::AMR_WAREHOUSE_LOCATION) {
                if let Ok(mut state) = control_state.lock() {
                    state.cobot_ready = true;
                }
                return;
            }
            //Si el AMR llegó a una tolva, registra la llegada con marca de tiempo
            if let Some(index) = parse_amr_location_index(&location) {
                if let Ok(mut state) = control_state.lock() {
                    match state.amr_pending_tolva {
                        Some(pending) if pending == index => {
                            state.amr_arrived_tolva = Some(index);
                            state.amr_arrived_at    = Some(std::time::Instant::now());
                        }
                        Some(pending) => error!("AMR llegó a {}, esperaba tolva {}", location, pending + 1),
                        None          => error!("AMR llegó a {} sin tolva pendiente", location),
                    }
                }
            } else {
                error!("Location AMR inválida: {}", location);
            }
        }

        //Si el cobot completó su tarea, registra el evento para que handle_cobot_completed lo procese
        RobotEvent::CobotCompleted { id_pallet } => {
            if let Ok(mut state) = control_state.lock() {
                info!("Cobot completó operación en pallet {}", id_pallet);
                state.cobot_completed_event = Some(id_pallet);
                state.cobot_in_progress     = false;
            }
        }
    }
}

//Funcion para extraer el indice de tolva desde la ubicacion reportada por el AMR
fn parse_amr_location_index(location: &str) -> Option<usize> {
    let normalized = location.trim().to_ascii_lowercase();
    let num_str    = normalized.strip_prefix("tolva_")?;
    let num        = num_str.parse::<usize>().ok()?;
    if (1..=6).contains(&num) { Some(num - 1) } else { None }
}

//Funcion que intenta generar tapas nuevas si el sistema no esta en emergencia.
fn try_spawn_caps<'a>(
    mqtt: &Arc<Mutex<MqttManager<'a>>>,
    control_state: &Arc<Mutex<ControlState>>,
    emergency_stop: &Arc<AtomicBool>,
) {
    //Verifica si hay emergencia activa
    if emergency_stop.load(Ordering::SeqCst) {
        return;
    }
    
    if let Ok(mut state) = control_state.try_lock() {
        //En modo auto, genera tapas hasta alcanzar el objetivo, verificando que la tolva no este llena antes del spawn.
        if state.mode == Mode::Auto && state.auto_spawned < state.auto_target {
            let color = get_random_color();
            let ci = color_to_index(color);
            if state.tolva_counts[ci] < config::AMR_TOLVA_THRESHOLD {
                if let Ok(mut mqtt_guard) = mqtt.try_lock() {
                    let id_cap = next_id_cap();
                    let spawn_msg = json!({
                        "cmd": "spawn",
                        "id_cap": id_cap,
                        "color": color,
                        "device": "ESP32-S3"
                    }).to_string();
                    //Publica el mensaje de spawn del topic robodk/action
                    mqtt_guard.publish_text(config::MQTT_TOPIC_ROBODK_ACTION, &spawn_msg);
                    state.auto_spawned += 1;
                }
            }
        //En modo manual, si hay un spawn pendiente, intenta generarlo en una tolva que no este llena, sino deja el spawn pendiente.
        } else if state.mode == Mode::Manual && state.manual_spawn_pending {
            let color = state.manual_color.clone();
            let ci = color_to_index(&color);
            if state.tolva_counts[ci] < config::AMR_TOLVA_THRESHOLD {
                if let Ok(mut mqtt_guard) = mqtt.try_lock() {
                    let id_cap = next_id_cap();
                    let spawn_msg = json!({
                        "cmd": "spawn",
                        "id_cap": id_cap,
                        "color": color,
                        "device": "ESP32-S3"
                    }).to_string();
                    mqtt_guard.publish_text(config::MQTT_TOPIC_ROBODK_ACTION, &spawn_msg);
                    state.manual_spawn_pending = false;
                }
            } else {
                info!("Tolva {} llena, spawn manual {} en espera", ci + 1, color);
            }
        }
    }
}

//Funcion para publicar el estado actual del sistema del SCADA
fn publish_status<'a>(
    mqtt: &Arc<Mutex<MqttManager<'a>>>,
    control_state: &Arc<Mutex<ControlState>>,
    nvs: &EspDefaultNvsPartition,
) {
    let mut amr_target: Option<(usize, String)> = None;
    let mut send_warehouse = false;
    let mut caja_payload: Option<(String, String, String, bool)> = None;
    let mut cobot_start: Option<(String, String, u64)> = None;
    let mut should_publish_status = false;
    let mut batch_complete = false;
    let mut reset_db: Option<String> = None;
    let mut tapas_lote: Option<(String, u32)> = None;

    
    if let Ok(mut state) = control_state.lock() {
        //Verifica si el estado solicito publicar el estado actual
        if state.status_requested {
            should_publish_status = true;
            state.status_requested = false;
        }
        //Verifica si el estado solicito marcar el lote como completo
        if state.batch_complete_pending {
            batch_complete = true;
            state.batch_complete_pending = false;
        }
        //Verifica si el estado solicito un reset de la base de datos
        if state.reset_db_pending {
            reset_db = state.id_lote.clone();
            state.reset_db_pending = false;
        }
        //Verifica si hay tapas clasificadas pendientes por reportar a la base de datos
        if state.tapas_clasificadas_pending > 0 {
            if let Some(lote) = &state.id_lote {
                tapas_lote = Some((lote.clone(), state.tapas_clasificadas_pending));
                state.tapas_clasificadas_pending = 0;
            }
        }
        //Prepara etiquetas de tolva pendiente y llegada del AMR, y calcula segundos de espera si el AMR acaba de llegar a la tolva
        let pending_label = state.amr_pending_tolva.map(|idx| format!("TOLVA_{}", idx + 1));
        //Si el AMR ha llegado a una tolva, muestra esa tolva. Si no, pero hay una tolva pendiente, muestra la tolva pendiente. Si no hay ninguna de las dos, muestra vacío
        let arrived_label = state.amr_arrived_tolva.map(|idx| format!("TOLVA_{}", idx + 1));
        //Calcula segundos de espera para llegada del AMR a la tolva, si el AMR acaba de llegar. Si no acaba de llegar, muestra 0
        let amr_wait_seconds = state
            .amr_arrived_at
            .map(|arrived_at| {
                let elapsed = arrived_at.elapsed().as_secs();
                if elapsed >= config::AMR_ARRIVAL_DELAY_SECS {
                    0
                } else {
                    config::AMR_ARRIVAL_DELAY_SECS - elapsed
                }
            })
            .unwrap_or(0);

        //Verifica si el AMR lleva mucho tiempo en camino a una tolva sin llegar — resetea para evitar bloqueo permanente
        if state.amr_pending_tolva.is_some() && state.amr_arrived_tolva.is_none() {
            if let Some(dispatched) = state.amr_dispatched_at {
                if dispatched.elapsed().as_secs() >= config::AMR_TIMEOUT_SECS {
                    error!("Timeout AMR — reseteando amr_pending_tolva");
                    state.amr_pending_tolva = None;
                    state.amr_dispatched_at = None;
                    state.amr_caja          = None;
                }
            }
        }

        //Si el AMR ya llegó a la tolva y ha pasado el tiempo de espera, prepara la caja y manda el AMR al almacén
        if let Some(arrived_at) = state.amr_arrived_at {
            if arrived_at.elapsed() >= Duration::from_secs(config::AMR_ARRIVAL_DELAY_SECS) {
                //Obtiene el color de la tolva y genera la etiqueta de la caja antes de enviarla al cobot
                let caja_color = state.amr_caja.as_ref()
                    .and_then(|(tolva, _)| tolva_index_to_color(*tolva))
                    .map(|c| c.to_string());
                let caja_id = state.amr_caja.take().map(|(_, id)| id);
                if let (Some(id), Some(color)) = (caja_id, caja_color) {
                    let etiqueta = next_etiqueta();
                    state.cobot_pending = Some((color.clone(), id.clone()));
                    caja_payload = Some((id, color, etiqueta, true));
                }

                //Resetea el conteo de la tolva recogida y guarda en NVS
                if let Some(tolva_index) = state.amr_arrived_tolva.or(state.amr_pending_tolva) {
                    state.tolva_counts[tolva_index] = 0;
                    if let Err(e) = state.save_tolva_counts(nvs) {
                        error!("No se pudo guardar tolvas en NVS tras recogida AMR: {:?}", e);
                    }
                }
                state.amr_pending_tolva = None;
                state.amr_arrived_tolva = None;
                state.amr_arrived_at    = None;
                state.amr_dispatched_at = None;
                send_warehouse = true;
            }
        //Si el AMR está libre, busca la primera tolva que haya alcanzado el umbral y lo despacha
        } else if state.amr_pending_tolva.is_none() {
            for (index, count) in state.tolva_counts.iter().enumerate() {
                if *count >= config::AMR_TOLVA_THRESHOLD {
                    let id_caja = next_id_caja();
                    state.amr_caja          = Some((index, id_caja.clone()));
                    state.amr_pending_tolva = Some(index);
                    state.amr_dispatched_at = Some(std::time::Instant::now());
                    amr_target = Some((index, id_caja));
                    break;
                }
            }
        }

        //Verifica si el cobot lleva demasiado tiempo en una tarea — resetea para evitar bloqueo permanente
        if state.cobot_in_progress {
            if let Some(started) = state.cobot_started_at {
                if started.elapsed().as_secs() >= config::COBOT_TIMEOUT_SECS {
                    error!("Timeout cobot — reseteando cobot_in_progress");
                    state.cobot_in_progress = false;
                    state.cobot_started_at  = None;
                    state.cobot_pending      = None;
                }
            }
        }

        //Si el cobot está listo y libre, inicia la siguiente tarea de paletizado
        if state.cobot_ready && !state.cobot_in_progress {
            let color = state.cobot_pending.as_ref().map(|(c, _)| c.clone()).unwrap_or_else(|| "red".to_string());
            let ci = color_to_index(&color);
            let id_pallet     = format!("P{:04}", state.pallets[ci].0);
            let boxes_stacked = state.pallets[ci].1;
            state.cobot_ready        = false;
            state.cobot_in_progress  = true;
            state.cobot_started_at   = Some(std::time::Instant::now());
            cobot_start = Some((id_pallet, color, boxes_stacked));
        }

        if let Ok(mut mqtt_guard) = mqtt.lock() {
            //Notifica a la base de datos las tapas clasificadas acumuladas desde el último ciclo
            if let Some((lote_id, cantidad)) = tapas_lote {
                let msg = json!({
                    "event":    "tapa_clasificada",
                    "id_lote":  lote_id,
                    "cantidad": cantidad
                }).to_string();
                mqtt_guard.publish_text(config::MQTT_TOPIC_DB_PUSH, &msg);
            }

            //Envía reset a la base de datos cuando el SCADA lo solicita
            if let Some(id_lote) = reset_db {
                let msg = json!({
                    "event":   "reset",
                    "id_lote": id_lote,
                    "device":  "ESP32-S3"
                }).to_string();
                mqtt_guard.publish_text(config::MQTT_TOPIC_DB_PUSH, &msg);
                info!("Reset de lote enviado a DB");
            }

            //Notifica al SCADA que el lote de producción se completó
            if batch_complete {
                let msg = json!({
                    "event":   "batch_complete",
                    "message": "Lote de producción completado",
                    "total":   state.auto_target,
                    "device":  "ESP32-S3"
                }).to_string();
                mqtt_guard.publish_text(config::MQTT_TOPIC_SCADA_STATUS, &msg);
            }

            //Publica el estado completo del sistema al SCADA si fue solicitado
            if should_publish_status {
                let status_msg = json!({
                    "mode": match state.mode {
                        Mode::Manual => "Manual",
                        Mode::Auto   => "Auto",
                    },
                    "id_lote":        state.id_lote.as_deref().unwrap_or(""),
                    "total_processed": state.total_processed,
                    "auto_target":    state.auto_target,
                    "auto_spawned":   state.auto_spawned,
                    "auto_validated": state.auto_validated,
                    "manual_remaining": state.manual_remaining,
                    "amr_pending_tolva": pending_label,
                    "amr_arrived_tolva": arrived_label,
                    "amr_wait_seconds":  amr_wait_seconds,
                    "pallets": {
                        "PALLET_1": state.pallets[0].1,
                        "PALLET_2": state.pallets[1].1,
                        "PALLET_3": state.pallets[2].1,
                        "PALLET_4": state.pallets[3].1,
                        "PALLET_5": state.pallets[4].1,
                        "PALLET_6": state.pallets[5].1,
                    },
                    "tolvas": {
                        "TOLVA_1": state.tolva_counts[0],
                        "TOLVA_2": state.tolva_counts[1],
                        "TOLVA_3": state.tolva_counts[2],
                        "TOLVA_4": state.tolva_counts[3],
                        "TOLVA_5": state.tolva_counts[4],
                        "TOLVA_6": state.tolva_counts[5]
                    },
                    "device": "ESP32-S3"
                }).to_string();
                mqtt_guard.publish_text(config::MQTT_TOPIC_SCADA_STATUS, &status_msg);
            }

            //Despacha el AMR hacia la tolva que alcanzó el umbral
            if let Some((tolva_index, _)) = amr_target {
                let cmd_msg = json!({
                    "cmd":      "goto",
                    "location": format!("TOLVA_{}", tolva_index + 1),
                    "device":   "ESP32-S3"
                }).to_string();
                mqtt_guard.publish_text(config::MQTT_TOPIC_AMR_ACTION, &cmd_msg);
            }

            //Manda el AMR al almacén con la caja recogida y registra la caja en la base de datos
            if send_warehouse {
                let mut cmd_msg = json!({
                    "cmd":      "goto",
                    "location": config::AMR_WAREHOUSE_LOCATION,
                    "device":   "ESP32-S3"
                });
                if let Some((caja_id, color, etiqueta, estado)) = caja_payload.clone() {
                    if let Some(obj) = cmd_msg.as_object_mut() {
                        obj.insert("id_caja".to_string(),         json!(caja_id));
                        obj.insert("color".to_string(),           json!(color));
                        obj.insert("codigo_etiqueta".to_string(), json!(etiqueta));
                        obj.insert("estado".to_string(),          json!(estado));
                    }
                    let lotes = state.id_lote.as_ref()
                        .map(|id| vec![id.clone()])
                        .unwrap_or_default();
                    let db_msg = json!({
                        "event":            "box_completed",
                        "id_caja":          caja_id,
                        "color":            color,
                        "codigo_etiqueta":  etiqueta,
                        "estado":           estado,
                        "lotes":            lotes
                    }).to_string();
                    mqtt_guard.publish_text(config::MQTT_TOPIC_DB_PUSH, &db_msg);
                }
                mqtt_guard.publish_text(config::MQTT_TOPIC_AMR_ACTION, &cmd_msg.to_string());
            }

            //Ordena al cobot iniciar el paletizado de la caja que el AMR acaba de entregar
            if let Some((id_pallet, color, boxes_stacked)) = cobot_start {
                let cmd_msg = json!({
                    "cmd":          "start",
                    "id_pallet":    id_pallet,
                    "color":        color,
                    "boxes_stacked": boxes_stacked,
                    "device":       "ESP32-S3"
                }).to_string();
                mqtt_guard.publish_text(config::MQTT_TOPIC_COBOT_ACTION, &cmd_msg);
            }
        }
    }
}

//Función que gestiona el evento de cobot completado: actualiza el pallet, registra en BD y notifica al SCADA si el pallet quedó lleno
fn handle_cobot_completed<'a>(
    mqtt: &Arc<Mutex<MqttManager<'a>>>,
    control_state: &Arc<Mutex<ControlState>>,
    pull_slot: &PullSlot,
) {
    //Extrae el evento pendiente del cobot. Si no hay ninguno, no hace nada
    let event = {
        if let Ok(mut state) = control_state.try_lock() {
            state.cobot_completed_event.take()
        } else {
            return;
        }
    };

    if event.is_none() { return; }

    //Actualiza el pallet correspondiente al color de la caja paletizada e incrementa el contador
    let (id_caja, id_color, id_pallet, pallet_full) = {
        if let Ok(mut state) = control_state.try_lock() {
            let (id_color, id_caja) = state.cobot_pending.take()
                .unwrap_or_else(|| ("red".to_string(), String::new()));
            state.cobot_started_at = None;
            let ci = color_to_index(&id_color);
            state.pallets[ci].1 += 1;
            let count     = state.pallets[ci].1;
            let id_pallet = format!("P{:04}", state.pallets[ci].0);
            let full = count >= config::PALLET_CAPACITY;
            //Si el pallet está lleno, avanza al siguiente incrementando el ID en 6 para mantener series disjuntas por color
            if full {
                info!("Pallet {} ({}) lleno ({} cajas) — iniciando siguiente", id_pallet, id_color, count);
                state.pallets[ci].0 += 6;
                state.pallets[ci].1  = 0;
            }
            (id_caja, id_color, id_pallet, full)
        } else {
            return;
        }
    };

    //Si el pallet quedó lleno, consulta un operario en la BD para asignarlo como responsable del cierre
    let id_operario = if pallet_full {
        query_operarios(mqtt, pull_slot)
    } else {
        None
    };

    //Registra la caja paletizada en la base de datos
    publish_caja_paletizada(mqtt, &id_caja, &id_pallet, &id_color, pallet_full, id_operario.as_deref());

    //Notifica al SCADA que el pallet está lleno y fue cerrado
    if pallet_full {
        if let Ok(mut mqtt_guard) = mqtt.try_lock() {
            let msg = json!({
                "event":    "pallet_full",
                "id_palet": id_pallet,
                "color":    id_color,
                "device":   "ESP32-S3"
            }).to_string();
            mqtt_guard.publish_text(config::MQTT_TOPIC_SCADA_STATUS, &msg);
        }
        info!("Pallet {} ({}) cerrado y notificado al SCADA", id_pallet, id_color);
    }
}

//Consulta la lista de operarios a la base de datos via MQTT y elige uno aleatoriamente para asignarlo al cierre del pallet
fn query_operarios<'a>(
    mqtt: &Arc<Mutex<MqttManager<'a>>>,
    pull_slot: &PullSlot,
) -> Option<String> {
    use std::sync::mpsc::sync_channel;

    //Crea un canal temporal para recibir la respuesta del bridge y lo deposita en el pull_slot
    let (tx, rx) = sync_channel::<String>(1);
    { pull_slot.lock().unwrap().replace(tx); }

    //Publica la consulta al bridge — la respuesta llegará por db/pull/response
    if let Ok(mut mg) = mqtt.try_lock() {
        let req = json!({"query": "operarios"}).to_string();
        mg.publish_text(config::MQTT_TOPIC_DB_PULL, &req);
    }

    //Espera la respuesta hasta 5 segundos. Si no llega, devuelve None y el pallet se cierra sin operario
    let result = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(json_str) => serde_json::from_str::<serde_json::Value>(&json_str).ok().and_then(|val| {
            let lista = val.get("operarios")?.as_array()?;
            if lista.is_empty() { return None; }
            //Elige un operario aleatorio usando los nanosegundos del reloj del sistema como índice
            let idx = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as usize) % lista.len();
            let id_operario = lista[idx].get("id_operario").and_then(|v| v.as_str()).map(|v| v.to_string());
            let nombre = lista[idx].get("nombre").and_then(|v| v.as_str()).unwrap_or("?");
            info!("Operario elegido: {:?} ({})", id_operario, nombre);
            id_operario
        }),
        Err(_) => {
            error!("Timeout esperando db/pull/response para operarios");
            None
        }
    };

    //Limpia el pull_slot para que quede libre para la siguiente consulta
    pull_slot.lock().unwrap().take();
    result
}

//Publica el evento caja_paletizada a la base de datos. Si el pallet quedó cerrado, incluye el operario responsable
fn publish_caja_paletizada<'a>(
    mqtt: &Arc<Mutex<MqttManager<'a>>>,
    id_caja: &str,
    id_palet: &str,
    id_color: &str,
    estado: bool,
    id_operario: Option<&str>,
) {
    let mut msg = json!({
        "event":    "caja_paletizada",
        "id_caja":  id_caja,
        "id_palet": id_palet,
        "id_color": id_color.to_ascii_uppercase(),
        "estado":   estado,
    });

    //El operario solo se incluye cuando el pallet está cerrado (estado = true)
    if let (true, Some(op)) = (estado, id_operario) {
        msg["id_operario"] = json!(op);
    }

    if let Ok(mut mg) = mqtt.try_lock() {
        mg.publish_text(config::MQTT_TOPIC_DB_PUSH, &msg.to_string());
        info!("caja_paletizada — caja={} palet={} estado={} operario={:?}", id_caja, id_palet, estado, id_operario);
    }
}

//Selecciona un color de forma rotativa para distribuir uniformemente las tapas en modo Auto
fn get_random_color() -> &'static str {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let idx = COUNTER.fetch_add(1, Ordering::Relaxed);
    config::VALID_COLORS[idx % config::VALID_COLORS.len()]
}

//Convierte un índice de tolva a su color correspondiente
fn tolva_index_to_color(index: usize) -> Option<&'static str> {
    match index {
        0 => Some("red"),
        1 => Some("yellow"),
        2 => Some("green"),
        3 => Some("white"),
        4 => Some("orange"),
        5 => Some("blue"),
        _ => None,
    }
}

//Convierte un color a su índice de tolva correspondiente
fn color_to_index(color: &str) -> usize {
    match color.to_ascii_lowercase().as_str() {
        "red"    => 0,
        "yellow" => 1,
        "green"  => 2,
        "white"  => 3,
        "orange" => 4,
        "blue"   => 5,
        _        => 0,
    }
}

//Genera un ID único de caja con formato B0001, B0002, etc
fn next_id_caja() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("B{:04}", id)
}

//Genera una etiqueta única con formato ETQ0000001, ETQ0000002, etc
fn next_etiqueta() -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ETQ{:07}", id)
}
