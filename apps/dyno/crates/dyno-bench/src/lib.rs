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
    /// The drive spins (random patterns measure for a shorter time).
    pub rotational: bool,
    /// Memory available at the start, bytes (the agent reads it; the runner never guesses).
    pub available_memory: Option<u64>,
    /// MiB of sustained write after the disk patterns (zero: none).
    pub sustained_write_mib: u32,
    /// Run the CPU kind as a sustained run of this many seconds instead of the short test.
    pub sustained_secs: Option<u32>,
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
        BenchKind::Cpu if s.sustained_secs.is_some() => {
            let metrics = cpu::sustained::run(s.sustained_secs.unwrap_or(0), &SystemClock::new(), &SleepPace, progress);
            if metrics.is_empty() {
                Err(Outcome::Failed("The sustained run was too short to judge.".into()))
            } else {
                Ok(metrics)
            }
        }
        BenchKind::Cpu => Ok(cpu::run(s.budget, s.repeats, &SystemClock::new(), &SleepPace, progress)),
        BenchKind::Memory => {
            let m = memory::Memory {
                caches: memory::cache::total(),
                available: s.available_memory,
            };
            memory::run(m, s.budget, s.repeats, &SystemClock::new(), &SleepPace, progress).map_err(Outcome::Unavailable)
        }
        BenchKind::Disk if !dyno_core::bench::DISK_TEST_ENABLED => Err(Outcome::Unavailable(dyno_core::bench::DISK_TEST_OFF.into())),
        BenchKind::Disk => {
            let dir = s
                .dir
                .ok_or_else(|| Outcome::Failed("The disk test needs a scratch folder.".into()))?;
            if !(dyno_core::bench::DISK_MIN_MIB..=dyno_core::bench::DISK_MAX_MIB).contains(&s.disk_mib) {
                return Err(Outcome::Failed(format!(
                    "The disk test file must be between {} and {} MiB.",
                    dyno_core::bench::DISK_MIN_MIB,
                    dyno_core::bench::DISK_MAX_MIB
                )));
            }
            progress.report(emit::kind_phase("disk", "Opening the test file", 0, s.repeats.reps));
            let file = disk::scratch::Scratch::create(dir).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Outcome::Unavailable(format!(
                        "Dyno was not allowed to write to this drive ({e}). On a Mac, allow Dyno under System Settings > Privacy & Security > Files and Folders, then run again."
                    ))
                } else {
                    Outcome::Failed(disk::describe(&e))
                }
            })?;
            progress.note(if file.mode() == disk::scratch::Mode::Direct {
                "Direct I/O: yes, the file system's cache is bypassed."
            } else {
                "Direct I/O: no, this file system refused it; reads may come from memory."
            });
            let run = disk::DiskRun {
                file_mib: s.disk_mib,
                reps: s.repeats.reps,
                rotational: s.rotational,
                sustained_mib: s.sustained_write_mib,
            };
            disk::run(&file, run, &SystemClock::new(), progress)
        }
        BenchKind::Gpu => Err(Outcome::Unavailable(
            "Graphics tests are run by dyno-bench-gpu, not by this program.".into(),
        )),
    }
}
