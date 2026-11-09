//! Reading the machine: hardware (for build and fit decisions) and live memory (for the release policy).

use crate::process;
use pitwall_core::fit::Room;
use pitwall_core::hardware::{self, Gpu, Hardware};
use pitwall_core::memory::System;
use std::sync::Arc;
use std::time::Duration;

/// Detect the hardware: CPU, RAM and GPUs (NVIDIA through `nvidia-smi` when present, Apple Silicon unified memory).
pub async fn detect_hardware() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    let total = sys.total_memory();
    let (os, arch) = (std::env::consts::OS.to_string(), std::env::consts::ARCH.to_string());
    let mut gpus: Vec<Gpu> = hardware::apple_silicon_gpu(&os, &arch, total).into_iter().collect();
    gpus.extend(nvidia_gpus().await);
    Hardware {
        os,
        arch,
        cpu_cores: sys.cpus().len().max(1),
        total_ram: total,
        gpus,
    }
}

async fn nvidia_gpus() -> Vec<Gpu> {
    let mut c = process::command("nvidia-smi", None);
    c.args([hardware::NVIDIA_SMI_QUERY, "--format=csv,noheader,nounits"]);
    match tokio::time::timeout(Duration::from_secs(5), c.output()).await {
        Ok(Ok(o)) if o.status.success() => hardware::parse_nvidia_smi(&String::from_utf8_lossy(&o.stdout)),
        _ => vec![],
    }
}

/// Live memory for the release policy; `vram_free` comes from the hardware's GPUs when known, the pressure and
/// foreground flags from the OS signals the release loop keeps current (`os_signals`).
pub fn read_system(hw: &Hardware) -> System {
    let mut s = sysinfo::System::new();
    s.refresh_memory();
    let sys = System {
        total: s.total_memory(),
        available: s.available_memory(),
        vram_free: hw.vram_free(),
        os_pressure: false,
        os_critical: false,
        foreground_busy: false,
    };
    crate::os_signals::current().apply(sys)
}

/// Where serving reads live memory from when it plans a load or a release: the host by default (`read_system`), a pinned
/// machine in tests, so a load's outcome never depends on how much memory the CI runner happens to have free.
pub type MemoryReader = Arc<dyn Fn(&Hardware) -> System + Send + Sync>;

/// The host's memory, read live.
pub fn host_memory() -> MemoryReader {
    Arc::new(read_system)
}

/// The room a fit check has over a memory reading already taken (see [`read_system`]).
pub fn room_from(sys: &System, hw: &Hardware, releasable: u64) -> Room {
    Room {
        ram_total: sys.total,
        ram_available: sys.available,
        ram_releasable: releasable,
        vram_free: hw.vram_free(),
        unified: hw.unified(),
    }
}

/// Free bytes on the volume holding `path` (0 when unknown).
pub fn free_disk(path: &std::path::Path) -> u64 {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    disks
        .list()
        .iter()
        .filter(|d| p.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map_or(0, sysinfo::Disk::available_space)
}

/// What a process really uses, in bytes (0 when unreadable): the physical footprint on macOS, PSS on Linux, the private
/// working set on Windows (rule 26, `paddock-proc`). The resident size is not used: on macOS it counts pages the kernel has
/// not reclaimed yet (a 535 MB worker showed 1,965 MB), which made a loaded model look much bigger than it is and
/// released models that fit.
pub fn memory(pid: u32) -> u64 {
    memory_with(pid, paddock_proc::process_memory)
}

/// [`memory`] over a reader (tests give a fake): `None` (no such process) is 0.
pub fn memory_with(pid: u32, read: impl Fn(u32) -> Option<u64>) -> u64 {
    read(pid).unwrap_or(0)
}

/// Asks a process to end (SIGTERM; `force`: kills it). Only ever used on engine processes this server started, by their
/// identity (rule 53): `id` is the pid with its start time, taken while the server holds the process, and paddock-proc acts
/// only while the pid still names that process. Where the request cannot be delivered the process is killed.
pub fn terminate(id: paddock_proc::ProcessId, force: bool) {
    if force || !paddock_proc::ask(id) {
        let _ = paddock_proc::end(id);
    }
}

/// Whether a process id is alive.
pub fn pid_alive(pid: u32) -> bool {
    let mut s = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    s.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), false);
    // A child that exited but was not reaped yet (a zombie on Linux) is gone for our purposes.
    s.process(p).is_some_and(|proc| proc.status() != sysinfo::ProcessStatus::Zombie)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_is_measured_by_what_it_really_uses_and_a_missing_one_is_zero() {
        // The reader is the shared one (footprint, PSS); a fake gives a footprint far below the resident size.
        assert_eq!(memory_with(7, |pid| (pid == 7).then_some(535 << 20)), 535 << 20);
        assert_eq!(memory_with(8, |pid| (pid == 7).then_some(535 << 20)), 0);
    }

    #[tokio::test]
    async fn hardware_and_memory_are_read() {
        let hw = detect_hardware().await;
        assert!(hw.total_ram > 0 && hw.cpu_cores >= 1);
        let s = read_system(&hw);
        assert!(s.total > 0 && s.available <= s.total);
        assert!(host_memory()(&hw).total > 0, "the default reader is the host");
        assert_eq!(room_from(&s, &hw, 7).ram_releasable, 7);
        assert!(pid_alive(std::process::id()));
        assert!(memory(std::process::id()) > 0);
        let _ = free_disk(std::path::Path::new("."));
    }
}
