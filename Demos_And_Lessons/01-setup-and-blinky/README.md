# 01 — Setup and Blinky: From C to Rust on the ESP32-S3

This lesson is the "Hello, World!" of embedded development: blinking an LED.
You probably already wrote one in C using ESP-IDF. Here you will see the same
program written in Rust, learn how to set up the Rust toolchain for the
ESP32-S3, and understand *why* the Rust version looks the way it does.

This guide assumes you already know:

- The basics of C/C++ (pointers, headers, `#define`, build systems).
- What a microcontroller is (GPIO, peripherals, registers, ISR).
- How to flash firmware to an ESP32 with ESP-IDF.

It does **not** assume you know any Rust.

---

## 1. Why Rust on a microcontroller?

C is the lingua franca of embedded, but it has well-known footguns: undefined
behavior, manual memory management, easy-to-misuse pointers, and almost no
help from the compiler when you wire a peripheral incorrectly.

Rust keeps the things we like about C (no garbage collector, predictable
performance, direct access to hardware) and adds:

- **Memory safety without a runtime.** The compiler proves at *compile time*
  that you cannot use freed memory, read uninitialized data, or share
  mutable state across tasks without synchronization.
- **Strong types for hardware.** A GPIO pin configured as an output is a
  *different type* than a pin configured as input. Calling `read()` on an
  output pin is a compile error, not a runtime bug.
- **No hidden cost.** What you don't use, you don't pay for. The compiled
  binary is comparable in size and speed to equivalent C.

The trade-off: a steeper learning curve and a smaller (but rapidly growing)
ecosystem.

---

## 2. Bare-metal: what does it actually mean?

In the C version of this lesson, your `app_main` runs *on top of FreeRTOS*,
which ESP-IDF starts for you behind the scenes. FreeRTOS gives you tasks,
queues, timers, `vTaskDelay`, and a heap.

In the Rust version, there is **no operating system**. The program is
*bare-metal*: the linker places your `main` function directly after the
reset vector, and your code is the only thing running on the CPU (apart
from interrupt handlers you register yourself).

Concretely, bare-metal Rust means:

- **`#![no_std]`** — the standard library (`std`) is not available. `std`
  assumes things a microcontroller does not have: an OS, a filesystem,
  threads, a heap by default. We use `core` instead, which is the subset
  of the standard library that does not require an OS.
- **`#![no_main]`** — there is no `fn main()` in the C sense (called by a
  C-runtime that initialized argc/argv/env). The entry point is wired up
  manually by a macro (`#[entry]`).
- **No `println!` to stdout** — there is no stdout. Logging goes over UART
  via crates like `esp-println`.
- **You bring your own panic handler.** If something panics, the runtime
  has to know what to do (halt? reboot? print a backtrace?). That is what
  `esp-backtrace` provides in this project.

If you want FreeRTOS-like multitasking in Rust, you reach for `embassy` (an
async runtime) or `RTIC`. For a blinky, bare-metal is more than enough.

---

## 3. HAL and why `esp-hal`

A **HAL (Hardware Abstraction Layer)** is a library that wraps raw register
access (`*((volatile uint32_t*)0x60004000) = ...`) into a typed, ergonomic
API (`led.set_high()`).

You already used a HAL in C without thinking about it: `gpio_set_level()`,
`gpio_set_direction()`, and `vTaskDelay()` are part of ESP-IDF's HAL/driver
layer. They abstract the GPIO matrix, the IO MUX, and the system timer so
you don't have to poke registers by hand.

Rust on ESP32 has two ecosystems:

| Ecosystem      | What it is                                            | When to use                          |
|----------------|-------------------------------------------------------|--------------------------------------|
| `esp-idf-hal`  | Rust bindings *on top of* ESP-IDF (still uses FreeRTOS, Wi-Fi, BT stacks). | When you need Wi-Fi/BT/heap-heavy IDF features. |
| `esp-hal`      | Pure Rust, **bare-metal**, no IDF, no FreeRTOS.       | Lessons, small projects, learning, deterministic firmware. |

