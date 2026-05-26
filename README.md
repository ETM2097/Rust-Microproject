# Rust on the ESP32-S3 — Lessons and Projects

A small curriculum that teaches Rust for embedded work on the ESP32-S3,
followed by two real projects that put the lessons into practice. The
lessons assume a working knowledge of C/C++ for microcontrollers and
explain what Rust adds, what it forbids, and why.

Everything in this repository targets the **ESP32-S3** dev board with
an LED and 330 Ω resistor on **GPIO2**.

## Repository layout

```
.
├── Demos_And_Lessons/    Numbered lessons (each one is a standalone Cargo project)
├── Project ESP32S3/      Capstone: plastic-cap sorting cell, multi-robot controller
├── E-Stop/               Wireless emergency stop for the TARS rover
└── LICENSE               Apache-2.0
```

## Lessons

Each lesson is self-contained under [Demos_And_Lessons/](Demos_And_Lessons/),
with its own `README.md`, `Cargo.toml`, and source files. Read them in order
— later lessons assume the earlier ones.

| # | Folder | Topic |
|---|---|---|
| 01 | [01-setup-and-blinky](Demos_And_Lessons/01-setup-and-blinky/) | Toolchain setup, `#![no_std]`, first program |
| 02 | [02-ownership-of-peripherals](Demos_And_Lessons/02-ownership-of-peripherals/) | Typed pin ownership; the bug C lets through |
| 03 | [03-borrowing](Demos_And_Lessons/03-borrowing/) | `&` vs `&mut`, the borrow rule, three things the compiler refuses to build |
| 04 | [04-option-result-and-enums](Demos_And_Lessons/04-option-result-and-enums/) | `Option`, `Result`, sum-type enums, exhaustive `match` |
| 05 | [05 - freertos](Demos_And_Lessons/05%20-%20freertos/) | Crossing into `esp-idf-svc`: FreeRTOS tasks via `std::thread` |

Lessons 01–04 use **bare-metal `esp-hal`** (`no_std`, single-threaded).
Lesson 05 switches to **`esp-idf-svc`** (`std`, FreeRTOS) — the same
stack the projects below are built on.

## Projects

### [Project ESP32S3/](Project%20ESP32S3/) — capstone

A complete firmware running in a plastic-cap manufacturing cell. The
ESP32-S3 coordinates three robots (Delta classifier, AMR transport,
Cobot palletizer) over MQTT while handling physical safety hardware,
persisting state across reboots, and talking to a SCADA and a database
concurrently. Every concept from the lessons shows up under real load.

There is also an `arduino/` directory that preserves the original
Arduino template the team chose not to use, kept for direct comparison.

### [E-Stop/](E-Stop/) — wireless safety side project

Wireless emergency-stop for the TARS rover. Two ESP32-S3 boards talking
over ESP-NOW plus a ROS2 bridge node on the host. Fail-closed by design:
any single fault — radio loss, cable break, MCU crash, USB unplug —
kills every ROS2 process on the rover within ~1 s.

A `shared/` `no_std` crate defines the wire format once; both firmwares
depend on it, and a frozen hex vector keeps the host bridge in lock-step.

## Building any lesson or project

Each subdirectory has its own `Cargo.toml` and is built independently.
General shape:

```bash
cd Demos_And_Lessons/01-setup-and-blinky/rust   # or any other lesson/project
cargo build --release                            # source-level check + binary
cargo run   --release                            # flash to the connected board
```

Lessons 01–04 use the Xtensa target `xtensa-esp32s3-none-elf` and need
`xtensa-esp32s3-elf-gcc` for linking. Lesson 05 and the projects use
`xtensa-esp32s3-espidf` and need `ldproxy` (`cargo install ldproxy`) plus
a local ESP-IDF install. See each lesson's README for the exact setup.

## License

Apache-2.0. See [LICENSE](LICENSE).
