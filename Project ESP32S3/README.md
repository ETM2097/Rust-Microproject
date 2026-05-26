# 07 — Project ESP32-S3: Plastic-Cap Sorting Cell in Rust

This is the capstone project of the lesson series: a complete, real firmware
running in a plastic-cap manufacturing cell. An ESP32-S3 coordinates three
robots over MQTT — a Delta (classifier), an AMR (transport), and a Cobot
(palletizer) — while handling physical emergency hardware, persisting state
across reboots, and talking to a SCADA and a database concurrently.

Every concept from lessons 01–06 shows up here under real load: typed
peripheral ownership, borrowing across threads, channels, mutexes, atomics,
NVS persistence, and dual-core task scheduling.

You already know:

- Lessons 01–03 — toolchain, `#![no_std]`, ownership, borrowing.
- Lesson 05 — `esp-idf-svc`, `std::thread`, `mpsc::sync_channel`.
- What MQTT is: broker, topics, publish, subscribe, callback.
- Basic FreeRTOS concepts: tasks, queues, ISRs.

You do **not** need to know Rust async or `embassy`. This project uses
`std::thread` over FreeRTOS tasks throughout.

### Why Rust instead of the Arduino template

In the PR2 course every team received the same Arduino template as their
starting point: a clean framework with a Wi-Fi library, an MQTT library, and
two student-facing files — `s_setup.ino` and `w_loop.ino`. The template is
preserved in `arduino/` for comparison.

Our team chose **not** to use it. Not because Arduino is bad for simple
things — it is fine. But we wanted to push into real Rust territory:
understand how ownership, the type system, and the concurrency primitives
behave under pressure. A multi-robot controller with concurrent I/O, shared
state across cores, hardware interrupts, and database round-trips is exactly
the kind of pressure that reveals what a language can and cannot do.

What we found is documented here. The short version: Arduino's single-threaded
model is a hard ceiling for this class of problem. Rust does not raise that
ceiling — it removes it.

---

## 1. The structural problem with Arduino at scale

In Arduino everything runs in one place:

```cpp
void loop() {
    wifi_loop();
    mqtt_loop();   // pumps the MQTT client — MUST be called every cycle
    on_loop();     // your code — do not block
}
```

`on_loop()` is where student code lives. The constraint is absolute: if
`on_loop()` blocks for more than ~200 ms, `mqtt_loop()` does not run, the
broker times out, and the connection drops.

This creates an unresolvable tension. Coordinating an AMR involves waiting for
it to arrive at a hopper (several seconds), then waiting a loading delay (6 s),
then routing it to the Cobot. Coordinating a Cobot involves waiting for a
`completed` confirmation (variable time). Querying a database for a worker
is a round-trip over MQTT (up to 5 s). None of these fit in a non-blocking
`on_loop()` without hand-rolling a state machine across dozens of flags — and
you still cannot safely share state with the MQTT callback at the same time,
because `volatile bool` is not safe on a dual-core MCU.

Open `arduino/w_loop.ino` and look at `on_loop()`. It is nearly empty.
That is not because the student stopped trying. That is the ceiling.

---

## 2. What the system does

The ESP32-S3 is the central coordinator of the cell. It receives production
orders from a SCADA, drives cap creation in RoboDK, and sequences the three
robots through the full cycle.

```
SCADA ──MQTT──► ESP32-S3 ──MQTT──► RoboDK  (cap spawn / simulation)
                    │    ◄──MQTT──── Delta  (sorting confirmation)
                    │    ──MQTT──►  AMR     (transport commands)
                    │    ──MQTT──►  Cobot   (palletizing commands)
                    │    ──MQTT──►  db/push (event log to database)
                    │    ──MQTT──►  db/pull (worker queries)
                    │
               GPIO38 ── Emergency stop button  (hardware ISR)
               GPIO39 ── Resume button          (hardware ISR)
               GPIO10 ── Emergency LED
               GPIO48 ── Buzzer
```

**The production loop:**

1. SCADA sends `gen` — Manual (one cap, specific color) or Auto (full batch,
   rotating colors across 6 hoppers).
2. ESP32 spawns caps in RoboDK, generating a unique `id_cap` each time.
3. Delta classifies each cap. ESP32 increments the matching hopper count and
   saves it to NVS flash so it survives a reboot.
4. When a hopper reaches 20 caps, ESP32 dispatches the AMR to that hopper,
   waits 6 s for loading, then routes it to the Cobot pick area.
5. Cobot palletizes the box. When a pallet fills (6 boxes), ESP32 queries the
   database for an available worker, assigns them, and notifies the SCADA.
