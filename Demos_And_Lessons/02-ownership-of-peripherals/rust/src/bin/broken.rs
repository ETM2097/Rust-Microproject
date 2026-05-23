#![no_std]
#![no_main]

// Two independent "modules" — a hypothetical `led` and `button` — both
// try to take GPIO2. In C, the second `gpio_set_direction` would
// silently re-configure the pin, defeating the first module. In Rust,
// `peripherals.GPIO2` is a unique value (type `GpioPin<2>`) that does
// not implement `Copy`. Once it is moved into `Output::new`, it no
// longer exists in this scope.
//
// Expected compiler error:
//
//   error[E0382]: use of moved value: `peripherals.GPIO2`
//
// To see it: `cargo build --release --bin broken`.

use esp_backtrace as _;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // "LED module" takes ownership of GPIO2.
    let mut _led = Output::new(
        peripherals.GPIO2,
        Level::Low,
        OutputConfig::default(),
    );

    // "Button module" tries to take GPIO2 too. ERROR: value already moved.
    let _button = Input::new(
        peripherals.GPIO2,
        InputConfig::default().with_pull(Pull::Up),
    );

    loop {}
}
