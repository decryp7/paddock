//! The real machine: `sysinfo` for CPU, memory and volumes, `paddock-proc` for the app's own memory. Native glue (rule 7:
//! a test cannot make the machine busy): each function is one OS call or one hand-over, and the logic is in `Sampler`.

use crate::disk::Mount;
use crate::sampler::MachineProbe;
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

/// The mounted volumes now.
pub(crate) fn mounts() -> Vec<Mount> {
    let disks = match Disks::new_with_refreshed_list() {
        Ok(disks) => disks,
        Err(why) => {
            log::warn!("the volume list is unavailable (sysinfo): {why}; no volumes are reported");
            return Vec::new();
        }
    };
    disks
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
    /// `None` when sysinfo could not start (logged once); the readings are then zero, which the sampler treats as unknown.
    sys: Option<System>,
}

impl Default for SystemProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemProbe {
    /// A probe; the first CPU reading is taken at once so the next one covers the time in between.
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        )
        .inspect_err(|why| {
            log::warn!(
                "the machine sampler is unavailable (sysinfo): {why}; CPU and memory read as zero"
            )
        })
        .ok()
        .map(|mut sys| {
            sys.refresh_cpu_usage();
            sys
        });
        SystemProbe { sys }
    }
}

impl MachineProbe for SystemProbe {
    fn cpu_permille(&mut self) -> u32 {
        let Some(sys) = self.sys.as_mut() else {
            return 0;
        };
        sys.refresh_cpu_usage();
        (sys.global_cpu_usage().clamp(0.0, 100.0) * 10.0).round() as u32
    }

    fn memory(&mut self) -> (u64, u64) {
        let Some(sys) = self.sys.as_mut() else {
            return (0, 0);
        };
        sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        let total = sys.total_memory();
        (total.saturating_sub(sys.available_memory()), total)
    }

    fn mounts(&mut self) -> Vec<Mount> {
        mounts()
    }

    fn process_memory(&mut self, pid: u32) -> u64 {
        paddock_proc::tree_memory(pid)
    }
}

/// The id of the volume holding `path` (device id on Unix, volume serial number on Windows), `None` when unreadable.
#[cfg(unix)]
pub(crate) fn volume_id(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

/// As the Unix one: the volume serial number of the volume holding `path` (a handle opened for metadata only).
#[cfg(windows)]
pub(crate) fn volume_id(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    // SAFETY: `info` is plain data the call fills in; the handle stays open until `file` drops at the end of the function.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) };
    (ok != 0).then(|| u64::from(info.dwVolumeSerialNumber))
}

/// The volume id of a folder entry already read: on Unix from its metadata (no extra call), on Windows from a handle.
pub(crate) fn dir_volume(path: &std::path::Path, meta: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Some(meta.dev())
    }
    #[cfg(windows)]
    {
        let _ = meta;
        volume_id(path)
    }
}
