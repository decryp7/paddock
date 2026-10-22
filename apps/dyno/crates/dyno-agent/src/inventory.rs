//! The hardware inventory: read once (warmed on a background thread at agent start, or on the first request) and
//! cached, never in the sampling loop. The slow part (the memory slot table) runs without holding the core lock.

use crate::agent::{lock, Shared};
use dyno_core::inventory::{CpuInfo, DiskInfo, SystemInventory};
use std::sync::Arc;

/// Starts reading the inventory in the background so the first request is instant. Failure to spawn is harmless:
/// the first request reads it.
pub fn warm(shared: &Arc<Shared>) {
    let s = shared.clone();
    let _ = std::thread::Builder::new().name("dyno-inventory".into()).spawn(move || {
        let _ = get(&s);
    });
}

/// The inventory, from the cache when present.
pub fn get(shared: &Shared) -> SystemInventory {
    if let Some(i) = lock(&shared.core).inventory.clone() {
        return i;
    }
    let (total, disks) = {
        let core = lock(&shared.core);
        let sample = core.recent.back();
        (
            sample.map_or(0, |s| s.memory.total),
            sample
                .map(|s| {
                    s.disks
                        .iter()
                        .map(|d| DiskInfo {
                            name: d.name.clone(),
                            total_bytes: d.total,
                            removable: d.removable,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )
    };
    let total = if total > 0 { total } else { os_total_memory() };
    let memory = dyno_platform::inventory::read_memory_layout(total);
    let inv = SystemInventory::new(os_name(), sysinfo::System::host_name(), cpu_info(), memory, disks);
    lock(&shared.core).inventory = Some(inv.clone());
    inv
}

fn os_name() -> String {
    sysinfo::System::long_os_version()
        .or_else(sysinfo::System::name)
        .unwrap_or_else(|| "Unknown OS".into())
}

fn os_total_memory() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory()
}

fn cpu_info() -> CpuInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    let cpus = sys.cpus();
    let first = cpus.first();
    CpuInfo {
        name: first.map(|c| c.brand().trim().to_string()).unwrap_or_default(),
        physical_cores: sysinfo::System::physical_core_count().map_or(0, |n| n as u32),
        logical_cores: cpus.len() as u32,
        frequency_mhz: first.map_or(0, |c| c.frequency() as u32),
    }
}
