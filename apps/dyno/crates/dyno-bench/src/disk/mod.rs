//! Disk (ADR 0030 section 1): the four patterns of CrystalDiskMark's defaults, each read and written, on one scratch file of the
//! chosen size inside the folder the agent made, with the cache bypassed. Per pattern a ramp that is not counted (at most 1/8
//! of the file or 1 s), then a measured part (at most the file's size or 5 s); three repetitions; every written byte counted
//! against the budget the person was shown. The file cannot be left behind ([`scratch`]).
//!
//! Layers: [`driver`] runs one phase against a [`BlockIo`] with an injected clock (fully testable with a fake drive);
//! [`scratch`] is the real file; [`sys`] holds the few OS calls; this module sequences the passes and builds the metrics.

pub mod driver;
pub mod scratch;
pub mod sys;

use crate::emit::{kind_phase, Progress};
use crate::Outcome;
use driver::{run_phase, BlockIo, PhaseEnd, PhaseResult, PhaseSpec, WriteBudget};
use dyno_core::bench::disk::{check_free, disk_write_budget_with, limits, metric_id, Pattern, MIB, PATTERNS};
use dyno_core::bench::BenchMetric;
use paddock_bench::Clock;
use std::io;
use std::time::Duration;

/// The drive under test, as the sequencing sees it.
pub trait Drive: BlockIo {
    /// Flushes written data to the device.
    fn flush(&self) -> io::Result<()>;
    /// Whether the cache is bypassed (false: reads may be served from memory, and every label says so).
    fn direct(&self) -> bool;
    /// Capacity and free bytes of the volume, when the OS says.
    fn space(&self) -> Option<(u64, u64)>;
}

/// What one disk run is.
#[derive(Debug, Clone, Copy)]
pub struct DiskRun {
    /// The scratch file's size, MiB.
    pub file_mib: u32,
    /// Repetitions of every pattern.
    pub reps: u32,
    /// A spinning disk: random patterns measure for 2 s.
    pub rotational: bool,
    /// The sustained write, MiB written past the drive's cache after the patterns (zero: none; the person confirmed the amount).
    pub sustained_mib: u32,
}

/// The words for an I/O error: a full drive and a refused write are named, the rest carry the OS's text.
pub fn describe(e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(28) | Some(112) => "The drive ran out of space while the test was writing. Free some space and run again.".into(),
        Some(13) | Some(5) | Some(1) | Some(1314) => format!("The drive refused the test ({e}). Check that Dyno may write to it."),
        _ => format!("The disk test failed: {e}"),
    }
}

/// The fill goes in this many pieces so the progress moves while a slow drive is being filled.
const FILL_PIECES: u64 = 16;
/// The most the fill may take, whatever the drive's speed (a stuck drive is for the person to cancel, not for us to wait out).
const FILL_MAX: Duration = Duration::from_secs(3_600);

