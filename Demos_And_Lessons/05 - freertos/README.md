# 05 — FreeRTOS from Rust: Tasks, Queues and the `std` Thread Model

The previous lessons used bare-metal `esp-hal`: a single thread of execution,
no operating system, `#![no_std]`. We are now switching ecosystems.

This lesson uses **`esp-idf-svc`**, the Rust bindings on top of ESP-IDF,
which does run FreeRTOS underneath. The new thing: in this ecosystem Rust
can use the full standard library (`std`), including the thread and channel
APIs of `std::sync::mpsc` — and FreeRTOS manages every thread as a real task.

The program we write is a **classic producer–consumer**: one task generates
numbers, another consumes them more slowly, and a FreeRTOS queue acts as a
buffer between them. On top of that, in the final section we extend the
example so the consumer **POSTs every value over HTTP to a server on the
local network that is also written in Rust** (with `axum` + `tokio`) and
visualizes it on a live web page. The whole stack (firmware + server) is
Rust.

> **AI disclosure:** the WiFi/HTTP connectivity extension (section 11
> onwards, including [src/wifi.rs](src/wifi.rs),
> [src/http_client.rs](src/http_client.rs), the updated
> [src/main.rs](src/main.rs), and the entire [server/](server/) directory)
> was **generated entirely by an AI assistant — specifically Anthropic's
> Claude Opus** — and reviewed/integrated by the repository author. The
> original producer/consumer lesson (sections 1–10) remains human-authored.

You should know:

- Lessons 01 to 03 — ESP32-S3 toolchain, ownership and borrowing.
- What a FreeRTOS task is (`xTaskCreate`, `xQueueSend`, `xQueueReceive`).
- Roughly, how concurrency works in C with FreeRTOS.

You do not need to know async Rust or `embassy`. That is for later lessons.

---

## 1. Why we switch from `esp-hal` to `esp-idf-svc`

| | `esp-hal` (lessons 01–03) | `esp-idf-svc` (this lesson) |
|---|---|---|
| **Base system** | Bare-metal, no RTOS | ESP-IDF + FreeRTOS |
| **`std` available** | No (`#![no_std]`) | Yes |
| **Threads** | No — single thread, you drive the loop | Yes — `std::thread` over FreeRTOS tasks |
| **Queues** | Hand-rolled or via `embassy` | `std::sync::mpsc` over FreeRTOS queues |
| **Wi-Fi, BT, MQTT** | Not available | Yes, via IDF services |
| **When to use it** | Small firmware, learning, fine-grained control | Connected projects, teams already on IDF |

What we care about here is the thread model: how Rust makes the
producer–consumer pattern **safer** than C without adding any runtime cost.

---

## 2. The producer–consumer pattern in C (classic FreeRTOS)

In C with FreeRTOS, the standard pattern looks like this:

```c
// A queue holding up to 8 32-bit integers
QueueHandle_t queue = xQueueCreate(8, sizeof(int32_t));

// Producer task
void producer_task(void *arg) {
    int32_t i = 0;
    while (1) {
        xQueueSend(queue, &i, portMAX_DELAY);   // blocks if the queue is full
        ESP_LOGI(TAG, "[Producer] sending: %ld", i);
        i++;
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}

// Consumer task
void consumer_task(void *arg) {
    int32_t value;
    while (1) {
        xQueueReceive(queue, &value, portMAX_DELAY);
        ESP_LOGI(TAG, "[Consumer] received: %ld", value);
        vTaskDelay(pdMS_TO_TICKS(1200));
    }
}

void app_main(void) {
    xTaskCreatePinnedToCore(producer_task, "producer", 4096, NULL, 5, NULL, 0);
    xTaskCreatePinnedToCore(consumer_task, "consumer", 4096, NULL, 5, NULL, 0);
}
```

It works, but there are several problems the C compiler will not help you
catch:

- **`queue` is global** — any task can write to or read from it, even if it
  shouldn't.
- **`&i` passes a pointer to the thread's stack** — if the queue has copy
  semantics, fine; otherwise the data may become invalid as the producer
  task moves on.
- **Nothing stops you from passing the wrong `QueueHandle_t`** — it's just
  an opaque pointer; the compiler does not know what kind of data it holds.

---

## 3. The Rust version: same pattern, safer

Look at [src/main.rs](src/main.rs):