We use **`esp-hal`** here because:

1. It is the cleanest way to *see* what bare-metal Rust looks like —
   nothing is hidden by an RTOS.
2. The compile/flash cycle is faster and the toolchain is simpler (no
   IDF dependency).
3. Its API uses Rust's type system to its full extent (the typed-pin
   pattern shown below), which is the main teaching point.

---

## 4. Toolchain setup

The ESP32-S3 uses a **Xtensa** CPU core, which is **not** supported by
upstream Rust (upstream Rust only targets Xtensa via a custom LLVM fork
maintained by Espressif). To build for it, you need the *esp* Rust
toolchain, installed via `espup`.

> If you ever target an ESP32-C3/C6/H2, those use **RISC-V**, which the
> stock `rustup` toolchain can build for. You would skip `espup` and use
> `rustup target add riscv32imc-unknown-none-elf` instead.

### 4.1 Install Rust

If you don't have Rust yet:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Restart your shell, then verify:

```bash
rustc --version
cargo --version
```

### 4.2 Install `espup` and the `esp` toolchain

`espup` is Espressif's installer for the Xtensa Rust toolchain.

```bash
cargo install espup --locked
espup install
```

`espup install` downloads the Xtensa LLVM, the `esp` Rust toolchain, and
the GCC sysroot, and writes a script that exports the right environment
variables. **Source it in every shell** before building:

```bash
. $HOME/export-esp.sh         # Linux/macOS
# Windows PowerShell:
# . $env:USERPROFILE\export-esp.ps1
```

Tip: add the `source` line to your `~/.bashrc` / `~/.zshrc` so you don't
forget.

### 4.3 Install the flasher

`espflash` is the equivalent of `idf.py flash monitor`. It detects the
serial port, programs the chip, and opens a serial monitor.

```bash
cargo install espflash --locked
```

Make sure your user has permission to access the serial device. On Linux:

```bash
sudo usermod -aG dialout $USER   # then log out and back in
```

### 4.4 Build, flash, and monitor

From the `rust/` directory of this lesson:

```bash
cargo run --release
```

`cargo run` builds the project, flashes the binary, and opens the serial
monitor. The `runner` and the target are pinned in `.cargo/config.toml`:

```toml
[target.xtensa-esp32s3-none-elf]
runner = "espflash flash --monitor"

[build]
target = "xtensa-esp32s3-none-elf"
```

That is why `cargo run` "just works" — no extra flags needed.

---

## 5. Walkthrough of `src/main.rs`

Here is the full file again, annotated section by section:

```rust
#![no_std]
#![no_main]
```

The two crate-level attributes that mark the project as bare-metal (see
section 2). Without them, the compiler assumes there is a hosted OS and
your build fails immediately.

```rust
use esp_backtrace as _;
```

Pulls in `esp-backtrace` *for its side effects only* — it registers a
panic handler and an exception handler. The `as _` tells the compiler
"I know I don't reference any names from this crate; don't warn me."
Equivalent to linking a C library that only provides startup symbols.

```rust
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output},
    prelude::*,
};
```

This is the HAL. `Output` is the GPIO-as-output type, `Level` is an enum
(`High` / `Low`), `Delay` is a blocking delay helper backed by the system
timer. The `prelude::*` brings in commonly used traits (including the
`#[entry]` macro).

```rust
#[entry]
fn main() -> ! {
```

`#[entry]` is a procedural macro from the HAL's startup crate. It tells
the linker: *this is the function to call after the reset vector and
after the runtime sets up the stack and `.bss`/`.data`*. Roughly the
equivalent of ESP-IDF wiring `app_main` for you.

The return type `!` is the **"never type"**. It is the type of an
expression that never produces a value: an infinite loop, a panic, a
function that halts the CPU. Declaring `fn main() -> !` makes the
compiler *prove* that the function never returns. If you accidentally
add a `return;` or let control fall off the end, you get a compile
error — not a crash at runtime when the program "ends" on a CPU that
has nowhere to return *to*.

