# GIIROB — Documentación del firmware ESP32-S3

Firmware embebido para ESP32-S3 que controla la línea de producción automatizada de tapas plásticas: coordina un robot Delta (clasificador), un AMR (transporte) y un Cobot (paletizado) a través de MQTT.

---

## Índice

1. [Visión general](#1-visión-general)
2. [Firmware ESP32-S3](#2-firmware-esp32-s3)
   - [Arquitectura de tareas](#21-arquitectura-de-tareas)
   - [Modos de operación](#22-modos-de-operación)
   - [Gestión Wi-Fi](#23-gestión-wi-fi)
   - [Gestión MQTT](#24-gestión-mqtt)
   - [Topics MQTT](#25-topics-mqtt)
   - [Comandos recibidos](#26-comandos-recibidos)
   - [Tarea de lógica](#27-tarea-de-lógica)
   - [Sistema de tolvas](#28-sistema-de-tolvas)
   - [Coordinación AMR](#29-coordinación-amr)
   - [Coordinación Cobot](#210-coordinación-cobot)
   - [Sistema de emergencia](#211-sistema-de-emergencia)
   - [Persistencia NVS](#212-persistencia-nvs)
   - [Estado compartido (ControlState)](#213-estado-compartido-controlstate)
3. [Secuencia de arranque](#3-secuencia-de-arranque)
4. [Ciclo de emergency_task](#4-ciclo-de-emergency_task)
5. [Ciclo de logic_task](#5-ciclo-de-logic_task)
6. [Flujo completo de una tapa](#6-flujo-completo-de-una-tapa)
7. [Flujo completo de una caja](#7-flujo-completo-de-una-caja)
8. [Configuración](#8-configuración)

---

## 1. Visión general

El ESP32-S3 actúa como controlador central de la célula de fabricación. Recibe órdenes del SCADA, genera tapas en RoboDK, valida la clasificación del Delta, coordina el transporte del AMR y el paletizado del Cobot, y publica eventos MQTT al broker.

```
SCADA ──MQTT──► ESP32-S3 ──MQTT──► RoboDK (simulación/spawn)
                    │   ◄──MQTT──── Delta (confirmación clasificación)
                    │   ──MQTT──►  AMR (transporte)
                    │   ──MQTT──►  Cobot (paletizado)
                    │   ──MQTT──►  db/push (eventos de datos)
                    │   ──MQTT──►  db/pull (consultas de datos)
```

---

## 2. Firmware ESP32-S3

### 2.1 Arquitectura de tareas

El firmware corre en un ESP32-S3 (Xtensa LX7 Dual-Core). La distribución de tareas aprovecha los dos núcleos:

| Tarea | Núcleo | Archivo | Función |
|---|---|---|---|
| `wifi_manager` | Core 0 | `wifi_manager.rs` | Conexión y reconexión Wi-Fi |
| `mqtt_manager` callback | Core 0 (FreeRTOS) | `mqtt_manager.rs` | Recepción y despacho de mensajes MQTT |
| ISR `emergency_button` | Core 0 | `emergency_task.rs` | Detecta pulsación por flanco negativo (GPIO38) |
| ISR `resume_button` | Core 0 | `emergency_task.rs` | Detecta pulsación por flanco negativo (GPIO39) |
| `logic_task` | Core 1 | `logic_task.rs` | Control principal: spawns, AMR, Cobot, publicaciones MQTT |
| `emergency_task` | Core 0 (hilo main) | `emergency_task.rs` | LED, buzzer y estado de emergencia |

**Comunicación entre tareas:**

| Mecanismo | Entre quién | Propósito |
|---|---|---|
| `mpsc::sync_channel(64)` | mqtt_manager → logic_task | Cola FIFO de eventos de robots |
| `Arc<Mutex<ControlState>>` | mqtt_manager ↔ logic_task | Estado compartido del sistema |
| `Arc<AtomicBool>` emergency_stop | Todos ↔ Todos | Señal de parada de emergencia |
| `PullSlot` `Arc<Mutex<Option<SyncSender>>>` | mqtt_manager ↔ logic_task | Canal temporal para respuestas externas |
| `Notification` + `AtomicBool` flags | ISRs → emergency_task | Señal de interrupción de botón |

**Invariante de diseño:** `logic_task` es el único punto del sistema que publica mensajes MQTT de salida. El callback de Core 0 solo encola intenciones (flags en `ControlState`) que `logic_task` ejecuta en su próxima iteración.

---

### 2.2 Modos de operación

**Modo Manual**
- Se genera una sola tapa por comando `gen`, con color específico.
- El Delta confirma la clasificación vía `delta/status`.
- `manual_remaining` se decrementa al recibir la confirmación.

**Modo Auto**
- Se genera un lote completo de N tapas con rotación cíclica de los 6 colores.
- El Delta confirma cada tapa; se acepta cualquier color.
- Al completarse el lote (`auto_validated >= auto_target`) se publica `batch_complete` al SCADA.

El modo se cambia con `set_mode`. Cambiar de modo limpia `id_lote`.

---

### 2.3 Gestión Wi-Fi

**Archivo:** `src/wifi_manager.rs`

- Conexión WPA2 Personal con `ScanMethod::FastScan`.
- Power-save del modem desactivado (`WIFI_PS_NONE`).
- Ante cualquier fallo: `disconnect` → `stop` → espera 2 s → `start` → reintenta.
- `wifi_ready: Arc<AtomicBool>` sincroniza el arranque: el hilo principal espera hasta que sea `true` antes de inicializar MQTT.

---

### 2.4 Gestión MQTT

**Archivo:** `src/mqtt_manager.rs`

- Cliente MQTT síncrono (`EspMqttClient`) con callback de eventos en Core 0.
- Tras iniciar espera 5 segundos antes de suscribirse a los topics.
- La suscripción a cada topic se reintenta con backoff de 2 s si falla.
- El callback despacha mensajes a funciones especializadas y encola eventos en `mpsc::sync_channel`.
- `publish_text` envía mensajes con QoS `AtLeastOnce` — solo se llama desde `logic_task` (Core 1).
- Con emergencia activa, los comandos SCADA `action` se ignoran por completo.

---

### 2.5 Topics MQTT

**Suscritos:**

| Topic | Descripción |
|---|---|
| `giirob/pr2-A1/devices/delta/status` | Confirmaciones de clasificación del Delta |
| `giirob/pr2-A1/devices/scada/action` | Comandos del SCADA |
| `giirob/pr2-A1/devices/amr/status` | Reportes de posición del AMR |
| `giirob/pr2-A1/devices/cobot/status` | Reportes de finalización del Cobot |
| `giirob/pr2-A1/system/emergency/action` | Comandos de emergencia remotos |
| `giirob/pr2-A1/db/pull/response` | Respuestas a consultas de datos |

**Publicados:**

| Topic | Descripción |
|---|---|
| `giirob/pr2-A1/devices/robodk/action` | Orden SPAWN a RoboDK |
| `giirob/pr2-A1/devices/amr/action` | Órdenes de movimiento al AMR |
| `giirob/pr2-A1/devices/cobot/action` | Órdenes de paletizado al Cobot |
| `giirob/pr2-A1/devices/scada/status` | Estado del sistema y eventos al SCADA |
| `giirob/pr2-A1/system/emergency/status` | Cambios de estado de emergencia |
| `giirob/pr2-A1/db/push` | Eventos de caja, paletizado y tapa clasificada |
| `giirob/pr2-A1/db/pull` | Consultas de datos (operarios) |

---

### 2.6 Comandos recibidos

#### SCADA action (`giirob/pr2-A1/devices/scada/action`)

**`gen`** — Genera tapas
```json
// Modo Auto
{ "cmd": "gen", "quantity": 100, "id_lote": "L0042" }

// Modo Manual
{ "cmd": "gen", "quantity": 1, "color": "red", "id_lote": "L0042" }
```

**`set_mode`** — Cambia el modo
```json
{ "cmd": "set_mode", "mode": "AUTO" }
{ "cmd": "set_mode", "mode": "MANUAL" }
```

**`status`** — Solicita reporte de estado completo
```json
{ "cmd": "status" }
```

**`reset`** — Reinicia todos los contadores y guarda en NVS
```json
{ "cmd": "reset" }
```

#### Delta status (`giirob/pr2-A1/devices/delta/status`)

```json
{ "status": "completed", "color": "red", "id_cap": "C0005" }
```
Incrementa `tolva_counts[color]`, `total_processed` y `tapas_clasificadas_pending`. En Auto: incrementa `auto_validated`. En Manual: decrementa `manual_remaining`.

#### AMR status (`giirob/pr2-A1/devices/amr/status`)

```json
{ "status": "arrived", "location": "TOLVA_1" }
{ "status": "arrived", "location": "cobot_pick" }
```

#### Cobot status (`giirob/pr2-A1/devices/cobot/status`)

```json
{ "status": "completed", "id_pallet": "P0002" }
```

#### Emergency action (`giirob/pr2-A1/system/emergency/action`)

```json
{ "cmd": "estop",  "source": "SCADA" }
{ "cmd": "resume", "source": "SCADA" }
```

---

### 2.7 Tarea de lógica

**Archivo:** `src/logic_task.rs` — Core 1, período 500 ms.

1. **Procesamiento de eventos:** drena la cola `mpsc::sync_channel` y procesa cada `DeltaCompleted`, `AmrArrived` y `CobotCompleted`.
2. **Spawn de tapas:** si hay tapas pendientes, genera `id_cap` único y publica `spawn` a RoboDK.
3. **Gestión del Cobot:** si hay un `cobot_completed_event` pendiente, actualiza el pallet, consulta operario (si aplica) y publica a `db/push`.
4. **Publicación de estado:** consume todos los flags de `ControlState` (`status_requested`, `batch_complete_pending`, `reset_db_pending`, `tapas_clasificadas_pending`), coordina AMR y Cobot, y publica todas las respuestas MQTT pendientes en un solo bloque.

Ver [§5 Ciclo de logic_task](#5-ciclo-de-logic_task) para el detalle de cada iteración.

---

### 2.8 Sistema de tolvas

6 tolvas físicas, cada una asociada a un color:

| Tolva | Color |
|---|---|
| TOLVA_1 | Rojo (red) |
| TOLVA_2 | Amarillo (yellow) |
| TOLVA_3 | Verde (green) |
| TOLVA_4 | Blanco (white) |
| TOLVA_5 | Naranja (orange) |
| TOLVA_6 | Azul (blue) |

`tolva_counts[i]` se incrementa al recibir `delta/status completed`. El AMR se despacha cuando `tolva_counts[i] >= AMR_TOLVA_THRESHOLD (20)`. Solo puede haber un AMR en tránsito a la vez.

Los conteos se persisten en NVS y sobreviven reinicios.

---

### 2.9 Coordinación AMR

```
tolva_counts[i] >= 20
        ↓
ESP32 genera id_caja, registra amr_dispatched_at
publica goto TOLVA_N → AMR
        ↓
AMR publica ARRIVED (location: TOLVA_N)
        ↓
ESP32 registra amr_arrived_tolva, amr_arrived_at
espera 6 segundos (AMR_ARRIVAL_DELAY_SECS)
        ↓
ESP32 publica goto cobot_pick → AMR
ESP32 publica box_completed → db/push
tolva_counts[i] = 0  (guardado en NVS)
        ↓
AMR publica ARRIVED (location: cobot_pick)
        ↓
cobot_ready = true
```

Timeout AMR: si no llega en 120 s (`AMR_TIMEOUT_SECS`), se cancelan `amr_pending_tolva`, `amr_dispatched_at` y `amr_caja`.

---

### 2.10 Coordinación Cobot

- Un pallet activo por color (`pallets[i]` = (id_pallet, cajas), formato `P0001`).
- `pallets[i].1` cuenta las cajas en el pallet actual.
- Solo una operación simultánea (`!cobot_in_progress`).

Al recibir `completed`:
1. `pallets[ci].1 += 1` — incrementa el contador de cajas del pallet activo para ese color.
2. Si `pallets[ci].1 >= PALLET_CAPACITY (6)`: cierra pallet, solicita operario vía `db/pull`, publica `caja_paletizada (estado=true, id_operario)`, notifica `pallet_full` al SCADA y avanza `pallets[ci].0 += 6`, `pallets[ci].1 = 0`.
3. Si `< 6`: publica `caja_paletizada (estado=false)`.
4. `cobot_in_progress` ya fue liberado al encolar el evento.

Timeout Cobot: si no responde en 60 s (`COBOT_TIMEOUT_SECS`), se limpian `cobot_in_progress`, `cobot_started_at` y `cobot_pending`.

---

### 2.11 Sistema de emergencia

**Archivo:** `src/emergency_task.rs`

- **GPIO38:** botón de emergencia (NegEdge).
- **GPIO39:** botón de reanudación (NegEdge).
- **GPIO10:** LED indicador (HIGH = emergencia activa).
- **GPIO48:** buzzer (activo durante emergencia).

Comportamiento:
- Al pulsar emergencia: `emergency_stop = true`, LED y buzzer activados, publica en `emergency/status`.
- Al pulsar reanudación: `emergency_stop = false`, LED y buzzer apagados.
- También responde a comandos MQTT `estop`/`resume` en `emergency/action`.
- Con `emergency_stop = true`: callback MQTT ignora comandos SCADA; `logic_task` suspende su ciclo.

Ver [§4 Ciclo de emergency_task](#4-ciclo-de-emergency_task) para el detalle de cada iteración.

---

### 2.12 Persistencia NVS

Namespace `tolva_counts`, claves `tolva_1` a `tolva_6` como `u64`.

- Se cargan al arrancar (si no existen, se usan ceros).
- Se guardan tras cada confirmación del Delta y tras cada recogida del AMR.
- Se borran (ponen a 0) al ejecutar `reset`.
- El namespace se crea automáticamente en el primer arranque.

---

### 2.13 Estado compartido (ControlState)

**Archivo:** `src/control_state.rs` — protegido por `Arc<Mutex<ControlState>>`.

| Campo | Tipo | Descripción |
|---|---|---|
| `mode` | `Mode` | Manual o Auto |
| `auto_target` | `u32` | Tapas totales solicitadas en Auto |
| `auto_spawned` | `u32` | Tapas ya generadas en RoboDK |
| `auto_validated` | `u32` | Tapas confirmadas por Delta en Auto |
| `id_lote` | `Option<String>` | Lote activo |
| `manual_remaining` | `u32` | Tapas manuales pendientes de confirmación |
| `manual_color` | `String` | Color esperado en modo Manual |
| `manual_spawn_pending` | `bool` | Flag para generar la siguiente tapa manual |
| `total_processed` | `u64` | Total de tapas procesadas |
| `tolva_counts` | `[u64; 6]` | Tapas confirmadas por tolva (red=0..blue=5) |
| `amr_pending_tolva` | `Option<usize>` | Tolva a la que se dirigió el AMR |
| `amr_dispatched_at` | `Option<Instant>` | Momento de despacho del AMR (timeout) |
| `amr_arrived_tolva` | `Option<usize>` | Tolva donde llegó el AMR |
| `amr_arrived_at` | `Option<Instant>` | Momento de llegada del AMR a la tolva |
| `amr_caja` | `Option<(usize, String)>` | Tolva de origen e ID de la caja que transporta el AMR |
| `cobot_ready` | `bool` | AMR llegó a cobot_pick con caja lista |
| `cobot_in_progress` | `bool` | Cobot ejecutando una operación |
| `cobot_started_at` | `Option<Instant>` | Momento de inicio del Cobot (timeout) |
| `cobot_pending` | `Option<(String, String)>` | Color e ID de la caja que el Cobot tiene pendiente |
| `cobot_completed_event` | `Option<String>` | id_pallet del último completed del Cobot |
| `pallets` | `[(u32, u64); 6]` | (ID de pallet activo, cajas en él) por color (red=0..blue=5) |
| `status_requested` | `bool` | Solicitud de estado pendiente |
| `batch_complete_pending` | `bool` | Lote Auto completado, pendiente de publicar |
| `reset_db_pending` | `bool` | Reset pendiente de publicar |
| `tapas_clasificadas_pending` | `u32` | Tapas clasificadas pendientes de publicar |

---

## 3. Secuencia de arranque

`main.rs` — ejecución lineal, no es un loop. Si Wi-Fi no conecta el sistema se bloquea; si MQTT falla, `main` termina. El último paso es `emergency_task`, que toma el hilo principal y nunca retorna.

```
main()
  │
  ├─ Inicializa logger y parches del sistema
  ├─ Toma periféricos (pines, modem)
  ├─ Clona NVS para cada módulo que lo necesita
  ├─ Crea recursos compartidos:
  │     wifi_ready     → AtomicBool (Wi-Fi listo?)
  │     emergency_stop → AtomicBool (emergencia activa?)
  │     control_state  → Arc<Mutex<ControlState>>
  │     pull_slot      → Arc<Mutex<Option<SyncSender>>> (hueco consultas externas)
  │     event_tx/rx    → canal Core 0 → Core 1 (capacidad 64)
  │
  ├─ Lanza Wi-Fi y BLOQUEA hasta que conecta
  │     wifi_manager::spawn_wifi_manager(...)
  │     wifi_manager::wait_until_ready(&wifi_ready)
  │
  ├─ Carga tolva_counts desde NVS
  │
  ├─ Crea cliente MQTT y registra callback (Core 0)
  │     mqtt_manager::connect_and_subscribe_with_state(...)
  │
  ├─ Lanza logic_task en hilo separado (Core 1)
  │     logic_task::spawn_logic_task(...)
  │
  └─ Entra en emergency_task — NUNCA RETORNA
        monitorea botones / LED / buzzer / MQTT
        loop infinito de 50ms
```

---

## 4. Ciclo de emergency_task

El hilo duerme esperando una interrupción de botón o un timeout de 50 ms. El timeout existe para detectar cambios provocados por comandos MQTT aunque no haya pulsación física.

```
Iteración N:
  │
  ├─ Crea Notification + flags en stack
  ├─ Registra ISR (closures apuntan al stack de esta iteración)
  ├─ Duerme hasta 50ms ──────────────────────────────────────┐
  │                                                           │
  │   [Si botón pulsado]                                      │
  │   Hardware → ISR → flag=true → notify → despierta ───────┘
  │                                                           │
  │   [Si no pasa nada / comando MQTT]                        │
  │   Timeout 50ms ───────────────────────────────────────────┘
  │
  ├─ Lee flags → actualiza emergency_stop
  ├─ ¿Cambió estado? → LED / buzzer / publica MQTT
  └─ Stack destruido → vuelve al inicio
```

Las ISRs son brevísimas (dos operaciones atómicas), por lo que Core 0 nunca se bloquea. Si hay cambio de estado MQTT sin pulsación física, el timeout de 50 ms lo detecta en la siguiente iteración.

---

## 5. Ciclo de logic_task

Período fijo de **500 ms** (salvo emergencia activa, donde colapsa a 100 ms inactivo). Toda publicación MQTT del sistema ocurre dentro de este ciclo.

```
Iteración N:
  │
  ├─ ¿emergency_stop == true?
  │     Sí → sleep 100ms → siguiente iteración
  │     No → continúa ↓
  │
  ├─ Drena la cola de eventos (mpsc::try_recv en loop)
  │     DeltaCompleted { color, id_cap }
  │       → tolva_counts[color] += 1
  │       → total_processed += 1
  │       → si Auto: auto_validated += 1 → ¿lote completo? → batch_complete_pending = true
  │       → si Manual: manual_remaining -= 1
  │       → guarda tolva_counts en NVS
  │     AmrArrived { location }
  │       → si location == cobot_pick: cobot_ready = true
  │       → si location == tolva_X: amr_arrived_tolva = index, registra timestamp
  │     CobotCompleted { id_pallet }
  │       → cobot_completed_event = Some(id_pallet)
  │       → cobot_in_progress = false
  │
  ├─ try_spawn_caps — genera tapas si no hay emergencia
  │     Modo AUTO y auto_spawned < auto_target:
  │       → color = get_random_color() (rotativo)
  │       → ¿tolva[color] < umbral? → publica robodk/action {"cmd":"spawn"} → auto_spawned += 1
  │     Modo MANUAL y manual_spawn_pending:
  │       → ¿tolva[color] < umbral? → publica robodk/action {"cmd":"spawn"} → pending = false
  │
  ├─ handle_cobot_completed — gestiona fin de paletizado
  │     ¿cobot_completed_event.take()?
  │       → pallets[ci].1 += 1
  │       → ¿pallet lleno (>= 6)?
  │           → query_operarios: publica db/pull, espera db/pull/response (5s, PullSlot)
  │           → elige operario aleatoriamente
  │           → publica db/push {"event":"caja_paletizada", estado=true, id_operario}
  │           → publica scada/status {"event":"pallet_full"}
  │           → pallets[i].1 = 0, pallets[i].0 += 6
  │       → ¿pallet abierto?
  │           → publica db/push {"event":"caja_paletizada", estado=false}
  │
  ├─ publish_status — toda la lógica de publicación MQTT
  │     Lee ControlState (lock) para recoger flags pendientes:
  │       status_requested       → publica scada/status (estado completo)
  │       batch_complete_pending → publica scada/status {"event":"batch_complete"}
  │       reset_db_pending       → publica db/push {"event":"reset"}
  │       tapas_clasificadas > 0 → publica db/push {"event":"tapa_clasificada"}
  │
  │     Lógica AMR:
  │       ¿AMR llegó a tolva y pasó el delay (6s)?
  │         → publica amr/action {"cmd":"goto", location:"cobot_pick"}
  │         → publica db/push {"event":"box_completed", ...}
  │         → tolva_counts[tolva] = 0 → guarda NVS
  │       ¿AMR libre y tolva >= umbral?
  │         → publica amr/action {"cmd":"goto", location:"TOLVA_X"}
  │       ¿AMR en camino > 120s timeout?
  │         → error log → resetea amr_pending_tolva
  │
  │     Lógica cobot:
  │       ¿cobot_ready && !cobot_in_progress?
  │         → publica cobot/action {"cmd":"start", id_pallet, color, boxes_stacked}
  │         → cobot_in_progress = true
  │       ¿cobot en progreso > 60s timeout?
  │         → error log → resetea cobot_in_progress
  │
  └─ espera absoluta hasta completar 500ms → siguiente iteración
```

---

## 6. Flujo completo de una tapa

```
SCADA envía gen (color=red, modo Manual)
    │
    ▼
ESP32: manual_spawn_pending = true, manual_color = "red"
    │
    ▼
logic_task detecta pending → genera id_cap="C0001"
publica spawn (color=red, id_cap=C0001) → RoboDK
manual_spawn_pending = false
    │
    ▼
RoboDK genera tapa, Delta la clasifica en TOLVA_1
Delta publica completed (color=red, id_cap=C0001) → delta/status
    │
    ▼
mqtt_manager encola DeltaCompleted en mpsc::sync_channel
    │
    ▼
logic_task drena cola → tolva_counts[0]++, total_processed++
tapas_clasificadas_pending++, manual_remaining--
guarda NVS
```

---

## 7. Flujo completo de una caja

```
tolva_counts[0] >= 20  (TOLVA_1 tiene 20 tapas: red)
    │
    ▼
logic_task detecta umbral → amr_pending_tolva = 0, amr_dispatched_at = now()
ESP32 genera id_caja="B0001"
publica goto TOLVA_1 → AMR
    │
    ▼
AMR llega a TOLVA_1 → publica ARRIVED (location=TOLVA_1)
    │
    ▼
ESP32: amr_arrived_tolva = 0, amr_arrived_at = now()
    │
    ▼ (después de 6 segundos)
logic_task detecta delay cumplido:
  tolva_counts[0] = 0  (guardado en NVS)
  publica goto cobot_pick → AMR
  publica box_completed (id_caja=B0001, color=red, lote=L0042) → db/push
    │
    ▼
AMR llega a cobot_pick → publica ARRIVED (location=cobot_pick)
    │
    ▼
ESP32: cobot_ready = true
    │
    ▼
logic_task: publica start (id_pallet=P0001, color=red, boxes_stacked=0) → Cobot
cobot_in_progress = true
    │
    ▼
Cobot paletiza → publica completed (id_pallet=P0001)
    │
    ▼
mqtt_manager encola CobotCompleted → logic_task procesa en siguiente iteración
pallets[0].1++ (red)
    │
    ├── Si pallets[0].1 >= 6:
    │       query_operarios (db/pull) → espera respuesta → elige id_operario
    │       publica caja_paletizada (estado=true, id_operario) → db/push
    │       publica pallet_full (id_palet=P0001) → scada/status
    │       pallets[0].1 = 0, pallets[0].0 += 6
    │
    └── Si pallets[0].1 < 6:
            publica caja_paletizada (estado=false) → db/push
```

---

## 8. Configuración

**Archivo:** `src/config.rs`

| Constante | Valor | Descripción |
|---|---|---|
| `WIFI_SSID` | `"HUAWEI-2.4G-pXj3"` | Red Wi-Fi |
| `MQTT_URL` | `"mqtt://broker.hivemq.com:1883"` | Broker MQTT |
| `MQTT_CLIENT_ID` | `"ESP32_PR2A1"` | ID del cliente MQTT |
| `AMR_TOLVA_THRESHOLD` | `20` | Tapas para despachar AMR (1 caja llena) |
| `AMR_ARRIVAL_DELAY_SECS` | `6` | Segundos de espera tras llegada del AMR a la tolva |
| `AMR_WAREHOUSE_LOCATION` | `"cobot_pick"` | Ubicación del área del Cobot |
| `AMR_TIMEOUT_SECS` | `120` | Timeout de espera para el AMR |
| `PALLET_CAPACITY` | `6` | Cajas por pallet |
| `COBOT_TIMEOUT_SECS` | `60` | Timeout de espera para el Cobot |
| `VALID_COLORS` | `["red","green","yellow","blue","white","orange"]` | Colores aceptados |