```rust
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    esp_idf_svc::sys::link_patches();

    // Ring buffer of capacity 8 — blocks the producer if it's full
    let (tx, rx) = mpsc::sync_channel::<i32>(8);

    // Producer task
    thread::Builder::new()
        .name("producer".into())
        .stack_size(4096)
        .spawn(move || {
            for i in 0.. {
                println!("[Producer] sending: {i}");
                tx.send(i).unwrap();
                thread::sleep(Duration::from_millis(500));
            }
        })
        .unwrap();

    // Consumer task — slower, so the queue will fill up over time
    thread::Builder::new()
        .name("consumer".into())
        .stack_size(4096)
        .spawn(move || {
            for value in rx {
                println!("[Consumer] received: {value}");
                thread::sleep(Duration::from_millis(1200));
            }
        })
        .unwrap();

    // Keep the main thread alive
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
```

Under the hood, `std::thread::spawn` calls `xTaskCreatePinnedToCore` and
`mpsc::sync_channel` wraps a FreeRTOS queue. The difference is not in
performance — it's in what the compiler guarantees.

---

## 4. Line-by-line walkthrough

### `esp_idf_svc::sys::link_patches()`

This call is **mandatory** at the top of `main` whenever you use
`esp-idf-svc`. It links the ESP-IDF patches that some static-linking
toolchains need to work correctly on Xtensa. If you omit it, the firmware
may panic at boot with a hard-to-diagnose error.

### `mpsc::sync_channel::<i32>(8)`

`sync_channel` creates a channel with a maximum capacity of `8` messages
(`sync` = synchronous = the producer blocks when the queue is full). It
returns a pair:

- `tx` — the sending end (*transmitter*). It is `Clone`-able; multiple
  producers can each have their own copy.
- `rx` — the receiving end (*receiver*). It is unique — there can be only
  one consumer. Trying to clone `rx` is a compile-time error.

The channel type is inferred from the `::<i32>` annotation — only `i32`
values can travel through it. Sending an `f32` or a pointer would be a
compile-time type error.

Compare to C:

```c
QueueHandle_t queue = xQueueCreate(8, sizeof(int32_t));
```

In C, the queue is an opaque pointer. Nothing stops you from passing a
`float*` to `xQueueSend` as long as the element size matches a `float`'s.

### `thread::Builder::new().name(...).stack_size(...).spawn(move || { ... })`

Each call creates a **FreeRTOS task**. The arguments are:

| Rust | FreeRTOS C equivalent |
|---|---|
| `.name("producer")` | The task name (visible in `vTaskList()`) |
| `.stack_size(4096)` | The `usStackDepth` of `xTaskCreate` (bytes here, words in C) |
| `.spawn(move \|\| { ... })` | The task function + capture of environment variables |

The `move` keyword is the most important part. It makes the closure **take
ownership** of every variable it uses from the surrounding scope:

- `tx` (the sender) is moved into the producer task.
- `rx` (the receiver) is moved into the consumer task.

After those two captures, the variables `tx` and `rx` **no longer exist**
in `main`. If you tried to use `tx` outside the closure, the compiler would
stop you with `use of moved value`. The same idea as with GPIO pins in
earlier lessons — ownership transfers and access becomes exclusive
automatically.

### `for value in rx { ... }`

`rx` implements the `Iterator` trait. The `for value in rx` loop calls
`rx.recv()` internally on every iteration, blocking until a message is
available. When the `tx` side disappears (for example, the producer task
ends or calls `drop(tx)`), the iterator returns `None` and the loop exits
cleanly.

In C, that "detect that the producer finished" logic is manual.

### The infinite loop in `main`

```rust
loop {
    thread::sleep(Duration::from_secs(60));
}
```

In ESP-IDF with `std`, `main()` is a FreeRTOS task like any other. If
`main` returns, FreeRTOS terminates that task and the behaviour is
undefined (the board usually reboots). The infinite loop keeps the `main`
task alive without consuming CPU.

---

## 5. What the compiler guarantees automatically

| Guarantee | C + FreeRTOS | Rust + `std::sync::mpsc` |
|---|---|---|
| Only one consumer can receive from the queue | Convention, comments | Type-enforced: `Receiver<T>` does not implement `Clone` |
| Messages have the right type | Only if `sizeof` matches | Compile-time type error if it doesn't |
| The producer doesn't use the queue after handing it to the task | Convention | `move` transfers ownership; using `tx` afterwards is a compile error |
| The consumer does not write to the queue | Convention | `Receiver<T>` only has `.recv()`, no `.send()` |
| Use of data after the task ends | Risk of use-after-free | The channel keeps data alive as long as something needs it |

