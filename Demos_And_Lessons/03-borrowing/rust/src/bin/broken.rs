#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};

// ----------------------------------------------------------------------------
// (1) Two mutable references to the same pin at the same time.
//
// In C this is the daily bug that ends with the LED flickering at random
// because two modules both "own" the pin and write to it. Rust refuses to
// compile it.
//
// The rule: a value can be borrowed
//   - by any number of *shared* references (&T),   OR
//   - by exactly one  *exclusive* reference (&mut T),
// never both, never overlapping in time.
//
//     error[E0499]: cannot borrow `*led` as mutable more than once at a time
// ----------------------------------------------------------------------------
fn double_mut_borrow(led: &mut Output<'static>) {
    let writer_a = &mut *led;
    let writer_b = &mut *led;     // second &mut while writer_a is still alive
    writer_a.set_high();
    writer_b.set_low();
}

// ----------------------------------------------------------------------------
// (2) Mixing a shared (read-only) and an exclusive (read/write) borrow.
//
// "What's the harm? one reads, the other writes." The harm is that the
// holder of `&T` assumes the value is stable for the duration of its
// borrow — and the optimizer is allowed to bake that assumption in. If a
// writer can poke the same value while a reader still holds the reference,
// the reader's reasoning breaks.
//
//     error[E0502]: cannot borrow `*led` as mutable because it is also
//                   borrowed as immutable
// ----------------------------------------------------------------------------
fn read_while_writing(led: &mut Output<'static>) {
    let observer: &Output<'static> = &*led;     // shared borrow starts here
    led.toggle();                                // exclusive use here ...
    let _alive = observer;                       // ... but observer lives until here
}

// ----------------------------------------------------------------------------
// (3) Trying to write to a pin through a *shared* reference.
//
// A `&T` (shared reference) gives read-only access. Even if you only
// hold one of them, you cannot use it to mutate. In C you would write
// through a `const T*` by casting the const away and the compiler
// would trust you. In Rust the read-only-ness is part of the type:
// without an explicit `&mut`, the methods that mutate are simply not
// available on the value you hold.
//
//     error[E0596]: cannot borrow `*led` as mutable, as it is behind
//                   a `&` reference
// ----------------------------------------------------------------------------
fn write_via_shared_ref(led: &Output<'static>) {
    led.set_high();
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

    // We call each broken function so the compiler is forced to look at
    // them. Each call site is fine on its own — the errors live *inside*
    // the function bodies above.
    double_mut_borrow(&mut led);
    read_while_writing(&mut led);
    write_via_shared_ref(&led);

    loop {
        delay.delay_millis(1000);
    }
}
