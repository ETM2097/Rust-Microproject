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