6. A physical or remote E-stop freezes everything instantly: MQTT commands are
   ignored, `logic_task` suspends, LED and buzzer activate. Resume reverses it.

---

## 3. Architecture: two cores, clear contracts

The ESP32-S3 has two Xtensa LX7 cores. The firmware maps each kind of work
to the core it belongs on:

```
Core 0 (PRO_CPU)                        Core 1 (APP_CPU)
────────────────────────────────        ─────────────────────────────
FreeRTOS + Wi-Fi + MQTT stack           logic_task — 500 ms cycle
MQTT callback ──────────────────────►     ├─ drain robot event queue
ISR emergency_button (GPIO38) ──────►     ├─ spawn caps in RoboDK
ISR resume_button    (GPIO39) ──────►     ├─ coordinate AMR
emergency_task — 50 ms poll               ├─ coordinate Cobot
wifi_manager — reconnect loop             ├─ handle pallet completion
                                          └─ publish ALL outgoing MQTT
```

**Core 0 never blocks.** The ISRs set an `AtomicBool` and fire a notification —
two instructions, done. The MQTT callback enqueues a typed event and exits —
it never publishes, never locks a heavy mutex.

**Core 1 owns all outgoing MQTT.** `logic_task` is the single point in the
firmware that calls `publish`. This is not a convention. It is enforced by
Rust's ownership system, as explained in section 5.

---

## 4. Replacing global variables with typed primitives

In a typical Arduino project at this scale the shared state looks like this:

```cpp
volatile bool emergency_active = false;
int tolva_counts[6] = {0};
String current_lote = "";

void mqttCallback(char* topic, byte* payload, unsigned int length) {
    // We are inside mqtt.loop() here.
    // tolva_counts[color]++ is a data race on a dual-core chip.
    // Publishing here risks a deadlock with the client already in use.
}
```

None of this is detectable by the compiler. The races appear under load,
weeks after shipping.

The Rust firmware replaces all of it with three typed primitives.

### 4.1 `mpsc::sync_channel::<RobotEvent>(64)` — the event queue

```rust
// Created once at startup in main.rs:
let (event_tx, event_rx) = mpsc::sync_channel::<RobotEvent>(64);
```

`event_tx` goes to the MQTT callback on Core 0. `event_rx` goes to
`logic_task` on Core 1. The `move` keyword transfers ownership into each
thread — after that, neither end is reachable from anywhere else:

```rust
// Core 0 callback — enqueue and return immediately:
if let Err(e) = event_tx.try_send(RobotEvent::DeltaCompleted { color, id_cap }) {
    error!("Queue full — event dropped: {:?}", e);
}

// Core 1 logic_task — drain every 500 ms cycle:
while let Ok(event) = event_rx.try_recv() {
    process_robot_event(event, &control_state, &nvs);
}
```

Only `RobotEvent` values can travel through the channel. Passing the wrong
type is a compile error. `try_send` and `try_recv` never block, so neither
core stalls waiting for the other.

Compare with C, where the queue is an opaque `QueueHandle_t` that accepts
any `void*` and any function can write into it at any time.

### 4.2 `Arc<Mutex<ControlState>>` — shared state

All state that both cores need to see lives in one struct, protected by a mutex:

```rust
// You cannot access any field without locking. This is a compile error:
//   control_state.status_requested = true;  // ERROR

// Core 0 callback — non-blocking lock (must not stall Core 0):
if let Ok(mut state) = control_state.try_lock() {
    state.status_requested = true;
}

// Core 1 logic_task — blocking lock (can wait briefly):
if let Ok(mut state) = control_state.lock() {
    if state.status_requested {
        mqtt_guard.publish_text(TOPIC_SCADA_STATUS, &build_status(&state));
        state.status_requested = false;
    }
}
```

In Arduino, `tolva_counts[color]++` inside a callback is a data race on a
dual-core MCU. The compiler accepts it. In Rust, the equivalent without a
mutex does not compile.

### 4.3 `Arc<AtomicBool>` — the emergency flag

For a single bit that every task needs to read as fast as possible:

```rust
let emergency_stop = Arc::new(AtomicBool::new(false));
```

An `AtomicBool` needs no mutex. CPU atomic instructions guarantee consistency
on both cores. ISRs, `logic_task`, the callback, and `emergency_task` all read
it with a single instruction at zero overhead:

```rust
// Top of every logic_task cycle:
if emergency_stop.load(Ordering::SeqCst) {
    thread::sleep(Duration::from_millis(100));
    continue;
}
```