```rust
let peripherals = esp_hal::init(esp_hal::Config::default());
```

`esp_hal::init` is called exactly once. It returns a struct that *owns*
every peripheral on the chip — every GPIO pin, every UART, every timer,
exactly once. This is the famous **"singleton pattern"** in embedded
Rust: you cannot call `init` twice (the second call won't compile in a
typical layout), and you cannot accidentally configure the same pin
from two places, because moving the pin into one driver means no one
else has access to it.

Contrast with C, where `GPIO_NUM_2` is just an integer — nothing stops
two modules from both calling `gpio_set_direction(GPIO_NUM_2, ...)` with
conflicting configurations.

```rust
let mut led = Output::new(peripherals.GPIO2, Level::Low);
```

`Output::new` *consumes* `peripherals.GPIO2` (note: no `&`, no `*` —
the value is moved) and gives back an `Output` driver bound to that
specific pin. The returned value's type only exposes output operations
(`set_high`, `set_low`, `toggle`). There is no `.read()` method on it,
because reading isn't valid for a pin configured as a push-pull output —
*and that is enforced by the type system, not by a runtime check*.

If later you needed a pin you can both read and drive, you would build
a `Flex` (input/output) instead, and you'd get a different type with
different methods.

`let mut` (vs just `let`) is required because `toggle()` mutates the
driver's internal state.

```rust
let delay = Delay::new();
```

A blocking delay source. Backed by the SoC's system timer; no FreeRTOS
tick involved.

```rust
loop {
    led.toggle();
    delay.delay_millis(500);
}
```

The forever loop. `loop {}` is the idiomatic infinite loop in Rust —
the compiler knows it never exits, which is what allows the `-> !`
return type to type-check.

---

## 6. Side-by-side: C vs Rust

Here is the same program, line-for-line, in both languages.

### The C version (`c/main/blinky.c`)

```c
#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#define LED_GPIO          GPIO_NUM_2
#define BLINK_PERIOD_MS   500U

void app_main(void)
{
    uint32_t led_level = 0U;

    gpio_reset_pin(LED_GPIO);
    gpio_set_direction(LED_GPIO, GPIO_MODE_OUTPUT);

    for (;;)
    {
        gpio_set_level(LED_GPIO, led_level);
        led_level = (0U == led_level) ? 1U : 0U;
        vTaskDelay(pdMS_TO_TICKS(BLINK_PERIOD_MS));
    }
}
```

### The Rust version (`rust/src/main.rs`)

```rust
#![no_std]
#![no_main]

// `esp_backtrace` registers the panic handler and the SoC exception handler.
use esp_backtrace as _;

// esp-hal is the Hardware Abstraction Layer for ESP32
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output},
    prelude::*,
};

// #[entry] is the bare-metal equivalent of app_main in ESP-IDF.
// The return type `!` (the "never type") is a compile-time promise
// that the function never returns. An accidental `return;` would be a compile error.
#[entry]
fn main() -> ! {
    // esp_hal::init returns a struct that owns every peripheral on the
    // chip exactly once. peripherals.GPIO2 is a value of a type unique
    // to that pin (`GpioPin<2>`), not an integer like `GPIO_NUM_2` in C.
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // Output::new(pin, level) *consumes* the pin and gives back an
    // Output<static>. The returned type only exposes output operations
    // (set_high, set_low, toggle) — you cannot call read() on it
    // because that method doesn't exist for this type, it exists
    // on Flex (input/output) or Input.
    let mut led = Output::new(peripherals.GPIO2, Level::Low);
    let delay = Delay::new();

    loop {
        led.toggle();
        delay.delay_millis(500);
    }
}
```

### Differences that matter

