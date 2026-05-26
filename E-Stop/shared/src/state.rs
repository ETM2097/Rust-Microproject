#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SafetyState {
    Armed = 0,
    Tripped = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LinkState {
    Live = 0,
    Lost = 1,
}

impl SafetyState {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl LinkState {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}