Arduino would use `volatile bool` here. `volatile` prevents compiler
reordering but does nothing about CPU-level cache coherency on a dual-core
chip. `AtomicBool` is the correct primitive.

---

## 5. Why `logic_task` owns all outgoing MQTT

`EspMqttClient` is not `Send` — the Rust compiler will not allow it to cross
a thread boundary. The firmware wraps the client in `Arc<Mutex<MqttManager>>`
and gives it exclusively to `logic_task`. The callback on Core 0 holds no
reference to it at all.

If you tried to call `publish` from the callback, you would get a compile error
about `MutexGuard` not being `Send` — not a mysterious dropped-message bug at
3 AM. The architecture is enforced by the type system, not by discipline.

When the callback needs to trigger a publish, it sets a flag in `ControlState`:

```rust
// Core 0 — intent only, does not publish:
state.status_requested = true;

// Core 1 — acts on the intent next cycle:
if state.status_requested {
    mqtt_guard.publish_text(TOPIC_SCADA_STATUS, &build_status(&state));
    state.status_requested = false;
}
```

In Arduino, calling `mqtt.publish()` inside the callback (which fires inside
`mqtt.loop()`) is a common source of deadlocks or dropped messages. The Rust
version makes it impossible to write that bug — the compiler refuses it.

---

## 6. Handling a request–response: the `PullSlot`

When a pallet closes, the firmware queries the database for an available worker
and waits up to 5 seconds for the answer before completing the event. In Rust
this uses a one-shot channel created for that specific query:

```rust
// logic_task creates the channel and registers the sender:
let (tx, rx) = sync_channel::<String>(1);
{ pull_slot.lock().unwrap().replace(tx); }

// Publishes the query — the response arrives later via db/pull/response:
mqtt_guard.publish_text(TOPIC_DB_PULL, &req);

// Waits up to 5 s:
match rx.recv_timeout(Duration::from_secs(5)) {
    Ok(json) => { /* parse worker list, pick one randomly */ }
    Err(_)   => { error!("db/pull timeout"); }
}

// Cleans up the slot for the next query:
pull_slot.lock().unwrap().take();
```

The channel is created, used, and dropped per query. No global response buffer.
No flag that a second concurrent query could accidentally overwrite.

---

## 7. Strengths and weaknesses

### Where Rust wins in this project

- **Dual-core safety without discipline.** The wrong synchronization primitive
  does not compile — the compiler tells you which one to use.
- **The callback can never publish.** In Arduino this is a guideline you may
  forget. In Rust it is a type invariant you cannot bypass.
- **Data races are compile errors.** `tolva_counts[color]++` in a callback
  with no mutex is rejected at `cargo build`. It never silently corrupts
  production counts.
- **Typed events.** The callback puts `RobotEvent` values into the channel —
  not raw bytes and topic strings. The compiler verifies every field at the
  call site. No `strcmp`, no `memcpy`, no mismatched payload.
- **NVS persistence with propagated errors.** Every `save_tolva_counts` call
  uses `?`. A failed flash write propagates to a log line — no silent loss.

### Where Rust was harder

- **Arc clones at every task boundary.** Moving shared state into multiple
  `move` closures requires an `Arc::clone` for every resource that crosses a
  thread boundary. The pattern is mechanical, but the first hour of errors is
  not pleasant.
- **Lifetime annotations on the MQTT client.** `EspMqttClient<'static>` forced
  lifetime annotations to propagate through `spawn_logic_task`. It compiles
  correctly, but the signature is noisy for a first-time reader.
- **Blocking waits hold the thread.** The 6 s AMR loading delay and the 5 s DB
  timeout block the `logic_task` thread. They work, but an `embassy` async
  rewrite would be cleaner. Left as future work.

### Where Arduino has a point

- **Prototyping is faster.** Adding a new topic or reading a new sensor is ten
  lines in `on_loop()`. In Rust you decide the ownership model first.
- **Simpler for simple problems.** If the firmware is one task doing one thing,
  Arduino's single-threaded model is a simplification, not a ceiling.
- **Larger ecosystem.** Vendor libraries for niche hardware are almost always
  C or Arduino first.

The point of this project is not "Rust is always better." It is: **for this
specific class of problem — concurrent tasks, shared state across cores, typed
events, hardware interrupts — Rust catches every structural mistake at compile
time, and Arduino cannot.**

---

## 8. Side-by-side summary