None of these guarantees has any runtime cost. The compiler verifies them
during `cargo build`, and the resulting binary is equivalent in speed to
the C code.

---

## 6. Program behaviour

When you flash and open the serial monitor, you will see something like:

```text
[Producer] sending: 0
[Consumer] received: 0
[Producer] sending: 1
[Producer] sending: 2
[Producer] sending: 3
[Consumer] received: 1
[Producer] sending: 4
...
```

The producer sends every 500 ms. The consumer processes every 1200 ms. The
queue (capacity 8) acts as a buffer. When the queue fills up, the producer
automatically blocks until the consumer drains a slot — without you
writing any explicit synchronization code.

---

## 7. C vs Rust comparison

### C (ESP-IDF + FreeRTOS)

```c
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "esp_log.h"

static const char *TAG = "demo";
static QueueHandle_t queue;

void producer_task(void *arg) {
    int32_t i = 0;
    for (;;) {
        xQueueSend(queue, &i, portMAX_DELAY);
        ESP_LOGI(TAG, "[Producer] sending: %ld", i);
        i++;
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}

void consumer_task(void *arg) {
    int32_t value;
    for (;;) {
        xQueueReceive(queue, &value, portMAX_DELAY);
        ESP_LOGI(TAG, "[Consumer] received: %ld", value);
        vTaskDelay(pdMS_TO_TICKS(1200));
    }
}

void app_main(void) {
    queue = xQueueCreate(8, sizeof(int32_t));
    xTaskCreatePinnedToCore(producer_task, "producer", 4096, NULL, 5, NULL, 0);
    xTaskCreatePinnedToCore(consumer_task, "consumer", 4096, NULL, 5, NULL, 0);
}
```

### Rust (`esp-idf-svc` + `std`)

```rust
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    esp_idf_svc::sys::link_patches();

    let (tx, rx) = mpsc::sync_channel::<i32>(8);

    thread::Builder::new()
        .name("producer".into())
        .stack_size(4096)
        .spawn(move || {
            for i in 0.. {
                println!("[Producer] sending: {i}");
                tx.send(i).unwrap();
                thread::sleep(Duration::from_millis(500));
            }
        })
        .unwrap();

    thread::Builder::new()
        .name("consumer".into())
        .stack_size(4096)
        .spawn(move || {
            for value in rx {
                println!("[Consumer] received: {value}");
                thread::sleep(Duration::from_millis(1200));
            }
        })
        .unwrap();

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
```

### Relevant differences

| Aspect | C (ESP-IDF) | Rust (`esp-idf-svc`) |
|---|---|---|
| **Queue type** | `QueueHandle_t` (opaque pointer) | `SyncSender<i32>` / `Receiver<i32>` (concrete types) |
| **Who can send** | Anyone with the handle | Only whoever owns `tx` |
| **Who can receive** | Anyone with the handle | Only whoever owns `rx` (unique by design) |
| **Message type** | Only size in bytes is checked | Compile-time error if types don't match |
| **Task creation** | `xTaskCreatePinnedToCore(fn, name, stack, arg, prio, handle, core)` | `thread::Builder::new().name().stack_size().spawn(closure)` |
| **Variables captured by the task** | Passed as `void*`, manual casting required | Captured via `move`, types checked at compile time |
| **Global queue** | Yes, reachable from anywhere | No — `tx` and `rx` are moved into their tasks |
| **End of channel** | Manual logic to detect that the producer finished | `for value in rx` ends automatically |

---

## 8. When to use `esp-idf-svc` vs `esp-hal`

This lesson uses `esp-idf-svc` because we need FreeRTOS and `std`. But the
choice is not trivial:

**Use `esp-hal` (bare-metal) when:**
- You don't need Wi-Fi, BT, or network services.
- You want the fastest compile cycle and the most precise control.
- You're learning embedded Rust from scratch (lessons 01–03).
- The firmware is small and deterministic.

**Use `esp-idf-svc` when:**
- You need Wi-Fi, MQTT, TLS, or any IDF service.
- You want to port existing ESP-IDF code to Rust gradually.
- Your team already knows FreeRTOS and you want to leverage that knowledge.
- You use `std::thread`, `std::sync::mpsc`, or any other `std` primitive.

