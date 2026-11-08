//! One phase of one pattern: threads issuing `pread`/`pwrite` against a block device port until a byte limit or a time limit
//! ends it, whichever comes first. Every byte limit is enforced here, in the loop, never derived from a speed; every write is
//! first reserved from the run's [`WriteBudget`], so the number shown to the person before the run is an upper bound.
//! The port and the clock are injected, so the tests move a fake clock and a fake drive instead of waiting (rule 60).

use dyno_core::bench::disk::Pattern;
use paddock_bench::Clock;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Where the bytes go: a file opened for direct I/O in the real runner, a scripted drive in the tests.
pub trait BlockIo: Sync {
    /// Reads `buf.len()` bytes at `offset`.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;
    /// Writes all of `buf` at `offset`.
    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()>;
}

/// The bytes a run may still write to the drive. Reserving before writing means the total can never pass the number shown.
#[derive(Debug)]
pub struct WriteBudget {
    total: u64,
    used: AtomicU64,
}

impl WriteBudget {
    /// A budget of `total` bytes.
    pub fn new(total: u64) -> WriteBudget {
        WriteBudget {
            total,
            used: AtomicU64::new(0),
        }
    }

    /// Takes `n` bytes; false (and nothing taken) when they are not left.
    pub fn reserve(&self, n: u64) -> bool {
        let mut cur = self.used.load(Ordering::SeqCst);
        loop {
            let next = cur.saturating_add(n);
            if next > self.total {
                return false;
            }
            match self.used.compare_exchange(cur, next, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return true,
                Err(seen) => cur = seen,
            }
        }
    }

    /// Bytes reserved so far.
    pub fn used(&self) -> u64 {
        self.used.load(Ordering::SeqCst)
    }
}

/// What a phase does.
#[derive(Debug, Clone, Copy)]
pub struct PhaseSpec {
    /// The access pattern.
    pub pattern: Pattern,
    /// Write (otherwise read).
    pub write: bool,
    /// The size of the file the offsets fall in.
    pub file_bytes: u64,
    /// The phase moves at most this many bytes.
    pub max_bytes: u64,
    /// The phase takes at most this long.
    pub max_time: Duration,
    /// Seed for the random offsets and the data (fixed: results do not depend on it, but runs are repeatable).
    pub seed: u64,
}

/// Why a phase ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseEnd {
    /// It moved its byte limit.
    Bytes,
    /// Its time ran out.
    Time,
    /// The write budget did not allow another write.
    Budget,
}

/// What a phase moved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseResult {
    /// Bytes moved.
    pub bytes: u64,
    /// Operations done.
    pub ops: u64,
    /// How long it ran, by the injected clock.
    pub elapsed: Duration,
    /// Why it ended.
    pub end: PhaseEnd,
}

impl PhaseResult {
    /// Megabytes (10^6 bytes) per second.
    pub fn mb_per_s(&self) -> f64 {
        self.bytes as f64 / 1e6 / self.elapsed.as_secs_f64().max(1e-9)
    }

    /// Operations per second.
    pub fn iops(&self) -> f64 {
        self.ops as f64 / self.elapsed.as_secs_f64().max(1e-9)
    }
}

/// A buffer whose start is aligned to `align` bytes (direct I/O needs it), made in safe code by over-allocating.
pub struct AlignedBuf {
    raw: Vec<u8>,
    start: usize,
    len: usize,
}

impl AlignedBuf {
    /// A zeroed buffer of `len` bytes aligned to `align` (a power of two).
    pub fn new(len: usize, align: usize) -> AlignedBuf {
        let raw = vec![0_u8; len + align];
        let start = raw.as_ptr().align_offset(align).min(align);
        AlignedBuf { raw, start, len }
    }

    /// The aligned bytes.
    pub fn bytes(&mut self) -> &mut [u8] {
        &mut self.raw[self.start..self.start + self.len]
    }
}

