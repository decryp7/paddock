//! The hardware inventory: read once (warmed on a background thread at agent start, or on the first request) and
//! cached, never in the sampling loop. The slow part (the memory slot table) runs without holding the core lock.

use crate::agent::{lock, Shared};
use dyno_core::inventory::{CpuInfo, DiskInfo, SystemInventory};
use std::sync::Arc;

/// Starts reading the inventory in the background so the first request is instant. Failure to spawn is harmless:
/// the first request reads it.
pub fn warm(shared: &Arc<Shared>) {
    in_background(shared.clone(), |s| {
        let _ = get(&s);
    });
}

/// Runs the warm-up `read` on its own thread as live work (rule 52): it holds the gate a Hardware page request waits on
/// with no deadline, so on Windows it must not stall behind busy normal threads.
fn in_background<T: Send + 'static>(state: T, read: impl FnOnce(T) + Send + 'static) {
    let _ = std::thread::Builder::new().name("dyno-inventory".into()).spawn(move || {
        dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
        read(state);
    });
}

/// The inventory, from the cache when present.
pub fn get(shared: &Shared) -> SystemInventory {
    if let Some(i) = lock(&shared.core).inventory.clone() {
        return i;
    }
    // One reader at a time (the warm-up thread and a request can arrive together), and wait a moment for the first
    // sample so the volumes are known; otherwise the two would cache different answers.
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = GATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(i) = lock(&shared.core).inventory.clone() {
        return i;
    }
    for _ in 0..30 {
        if lock(&shared.core).recent.back().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
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

#[cfg(test)]
mod tests {
    /// Rule 52 (coordinator re-review, 2026-10-30): the warm-up holds the gate a Hardware page request waits on, so it runs
    /// as live work.
    #[test]
    fn the_warm_up_runs_as_live_work() {
        let (tx, rx) = std::sync::mpsc::channel();
        super::in_background(tx, |tx| {
            let _ = tx.send(dyno_platform::priority::current_role());
        });
        let role = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the warm-up ran (a hang guard)");
        assert_eq!(role, Some(dyno_platform::priority::ThreadRole::Live));
    }
}
