//! ESP-NOW receive side: an Embassy task that pumps frames into `RX_CHAN`.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use esp_radio::esp_now::EspNowReceiver;
use shared::{Frame, FrameError, FrameKind, decode};

#[derive(Debug, Clone, Copy)]
pub enum RxEvent {
    Heartbeat,
    Stop,
    BadFrame,
}

pub static RX_CHAN: Channel<CriticalSectionRawMutex, RxEvent, 8> = Channel::new();

fn classify(bytes: &[u8]) -> RxEvent {
    match decode(bytes) {
        Ok(Frame { kind: FrameKind::Heartbeat, .. }) => RxEvent::Heartbeat,
        Ok(Frame { kind: FrameKind::Stop, .. }) => RxEvent::Stop,
        Ok(_) => RxEvent::BadFrame, // Unexpected frame kind on the radio.
        Err(FrameError::TooShort | FrameError::BadMagic) => RxEvent::BadFrame,
        Err(_) => RxEvent::BadFrame,
    }
}

// Embassy task body: block on the radio, classify each frame, push it to
// `RX_CHAN`. Drop the event if the queue is full
#[embassy_executor::task]
pub async fn rx_task(mut receiver: EspNowReceiver<'static>) {
    loop {
        let frame = receiver.receive_async().await;
        let _ = RX_CHAN.try_send(classify(frame.data()));
    }
}
