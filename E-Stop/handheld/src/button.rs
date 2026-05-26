//! GPIO9 NC e-stop loop with hardware-interrupt-driven debounce.
//!
//! Wired: GPIO9 — NC pushbutton — GND, internal pull-up enabled. The pin
//! sees:
//!   - LOW  → NC contacts closed: button at rest, wiring intact → ARMED.
//!   - HIGH → NC contacts open : button pressed, OR cable broken,
//!            OR connector loose → STOP.
//!
//! This is fail-safe: any wiring fault opens the loop and trips the system.
//!
//! `is_loop_closed()` exposes the current debounced state so the heartbeat
//! thread can gate transmission. `run()` invokes `on_open` on each
//! closed→open transition for an immediate Stop burst (faster than waiting
//! for the pod's heartbeat timeout).

use core::num::NonZero;
use core::sync::atomic::{AtomicBool, Ordering};
use esp_idf_svc::hal::gpio::{Gpio9, InterruptType, PinDriver, Pull};
use esp_idf_svc::hal::task::notification::Notification;
use std::time::Duration;

const DEBOUNCE_MS: u64 = 30;

static LOOP_CLOSED: AtomicBool = AtomicBool::new(false);

/// True while the NC loop is intact (pin LOW). False on press or cable fault.
pub fn is_loop_closed() -> bool {
    LOOP_CLOSED.load(Ordering::Acquire)
}

/// Block forever, watching for debounced state changes on the e-stop pin.
/// Invokes `on_open` once per closed→open transition.
pub fn run<F>(pin: Gpio9<'static>, mut on_open: F) -> !
where
    F: FnMut() + Send + 'static,
{
    let mut pin = PinDriver::input(pin, Pull::Up).expect("button: PinDriver::input");
    pin.set_interrupt_type(InterruptType::AnyEdge)
        .expect("button: set_interrupt_type");

    // Seed the state from the pin so we don't fire a spurious open on boot.
    LOOP_CLOSED.store(pin.is_low(), Ordering::Release);

    let notification = Notification::new();
    let notifier = notification.notifier();
    // SAFETY: `notification` lives for the rest of `run`, which is `-> !`,
    // so the ISR's reference is valid for the entire program lifetime.
    unsafe {
        pin.subscribe(move || {
            let _ = notifier.notify(NonZero::new(1).unwrap());
        })
        .expect("button: subscribe ISR");
    }

    loop {
        // esp-idf-hal auto-disables the interrupt on each ISR firing; re-arm.
        pin.enable_interrupt().expect("button: enable_interrupt");
        notification.wait_any();

        // Wait out the bounce, then sample the settled level. Any further
        // ISRs during the sleep just queue one notification we'll consume on
        // the next iteration — they don't desync the state.
        std::thread::sleep(Duration::from_millis(DEBOUNCE_MS));

        let closed = pin.is_low();
        let prev = LOOP_CLOSED.swap(closed, Ordering::AcqRel);
        if prev && !closed {
            on_open();
        }
    }
}
