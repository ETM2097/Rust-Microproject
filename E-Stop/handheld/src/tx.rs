//! ESP-NOW transmitter init + per-thread helpers.
//!
//! The handheld brings up Wi-Fi in STA mode without connecting to any AP —
//! ESP-NOW piggy-backs on the WiFi MAC layer, so a STA interface (started,
//! but disconnected) is all we need. The radio is pinned to `WIFI_CHANNEL`
//! so the pod can match it.

use anyhow::Context;
use esp_idf_svc::espnow::{EspNow, PeerInfo};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{esp, esp_wifi_set_channel, wifi_interface_t_WIFI_IF_STA};
use esp_idf_svc::wifi::{ClientConfiguration, Configuration, EspWifi};
use shared::{FrameKind, MAX_FRAME_LEN, encode};
use std::sync::Arc;

/// MAC address of the paired pod. Discover via `espflash board-info` on the pod.
pub const PEER_MAC: [u8; 6] = [0x1c, 0xdb, 0xd4, 0x74, 0xac, 0x1c];

/// Wi-Fi primary channel
pub const WIFI_CHANNEL: u8 = 1;

/// Owns the long-lived radio state.
pub struct Radio {
    pub esp_now: Arc<EspNow<'static>>, // Arc is a cloneable handle to the ESP-NOW driver, which is needed in multiple threads.
    _wifi: EspWifi<'static>,
}

pub fn init(modem: Modem<'static>) -> anyhow::Result<Radio> {
    let sysloop = EspSystemEventLoop::take().context("EspSystemEventLoop::take")?;
    let nvs = EspDefaultNvsPartition::take().context("EspDefaultNvsPartition::take")?;

    let mut wifi = EspWifi::new(modem, sysloop, Some(nvs)).context("EspWifi::new")?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))
        .context("EspWifi::set_configuration")?;
    wifi.start().context("EspWifi::start")?;

    // Extern C function call to set the Wi-Fi channel
    esp!(unsafe { esp_wifi_set_channel(WIFI_CHANNEL, 0) }).context("esp_wifi_set_channel")?;

    let esp_now = EspNow::take().context("EspNow::take")?;

    let peer = PeerInfo {
        peer_addr: PEER_MAC,
        channel: WIFI_CHANNEL,
        ifidx: wifi_interface_t_WIFI_IF_STA,
        encrypt: false,
        ..Default::default()
    };
    esp_now.add_peer(peer).context("EspNow::add_peer")?;

    esp_now
        .register_send_cb(|_peer, status| {
            if matches!(status, esp_idf_svc::espnow::SendStatus::FAIL) {
                log::warn!("esp-now send FAIL");
            }
        })
        .context("EspNow::register_send_cb")?;

    Ok(Radio {
        esp_now: Arc::new(esp_now),
        _wifi: wifi,
    })
}

/// Encode and send one frame. Returns `Ok` on enqueue
pub fn send_frame(esp_now: &EspNow<'static>, kind: FrameKind) -> anyhow::Result<()> {
    let mut buf = [0u8; MAX_FRAME_LEN];
    let n =
        encode(kind, &[], &mut buf).map_err(|e| anyhow::anyhow!("shared::encode: {e:?}"))?;
    esp_now.send(PEER_MAC, &buf[..n]).context("EspNow::send")?;
    // If no payload was received
    Ok(())
}
