//! The real machine: `sysinfo` for CPU, memory and volumes, `paddock-proc` for the app's own memory. Native glue (rule 7:
//! a test cannot make the machine busy): each function is one OS call or one hand-over, and the logic is in `Sampler`.

use crate::disk::Mount;
use crate::sampler::MachineProbe;
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

/// The mounted volumes now.
pub(crate) fn mounts() -> Vec<Mount> {
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| Mount {
            path: d.mount_point().to_path_buf(),
            free: d.available_space(),
            total: d.total_space(),
        })
        .collect()
}

/// A [`MachineProbe`] reading this machine.
pub struct SystemProbe {
    sys: System,
}

impl Default for SystemProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemProbe {
    /// A probe; the first CPU reading is taken at once so the next one covers the time in between.
    pub fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        );
        sys.refresh_cpu_usage();
        SystemProbe { sys }
    }
}

impl MachineProbe for SystemProbe {
    fn cpu_permille(&mut self) -> u32 {
        self.sys.refresh_cpu_usage();
        (self.sys.global_cpu_usage().clamp(0.0, 100.0) * 10.0).round() as u32
    }

    fn memory(&mut self) -> (u64, u64) {
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        let total = self.sys.total_memory();
        (total.saturating_sub(self.sys.available_memory()), total)
    }

    fn mounts(&mut self) -> Vec<Mount> {
        mounts()
    }

    fn process_memory(&mut self, pid: u32) -> u64 {
        paddock_proc::tree_memory(pid)
    }
}
