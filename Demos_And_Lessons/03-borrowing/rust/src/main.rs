#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};

// Helper that takes a *mutable reference* to a pin and toggles it twice.
// `&mut` borrows the pin with permission to mutate it.
//
// While this function holds the borrow, the caller cannot use `led`
// directly: the borrow checker rejects any concurrent access. Once the
// function returns, the borrow ends and `led` is usable again.
fn blink_once(led: &mut Output<'static>, delay: &Delay) {
    led.toggle();
    delay.delay_millis(250);
    led.toggle();
    delay.delay_millis(250);
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // Ownership stays in `main`. The helper only borrows.
    let mut led = Output::new(
        peripherals.GPIO2,
        Level::Low,
        OutputConfig::default(),
    );
    let delay = Delay::new();

    loop {
        // Pass a mutable reference; ownership is not moved.
        blink_once(&mut led, &delay);

        // We can use `led` here again because the borrow ended when
        // `blink_once` returned. The borrow checker enforced the rule:
        //   many shared (&T) borrows OR one exclusive (&mut T) borrow,
        //   never both at the same time, never overlapping.
        led.set_low();
        delay.delay_millis(500);
    }
}

// More info on borrowing:
// - https://doc.rust-lang.org/rust-by-example/scope/borrow.html
