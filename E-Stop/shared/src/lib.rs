#![cfg_attr(not(test), no_std)]

pub mod cobs;
pub mod frame;
pub mod safety;
pub mod state;

pub use frame::{Frame, FrameError, FrameKind, decode, encode, MAGIC, MAX_FRAME_LEN, MAX_PAYLOAD, VERSION};
pub use safety::{Safety, Event as SafetyEvent, HEARTBEAT_TIMEOUT_MS};
pub use state::{LinkState, SafetyState};
