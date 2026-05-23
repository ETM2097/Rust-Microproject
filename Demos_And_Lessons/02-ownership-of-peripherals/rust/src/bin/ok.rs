#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};

// The "correct" version. A single owner takes GPIO2 once; there is no
// conflict for the compiler to detect.
#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let mut led = Output::new(
        peripherals.GPIO2,
        Level::Low,
        OutputConfig::default(),
    );
    let delay = Delay::new();

    loop {
        led.toggle();
        delay.delay_millis(500);
    }
}