/// xorshift64*: fast, fixed-seed, good enough to make data the drive cannot compress and offsets that do not repeat in order.
pub fn next_random(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Fills `buf` with random bytes.
pub fn fill_random(buf: &mut [u8], state: &mut u64) {
    for chunk in buf.chunks_mut(8) {
        let r = next_random(state).to_le_bytes();
        chunk.copy_from_slice(&r[..chunk.len()]);
    }
}

/// The alignment of buffers and offsets: 4 KiB covers the logical sector of every drive we meet.
pub const ALIGN: usize = 4096;

/// Runs one phase. The first I/O error stops every thread and is returned.
pub fn run_phase(io: &dyn BlockIo, clock: &(dyn Clock + Sync), spec: &PhaseSpec, budget: &WriteBudget) -> io::Result<PhaseResult> {
    let block = spec.pattern.block as u64;
    let slots = (spec.file_bytes / block).max(1);
    let threads = spec.pattern.threads.max(1);
    let moved = AtomicU64::new(0);
    let ops = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    let out_of_budget = AtomicBool::new(false);
    let failure: Mutex<Option<io::Error>> = Mutex::new(None);
    let start = clock.now();
    std::thread::scope(|s| {
        for t in 0..threads {
            let (moved, ops, stop, out_of_budget, failure) = (&moved, &ops, &stop, &out_of_budget, &failure);
            s.spawn(move || {
                let mut buf = AlignedBuf::new(spec.pattern.block, ALIGN);
                let mut rng = spec.seed ^ ((t as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
                if rng == 0 {
                    rng = 1;
                }
                if spec.write {
                    fill_random(buf.bytes(), &mut rng);
                }
                // Each thread has its own stretch of the file for sequential work, so threads do not collide.
                let stretch = (slots / threads as u64).max(1);
                let mut cursor = (t as u64 * stretch) % slots;
                let first = cursor;
                while !stop.load(Ordering::Relaxed) && clock.now().saturating_sub(start) < spec.max_time {
                    if moved.fetch_add(block, Ordering::SeqCst) + block > spec.max_bytes {
                        moved.fetch_sub(block, Ordering::SeqCst);
                        break;
                    }
                    if spec.write && !budget.reserve(block) {
                        moved.fetch_sub(block, Ordering::SeqCst);
                        out_of_budget.store(true, Ordering::SeqCst);
                        break;
                    }
                    let slot = if spec.pattern.random {
                        next_random(&mut rng) % slots
                    } else {
                        let c = cursor;
                        cursor = if cursor + 1 >= (first + stretch).min(slots) {
                            first
                        } else {
                            cursor + 1
                        };
                        c
                    };
                    let offset = slot * block;
                    let res = if spec.write {
                        // The first bytes change on every write, so no two blocks the drive sees are equal.
                        let b = buf.bytes();
                        b[..8].copy_from_slice(&next_random(&mut rng).to_le_bytes());
                        io.write_at(b, offset)
                    } else {
                        io.read_at(buf.bytes(), offset)
                    };
                    match res {
                        Ok(()) => {
                            ops.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            moved.fetch_sub(block, Ordering::SeqCst);
                            stop.store(true, Ordering::SeqCst);
                            failure.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert(e);
                            break;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap_or_else(|p| p.into_inner()) {
        return Err(e);
    }
    let bytes = moved.load(Ordering::SeqCst);
    let end = if out_of_budget.load(Ordering::SeqCst) {
        PhaseEnd::Budget
    } else if bytes + block > spec.max_bytes {
        PhaseEnd::Bytes
    } else {
        PhaseEnd::Time
    };
    Ok(PhaseResult {
        bytes,
        ops: ops.load(Ordering::SeqCst),
        elapsed: clock.now().saturating_sub(start),
        end,
    })
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// A clock that only moves when the fake drive says an operation took time.
    #[derive(Default)]
    pub struct AtomicClock(pub AtomicU64);

    impl Clock for AtomicClock {
        fn now(&self) -> Duration {
            Duration::from_nanos(self.0.load(Ordering::SeqCst))
        }
    }

    /// A drive that takes `cost` per operation on the fake clock, records what it was asked and can be made to fail.
    pub struct FakeDrive<'a> {
        pub clock: &'a AtomicClock,
        pub cost: Duration,
        pub written: AtomicU64,
        pub read: AtomicU64,
        pub offsets: Mutex<Vec<u64>>,
        pub fail_after: Option<u64>,
        pub aligned: AtomicBool,
    }

    impl<'a> FakeDrive<'a> {
        pub fn new(clock: &'a AtomicClock, cost: Duration) -> FakeDrive<'a> {
            FakeDrive {
                clock,
                cost,
                written: AtomicU64::new(0),
                read: AtomicU64::new(0),
                offsets: Mutex::new(Vec::new()),
                fail_after: None,
                aligned: AtomicBool::new(true),
            }
        }

        fn touch(&self, buf_ptr: usize, len: usize, offset: u64) -> io::Result<()> {
            if !buf_ptr.is_multiple_of(ALIGN) || !offset.is_multiple_of(ALIGN as u64) || !len.is_multiple_of(ALIGN) {
                self.aligned.store(false, Ordering::SeqCst);
            }
            self.offsets.lock().unwrap().push(offset);
            self.clock.0.fetch_add(self.cost.as_nanos() as u64, Ordering::SeqCst);
            let n = self.written.load(Ordering::SeqCst) + self.read.load(Ordering::SeqCst);
            if self.fail_after.is_some_and(|f| n >= f) {
                return Err(io::Error::other("scripted failure"));
            }
            Ok(())
        }
    }

    impl BlockIo for FakeDrive<'_> {
        fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
            self.touch(buf.as_ptr() as usize, buf.len(), offset)?;
            self.read.fetch_add(buf.len() as u64, Ordering::SeqCst);
            Ok(())
        }

        fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
            self.touch(buf.as_ptr() as usize, buf.len(), offset)?;
            self.written.fetch_add(buf.len() as u64, Ordering::SeqCst);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;
    use dyno_core::bench::disk::{MIB, PATTERNS};

    fn spec(p: usize, write: bool, file: u64, max_bytes: u64, max_time: Duration) -> PhaseSpec {
        PhaseSpec {
            pattern: PATTERNS[p],
            write,
            file_bytes: file,
            max_bytes,
            max_time,
            seed: 7,
        }
    }

    #[test]
    fn a_phase_stops_at_its_byte_limit_and_counts_exactly_those_bytes() {
        let clock = AtomicClock::default();
        let drive = FakeDrive::new(&clock, Duration::from_millis(1));
        let budget = WriteBudget::new(u64::MAX);
        let r = run_phase(&drive, &clock, &spec(0, true, 64 * MIB, 8 * MIB, Duration::from_secs(60)), &budget).unwrap();
        assert_eq!((r.bytes, r.ops, r.end), (8 * MIB, 8, PhaseEnd::Bytes));
        assert_eq!(r.elapsed, Duration::from_millis(8));
        assert_eq!(budget.used(), 8 * MIB, "every written byte was reserved");
    }

    #[test]
    fn a_phase_stops_at_its_time_limit_by_the_injected_clock() {
        let clock = AtomicClock::default();
        let drive = FakeDrive::new(&clock, Duration::from_millis(10));
        let r = run_phase(
            &drive,
            &clock,
            &spec(2, false, 64 * MIB, 64 * MIB, Duration::from_millis(100)),
            &WriteBudget::new(0),
        )
        .unwrap();
        assert_eq!(r.end, PhaseEnd::Time);
        assert_eq!(r.ops, 10);
        assert!((r.iops() - 100.0).abs() < 1e-6, "10 ops in 100 ms");
        assert!((r.mb_per_s() - 10.0 * 4096.0 / 1e6 / 0.1).abs() < 1e-6);
    }

    #[test]
    fn a_ramp_that_would_take_the_whole_time_is_cut_by_its_byte_limit_first() {
        // A fast drive: 1 GiB file, the ramp is 1/8 of it and ends on bytes long before one second.
        let clock = AtomicClock::default();
        let drive = FakeDrive::new(&clock, Duration::from_micros(100));
        let r = run_phase(
            &drive,
            &clock,
            &spec(0, true, 1024 * MIB, 128 * MIB, Duration::from_secs(1)),
            &WriteBudget::new(u64::MAX),
        )
        .unwrap();
        assert_eq!(r.end, PhaseEnd::Bytes);
        assert_eq!(r.bytes, 128 * MIB);
    }

    #[test]
    fn writes_never_pass_the_budget_and_the_phase_says_why_it_ended() {
        let clock = AtomicClock::default();
        let drive = FakeDrive::new(&clock, Duration::from_micros(1));
        let budget = WriteBudget::new(3 * MIB);
        let r = run_phase(&drive, &clock, &spec(1, true, 64 * MIB, 64 * MIB, Duration::from_secs(60)), &budget).unwrap();
        assert_eq!(r.end, PhaseEnd::Budget);
        assert!(drive.written.load(Ordering::SeqCst) <= 3 * MIB);
        assert_eq!(
            r.bytes,
            drive.written.load(Ordering::SeqCst),
            "no byte is counted that was not written"
        );
        assert_eq!(budget.used(), 3 * MIB);
    }

    #[test]
    fn reads_do_not_use_the_write_budget() {
        let clock = AtomicClock::default();
        let drive = FakeDrive::new(&clock, Duration::from_micros(1));
        let budget = WriteBudget::new(0);
        let r = run_phase(
            &drive,
            &clock,
            &spec(0, false, 16 * MIB, 16 * MIB, Duration::from_secs(60)),
            &budget,
        )
        .unwrap();
        assert_eq!(r.bytes, 16 * MIB);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn buffers_and_offsets_are_aligned_and_inside_the_file() {
        let clock = AtomicClock::default();
        for (p, pattern) in PATTERNS.iter().enumerate() {
            let drive = FakeDrive::new(&clock, Duration::from_micros(1));
            run_phase(
                &drive,
                &clock,
                &spec(p, p % 2 == 0, 32 * MIB, 4 * MIB, Duration::from_secs(60)),
                &WriteBudget::new(u64::MAX),
            )
            .unwrap();
            assert!(drive.aligned.load(Ordering::SeqCst), "pattern {p}");
            let offsets = drive.offsets.lock().unwrap();
            assert!(offsets.iter().all(|o| o + pattern.block as u64 <= 32 * MIB), "pattern {p}");
        }
    }

    #[test]
    fn many_threads_never_move_more_than_the_limit_and_use_their_own_stretches() {
        let clock = AtomicClock::default();
        let drive = FakeDrive::new(&clock, Duration::from_micros(10));
        let budget = WriteBudget::new(u64::MAX);
        let r = run_phase(&drive, &clock, &spec(3, true, 64 * MIB, 2 * MIB, Duration::from_secs(60)), &budget).unwrap();
        assert_eq!(r.bytes, 2 * MIB, "32 threads share one byte limit");
        assert_eq!(drive.written.load(Ordering::SeqCst), 2 * MIB);
        let seq = {
            let d = FakeDrive::new(&clock, Duration::from_micros(10));
            run_phase(&d, &clock, &spec(1, false, 64 * MIB, 8 * MIB, Duration::from_secs(60)), &budget).unwrap();
            let mut o = d.offsets.lock().unwrap().clone();
            o.sort_unstable();
            o
        };
        // Eight threads, eight different starting points.
        assert_eq!(seq.len(), 8);
        let mut uniq = seq.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), 8);
    }

    #[test]
    fn an_io_error_ends_the_phase_and_is_returned() {
        let clock = AtomicClock::default();
        let mut drive = FakeDrive::new(&clock, Duration::from_micros(1));
        drive.fail_after = Some(3);
        let err = run_phase(
            &drive,
            &clock,
            &spec(0, true, 64 * MIB, 64 * MIB, Duration::from_secs(60)),
            &WriteBudget::new(u64::MAX),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "scripted failure");
    }

    #[test]
    fn the_budget_is_exact_under_contention() {
        let b = WriteBudget::new(1000);
        let granted: u64 = std::thread::scope(|s| {
            let hs: Vec<_> = (0..8)
                .map(|_| s.spawn(|| (0..500).filter(|_| b.reserve(3)).count() as u64))
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).sum()
        });
        assert_eq!(granted, 333);
        assert_eq!(b.used(), 999);
    }

    #[test]
    fn random_data_is_not_compressible_in_the_obvious_way() {
        let mut state = 1;
        let mut a = vec![0_u8; 4096];
        fill_random(&mut a, &mut state);
        let distinct = a.iter().collect::<std::collections::HashSet<_>>().len();
        assert!(distinct > 200, "{distinct}");
        let mut b = vec![0_u8; 4096];
        fill_random(&mut b, &mut state);
        assert_ne!(a, b, "the generator moves");
    }

    #[test]
    fn aligned_buffers_are_aligned() {
        let mut b = AlignedBuf::new(8192, ALIGN);
        assert_eq!(b.bytes().as_ptr() as usize % ALIGN, 0);
        assert_eq!(b.bytes().len(), 8192);
    }
}
