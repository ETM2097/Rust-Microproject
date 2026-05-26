//! USB-CDC TX: send COBS-framed frames to the host.

use esp_hal::{Blocking, usb_serial_jtag::UsbSerialJtag};

pub struct UsbTx<'a> {
    inner: UsbSerialJtag<'a, Blocking>,
}

impl<'a> UsbTx<'a> {
    pub fn new(usb: UsbSerialJtag<'a, Blocking>) -> Self {
        Self { inner: usb }
    }

    /// Send a complete COBS-encoded frame (caller is responsible for COBS).
    pub fn write_all(&mut self, bytes: &[u8]) {
        use embedded_io::Write;
        let _ = self.inner.write_all(bytes);
        let _ = self.inner.flush();
    }
}
