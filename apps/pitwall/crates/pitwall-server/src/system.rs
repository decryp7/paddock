//! Reading the machine: hardware (for build and fit decisions) and live memory (for the release policy).

use crate::process;
use pitwall_core::fit::Room;
use pitwall_core::hardware::{self, Gpu, Hardware};
use pitwall_core::memory::System;
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
    Hardware { os, arch, cpu_cores: sys.cpus().len().max(1), total_ram: total, gpus }
}

async fn nvidia_gpus() -> Vec<Gpu> {
    let mut c = process::command("nvidia-smi", None);
    c.args([hardware::NVIDIA_SMI_QUERY, "--format=csv,noheader,nounits"]);
    match tokio::time::timeout(Duration::from_secs(5), c.output()).await {
        Ok(Ok(o)) if o.status.success() => hardware::parse_nvidia_smi(&String::from_utf8_lossy(&o.stdout)),
        _ => vec![],
    }
}

/// Live memory for the release policy; `vram_free` comes from the hardware's GPUs when known.
pub fn read_system(hw: &Hardware) -> System {
    let mut s = sysinfo::System::new();
    s.refresh_memory();
    System { total: s.total_memory(), available: s.available_memory(), vram_free: hw.vram_free(), os_pressure: false, os_critical: false, foreground_busy: false }
}

/// The room a fit check has right now.
pub fn room(hw: &Hardware, releasable: u64) -> Room {
    let sys = read_system(hw);
    Room { ram_total: sys.total, ram_available: sys.available, ram_releasable: releasable, vram_free: hw.vram_free(), unified: hw.unified() }
}

/// Free bytes on the volume holding `path` (0 when unknown).
pub fn free_disk(path: &std::path::Path) -> u64 {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    disks.list().iter().filter(|d| p.starts_with(d.mount_point())).max_by_key(|d| d.mount_point().as_os_str().len()).map_or(0, sysinfo::Disk::available_space)
}

/// Resident memory of a process in bytes (0 when unreadable).
pub fn rss(pid: u32) -> u64 {
    let mut s = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    s.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
    s.process(p).map_or(0, sysinfo::Process::memory)
}

/// Whether a process id is alive.
pub fn pid_alive(pid: u32) -> bool {
    let mut s = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    s.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), false);
    s.process(p).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hardware_and_memory_are_read() {
        let hw = detect_hardware().await;
        assert!(hw.total_ram > 0 && hw.cpu_cores >= 1);
        let s = read_system(&hw);
        assert!(s.total > 0 && s.available <= s.total);
        assert!(room(&hw, 0).ram_total > 0);
        assert!(pid_alive(std::process::id()));
        assert!(rss(std::process::id()) > 0);
        let _ = free_disk(std::path::Path::new("."));
    }
}
