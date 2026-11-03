//! What the picker shows before a run: about how long each kind takes and how much the disk test writes per drive. The numbers are
//! the engine's (ADR 0030 section 4: "the estimator's numbers"); the page only adds them up for what is chosen.

use super::disk::{disk_write_budget, DEFAULT_MIB, QUICK_MIB, REPETITIONS};
use super::wire::kind_duration;
use super::{cpu, BenchKind};
use serde::{Deserialize, Serialize};

/// About how long the graphics test takes: calibration, two warm-up submissions and five repetitions of four measurements.
pub const GPU_SECONDS: u64 = 45;
/// A typical solid-state drive's speed for the estimate of the fill, bytes per second.
pub const TYPICAL_DRIVE_BYTES_PER_S: u64 = 500_000_000;
/// A typical time of one disk measurement (ramp plus measured part), seconds.
pub const TYPICAL_DISK_PHASE_SECS: u64 = 3;

/// About how long one kind takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindSeconds {
    /// The kind.
    pub kind: BenchKind,
    /// Seconds.
    pub seconds: u64,
}

/// The disk test at one file size, per drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskEstimate {
    /// The scratch file's size, MiB.
    pub mib: u32,
    /// The most bytes the test writes to the drive ([`disk_write_budget`]).
    pub bytes_written: u64,
    /// About how long it takes on a typical solid-state drive, seconds.
    pub seconds: u64,
}

/// The sustained disk write at one size, per drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SustainedWriteEstimate {
    /// The amount written, GiB.
    pub gib: u32,
    /// About how long it takes on a typical solid-state drive, seconds.
    pub seconds: u64,
}

/// Everything the picker needs to show estimates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Estimates {
    /// Seconds per kind (the disk's is per drive at the default size; see `disk`).
    pub kinds: Vec<KindSeconds>,
    /// The disk test at each offered file size.
    pub disk: Vec<DiskEstimate>,
    /// The sustained CPU lengths offered, minutes.
    pub sustained_minutes: Vec<u32>,
    /// The longest sustained run where no temperature can be read, minutes.
    pub no_sensor_max_minutes: u32,
    /// The most the sustained disk write may write per drive, GiB.
    pub sustained_write_max_gib: u32,
    /// The disk test's file size for the Quick preset, MiB.
    pub quick_mib: u32,
    /// The sustained disk write at each offered amount.
    pub sustained_write: Vec<SustainedWriteEstimate>,
}

/// What a run is going to be, as far as the estimate is concerned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateRequest {
    /// The kinds chosen.
    pub kinds: Vec<BenchKind>,
    /// The disk test's file size, MiB (`None`: the Quick preset's).
    pub disk_mib: Option<u32>,
    /// How many drives are ticked (the disk test runs once on each).
    pub drives: u32,
    /// The CPU kind is a sustained run of this many minutes.
    pub sustained_minutes: Option<u32>,
    /// The sustained disk write, GiB per drive, when asked for.
    pub sustained_write_gib: Option<u32>,
}

/// About how long a run takes and the most it writes to each drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEstimate {
    /// About how long, seconds.
    pub seconds: u64,
    /// The most bytes written to each drive (`disk_write_budget` plus the sustained write); zero without the disk test.
    pub bytes_per_drive: u64,
}

/// Estimates a run from what is chosen. `None` when the disk test is chosen and no drive is: nothing can be said until a drive is.
pub fn estimate_run(e: &Estimates, r: &EstimateRequest) -> Option<RunEstimate> {
    let disk = r.kinds.contains(&BenchKind::Disk);
    if disk && r.drives == 0 {
        return None;
    }
    let mut seconds = 0;
    let mut bytes = 0;
    for k in &r.kinds {
        match k {
            BenchKind::Disk => {
                let mib = r.disk_mib.unwrap_or(e.quick_mib);
                let d = e.disk.iter().find(|d| d.mib == mib).or(e.disk.first())?;
                seconds += d.seconds * u64::from(r.drives);
                bytes = d.bytes_written;
                if let Some(g) = r.sustained_write_gib {
                    seconds += e.sustained_write.iter().find(|w| w.gib == g).map_or(0, |w| w.seconds) * u64::from(r.drives);
                    bytes += u64::from(g) * super::disk::GIB;
                }
            }
            BenchKind::Cpu if r.sustained_minutes.is_some() => seconds += u64::from(r.sustained_minutes.unwrap_or(0)) * 60,
            other => seconds += e.kinds.iter().find(|x| x.kind == *other).map_or(0, |x| x.seconds),
        }
    }
    Some(RunEstimate {
        seconds,
        bytes_per_drive: bytes,
    })
}

