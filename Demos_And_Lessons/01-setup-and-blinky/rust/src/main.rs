#![no_std]
#![no_main]

// esp_backtrace registers the panic handler and the SoC exception handler.
use esp_backtrace as _;

// esp-hal is the Hardware Abstraction Layer for ESP32.
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};

// #[esp_hal::main] places this function at the reset vector and
// forbids it from returning. The return type `!` (the "never type")
// is a compile-time promise that the function never returns —
// an accidental `return;` would be a compile error.
#[esp_hal::main]
fn main() -> ! {
    // esp_hal::init returns a struct that owns every peripheral on the
    // chip exactly once. peripherals.GPIO2 is a value of a type unique
    // to that pin (`GpioPin<2>`), not an integer like `GPIO_NUM_2` in C.
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // Output::new(pin, initial_level, config) *consumes* the pin and
    // returns an Output driver. The returned type only exposes output
    // operations (set_high, set_low, toggle) — you cannot call read()
    // on it because that method doesn't exist for this type; it lives
    // on Flex (input/output) or Input.
    //
    // OutputConfig::default() = push-pull, no pull resistors, default
    // drive strength. Tune it here if you need open-drain, a pull-up,
    // a higher drive strength, etc.
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