| Topic                | C (ESP-IDF)                                              | Rust (`esp-hal`)                                                |
|----------------------|----------------------------------------------------------|------------------------------------------------------------------|
| **Runtime**          | FreeRTOS started by IDF before `app_main`.               | Bare-metal: no RTOS, code runs on the reset vector.              |
| **Entry point**      | `void app_main(void)` — discovered by name.              | `#[entry] fn main() -> !` — wired up by a macro; must never return. |
| **Standard library** | libc + IDF + FreeRTOS available.                         | `#![no_std]` — only `core` plus what you opt into.               |
| **Panic / fault**    | Default IDF panic handler.                               | Provided explicitly by `esp_backtrace` (we *chose* it as a dep). |
| **Pin identity**     | `GPIO_NUM_2` is an `int` enum value.                     | `peripherals.GPIO2` is a unique *type* (`GpioPin<2>`).           |
| **Pin direction**    | Set at runtime via `gpio_set_direction(...)`.            | Encoded in the type: `Output<...>` vs `Input<...>` vs `Flex<...>`. |
| **Pin aliasing**     | Any function can grab `GPIO_NUM_2` and reconfigure it.   | Pin is *moved* into the driver; nothing else can touch it.       |
| **Wrong direction call** | `gpio_get_level(out_pin)` compiles and silently misbehaves. | `out_pin.read()` is a **compile error** — method doesn't exist.  |
| **Toggle**           | Manual: keep a `led_level` variable, flip with `? :`.    | `led.toggle()` — driver tracks state.                            |
| **Delay**            | `vTaskDelay(pdMS_TO_TICKS(500))` — uses RTOS tick.       | `delay.delay_millis(500)` — uses HW timer, blocks the CPU.       |
| **Build system**     | CMake + `idf.py build flash monitor`.                    | Cargo + `cargo run --release` (espflash is the runner).          |
| **Toolchain**        | `xtensa-esp32s3-elf-gcc` from IDF.                       | `esp` Rust channel (Xtensa LLVM fork) installed via `espup`.     |
| **Dependencies**     | Listed implicitly through `idf_component_register`.      | Listed explicitly in `Cargo.toml` with semver versions.          |

### A subtle point about the delay

The C version yields to FreeRTOS during `vTaskDelay`, so other tasks (Wi-Fi,
the IDLE task that resets the watchdog, etc.) can run. The Rust version
calls a **blocking** delay: the CPU spins in a tight loop checking a timer.
That is fine for a single-task blinky on bare-metal, but if you later add
Wi-Fi or async code, you would use `embassy_time::Timer::after(...).await`
instead — same idea, but cooperative.

---

## 7. Quick reference

```text
.
├── c/                      # ESP-IDF + FreeRTOS version
│   ├── CMakeLists.txt
│   ├── sdkconfig.defaults  # CONFIG_IDF_TARGET="esp32s3", FreeRTOS HZ
│   └── main/
│       ├── blinky.c
│       └── CMakeLists.txt
└── rust/                   # Bare-metal Rust version with esp-hal
    ├── Cargo.toml          # Dependencies (esp-hal, esp-backtrace, ...)
    ├── rust-toolchain.toml # Pins the `esp` Xtensa toolchain
    ├── .cargo/config.toml  # Target triple + espflash runner + linker args
    └── src/main.rs
```

Common commands:

```bash
# C version
cd c/
idf.py set-target esp32s3
idf.py build flash monitor

# Rust version (after `. ~/export-esp.sh`)
cd rust/
cargo run --release
```

---

## 8. Where to go next

- Read the [`esp-hal` book](https://docs.espressif.com/projects/rust/esp-hal/latest/)
  for a tour of the other peripherals.
- Try changing `Output::new(..., Level::Low)` to `Level::High` and watch
  the LED start in the opposite state.
- Replace the blocking `Delay` with `embassy` async timers — that is the
  next lesson once you are comfortable with bare-metal.

If anything in this guide felt hand-wavy, the answer is almost always in
the Rust source file itself: every line in `src/main.rs` has a reason.
Read it again with this guide next to you.