/// Runs the whole disk test on `drive`. Returns the metrics of every repetition, or why it could not.
pub fn run(drive: &dyn Drive, cfg: DiskRun, clock: &(dyn Clock + Sync), progress: &dyn Progress) -> Result<Vec<BenchMetric>, Outcome> {
    let file_bytes = u64::from(cfg.file_mib) * MIB;
    let reps = cfg.reps.max(1);
    let budget = WriteBudget::new(disk_write_budget_with(cfg.file_mib, reps, cfg.sustained_mib.div_ceil(1024)));
    let fail = |e: io::Error| Outcome::Failed(describe(&e));

    // The fill: one pass of the file, so the read patterns read real data and not holes.
    progress.report(kind_phase("disk", "Preparing the test file", 0, reps));
    let piece = (file_bytes / FILL_PIECES).max(MIB);
    let mut filled = 0_u64;
    let mut seed = 0xF111_u64;
    while filled < file_bytes {
        let take = piece.min(file_bytes - filled);
        let spec = PhaseSpec {
            pattern: PATTERNS[0],
            write: true,
            file_bytes: take,
            max_bytes: take,
            max_time: FILL_MAX,
            seed,
        };
        // Sequential writes of one piece at its place in the file: the driver writes offsets 0.. of `take`; shift them with a view.
        let view = Shifted {
            inner: drive,
            base: filled,
        };
        run_phase(&view, clock, &spec, &budget).map_err(fail)?;
        filled += take;
        seed += 1;
        progress.report(kind_phase(
            "disk",
            &format!("Preparing the test file · {} of {} MiB", filled / MIB, cfg.file_mib),
            0,
            reps,
        ));
    }
    drive.flush().map_err(fail)?;

    let mut samples: Vec<Samples> = Vec::new();
    let mut writes_stopped = false;
    for n in 1..=reps {
        for (pi, pattern) in PATTERNS.iter().enumerate() {
            for write in [false, true] {
                if write && writes_stopped {
                    continue;
                }
                if write {
                    if let Some((cap, free)) = drive.space() {
                        // The file exists already; what must stay free is the margin.
                        check_free("the drive", 0, Some(cap), Some(free)).map_err(Outcome::Failed)?;
                    }
                }
                let what = if write { "write" } else { "read" };
                progress.report(kind_phase(
                    "disk",
                    &format!("Pass {n} of {reps} · {} · {what}", pattern.label),
                    n - 1,
                    reps,
                ));
                let seed = 0xA000 + u64::from(n) * 100 + pi as u64 * 2 + u64::from(write);
                match measure(drive, clock, &budget, cfg, pattern, write, seed).map_err(fail)? {
                    Some(r) => record(&mut samples, pattern, write, &r),
                    None => {
                        // The budget would have been passed: stop writing, keep what was measured, say so.
                        eprintln!(
                            "dyno-bench: the write budget was reached before {} · write finished; no more writes",
                            pattern.label
                        );
                        writes_stopped = true;
                    }
                }
                if write {
                    drive.flush().map_err(fail)?;
                }
            }
        }
        progress.report(kind_phase("disk", &format!("Pass {n} of {reps} done"), n, reps));
    }
    let mut extra = Vec::new();
    if cfg.sustained_mib > 0 && !writes_stopped {
        extra = sustained_write(drive, clock, &budget, cfg, progress).map_err(fail)?;
    }
    let mut metrics: Vec<BenchMetric> = samples
        .into_iter()
        .map(|s| BenchMetric::measured(&s.id, &s.label, &s.values, s.unit, true))
        .collect();
    metrics.extend(extra);
    Ok(metrics)
}

/// The most the sustained write takes, by the injected clock.
const SUSTAINED_MAX: Duration = Duration::from_secs(dyno_core::bench::disk::SUSTAINED_WRITE_MAX_SECS);
/// The speed at the start and the end is the mean of this many seconds.
const SUSTAINED_EDGE_SECS: usize = 5;