/// The estimates for this build.
pub fn standard() -> Estimates {
    let kinds = BenchKind::ALL
        .iter()
        .map(|k| KindSeconds {
            kind: *k,
            seconds: match k {
                BenchKind::Gpu => GPU_SECONDS,
                _ => kind_duration(*k).as_secs(),
            },
        })
        .collect();
    let disk = [QUICK_MIB, DEFAULT_MIB, 4096]
        .iter()
        .map(|mib| DiskEstimate {
            mib: *mib,
            bytes_written: disk_write_budget(*mib, REPETITIONS),
            seconds: u64::from(*mib) * super::disk::MIB / TYPICAL_DRIVE_BYTES_PER_S + u64::from(REPETITIONS) * 8 * TYPICAL_DISK_PHASE_SECS,
        })
        .collect();
    Estimates {
        kinds,
        disk,
        sustained_minutes: cpu::SUSTAINED_MINUTES.to_vec(),
        no_sensor_max_minutes: cpu::NO_SENSOR_MAX_MINUTES,
        sustained_write_max_gib: super::disk::SUSTAINED_WRITE_MAX_GIB,
        quick_mib: QUICK_MIB,
        sustained_write: [8_u32, 16, 32, 64]
            .iter()
            .map(|g| SustainedWriteEstimate {
                gib: *g,
                seconds: u64::from(*g) * super::disk::GIB / TYPICAL_DRIVE_BYTES_PER_S,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimates_come_from_the_methods_own_numbers() {
        let e = standard();
        let secs = |k| e.kinds.iter().find(|x| x.kind == k).unwrap().seconds;
        assert_eq!((secs(BenchKind::Cpu), secs(BenchKind::Memory), secs(BenchKind::Gpu)), (49, 64, 45));
        let d: Vec<(u32, u64)> = e.disk.iter().map(|d| (d.mib, d.bytes_written)).collect();
        assert_eq!(
            d,
            vec![
                (256, 3_712 * 1024 * 1024),
                (1024, 14_848 * 1024 * 1024),
                (4096, 59_392 * 1024 * 1024)
            ]
        );
        assert!(e.disk[0].seconds < e.disk[1].seconds && e.disk[1].seconds < e.disk[2].seconds);
        assert_eq!(e.sustained_write.iter().map(|s| s.gib).collect::<Vec<_>>(), vec![8, 16, 32, 64]);
        assert_eq!(e.sustained_write[0].seconds, 17, "8 GiB at 500 MB/s");
        assert_eq!(
            (e.sustained_minutes.clone(), e.no_sensor_max_minutes, e.sustained_write_max_gib),
            (vec![2, 5, 10, 20], 5, 64)
        );
    }

    #[test]
    fn a_quick_run_is_a_few_minutes_for_the_cpu_memory_and_one_drive() {
        let e = standard();
        let total: u64 = [BenchKind::Cpu, BenchKind::Memory]
            .iter()
            .map(|k| e.kinds.iter().find(|x| x.kind == *k).unwrap().seconds)
            .sum::<u64>()
            + e.disk[0].seconds;
        assert!((150..=260).contains(&total), "{total}");
    }

    fn req(kinds: &[BenchKind], drives: u32) -> EstimateRequest {
        EstimateRequest {
            kinds: kinds.to_vec(),
            disk_mib: None,
            drives,
            sustained_minutes: None,
            sustained_write_gib: None,
        }
    }

    #[test]
    fn a_run_is_estimated_from_the_ticked_drives_and_the_sustained_amount() {
        let e = standard();
        let one = estimate_run(&e, &req(&[BenchKind::Disk], 1)).unwrap();
        let two = estimate_run(&e, &req(&[BenchKind::Disk], 2)).unwrap();
        assert_eq!(two.seconds, one.seconds * 2, "the disk test runs on every ticked drive");
        assert_eq!(
            two.bytes_per_drive, one.bytes_per_drive,
            "what is written to each drive does not grow with the number of drives"
        );
        let with = estimate_run(
            &e,
            &EstimateRequest {
                sustained_write_gib: Some(8),
                ..req(&[BenchKind::Disk], 2)
            },
        )
        .unwrap();
        assert_eq!(with.bytes_per_drive, one.bytes_per_drive + 8 * super::super::disk::GIB);
        assert_eq!(with.seconds, two.seconds + 17 * 2);
    }

    #[test]
    fn nothing_is_said_about_the_disk_until_a_drive_is_chosen() {
        let e = standard();
        assert_eq!(estimate_run(&e, &req(&[BenchKind::Cpu, BenchKind::Disk], 0)), None);
        assert!(estimate_run(&e, &req(&[BenchKind::Cpu], 0)).is_some(), "no disk, no drive needed");
    }

    #[test]
    fn a_sustained_cpu_run_takes_its_minutes_and_the_other_kinds_their_own_time() {
        let e = standard();
        let s = estimate_run(
            &e,
            &EstimateRequest {
                sustained_minutes: Some(5),
                ..req(&[BenchKind::Cpu], 0)
            },
        )
        .unwrap();
        assert_eq!((s.seconds, s.bytes_per_drive), (300, 0));
        let m = estimate_run(&e, &req(&[BenchKind::Memory, BenchKind::Gpu], 0)).unwrap();
        assert_eq!(m.seconds, 64 + 45);
    }
}