---

## 9. Build, flash and monitor

### Prerequisites

Unlike lessons 01–03, this project uses `esp-idf-svc`, which requires
ESP-IDF to be installed on your system (CMake, Ninja, Python, the Xtensa
toolchain).

Follow the official install guide if you don't have IDF set up:
`docs.espressif.com/projects/esp-idf/en/latest/esp32s3/get-started`

You also need the Rust toolchain environment variables for Xtensa:

```powershell
# Windows PowerShell (after installing espup)
. $env:USERPROFILE\export-esp.ps1
```

### Compile and flash

```powershell
cd "05 - freertos"
cargo run --release
```

`cargo run` compiles the project, flashes it and opens the serial monitor.
The runner configuration lives in [.cargo/config.toml](.cargo/config.toml).

### If you get ESP-IDF link errors

If the build fails looking for IDF headers, make sure that:

1. `IDF_PATH` points to your ESP-IDF install.
2. You have activated the IDF environment in the current terminal.
3. The version of `esp-idf-svc` in `Cargo.toml` is compatible with your
   installed IDF version.

---

## 10. Project structure

```text
05 - freertos/
├── Cargo.toml          # Dependencies: esp-idf-svc, esp-idf-hal, embuild
├── Cargo.lock
├── rust-toolchain.toml # Pins the Xtensa `esp` toolchain
├── build.rs            # Runs embuild to configure ESP-IDF
├── .gitignore
└── src/
    └── main.rs         # Producer–consumer with std::thread and mpsc
```

`build.rs` uses `embuild` to locate the ESP-IDF install, generate the
needed C bindings and configure the linker environment variables. It is
standard for `esp-idf-svc` projects — you don't need to touch it.

---

## 11. Extension: streaming the data over HTTP to a web page

> **AI-authored section.** Everything from here onward — the WiFi module,
> the HTTP client, the updated `main.rs`, and the native Rust server under
> `server/` — was **generated entirely by Anthropic's Claude Opus** and
> integrated by the author. The whole stack (firmware + server) is Rust,
> with no Python or Flask anywhere. Treat this as reference material for
> connectivity patterns, not as hand-written teaching material like
> sections 1–10.

So far the consumer just prints each value to the serial monitor. We now
extend it so the consumer **POSTs every value over HTTP to a Rust server**
running on your PC. The server keeps a rolling history and exposes a
self-contained dashboard you can open in your browser.

The producer/consumer pattern doesn't change — we just attach an HTTP
client to the consumer that posts each value to `/api/data`.

```
[Producer]  --mpsc-->  [Consumer]  --HTTP POST-->  axum server  -->  Web page
```

### 11.1 Extended project layout

```text
05 - freertos/
├── Cargo.toml              # firmware deps: esp-idf-svc/-hal, embedded-svc, anyhow, log
├── sdkconfig.defaults      # stack sizes and WiFi buffers
├── src/                    # ESP32 firmware
│   ├── main.rs             # producer + consumer that POSTs
│   ├── wifi.rs             # WiFi bring-up (STA + DHCP)
│   └── http_client.rs      # HTTP client that POSTs JSON
└── server/                 # native Rust server (runs on your PC)
    ├── Cargo.toml          # axum + tokio + serde
    ├── rust-toolchain.toml # pins stable toolchain (NOT the esp one)
    └── src/
        ├── main.rs         # axum router, ingest + history endpoints
        └── index.html      # embedded dashboard, auto-refresh every 1 s
```

Two separate Cargo packages live in the same lesson folder: the firmware
at the root (toolchain `esp`, target `xtensa-esp32s3-espidf`) and the
server under `server/` (toolchain `stable`, native target). Each one has
its own `rust-toolchain.toml`, so `cargo` picks the right compiler
automatically depending on the directory.

### 11.2 The [src/wifi.rs](src/wifi.rs) module (firmware)

Wraps the whole WiFi bring-up into a single `connect` function. It:

1. Takes the system event loop (`EspSystemEventLoop`) and the NVS partition
   (where ESP-IDF persists WiFi credentials and other parameters).
2. Wraps the `modem` peripheral in a `BlockingWifi<EspWifi>` — the blocking
   variant is the simplest: each call waits for the matching event before
   returning.
3. Configures station (STA) mode with SSID and password.
4. `start()` powers the radio on, `connect()` triggers association with
   the AP, and `wait_netif_up()` blocks until the network stack (lwIP)
   has an IP from DHCP.

