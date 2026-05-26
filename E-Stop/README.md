# TARS Rust Pod — Wireless E-Stop

Wireless emergency-stop for the TARS rover. Two ESP32-S3 boards talking
over ESP-NOW, plus a ROS2 bridge node on the host. Fail-closed by design:
any single fault — radio loss, cable break, MCU crash, USB unplug — kills
every ROS2 process on the rover within ~1 s.

```
  Handheld (ESP32-S3)        Pod (ESP32-S3)              TARS host
  ┌───────────────┐          ┌───────────────┐          ┌──────────────┐
  │ GPIO9 NC loop │          │ Safety SM     │          │ bridge_node  │
  │ Heartbeat 5Hz │ ESP-NOW  │ Heartbeat RX  │ USB-CDC  │ stop_all.sh  │
  │ Stop burst    ├─────────►│ Stop RX       ├─────────►│ kill on trip │
  └───────────────┘  ch 1    └───────────────┘  COBS    └──────────────┘
```

## Wire format (5–7 bytes per frame)

```
 magic │ ver │ kind │ payload (0..2 B) │ CRC-16/KERMIT
 0xA5  │ 01  │  01  Heartbeat          │
                    02  Stop           │
                    11  StateChange    │  [safety, link]
```

A `shared/` `no_std` crate defines the format once. Both firmwares depend
on it. The host bridge re-implements the same format in Python and a
frozen hex vector (`a5010107006f0b`) keeps the two in lock-step.

## Handheld — fail-safe deadman loop

GPIO9 wired to a normally-closed pushbutton against GND, internal pull-up.

| Physical state                | Pin   | Behaviour                        |
| ---                           | ---   | ---                              |
| NC at rest, cable intact      | LOW   | armed — sends `Heartbeat` @ 5 Hz |
| Button pressed                | HIGH  | sends 3× `Stop` then silent      |
| Wire broken / connector loose | HIGH  | silent (no false negative)       |
| Battery dies                  | —     | silent                           |

The pin level drives everything. A hardware-interrupt-driven debounce
(`AnyEdge`, 30 ms settle) flips an `AtomicBool` that the heartbeat task
checks before transmitting. **A wiring fault and a deliberate press are
indistinguishable to the firmware — both open the loop, both stop the
rover.** That's the point.

## Pod — Embassy async safety machine

Single async loop driven by `select!` over two futures:

```rust
match select(Timer::after(100ms), RX_CHAN.receive()).await {
    Either::First(_)   => safety.step(Tick { now_ms }),
    Either::Second(ev) => safety.step(HeartbeatRx | StopRx),
}
```

The safety state machine lives in `shared/src/safety.rs` and is
unit-tested on the host. Transitions:

- Boot → `(Armed, Lost)`.
- Heartbeat RX → `link = Live`.
- Stop RX → `safety = Tripped` (latched until reboot).
- 600 ms without RX → `link = Lost`, `safety = Tripped`.

Every 200 ms (and on every transition) the pod emits a COBS-framed
`StateChange` over USB-Serial/JTAG. The bridge uses this as a USB
heartbeat — silence ≥ 1 s means the pod is gone.

## Host bridge — ROS2 node

`tars_rust_pod_bridge` reads framed USB-CDC, publishes `/emergency_stop`
(Bool) and `/tars_rust_pod/link_state` (String) for observability, and on
confirmed trip invokes `/home/tars/scripts/stop_all.sh` which kills every
ROS2 process on the host — including itself.

A configurable boot grace (default 10 s) prevents the kill from firing
while the operator is still bringing the system up. After grace, **any**
of {`Tripped`, link `Lost`, no pod, USB silent ≥ 1 s} fires the kill
exactly once.

## Three independent paths to safe state

The system is engineered so the rover stops even if any single mechanism
fails:

1. **Explicit** — the handheld sends a 3-frame `Stop` burst on loop-open;
   the pod latches `Tripped`.
2. **Implicit** — the handheld stops transmitting; the pod times out at
   600 ms and latches `Tripped`.
3. **Host-side** — the bridge stops seeing pod frames; after 1 s it
   declares `NoPod` and fires the kill.

A radio glitch breaks (1) but (2) catches it. A pod reboot breaks (1) and
(2) but (3) catches it. Killing the bridge itself doesn't disarm the
system — there's nothing left forwarding `false` to `/emergency_stop`.

## What Rust buys us over a C equivalent

This system is a textbook case for Rust on bare metal. Concrete wins
shipped in this codebase:

- **One wire format, host-tested.** `shared/` is `#![no_std]`-compatible
  but builds and runs under `cargo test` on x86. 23 tests cover encode,
  decode, CRC, and the safety state machine — caught before flashing.
  In C you'd typically duplicate the struct between firmware and host
  parser and hope they stay in sync.

- **Exhaustive `match` on `FrameKind`.** `#[repr(u8)]` enums *are* the
  wire byte. Adding a new frame kind is a compile error in every site
  that switches on it — no silent `default: break;` paths.

- **`Result<T, E>` everywhere instead of `esp_err_t`.** The `?` operator
  forces error handling at the call site. There is no path in this
  codebase where a non-zero return is ignored, because the type system
  refuses to compile it.

- **Zero-copy frame decode with lifetimes.** `Frame<'a> { payload: &'a [u8] }`
  borrows the input slice. The compiler proves the borrow never outlives
  the buffer. No `memcpy`, no allocator, no dangling-pointer footgun.

- **Async without an RTOS task per concurrency.** On the pod, the Embassy
  executor runs the RX task and the safety loop cooperatively in a single
  thread of execution. `select!` makes the two scheduling points explicit
  in the source. The equivalent in C would be either two FreeRTOS tasks
  with queue plumbing or a hand-rolled state machine.

- **Ownership-typed peripherals.** `Peripherals::take()` is a one-shot;
  the `WIFI`, `USB_DEVICE` and `GPIO9` get *moved* into the tasks that
  use them. Two tasks racing on the same peripheral is unrepresentable.
  In C, anyone with `<driver/gpio.h>` and the right pin number can
  reconfigure your pin from a different file.

- **Safe shared state across ISR ↔ task.** The handheld's debounce uses
  `AtomicBool` for the loop state and a typed `Notification` for the ISR
  wakeup. No raw `xQueueSendFromISR` with manual `portYIELD_FROM_ISR`,
  no priority-inversion footguns.

## Build

```bash
# Host tests
cd shared      && cargo test                  # 23 tests
cd host_bridge && python3 -m pytest -q        # 19 tests

# Firmware (needs `espup` toolchain, then `. ~/export-esp.sh`)
cd pod      && cargo run --release            # flash + monitor
cd handheld && cargo run --release            # flash + monitor

# ROS2 bridge
colcon build --packages-select tars_rust_pod_bridge
ros2 run tars_rust_pod_bridge bridge_node \
    --ros-args -p port:=/dev/tars_pod
```

`/dev/tars_pod` requires the udev rule in `docs/erc_safety.md` (it also
marks the pod as ignored by ModemManager, which otherwise injects AT
commands on connect and corrupts the USB-CDC stream).

## Layout

- `shared/` — `no_std` wire format + safety state machine. Host-testable.
- `pod/` — Embassy firmware (target `xtensa-esp32s3-none-elf`).
- `handheld/` — `esp-idf-svc` + FreeRTOS firmware (target
  `xtensa-esp32s3-espidf`).
- `host_bridge/` — ROS2 ament_python package.
- `docs/erc_safety.md` — operational guide, udev rule, kill-script
  integration with `demo_bringup.launch.py`.

## License

Apache-2.0
