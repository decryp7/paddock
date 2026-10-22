//! Time and randomness as injected values: the engine never reads a clock or a random source directly, so the
//! scheduler, backoff and digests are tested with a manual clock.
use spotter_core::time::Timestamp;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering};

/// The time source.
pub trait Clock: Send + Sync {
    /// Now.
    fn now(&self) -> Timestamp;
    /// The local time zone's offset from UTC in minutes (for quiet hours and digests).
    fn utc_offset_minutes(&self) -> i32;
}

/// The real clock. The shell sets the local offset (reading it is not thread-safe in some libc builds).
#[derive(Debug, Default)]
pub struct SystemClock {
    offset_minutes: AtomicI32,
}

impl SystemClock {
    /// A clock in UTC.
    pub fn new() -> SystemClock {
        SystemClock::default()
    }

    /// Sets the local offset from UTC in minutes.
    pub fn set_utc_offset_minutes(&self, minutes: i32) {
        self.offset_minutes.store(minutes, Ordering::Relaxed);
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        Timestamp(ms)
    }

    fn utc_offset_minutes(&self) -> i32 {
        self.offset_minutes.load(Ordering::Relaxed)
    }
}

/// A clock the test moves by hand.
#[derive(Debug)]
pub struct ManualClock {
    now_ms: AtomicI64,
    offset_minutes: AtomicI32,
}

impl ManualClock {
    /// A clock at `now` in UTC.
    pub fn new(now: Timestamp) -> ManualClock {
        ManualClock {
            now_ms: AtomicI64::new(now.0),
            offset_minutes: AtomicI32::new(0),
        }
    }

    /// Moves the clock forward.
    pub fn advance_secs(&self, secs: u64) {
        self.now_ms
            .fetch_add(i64::try_from(secs).unwrap_or(0).saturating_mul(1000), Ordering::Relaxed);
    }

    /// Sets the time.
    pub fn set(&self, now: Timestamp) {
        self.now_ms.store(now.0, Ordering::Relaxed);
    }

    /// Sets the local offset.
    pub fn set_utc_offset_minutes(&self, minutes: i32) {
        self.offset_minutes.store(minutes, Ordering::Relaxed);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.now_ms.load(Ordering::Relaxed))
    }

    fn utc_offset_minutes(&self) -> i32 {
        self.offset_minutes.load(Ordering::Relaxed)
    }
}

/// The local calendar position of `now` for the digest and quiet-hours policies.
pub fn local_parts(now: Timestamp, offset_minutes: i32) -> (spotter_core::notify::DigestClock, spotter_core::time::ClockTime) {
    let local_ms = now.0.saturating_add(i64::from(offset_minutes) * 60_000);
    let day = local_ms.div_euclid(86_400_000);
    let minutes = (local_ms.rem_euclid(86_400_000) / 60_000) as u16;
    // 1970-01-01 was a Thursday (weekday 3 with Monday = 0).
    let weekday = ((day + 3).rem_euclid(7)) as u8;
    let clock = spotter_core::time::ClockTime::from_minutes(minutes).unwrap_or_default();
    (spotter_core::notify::DigestClock { day, weekday, minutes }, clock)
}

/// A source of random numbers.
pub trait Randomness: Send + Sync {
    /// A random 64-bit number.
    fn next_u64(&self) -> u64;

    /// A random number in `[0, 1)`.
    fn unit(&self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Randomness from the standard library's randomly keyed hasher (enough for ids and jitter; not for secrets).
#[derive(Debug, Default)]
pub struct SystemRandom {
    counter: AtomicU64,
}

impl Randomness for SystemRandom {
    fn next_u64(&self) -> u64 {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(self.counter.fetch_add(1, Ordering::Relaxed));
        h.finish()
    }
}

/// Randomness that counts up, for tests.
#[derive(Debug, Default)]
pub struct SequenceRandom {
    next: AtomicU64,
}

impl Randomness for SequenceRandom {
    fn next_u64(&self) -> u64 {
        self.next.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
    }

    fn unit(&self) -> f64 {
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_clock_moves_when_told() {
        let c = ManualClock::new(Timestamp(1000));
        c.advance_secs(2);
        assert_eq!(c.now(), Timestamp(3000));
        c.set(Timestamp(5));
        assert_eq!(c.now(), Timestamp(5));
        c.set_utc_offset_minutes(-90);
        assert_eq!(c.utc_offset_minutes(), -90);
    }

    #[test]
    fn the_system_clock_is_recent_and_keeps_its_offset() {
        let c = SystemClock::new();
        assert!(c.now().0 > 1_700_000_000_000);
        c.set_utc_offset_minutes(120);
        assert_eq!(c.utc_offset_minutes(), 120);
    }

    #[test]
    fn local_parts_follow_the_offset() {
        // 2026-10-22 (Thursday) 23:30 UTC.
        let t = Timestamp::parse("2026-10-22T23:30:00Z").unwrap();
        let (utc, clock) = local_parts(t, 0);
        assert_eq!((utc.weekday, utc.minutes, clock.minutes()), (3, 23 * 60 + 30, 23 * 60 + 30));
        let (next, clock) = local_parts(t, 60);
        assert_eq!((next.day, next.weekday, next.minutes, clock.minutes()), (utc.day + 1, 4, 30, 30));
        let (prev, _) = local_parts(Timestamp(0), -60);
        assert_eq!((prev.day, prev.weekday), (-1, 2));
    }

    #[test]
    fn randomness_is_in_range_and_varies() {
        let r = SystemRandom::default();
        let a = r.next_u64();
        assert_ne!(a, r.next_u64());
        for _ in 0..50 {
            let u = r.unit();
            assert!((0.0..1.0).contains(&u));
        }
        let s = SequenceRandom::default();
        assert_ne!(s.next_u64(), s.next_u64());
        assert_eq!(s.unit(), 0.5);
    }
}