| Concern | Arduino template | Rust firmware |
|---|---|---|
| **Main loop** | `mqtt_loop()` + `on_loop()` — one cannot block the other | Core 0 I/O and Core 1 logic run independently |
| **MQTT callback** | Fires inside `mqtt.loop()`; publishing risks deadlock | Enqueues a typed `RobotEvent` via `try_send` and exits |
| **Shared state** | Global variables — dual-core races are silent | `Arc<Mutex<ControlState>>` — compiler enforces the lock |
| **Emergency flag** | `volatile bool` — single-core safe only | `Arc<AtomicBool>` — CPU atomics, correct on both cores |
| **Event typing** | Untyped bytes, `strcmp` on topic, `memcpy` on payload | `enum RobotEvent` — wrong type is a compile error |
| **Who can publish** | Anywhere, including inside the callback | `logic_task` only — enforced by the type system |
| **Request–response** | Global buffer + timeout flag | One-shot `sync_channel(1)` — created, used, dropped |
| **Data race** | Silent at compile time; wrong counts under load | Does not compile |
| **NVS persistence** | Manual — easy to forget a flush | `?` propagates write errors; saved after every state change |
| **Wi-Fi reconnect** | Blocking `delay()` stalls the whole loop | Non-blocking loop in a dedicated thread |

---

## 9. Folder layout

```
07-project-esp32s3/
├── README.md                     ← this file
├── arduino/                      ← course-provided template (unmodified)
│   ├── ESP32-S3-IoT-Device.ino   ← setup() and loop() — do not modify
│   ├── Config.h                  ← Wi-Fi, MQTT broker, device ID
│   ├── s_setup.ino               ← on_setup() — student code
│   ├── w_loop.ino                ← on_loop() — student code (the ceiling)
│   ├── g_comunicaciones.ino      ← subscription and callback
│   ├── f_funciones.ino           ← helpers
│   ├── d_wifi_lib_no_tocar.ino   ← Wi-Fi library — do not modify
│   └── e_mqtt_lib_no_tocar.ino   ← MQTT library — do not modify
├── documentacion/
│   ├── SISTEMA.md                ← every task, field, flow, timeout
│   ├── mqtt_messages.md          ← all topics and JSON payloads
│   ├── Pruebas_sistema.md        ← integration test guide
│   ├── SETUP_RUST_ESP.md         ← full toolchain setup
│   └── diagrama_tareas_esp32.pdf ← task architecture diagram
└── rust/
    ├── Cargo.toml
    ├── build.rs
    ├── rust-toolchain.toml
    ├── sdkconfig.defaults
    ├── .cargo/config.toml        ← target triple + espflash runner
    └── src/
        ├── main.rs               ← startup, shared resources, task launch
        ├── config.rs             ← credentials, topics, thresholds
        ├── control_state.rs      ← ControlState + RobotEvent
        ├── mqtt_manager.rs       ← MQTT callback and topic dispatch
        ├── logic_task.rs         ← Core 1 loop — all outgoing MQTT lives here
        ├── emergency_task.rs     ← buttons, LED, buzzer
        └── wifi_manager.rs       ← connection and reconnect
```

---

## 10. Build and flash

This project uses `esp-idf-svc` (Rust bindings over ESP-IDF + FreeRTOS), so
ESP-IDF must be installed. See `documentacion/SETUP_RUST_ESP.md` for the
full toolchain guide.

```powershell
# Activate the Xtensa Rust toolchain (Windows PowerShell — after espup install)
. $env:USERPROFILE\export-esp.ps1

# From the rust/ directory:
cd rust
cargo run
```

`cargo run` compiles, flashes via `espflash`, and opens the serial monitor.
Wi-Fi credentials and the MQTT broker URL are in `src/config.rs`.

---

## 11. Where to go next

- Open `arduino/w_loop.ino`. Look at `on_loop()`. Now imagine adding three
  robots, a pallet counter, NVS persistence, and an ISR-driven E-stop without
  blocking `mqtt_loop()`. Count the globals you would need. Then open
  `rust/src/logic_task.rs`.
- Read `documentacion/SISTEMA.md` — it documents every task, every field of
  `ControlState`, every timeout, and every state transition in detail.
- Look at `rust/src/control_state.rs`. Notice that `RobotEvent` is an `enum`
  with named fields. The callback can only put `RobotEvent` values into the
  channel — nothing else fits. That is the type system replacing `strcmp` and
  `memcpy`.
- If the blocking waits in `logic_task` bother you (the 6 s AMR delay, the
  5 s DB timeout), look into `embassy` — the async embedded runtime for Rust.
  The ownership and concurrency model is the same; `thread::sleep` becomes
  `Timer::after(...).await`.
