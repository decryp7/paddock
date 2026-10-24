//! The machine sampler.

use crate::disk::{Mount, Space, volume_free};
use crate::machine::MachineSample;
use paddock_jobs::time::Clock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long the list of volumes is reused (free space changes slowly and listing volumes is the costly call).
pub const DEFAULT_MOUNTS_EVERY: Duration = Duration::from_secs(60);

/// What the sampler needs from the OS. Implemented by [`SystemProbe`](crate::SystemProbe); tests script it.
pub trait MachineProbe: Send {
    /// CPU use of the whole machine since the previous call, per mille (0 to 1000).
    fn cpu_permille(&mut self) -> u32;
    /// Memory in use and installed, in bytes.
    fn memory(&mut self) -> (u64, u64);
    /// The mounted volumes.
    fn mounts(&mut self) -> Vec<Mount>;
    /// What the process tree at `pid` uses, in bytes (PSS on Linux), 0 when it is gone.
    fn process_memory(&mut self, pid: u32) -> u64;
}

/// Reads the machine on a [`Clock`]: one [`MachineSample`] per call, stamped with the clock's wall time. The list of
/// volumes is cached for [`mounts_every`](Self::mounts_every) on the clock's monotonic reading.
pub struct Sampler<P: MachineProbe> {
    probe: P,
    clock: Arc<dyn Clock>,
    disk: Option<PathBuf>,
    pid: Option<u32>,
    mounts_every: Duration,
    mounts: Option<(Duration, Vec<Mount>)>,
}

impl<P: MachineProbe> Sampler<P> {
    /// A sampler reading `probe` with times from `clock`.
    pub fn new(probe: P, clock: Arc<dyn Clock>) -> Self {
        Sampler {
            probe,
            clock,
            disk: None,
            pid: None,
            mounts_every: DEFAULT_MOUNTS_EVERY,
            mounts: None,
        }
    }

    /// Reports the free space of the volume holding `path` in every sample.
    #[must_use]
    pub fn watch_disk(mut self, path: impl Into<PathBuf>) -> Self {
        self.disk = Some(path.into());
        self
    }

    /// Reports what the process tree at `pid` uses in every sample.
    #[must_use]
    pub fn watch_process(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    /// Reuses the list of volumes for this long.
    #[must_use]
    pub fn mounts_every(mut self, every: Duration) -> Self {
        self.mounts_every = every;
        self
    }

    /// Free and total space of the volume holding `path`, from the cached list of volumes. `None` when no volume holds it.
    pub fn disk_space(&mut self, path: &Path) -> Option<Space> {
        let now = self.clock.monotonic();
        let stale = self
            .mounts
            .as_ref()
            .is_none_or(|(at, _)| now.saturating_sub(*at) >= self.mounts_every);
        if stale {
            self.mounts = Some((now, self.probe.mounts()));
        }
        let mounts = self
            .mounts
            .as_ref()
            .map(|(_, m)| m.as_slice())
            .unwrap_or_default();
        // A path that does not exist yet is looked up through its nearest existing ancestor.
        let path = crate::disk::existing_ancestor(path).unwrap_or_else(|| path.to_path_buf());
        volume_free(&path, mounts)
    }

    /// Reads the machine now. A disk that cannot be found reports 0 free of 0.
    pub fn sample(&mut self) -> MachineSample {
        let at = u64::try_from(self.clock.wall_ms()).unwrap_or(0);
        let cpu = self.probe.cpu_permille().min(1000);
        let (used, total) = self.probe.memory();
        let space = match self.disk.clone() {
            Some(path) => self.disk_space(&path),
            None => None,
        };
        let own = match self.pid {
            Some(pid) => self.probe.process_memory(pid),
            None => 0,
        };
        MachineSample {
            at,
            cpu_permille: cpu,
            cpu_peak_permille: cpu,
            mem_used_bytes: used.min(total),
            mem_total_bytes: total,
            disk_free_bytes: space.map_or(0, |s| s.free),
            disk_total_bytes: space.map_or(0, |s| s.total),
            own_mem_bytes: own,
        }
    }
}
