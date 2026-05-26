#![no_std]
#![no_main]

mod radio;
mod usb;

use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock, interrupt::software::SoftwareInterruptControl, timer::timg::TimerGroup,
    usb_serial_jtag::UsbSerialJtag,
};
use shared::cobs::MAX_ENCODED_LEN;
use shared::{FrameKind, LinkState, MAX_FRAME_LEN, Safety, SafetyEvent, SafetyState, cobs, encode};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

// Wi-Fi primary channel;
const WIFI_CHANNEL: u8 = 1;

const TICK_INTERVAL: Duration = Duration::from_millis(100);
const USB_HEARTBEAT_TICKS: u32 = 2;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // esp-radio heap.
    esp_alloc::heap_allocator!(size: 72 * 1024);

    // Bring up the cooperative RTOS layer
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    // Wi-Fi controller + interfaces.
    static CONTROLLER: StaticCell<esp_radio::wifi::WifiController<'static>> = StaticCell::new();
    let (controller, interfaces) = esp_radio::wifi::new(peripherals.WIFI, Default::default())
        .expect("esp_radio::wifi::new");
    let _controller = CONTROLLER.init(controller);

    let esp_now = interfaces.esp_now;
    esp_now
        .set_channel(WIFI_CHANNEL)
        .expect("EspNow::set_channel");

    let (_manager, _sender, receiver) = esp_now.split();
    spawner.spawn(radio::rx_task(receiver).unwrap());

    let usb_dev = UsbSerialJtag::new(peripherals.USB_DEVICE);
    let mut tx = usb::UsbTx::new(usb_dev);

    // Boot signal
    let mut safety = Safety::new();
    send_state(&mut tx, safety.safety(), safety.link());
    let mut ticks_since_send: u32 = 0;

    loop {
        let tick = Timer::after(TICK_INTERVAL);
        let rx = radio::RX_CHAN.receive();
        let changed = match select(tick, rx).await {
            Either::First(_) => {
                let now_ms = Instant::now().as_millis();
                safety.step(SafetyEvent::Tick { now_ms })
            }
            Either::Second(ev) => {
                let now_ms = Instant::now().as_millis();
                safety.mark_rx(now_ms);
                match ev {
                    radio::RxEvent::Heartbeat => safety.step(SafetyEvent::HeartbeatRx),
                    radio::RxEvent::Stop => safety.step(SafetyEvent::StopRx),
                    radio::RxEvent::BadFrame => false,
                }
            }
        };
        ticks_since_send += 1;
        if changed || ticks_since_send >= USB_HEARTBEAT_TICKS {
            send_state(&mut tx, safety.safety(), safety.link());
            ticks_since_send = 0;
        }
    }
}

fn send_state(tx: &mut usb::UsbTx<'_>, safety: SafetyState, link: LinkState) {
    let mut raw = [0u8; MAX_FRAME_LEN];
    let mut framed = [0u8; MAX_ENCODED_LEN];
    let payload = [safety.as_u8(), link.as_u8()];
    let n = encode(FrameKind::StateChange, &payload, &mut raw).expect("encode StateChange");
    let m = cobs::encode(&raw[..n], &mut framed).expect("cobs StateChange");
    tx.write_all(&framed[..m]);
}
