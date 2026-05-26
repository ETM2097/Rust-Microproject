//! Wire-format frame, encode/decode, CRC.
//! Layout: magic(1) + version(1) + kind(1) + payload(0..=MAX_PAYLOAD) + crc(2).

const CRC_ALG: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_KERMIT);

pub fn crc16(bytes: &[u8]) -> u16 {
    CRC_ALG.checksum(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Heartbeat = 0x01,
    Stop = 0x02,
    StateChange = 0x11,
}

impl FrameKind {
    pub fn from_u8(b: u8) -> Result<Self, FrameError> {
        match b {
            0x01 => Ok(Self::Heartbeat),
            0x02 => Ok(Self::Stop),
            0x11 => Ok(Self::StateChange),
            _ => Err(FrameError::UnknownKind(b)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    TooShort,
    BadMagic,
    BadVersion(u8),
    UnknownKind(u8),
    BadCrc,
    BadPayload,
}

// MAGIC is for checking and avoid garbage frames.
pub const MAGIC: u8 = 0xA5;
// VERSION is for future-proofing the format.
pub const VERSION: u8 = 0x01;
pub const MAX_PAYLOAD: usize = 32;
/// magic + ver + kind + payload + crc(2)
pub const MAX_FRAME_LEN: usize = 1 + 1 + 1 + MAX_PAYLOAD + 2;
/// Smallest legal frame: empty payload (Heartbeat / Stop).
const MIN_FRAME_LEN: usize = 1 + 1 + 1 + 2;

/// Encode a frame into `out`. Returns the number of bytes written, or
/// `Err(FrameError::BadPayload)` if `payload` is longer than `MAX_PAYLOAD`
/// or `out` is too small.
pub fn encode(kind: FrameKind, payload: &[u8], out: &mut [u8]) -> Result<usize, FrameError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(FrameError::BadPayload);
    }
    let total = 1 + 1 + 1 + payload.len() + 2;
    if out.len() < total {
        return Err(FrameError::BadPayload);
    }
    out[0] = MAGIC;
    out[1] = VERSION;
    out[2] = kind as u8;
    out[3..3 + payload.len()].copy_from_slice(payload);
    let crc = crc16(&out[1..3 + payload.len()]);
    let crc_off = 3 + payload.len();
    out[crc_off..crc_off + 2].copy_from_slice(&crc.to_le_bytes());
    Ok(total)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    pub kind: FrameKind,
    pub payload: &'a [u8],
}

pub fn decode(buf: &[u8]) -> Result<Frame<'_>, FrameError> {
    if buf.len() < MIN_FRAME_LEN {
        return Err(FrameError::TooShort);
    }
    if buf[0] != MAGIC {
        return Err(FrameError::BadMagic);
    }
    if buf[1] != VERSION {
        return Err(FrameError::BadVersion(buf[1]));
    }
    let kind = FrameKind::from_u8(buf[2])?;
    let payload_end = buf.len() - 2;
    let payload = &buf[3..payload_end];
    let expected_crc = u16::from_le_bytes([buf[payload_end], buf[payload_end + 1]]);
    let actual_crc = crc16(&buf[1..payload_end]);
    if expected_crc != actual_crc {
        return Err(FrameError::BadCrc);
    }
    Ok(Frame { kind, payload })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_payload() {
        let payload = [1u8, 0u8]; // StateChange{Tripped, Live}
        let mut out = [0u8; MAX_FRAME_LEN];
        let n = encode(FrameKind::StateChange, &payload, &mut out).unwrap();
        let f = decode(&out[..n]).unwrap();
        assert_eq!(f.kind, FrameKind::StateChange);
        assert_eq!(f.payload, &payload);
    }

    #[test]
    fn decode_rejects_bad_crc() {
        let mut out = [0u8; MAX_FRAME_LEN];
        let n = encode(FrameKind::Heartbeat, &[], &mut out).unwrap();
        out[n - 1] ^= 0xFF;
        assert_eq!(decode(&out[..n]), Err(FrameError::BadCrc));
    }

    /// Frozen vector: prints the bytes of a Heartbeat frame so the Python
    /// implementation can pin against the same wire format. Run with
    /// `cargo test -- --nocapture vector_dump_heartbeat` to capture.
    #[test]
    fn vector_dump_heartbeat() {
        let mut out = [0u8; MAX_FRAME_LEN];
        let n = encode(FrameKind::Heartbeat, &[], &mut out).unwrap();
        let hex: String = out[..n].iter().map(|b| format!("{b:02x}")).collect();
        println!("heartbeat_empty_hex={}", hex);
    }
}
