//! Hybrid logical clocks, so merges agree across devices whose wall clocks differ.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// `2026-11-02T10:00:00.000Z` to milliseconds.
pub fn parse_date(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from civil, after Howard Hinnant.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut millis = 0;
    if bytes.get(19) == Some(&b'.') {
        let fraction: String = value[20..].chars().take_while(char::is_ascii_digit).take(3).collect();
        let scale = 10_i64.pow(3 - u32::try_from(fraction.len()).ok()?);
        millis = fraction.parse::<i64>().ok()? * scale;
    }
    u64::try_from((days * 86_400 + hour * 3600 + minute * 60 + second) * 1000 + millis).ok()
}

/// Milliseconds to the API's date format.
pub fn format_date(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        ms % 1000
    )
}

/// One moment. Ordered by time, then counter, then device, so any two values have
/// a winner and every device picks the same one.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch, as far as the writer knew.
    pub ms: u64,
    /// Tiebreak within one millisecond.
    pub counter: u32,
    /// Writer's device number.
    pub device: u32,
}

impl Hlc {
    /// Whether this is the zero moment, which stands for "never written".
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    /// Whether `ms` is the writer's own wall clock. A tick that moved on from the wall clock
    /// starts its counter at zero; one that had to sort after a moment seen elsewhere, or after
    /// another tick in the same millisecond, counts up instead. So a zero counter is a reading
    /// of the writer's clock, and a non-zero one may be another device's time carried along.
    pub fn is_wall_time(&self) -> bool {
        self.counter == 0 && self.ms > 0
    }
}

/// How far ahead of this device's wall clock a moment seen elsewhere may pull this clock: a day.
///
/// The order of two moments never depends on who compares them: [`Hlc`]'s order is fixed, so
/// every device decides every merge the same way whatever its own clock says, and they converge.
/// What a device's own clock does with a moment from elsewhere is a separate, local choice. A
/// plain hybrid logical clock jumps to any later moment it sees, so one device whose clock runs
/// a year fast would drag every device it syncs with a year ahead, and their writes would carry
/// that wrong time from then on. Here a moment up to a day ahead is taken as usual (ordinary
/// clock drift, and a device a few hours off, sort as they always did), and one further ahead
/// waits until this device's wall clock is within a day of it.
///
/// An edit must still beat the value it replaces, or the fast device's copy would win it back on
/// the next sync. [`Clock::tick_over`] makes the one moment an edit needs sort after that value,
/// without moving the clock: only what was written after the far-ahead write is lifted past it.
pub const MAX_AHEAD_MS: u64 = 24 * 60 * 60 * 1000;

/// A device's clock. It never goes backwards, even if the wall clock does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clock {
    last: Hlc,
    /// Moments seen elsewhere, later than `last`, not yet taken: whether one is too far ahead
    /// depends on the wall clock, which only a tick knows.
    seen: BTreeSet<Hlc>,
}

impl Clock {
    /// A clock for a device.
    pub fn new(device: u32) -> Self {
        Self {
            last: Hlc {
                ms: 0,
                counter: 0,
                device,
            },
            seen: BTreeSet::new(),
        }
    }

    /// The next moment on this device.
    pub fn tick(&mut self, now_ms: u64) -> Hlc {
        self.take_seen(now_ms);
        if now_ms > self.last.ms {
            self.last.ms = now_ms;
            self.last.counter = 0;
        } else {
            self.last.counter = self.last.counter.saturating_add(1);
        }
        self.last
    }

    /// The next moment on this device, made to sort after `prior`, the value about to be
    /// replaced. Usually the same as [`Clock::tick`]. When `prior` is further ahead than the
    /// clock may be pulled, the moment returned is just after it, and the clock itself stays.
    pub fn tick_over(&mut self, now_ms: u64, prior: Hlc) -> Hlc {
        let at = self.tick(now_ms);
        if at > prior {
            return at;
        }
        Hlc {
            ms: prior.ms,
            counter: prior.counter.saturating_add(1),
            device: self.last.device,
        }
    }

    /// Takes note of a moment seen elsewhere, so later ticks sort after it, unless it is more
    /// than [`MAX_AHEAD_MS`] ahead of the wall clock when they come.
    pub fn observe(&mut self, seen: Hlc) {
        if (seen.ms, seen.counter) > (self.last.ms, self.last.counter) {
            self.seen.insert(seen);
        }
    }

    fn take_seen(&mut self, now_ms: u64) {
        if self.seen.is_empty() {
            return;
        }
        let bound = now_ms.saturating_add(MAX_AHEAD_MS);
        if let Some(seen) = self.seen.iter().rev().find(|seen| seen.ms <= bound).copied()
            && (seen.ms, seen.counter) > (self.last.ms, self.last.counter)
        {
            self.last.ms = seen.ms;
            self.last.counter = seen.counter;
        }
        let last = (self.last.ms, self.last.counter);
        self.seen.retain(|seen| (seen.ms, seen.counter) > last);
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
        let peer = Hlc {
            ms: 9000,
            counter: 4,
            device: 2,
        };
        clock.observe(peer);
        assert!(clock.tick(1000) > peer);
    }

    const DAY: u64 = MAX_AHEAD_MS;

    #[test]
    fn a_peer_more_than_a_day_ahead_does_not_drag_the_clock() {
        let mut clock = Clock::new(1);
        let now = 10 * DAY;
        let peer = Hlc { ms: now + 2 * DAY, counter: 0, device: 2 };
        clock.observe(peer);
        let at = clock.tick(now);
        assert_eq!(at.ms, now, "the clock keeps to its own time");
        // A peer a few hours ahead is taken as before.
        let near = Hlc { ms: now + DAY / 4, counter: 3, device: 2 };
        clock.observe(near);
        assert!(clock.tick(now + 1) > near);
        // Once the wall clock is within a day of the far one, it is taken too.
        assert!(clock.tick(now + DAY + 1) > peer);
    }

    #[test]
    fn an_edit_sorts_after_the_value_it_replaces_without_moving_the_clock() {
        let mut clock = Clock::new(1);
        let far = Hlc { ms: 50 * DAY, counter: 7, device: 2 };
        clock.observe(far);
        let edit = clock.tick_over(DAY, far);
        assert!(edit > far);
        assert!(!edit.is_wall_time(), "a lifted moment is not this device's time");
        let next = clock.tick(DAY + 5);
        assert_eq!(next.ms, DAY + 5);
        assert!(next.is_wall_time());
    }

    #[test]
    fn device_breaks_exact_ties() {
        let a = Hlc {
            ms: 1,
            counter: 0,
            device: 1,
        };
        let b = Hlc {
            ms: 1,
            counter: 0,
            device: 2,
        };
        assert!(a < b);
    }
}
