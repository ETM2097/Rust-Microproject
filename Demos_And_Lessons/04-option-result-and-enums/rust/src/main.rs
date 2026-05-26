#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};
use esp_println::println;

// A small command set parsed from a pretend serial input. Sum-type
// enums in Rust carry data per variant — closer to C++ `std::variant`
// than to a C `enum`. The compiler will not let a `match` on this
// type skip a case.
enum Command {
    Toggle,
    SetPeriod(u32),
    Halt,
}

// Parser that may fail. No `NULL`, no `-1`, no `errno`. Failure has
// its own type and the caller must handle it.
fn parse_command(input: &str) -> Result<Command, &'static str> {
    if input == "toggle" {
        Ok(Command::Toggle)
    } else if let Some(rest) = input.strip_prefix("period ") {
        // `.parse::<u32>()` returns `Result<u32, ParseIntError>`.
        // `.map_err` converts the error into our own `&'static str`.
        // The `?` then propagates an `Err` upward if parsing failed.
        let n: u32 = rest.parse().map_err(|_| "period requires a u32")?;
        Ok(Command::SetPeriod(n))
    } else if input == "halt" {
        Ok(Command::Halt)
    } else {
        Err("unknown command")
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut led = Output::new(
        peripherals.GPIO2,
        Level::Low,
        OutputConfig::default(),
    );
    let delay = Delay::new();

    // A canned input list. In a real product these would come from
    // UART; here we just iterate over a slice so the lesson stays
    // focused on the type-system point.
    let simulated_uart_inputs = ["toggle", "period 250", "junk", "halt"];

    for raw in simulated_uart_inputs {
        match parse_command(raw) {
            Ok(Command::Toggle) => {
                println!("toggling");
                led.toggle();
            }
            Ok(Command::SetPeriod(ms)) => {
                println!("set period to {} ms", ms);
            }
            Ok(Command::Halt) => {
                println!("halt requested");
                break;
            }
            Err(msg) => {
                // No exception, no longjmp, no special errno: the
                // error is just another value, matched like any other.
                println!("parse error: {}", msg);
            }
        }
        delay.delay_millis(500);
    }

    loop {
        // After the canned demo, blink slowly forever.
        led.toggle();
        delay.delay_millis(1000);
    }
}
