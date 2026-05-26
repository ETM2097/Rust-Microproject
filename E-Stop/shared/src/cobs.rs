//! COBS framing for USB-CDC byte stream. Frames are delimited by `0x00`.

use crate::frame::MAX_FRAME_LEN;

/// Worst case COBS expansion: input + ceil(input / 254) + 1 delimiter.
pub const MAX_ENCODED_LEN: usize = MAX_FRAME_LEN + (MAX_FRAME_LEN / 254) + 2;

#[allow(clippy::result_unit_err)]
/// Encode `frame` into `out`, appending a trailing `0x00` delimiter.
pub fn encode(frame: &[u8], out: &mut [u8]) -> Result<usize, ()> {
    if out.len() < frame.len() + 2 {
        return Err(());
    }
    let n = cobs::encode(frame, &mut out[..]);
    out[n] = 0x00;
    Ok(n + 1)
}

#[allow(clippy::result_unit_err)]
/// Decode a single COBS frame
pub fn decode(encoded: &[u8], out: &mut [u8]) -> Result<usize, ()> {
    if encoded.is_empty() {
        return Ok(0);
    }
    cobs::decode(encoded, out)
        .map(|report| report.frame_size())
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_with_zeros_round_trips() {
        let frame = [0xA5, 0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x34];
        let mut enc = [0u8; MAX_ENCODED_LEN];
        let mut dec = [0u8; MAX_FRAME_LEN];
        let n = encode(&frame, &mut enc).unwrap();
        assert!(enc[..n - 1].iter().all(|b| *b != 0x00));
        let m = decode(&enc[..n - 1], &mut dec).unwrap();
        assert_eq!(&dec[..m], &frame);
    }
}