This is equivalent to the classic ESP-IDF C pattern where you would
register handlers for `WIFI_EVENT_STA_START`, `WIFI_EVENT_STA_CONNECTED`
and `IP_EVENT_STA_GOT_IP`. `BlockingWifi` performs that choreography for
you.

### 11.3 The [src/http_client.rs](src/http_client.rs) module (firmware)

Exposes a `DataUploader` struct with a `post_value(value: i32)` method.
Each call:

1. Opens a fresh HTTP connection (`EspHttpConnection` wraps the ESP-IDF
   HTTP client, which internally uses `esp_http_client`).
2. Builds a `{"value": <i32>}` JSON payload with `Content-Type` and
   `Content-Length` headers.
3. POSTs to the configured URL and checks the response status.

> **Why no `serde_json` on the firmware side**: the payload is trivially
> small (`{"value":<i32>}`), so formatting it by hand avoids dragging the
> dependency into the constrained ESP32 build. The server, which runs on
> your PC, *does* use `serde_json` because there it costs nothing.

### 11.4 Updated [src/main.rs](src/main.rs) (firmware)

The ESP-IDF "main" task ships with a small default stack (~3.5 KB) which
is not enough to bring WiFi up from Rust — `wifi_init` overflows it
immediately. To work around that without recompiling ESP-IDF, the new
`main` does almost nothing: it just spawns a **coordinator thread**
(`coord`) with a generous 32 KB stack and sleeps forever. All the real
work (peripherals, WiFi, producer, consumer) lives inside `coord`.

So the structure is:

1. `main` initializes logs, spawns `coord` with `.stack_size(32 * 1024)`,
   and enters an idle `loop`.
2. `coord` takes the `Peripherals` and hands the modem to `wifi::connect`.
3. `coord` creates the same `mpsc::sync_channel` as before.
4. The producer is identical, but its stack is now 8 KB (the default 4 KB
   is too tight once `EspLogger` is involved).
5. The consumer constructs a `DataUploader` inside its own thread (16 KB
   stack, because the ESP-IDF HTTP client allocates a lot) and, for every
   value received, calls `post_value`. If the POST fails it logs an
   `error!` but **does not abort the thread** — the consumer keeps going.
6. `coord` finishes by entering its own infinite `loop` so that the
   `_wifi` binding stays alive (if the function returned, `_wifi` would
   drop and the radio would be torn down).

Three constants at the top of the file **must be edited** before flashing:

```rust
const WIFI_SSID: &str = "YOUR_WIFI_SSID";
const WIFI_PASS: &str = "YOUR_WIFI_PASSWORD";
const SERVER_URL: &str = "http://192.168.1.100:5000/api/data";
```

> ⚠️ `SERVER_URL` must point to the **LAN IP of your PC**, not to
> `localhost` or `127.0.0.1`. The ESP32 is a separate host. Check the IP
> with `ipconfig` (Windows) or `ip a` (Linux/macOS).

### 11.5 The native Rust server ([server/](server/))

[server/src/main.rs](server/src/main.rs) is an `axum` + `tokio` HTTP
server that runs on your laptop. Three endpoints:

| Method | Path             | Description                                  |
|--------|------------------|----------------------------------------------|
| GET    | `/`              | Embedded dashboard (HTML, refreshes every 1s)|
| POST   | `/api/data`      | Receives `{"value": <i32>}` from the ESP32   |
| GET    | `/api/history`   | JSON array of the last 100 samples           |

State is a single `Arc<Mutex<VecDeque<Sample>>>` capped at 100 entries — a
ring buffer in memory. Restarting the server loses the history (it's the
simplest possible example; swap the deque for SQLite or a file if you need
persistence).

The HTML dashboard lives in [server/src/index.html](server/src/index.html)
and is embedded into the binary with `include_str!`, so the server ships
as a single executable with no static-file directory to worry about. The
page polls `/api/history` once a second with `fetch` and rewrites the DOM.
No frameworks, no build step, no WebSocket.

### 11.6 How to run it

**Terminal 1 — start the Rust server on your PC**:

```powershell
cd "05 - freertos/server"
cargo run --release
```

The server listens on `0.0.0.0:5000`. Open `http://localhost:5000` in your
browser. You will see "waiting for data..." until the ESP starts posting.

