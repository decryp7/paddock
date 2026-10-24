//! The machine series every app shows.

use crate::series::{Mean, Millis, Series};
use serde::{Deserialize, Serialize};

/// One reading of the machine (or one aggregated point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSample {
    /// When (the start of the bucket for aggregated points).
    pub at: Millis,
    /// CPU use of the whole machine, per mille (average over the bucket).
    pub cpu_permille: u32,
    /// Peak CPU use in the bucket, per mille.
    pub cpu_peak_permille: u32,
    /// Memory in use (average).
    pub mem_used_bytes: u64,
    /// Memory installed.
    pub mem_total_bytes: u64,
    /// Free space on the watched disk (the minimum over the bucket: the number that makes work fail).
    pub disk_free_bytes: u64,
    /// Size of that disk.
    pub disk_total_bytes: u64,
    /// What the app itself and its children use (average; PSS on Linux).
    pub own_mem_bytes: u64,
}

/// The running aggregate of a machine bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineAcc {
    start: Millis,
    cpu: Mean,
    peak: u32,
    mem: Mean,
    mem_total: u64,
    disk_min: u64,
    disk_total: u64,
    own: Mean,
}

impl Series for MachineSample {
    type Acc = MachineAcc;

    fn at(&self) -> Millis {
        self.at
    }

    fn begin(start: Millis, s: &MachineSample) -> MachineAcc {
        MachineAcc {
            start,
            cpu: Mean::of(u64::from(s.cpu_permille)),
            peak: s.cpu_peak_permille.max(s.cpu_permille),
            mem: Mean::of(s.mem_used_bytes),
            mem_total: s.mem_total_bytes,
            disk_min: s.disk_free_bytes,
            disk_total: s.disk_total_bytes,
            own: Mean::of(s.own_mem_bytes),
        }
    }

    fn absorb(acc: &mut MachineAcc, s: &MachineSample) {
        acc.cpu.add(u64::from(s.cpu_permille));
        acc.peak = acc.peak.max(s.cpu_peak_permille).max(s.cpu_permille);
        acc.mem.add(s.mem_used_bytes);
        acc.mem_total = s.mem_total_bytes;
        acc.disk_min = acc.disk_min.min(s.disk_free_bytes);
        acc.disk_total = s.disk_total_bytes;
        acc.own.add(s.own_mem_bytes);
    }

    fn point(acc: &MachineAcc) -> MachineSample {
        MachineSample {
            at: acc.start,
            cpu_permille: u32::try_from(acc.cpu.mean()).unwrap_or(u32::MAX),
            cpu_peak_permille: acc.peak,
            mem_used_bytes: acc.mem.mean(),
            mem_total_bytes: acc.mem_total,
            disk_free_bytes: acc.disk_min,
            disk_total_bytes: acc.disk_total,
            own_mem_bytes: acc.own.mean(),
        }
    }

    fn start(acc: &MachineAcc) -> Millis {
        acc.start
    }
}
