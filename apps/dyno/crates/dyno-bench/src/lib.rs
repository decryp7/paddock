//! The benchmark kernels. Each takes a time budget and returns metrics; none touches anything outside its own
//! buffers, except the disk test, which writes one capped scratch file inside the folder it is given and removes it.
//! The kernels use only safe code and `std`, so they behave the same on every OS.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod cpu;
pub mod disk;
pub mod emit;
pub mod memory;
pub mod repeat;

use dyno_core::bench::{BenchKind, BenchMetric};
use emit::Progress;
use paddock_bench::SystemClock;
use repeat::{Repeats, SleepPace};
use std::path::Path;
use std::time::Duration;

/// What one benchmark needs.
#[derive(Debug, Clone)]
pub struct Settings<'a> {
    /// The time budget of each timed section.
    pub budget: Duration,
    /// Warm-up, repetitions and the gap between them.
    pub repeats: Repeats,
    /// The scratch folder for the disk test (created and removed by the caller).
    pub dir: Option<&'a Path>,
    /// The scratch file size for the disk test, MiB.
    pub disk_mib: u32,
}

/// Why a benchmark didn't produce numbers.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not possible here.
    Unavailable(String),
    /// It failed.
    Failed(String),
}

/// Runs one kind, reporting where it is through `progress`.
pub fn run(kind: BenchKind, s: &Settings<'_>, progress: &dyn Progress) -> Result<Vec<BenchMetric>, Outcome> {
    match kind {
        BenchKind::Cpu => Ok(cpu::run(s.budget, s.repeats, &SystemClock::new(), &SleepPace, progress)),
        BenchKind::Memory => Ok(memory::run(s.budget, s.repeats, &SystemClock::new(), &SleepPace, progress)),
        BenchKind::Disk if !dyno_core::bench::DISK_TEST_ENABLED => Err(Outcome::Unavailable(dyno_core::bench::DISK_TEST_OFF.into())),
        BenchKind::Disk => {
            let dir = s
                .dir
                .ok_or_else(|| Outcome::Failed("The disk test needs a scratch folder.".into()))?;
            progress.report(emit::phase("disk", "Disk test", 0, 1));
            disk::run(dir, s.disk_mib, s.budget).map_err(|e| Outcome::Failed(format!("The disk test failed: {e}")))
        }
        BenchKind::Gpu => Err(Outcome::Unavailable(
            "GPU compute tests need a graphics API that this build does not include, so none is run.".into(),
        )),
    }
}
