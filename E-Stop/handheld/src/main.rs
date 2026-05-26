mod button;
mod tx;

use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::sys::link_patches;
use log::{error, info};
use shared::FrameKind;
use std::time::Duration;

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(200);
const STOP_BURST_COUNT: usize = 3;

fn main() -> anyhow::Result<()> {
    link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let radio = tx::init(peripherals.modem)?;
    info!(
        "handheld up — peer MAC {:02x?}, channel {}",
        tx::PEER_MAC,
        tx::WIFI_CHANNEL
    );

    let gpio9 = peripherals.pins.gpio9;
    let on_open = {
        let esp_now = radio.esp_now.clone();
        move || {
            info!("e-stop loop opened — sending {} Stop frame(s)", STOP_BURST_COUNT);
            for _ in 0..STOP_BURST_COUNT {
                if let Err(e) = tx::send_frame(&esp_now, FrameKind::Stop) {
                    error!("Stop send failed: {e}");
                }
            }
        }
    };

    std::thread::Builder::new()
        .stack_size(4096)
        .name("button".into())
        .spawn(move || button::run(gpio9, on_open))?;

    // Heartbeat thread — only transmits while the NC loop is closed (pin LOW).
    // An open loop (press or cable fault) stops the heartbeat stream; the pod
    // also times out at 600 ms even if the explicit Stop burst is lost on the
    // radio. Two independent paths to safe state.
    let hb_esp_now = radio.esp_now.clone();
    std::thread::Builder::new()
        .stack_size(4096)
        .name("heartbeat".into())
        .spawn(move || {
            loop {
                std::thread::sleep(HEARTBEAT_INTERVAL);
                if !button::is_loop_closed() {
                    continue;
                }
                if let Err(e) = tx::send_frame(&hb_esp_now, FrameKind::Heartbeat) {
                    error!("Heartbeat send failed: {e}");
                }
            }
        })?;

    // Keep `radio` alive.
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
