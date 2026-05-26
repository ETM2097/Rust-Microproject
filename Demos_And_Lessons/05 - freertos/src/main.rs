use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use esp_idf_svc::hal::peripherals::Peripherals;
use log::{error, info};

mod http_client;
mod wifi;

use http_client::DataUploader;

// --- Configuration ---
// Replace with your WiFi credentials and the LAN IP of the PC running the
// Rust server in ./server.
const WIFI_SSID: &str = "YOUR_WIFI_SSID";
const WIFI_PASS: &str = "YOUR_WIFI_PASSWORD";

// LAN IP of the PC running the server. Do NOT use 127.0.0.1 / localhost —
// the ESP32 is a separate host on the network.
const SERVER_URL: &str = "http://192.168.1.100:5000/api/data";

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // The ESP-IDF "main" task ships with a small stack (~3.5 KB) which is
    // not enough to bring WiFi up from Rust. We move all the real work into
    // a coordinator thread with a generous stack so the WiFi/HTTP init can
    // run safely.
    thread::Builder::new()
        .name("coord".into())
        .stack_size(32 * 1024)
        .spawn(|| {
            if let Err(e) = run() {
                error!("coordinator exited with error: {e}");
            }
        })?;

    // Keep the main task alive — it just sleeps.
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn run() -> anyhow::Result<()> {
    let peripherals = Peripherals::take()?;

    // 1) Bring WiFi up
    let _wifi = wifi::connect(peripherals.modem, WIFI_SSID, WIFI_PASS)?;

    // 2) Producer/consumer channel (capacity 8)
    let (tx, rx) = mpsc::sync_channel::<i32>(8);

    // Producer — runs on its own FreeRTOS task
    thread::Builder::new()
        .name("producer".into())
        .stack_size(8 * 1024)
        .spawn(move || {
            for i in 0.. {
                info!("[Producer] sending: {i}");
                if tx.send(i).is_err() {
                    error!("[Producer] channel closed");
                    break;
                }
                thread::sleep(Duration::from_millis(500));
            }
        })?;

    // Consumer — receives from the channel and uploads each value via HTTP
    thread::Builder::new()
        .name("consumer".into())
        .stack_size(16 * 1024)
        .spawn(move || {
            let mut uploader = match DataUploader::new(SERVER_URL) {
                Ok(u) => u,
                Err(e) => {
                    error!("[Consumer] failed to build HTTP client: {e}");
                    return;
                }
            };

            for value in rx {
                info!("[Consumer] received: {value}");
                match uploader.post_value(value) {
                    Ok(_) => info!("[Consumer] uploaded to server"),
                    Err(e) => error!("[Consumer] error uploading {value}: {e}"),
                }
                thread::sleep(Duration::from_millis(1200));
            }
        })?;

    // Hold WiFi alive — if this function returns, `_wifi` drops and the
    // radio is torn down.
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
