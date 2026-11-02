//! Hybrid logical clocks, so merges agree across devices whose wall clocks differ.

use serde::{Deserialize, Serialize};

/// One moment. Ordered by time, then counter, then device, so any two values have
/// a winner and every device picks the same one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch, as far as the writer knew.
    pub ms: u64,
    /// Tiebreak within one millisecond.
    pub counter: u32,
    /// Writer's device number.
    pub device: u32,
}

/// A device's clock. It never goes backwards, even if the wall clock does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clock {
    last: Hlc,
}

impl Clock {
    /// A clock for a device.
    pub fn new(device: u32) -> Self {
        Self { last: Hlc { ms: 0, counter: 0, device } }
    }

    /// The next moment on this device.
    pub fn tick(&mut self, now_ms: u64) -> Hlc {
        if now_ms > self.last.ms {
            self.last.ms = now_ms;
            self.last.counter = 0;
        } else {
            self.last.counter = self.last.counter.saturating_add(1);
        }
        self.last
    }

    /// Takes note of a moment seen elsewhere, so later ticks sort after it.
    pub fn observe(&mut self, seen: Hlc) {
        if seen.ms > self.last.ms || (seen.ms == self.last.ms && seen.counter > self.last.counter) {
            self.last.ms = seen.ms;
            self.last.counter = seen.counter;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_rise_even_when_the_wall_clock_falls() {
        let mut clock = Clock::new(1);
        let a = clock.tick(1000);
        let b = clock.tick(1000);
        let c = clock.tick(500);
        assert!(a < b && b < c);
    }

    #[test]
    fn observing_moves_past_a_fast_peer() {
        let mut clock = Clock::new(1);
        clock.tick(1000);
        let peer = Hlc { ms: 9000, counter: 4, device: 2 };
        clock.observe(peer);
        assert!(clock.tick(1000) > peer);
    }

    #[test]
    fn device_breaks_exact_ties() {
        let a = Hlc { ms: 1, counter: 0, device: 1 };
        let b = Hlc { ms: 1, counter: 0, device: 2 };
        assert!(a < b);
    }
}
