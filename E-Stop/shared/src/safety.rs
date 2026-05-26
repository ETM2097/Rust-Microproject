//! Safety state machine. Pure logic, host-testable.

use crate::{LinkState, SafetyState};

pub const HEARTBEAT_TIMEOUT_MS: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    HeartbeatRx,
    StopRx,
    Tick { now_ms: u64 },
}

pub struct Safety {
    safety: SafetyState,
    link: LinkState,
    last_rx_ms: u64,
}

impl Safety {
    pub fn new() -> Self {
        Self {
            safety: SafetyState::Armed,
            link: LinkState::Lost,
            last_rx_ms: 0,
        }
    }

    pub fn safety(&self) -> SafetyState { self.safety }
    pub fn link(&self) -> LinkState { self.link }

    /// Returns `true` if either state changed.
    pub fn step(&mut self, ev: Event) -> bool {
        let prev = (self.safety, self.link);
        match ev {
            Event::HeartbeatRx => {
                // Caller is expected to call `mark_rx(now_ms)` before `step`.
                self.link = LinkState::Live;
            }
            Event::StopRx => {
                self.link = LinkState::Live;
                self.safety = SafetyState::Tripped;
            }
            Event::Tick { now_ms } => {
                if now_ms.saturating_sub(self.last_rx_ms) > HEARTBEAT_TIMEOUT_MS {
                    self.link = LinkState::Lost;
                    self.safety = SafetyState::Tripped;
                }
            }
        }
        (self.safety, self.link) != prev
    }

    /// Record that `now_ms` was when the last RX happened.
    pub fn mark_rx(&mut self, now_ms: u64) {
        self.last_rx_ms = now_ms;
    }
}

impl Default for Safety {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boots_armed_link_lost() {
        let s = Safety::new();
        assert_eq!(s.safety(), SafetyState::Armed);
        assert_eq!(s.link(), LinkState::Lost);
    }

    #[test]
    fn heartbeat_moves_link_to_live() {
        let mut s = Safety::new();
        s.mark_rx(100);
        assert!(s.step(Event::HeartbeatRx));
        assert_eq!(s.link(), LinkState::Live);
    }

    #[test]
    fn stop_trips_safety() {
        let mut s = Safety::new();
        s.mark_rx(100);
        let _ = s.step(Event::HeartbeatRx);
        assert!(s.step(Event::StopRx));
        assert_eq!(s.safety(), SafetyState::Tripped);
    }

    #[test]
    fn tick_after_timeout_trips_and_loses_link() {
        let mut s = Safety::new();
        s.mark_rx(100);
        let _ = s.step(Event::HeartbeatRx);
        assert!(s.step(Event::Tick { now_ms: 100 + HEARTBEAT_TIMEOUT_MS + 1 }));
        assert_eq!(s.link(), LinkState::Lost);
        assert_eq!(s.safety(), SafetyState::Tripped);
    }

    #[test]
    fn step_returns_false_when_no_change() {
        let mut s = Safety::new();
        s.mark_rx(0);
        let _ = s.step(Event::Tick { now_ms: 10_000 });
        let changed = s.step(Event::Tick { now_ms: 20_000 });
        assert!(!changed);
    }
}
