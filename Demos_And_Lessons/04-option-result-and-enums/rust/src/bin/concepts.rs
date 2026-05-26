#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};
use esp_println::println;

// 1. Plain enum - just named tags. Same shape as a C enum.
enum Power {
    Off,
    On,
    Sleep,
}

fn step1_plain_enum() {
    for p in [Power::Off, Power::On, Power::Sleep] {
        match p {
            Power::Off => println!("Step 1: power off"),
            Power::On => println!("Step 1: power on"),
            Power::Sleep => println!("Step 1: power sleep"),
        }
    }
}

// 2. Sum-type enum - Pwm carries a duty cycle; Off and On don't.
enum Signal {
    Off,
    On,
    Pwm(u8),
}

fn step2_sum_type_enum() {
    for s in [Signal::Off, Signal::On, Signal::Pwm(128)] {
        match s {
            Signal::Off => println!("Step 2: signal off"),
            Signal::On => println!("Step 2: signal on"),
            Signal::Pwm(duty) => println!("Step 2: signal pwm, duty={}", duty),
        }
    }
}

// 3. Option<T> — a sensor reading that is only present when the sensor
// is ready. None = "not ready yet", Some(t) = "here is the value".
fn read_temperature(sensor_ready: bool) -> Option<i16> {
    if sensor_ready {
        Some(23)
    } else {
        None
    }
}

fn step3_option() {
    for ready in [true, false] {
        match read_temperature(ready) {
            Some(t) => println!("Step 3: ready={} -> Some({} C)", ready, t),
            None => println!("Step 3: ready={} -> None", ready),
        }
    }
}

// 4. Result<T, E> — setting brightness fails if the percentage is out
// of range. Ok(v) = accepted, Err(msg) = rejected with a reason.
fn set_brightness(percent: u8) -> Result<u8, &'static str> {
    if percent > 100 {
        Err("over 100%")
    } else {
        Ok(percent)
    }
}

fn step4_result() {
    for p in [50u8, 200u8] {
        match set_brightness(p) {
            Ok(v) => println!("Step 4: {}% -> Ok({})", p, v),
            Err(msg) => println!("Step 4: {}% -> Err({:?})", p, msg),
        }
    }
}

// 5. Exhaustive match — reuse Signal from step 2. The compiler proves
// every variant is handled.
fn step5_match_exhaustive() {
    let s = Signal::Pwm(64);
    match s {
        Signal::Off => println!("Step 5: off"),
        Signal::On => println!("Step 5: on"),
        Signal::Pwm(duty) => println!("Step 5: pwm, duty={}", duty),
        // Add `Pulse` to Signal above and this match stops compiling.
    }
}

// 6. All four pieces together. Imagine bytes arriving over UART or SPI:
// reading the next byte may have nothing yet (Option), decoding the
// byte may fail (Result), the decoded command is a sum-type enum, and
// dispatch is one exhaustive match.
enum Command {
    Off,
    On,
    Brightness(u8),
}

fn decode(byte: u8) -> Result<Command, &'static str> {
    match byte {
        0x00 => Ok(Command::Off),
        0x01 => Ok(Command::On),
        0x80..=0xFF => Ok(Command::Brightness((byte - 0x80) * 2)),
        _ => Err("unknown opcode"),
    }
}

fn next_byte(idx: usize) -> Option<u8> {
    let queue = [0x01u8, 0xC0, 0x42, 0x00];
    queue.get(idx).copied()
}

fn step6_composed() {
    for i in 0..5 {
        match next_byte(i) {
            None => println!("Step 6: i={} -> queue empty", i),
            Some(b) => match decode(b) {
                Ok(Command::Off) => println!("Step 6: 0x{:02X} -> Off", b),
                Ok(Command::On) => println!("Step 6: 0x{:02X} -> On", b),
                Ok(Command::Brightness(d)) => {
                    println!("Step 6: 0x{:02X} -> Brightness({})", b, d)
                }
                Err(msg) => println!("Step 6: 0x{:02X} -> Err({:?})", b, msg),
            },
        }
    }
}



#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let delay = Delay::new();

    step1_plain_enum();
    step2_sum_type_enum();
    step3_option();
    step4_result();
    step5_match_exhaustive();
    step6_composed();

    loop {
        led.toggle();
        delay.delay_millis(1000);
    }
}