/// Sequential 1 MiB writes, one second at a time, until `cfg.sustained_mib` has been written or the time is up: the speed after
/// the drive's fast cache has filled. Each second is a sample for the live chart. Returns the start speed, the end speed and
/// the share kept; nothing when fewer seconds than two edges were measured (a fast drive finished before it could tire).
fn sustained_write(
    drive: &dyn Drive,
    clock: &(dyn Clock + Sync),
    budget: &WriteBudget,
    cfg: DiskRun,
    progress: &dyn Progress,
) -> io::Result<Vec<BenchMetric>> {
    let total = u64::from(cfg.sustained_mib) * MIB;
    let file_bytes = u64::from(cfg.file_mib) * MIB;
    let (mut done, mut secs, mut rates) = (0_u64, 0_u32, Vec::<f64>::new());
    let begun = clock.now();
    while done < total && clock.now().saturating_sub(begun) < SUSTAINED_MAX {
        let spec = PhaseSpec {
            pattern: PATTERNS[0],
            write: true,
            file_bytes,
            max_bytes: total - done,
            max_time: Duration::from_secs(1),
            seed: 0x5057 + u64::from(secs),
        };
        let r = run_phase(drive, clock, &spec, budget)?;
        if r.bytes == 0 {
            break;
        }
        done += r.bytes;
        secs += 1;
        rates.push(r.mb_per_s());
        let mut p = kind_phase(
            "disk-sustained",
            &format!(
                "Sustained write · {} of {} MiB · {:.0} MB/s",
                done / MIB,
                cfg.sustained_mib,
                r.mb_per_s()
            ),
            0,
            1,
        );
        p.count = done / MIB;
        p.total = Some(u64::from(cfg.sustained_mib));
        progress.report(p);
        progress.sample(secs, r.mb_per_s());
        if r.end == PhaseEnd::Budget {
            break;
        }
    }
    drive.flush()?;
    if rates.len() < SUSTAINED_EDGE_SECS * 2 {
        return Ok(Vec::new());
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (start, end) = (
        mean(&rates[..SUSTAINED_EDGE_SECS]),
        mean(&rates[rates.len() - SUSTAINED_EDGE_SECS..]),
    );
    Ok(vec![
        BenchMetric::higher("disk-sustained-write-start", "Sustained write · first seconds", start, "MB/s"),
        BenchMetric::higher("disk-sustained-write-end", "Sustained write · last seconds", end, "MB/s"),
        BenchMetric::higher(
            "disk-sustained-write-kept",
            "Sustained write · share kept",
            if start > 0.0 { end / start * 100.0 } else { 0.0 },
            "%",
        ),
    ])
}

/// A ramp, then the measured part. `None` when the write budget ended the phase early (nothing trustworthy to report).
fn measure(
    drive: &dyn Drive,
    clock: &(dyn Clock + Sync),
    budget: &WriteBudget,
    cfg: DiskRun,
    pattern: &Pattern,
    write: bool,
    seed: u64,
) -> io::Result<Option<PhaseResult>> {
    let file_bytes = u64::from(cfg.file_mib) * MIB;
    let l = limits(file_bytes, pattern, cfg.rotational);
    let spec = |max_bytes, max_time, seed| PhaseSpec {
        pattern: *pattern,
        write,
        file_bytes,
        max_bytes,
        max_time,
        seed,
    };
    // The ramp is not counted (fio's ramp_time): the drive's clocks and caches settle first.
    let ramp = run_phase(drive, clock, &spec(l.ramp_bytes, l.ramp_time, seed), budget)?;
    if ramp.end == PhaseEnd::Budget {
        return Ok(None);
    }
    let r = run_phase(drive, clock, &spec(l.measure_bytes, l.measure_time, seed + 7), budget)?;
    Ok((r.end != PhaseEnd::Budget && r.ops > 0).then_some(r))
}

/// A view of the drive that adds a base to every offset.
struct Shifted<'a> {
    inner: &'a dyn Drive,
    base: u64,
}

impl BlockIo for Shifted<'_> {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        self.inner.read_at(buf, offset + self.base)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
        self.inner.write_at(buf, offset + self.base)
    }
}

/// The repetitions of one reported number.
struct Samples {
    id: String,
    label: String,
    unit: &'static str,
    values: Vec<f64>,
}