> The first time you run the server, Windows Firewall will ask whether to
> allow incoming connections. Allow at least for **private networks**,
> otherwise the ESP32 will not be able to reach port 5000.

**Terminal 2 — flash the ESP32**:

```powershell
cd "05 - freertos"
cargo run --release
```

> `--release` matters: in `debug` the firmware binary is huge and the
> WiFi/HTTP stack is noticeably more stable when optimized.

### 11.7 What this extension demonstrates

- The producer/consumer pattern scales: adding a **third logical consumer**
  (the server) does not require touching the channel. You just add work
  inside the existing consumer thread.
- The `sync_channel` still acts as **backpressure** — if the network is
  flaky and POSTs slow down, the channel fills up and the producer blocks
  automatically. No extra synchronization code.
- `esp-idf-svc` gives you WiFi, HTTP, TLS, MQTT and NVS with idiomatic
  Rust APIs on the device side. `axum` gives you a production-grade async
  HTTP server on the host side. **The whole stack is Rust**, with shared
  vocabulary (`Result`, `Serialize`, `Deserialize`, ownership) across the
  network boundary.

### 11.8 Common issues

- **`mismatched types *mut *const u8` when compiling `esp-idf-svc`**:
  outdated crate versions. On Xtensa, `c_char` is `u8`, and only
  `esp-idf-svc >= 0.51` casts to the correct pointer type. Make sure the
  firmware `Cargo.toml` has `esp-idf-svc = "0.51"`,
  `esp-idf-hal = "0.45"` and `embedded-svc = "0.28"`, then `cargo clean`
  before rebuilding.
- **`undefined reference to __pender` at link time**: the
  `embassy-time-driver` feature pulls in `embassy-executor`, which needs
  an arch backend that defines `__pender`. This lesson doesn't use
  embassy at all, so just drop the feature: `features = ["std"]`.
- **`***ERROR*** A stack overflow in task main`** during `wifi_init`:
  the ESP-IDF main task has only ~3.5 KB of stack, not enough to bring
  WiFi up from Rust. The fix is the `coord` thread pattern shown in
  `main.rs` — push all real work into a thread with `.stack_size(32 *
  1024)` and let `main` just sleep.
- **`***ERROR*** A stack overflow in task pthread`** right after `IP
  acquired`: it's actually the `producer` or `consumer` thread crashing
  before `thread::Builder::name(...)` could apply its name. The
  `EspLogger` and `EspHttpConnection` are stack-hungry — bump the
  producer to 8 KB and the consumer to 16 KB.
- **WiFi connects but goes `run -> init (f00)` one second later and then
  times out with `ESP_ERR_TIMEOUT`**: the SSID associated but the WPA2
  handshake failed. Almost always wrong SSID or password (check
  case-sensitivity, hidden whitespace, 2.4 GHz vs 5 GHz band).
- **ESP connects to WiFi but POSTs fail**: most likely the PC firewall,
  or `SERVER_URL` points to `localhost` / `127.0.0.1` instead of the real
  LAN IP of the PC.
- **Page stays on "waiting for data..."**: open the browser console (F12).
  If `/api/history` returns `[]`, the server is not receiving POSTs —
  check the server's stdout. If it returns data but the UI doesn't
  update, the issue is the `fetch` JavaScript.
- **`cargo run` inside `server/` tries to use the `esp` toolchain**: the
  `rust-toolchain.toml` inside `server/` pins `stable` precisely to avoid
  this, and `server/.cargo/config.toml` overrides the parent target. If
  it still happens, you have stray `RUSTUP_TOOLCHAIN` env vars set in
  your shell — start a fresh terminal.

---

## 12. Where to go next

- Change the channel capacity from `8` to `1` and watch the producer block
  much more often — the queue can no longer absorb the speed difference.
- Add a second producer: build another `tx` with `tx.clone()` and spawn a
  third thread. The compiler accepts this because `SyncSender<T>`
  implements `Clone` — but `Receiver<T>` does not, so the consumer is
  still unique.
- Read the **"Fearless Concurrency"** chapter of *The Rust Programming
  Language* (`rust-lang.org/book`, chapter 16). It uses `String` instead
  of GPIO or queues, but the principles of `Send`, `Sync`, `move` and
  `mpsc` are exactly the same ones you saw here.
- If you need shared state (not channels) between tasks, the next step is
  `Arc<Mutex<T>>` — the safe way to share mutable data between threads
  without data races.