fn record(all: &mut Vec<Samples>, pattern: &Pattern, write: bool, r: &PhaseResult) {
    let dir = if write { "write" } else { "read" };
    let mut push = |id: String, label: String, unit: &'static str, v: f64| match all.iter_mut().find(|s| s.id == id) {
        Some(s) => s.values.push(v),
        None => all.push(Samples {
            id,
            label,
            unit,
            values: vec![v],
        }),
    };
    if pattern.random {
        push(
            metric_id(pattern, write, "iops"),
            format!("{} · {dir} (operations)", pattern.label),
            "IOPS",
            r.iops(),
        );
        push(
            metric_id(pattern, write, "mbps"),
            format!("{} · {dir}", pattern.label),
            "MB/s",
            r.mb_per_s(),
        );
    } else {
        push(
            metric_id(pattern, write, ""),
            format!("{} · {dir}", pattern.label),
            "MB/s",
            r.mb_per_s(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::driver::fakes::{AtomicClock, FakeDrive};
    use super::*;
    use crate::emit::NoProgress;
    use std::sync::atomic::Ordering;

    struct Fake<'a> {
        d: FakeDrive<'a>,
        direct: bool,
        space: Option<(u64, u64)>,
        flushes: std::sync::atomic::AtomicU32,
    }

    impl BlockIo for Fake<'_> {
        fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
            self.d.read_at(buf, offset)
        }

        fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<()> {
            self.d.write_at(buf, offset)
        }
    }

    impl Drive for Fake<'_> {
        fn flush(&self) -> io::Result<()> {
            self.flushes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn direct(&self) -> bool {
            self.direct
        }

        fn space(&self) -> Option<(u64, u64)> {
            self.space
        }
    }

    fn fake(clock: &AtomicClock, cost: Duration) -> Fake<'_> {
        Fake {
            d: FakeDrive::new(clock, cost),
            direct: true,
            space: None,
            flushes: Default::default(),
        }
    }

    fn cfg(mib: u32, reps: u32) -> DiskRun {
        DiskRun {
            file_mib: mib,
            reps,
            rotational: false,
            sustained_mib: 0,
        }
    }

    #[test]
    fn a_full_run_gives_the_ten_numbers_with_every_repetition_kept() {
        let clock = AtomicClock::default();
        let d = fake(&clock, Duration::from_micros(50));
        let m = run(&d, cfg(16, 3), &clock, &NoProgress).unwrap();
        // Sequential: 2 patterns x read/write; random: 2 patterns x read/write x (IOPS, MB/s).
        assert_eq!(m.len(), 4 + 8);
        assert!(m.iter().all(|x| x.samples.len() == 3 && x.value > 0.0 && x.higher_is_better));
        assert!(m.iter().any(|x| x.id == "disk-rand4k-t32-write-iops" && x.unit == "IOPS"));
        assert!(m.iter().any(|x| x.id == "disk-seq1m-t1-read" && x.unit == "MB/s"));
    }

    #[test]
    fn a_scripted_drive_that_writes_as_fast_as_allowed_never_passes_the_shown_budget() {
        for (mib, reps) in [(16, 1), (16, 3), (64, 3), (256, 3)] {
            let clock = AtomicClock::default();
            // A drive so fast that time limits never end a phase: only the byte limits and the budget do.
            let d = fake(&clock, Duration::from_nanos(1));
            run(&d, cfg(mib, reps), &clock, &NoProgress).unwrap();
            let written = d.d.written.load(Ordering::SeqCst);
            let shown = dyno_core::bench::disk::disk_write_budget(mib, reps);
            assert!(written <= shown, "{mib} MiB x {reps}: wrote {written}, shown {shown}");
            // And the bound is tight for the default shape: the whole budget is used when nothing ends a phase early.
            assert_eq!(written, shown, "{mib} MiB x {reps}");
        }
    }

    #[test]
    fn the_progress_phase_is_one_steady_id_on_the_scale_of_passes() {
        struct Collect(std::sync::Mutex<Vec<dyno_core::bench::wire::PhaseProgress>>);
        impl Progress for Collect {
            fn report(&self, p: dyno_core::bench::wire::PhaseProgress) {
                self.0.lock().unwrap().push(p);
            }
        }
        let clock = AtomicClock::default();
        let d = fake(&clock, Duration::from_micros(50));
        let c = Collect(Default::default());
        run(&d, cfg(16, 2), &clock, &c).unwrap();
        let all = c.0.lock().unwrap();
        assert!(all.iter().all(|p| p.phase == "disk" && p.of == 2));
        assert!(all.windows(2).all(|w| w[1].rep >= w[0].rep), "progress never goes backwards");
        assert!(all[0].label.starts_with("Preparing"));
        assert!(all.iter().any(|p| p.label == "Pass 1 of 2 · Random 4 KiB, 32 threads · write"));
        assert_eq!(all.last().unwrap().rep, 2);
    }

    #[test]
    fn a_cached_drive_is_said_in_the_runner_note_not_in_the_metric_labels() {
        // The labels stay the same whether or not the cache was bypassed; the runner's note ("Direct I/O: no ...") and the page's badge say it.
        let clock = AtomicClock::default();
        let mut d = fake(&clock, Duration::from_micros(50));
        d.direct = false;
        let m = run(&d, cfg(16, 1), &clock, &NoProgress).unwrap();
        assert!(m.iter().all(|x| !x.label.contains("cached")));
    }

    #[test]
    fn every_write_pass_is_flushed_to_the_device() {
        let clock = AtomicClock::default();
        let d = fake(&clock, Duration::from_micros(50));
        run(&d, cfg(16, 2), &clock, &NoProgress).unwrap();
        // The fill, then 4 write patterns x 2 repetitions.
        assert_eq!(d.flushes.load(Ordering::SeqCst), 1 + 8);
    }

    #[test]
    fn a_drive_that_is_nearly_full_ends_the_run_before_the_next_write_pass() {
        let clock = AtomicClock::default();
        let mut d = fake(&clock, Duration::from_micros(50));
        d.space = Some((100 * MIB * 1024, MIB));
        let e = run(&d, cfg(16, 1), &clock, &NoProgress).unwrap_err();
        assert!(matches!(&e, Outcome::Failed(r) if r.contains("free")), "{e:?}");
    }

    #[test]
    fn a_full_disk_error_is_named_and_other_errors_carry_their_text() {
        assert!(describe(&io::Error::from_raw_os_error(28)).contains("ran out of space"));
        assert!(describe(&io::Error::other("boom")).contains("boom"));
        let clock = AtomicClock::default();
        let mut d = fake(&clock, Duration::from_micros(50));
        d.d.fail_after = Some(5);
        assert!(matches!(run(&d, cfg(16, 1), &clock, &NoProgress), Err(Outcome::Failed(r)) if r.contains("scripted")));
    }

    #[test]
    fn a_sustained_write_reports_start_end_and_kept_and_counts_in_the_budget() {
        let clock = AtomicClock::default();
        // 1 MiB takes 1 ms: a second moves 1000 MiB; the file is 16 MiB, so the write cycles over it.
        let d = fake(&clock, Duration::from_millis(1));
        let mut c = cfg(16, 1);
        c.sustained_mib = 12 * 1000;
        let m = run(&d, c, &clock, &NoProgress).unwrap();
        let by = |id: &str| m.iter().find(|x| x.id == id).map(|x| x.value);
        assert!(by("disk-sustained-write-start").unwrap() > 0.0);
        assert!(
            (by("disk-sustained-write-kept").unwrap() - 100.0).abs() < 1.0,
            "a drive of constant speed keeps its speed"
        );
        let written = d.d.written.load(Ordering::SeqCst);
        assert!(
            written <= disk_write_budget_with(16, 1, c.sustained_mib.div_ceil(1024)),
            "the sustained write stays inside the budget"
        );
        assert!(written > 12_000 * MIB, "it wrote what it was asked to");
    }

    #[test]
    fn a_drive_that_finishes_before_it_can_tire_gives_no_sustained_numbers() {
        let clock = AtomicClock::default();
        let d = fake(&clock, Duration::from_micros(10));
        let mut c = cfg(16, 1);
        c.sustained_mib = 64;
        let m = run(&d, c, &clock, &NoProgress).unwrap();
        assert!(m.iter().all(|x| !x.id.starts_with("disk-sustained")));
    }

    #[test]
    fn the_fill_covers_the_whole_file_once() {
        let clock = AtomicClock::default();
        let d = fake(&clock, Duration::from_nanos(1));
        let mut cfg1 = cfg(16, 1);
        cfg1.reps = 1;
        run(&d, cfg1, &clock, &NoProgress).unwrap();
        let offsets = d.d.offsets.lock().unwrap();
        // The first 16 operations (1 MiB each) are the fill: offsets 0, 1 MiB, ... 15 MiB, in order.
        assert!(
            offsets.iter().take(16).enumerate().all(|(i, o)| *o == i as u64 * MIB),
            "{:?}",
            &offsets[..16]
        );
    }
}
