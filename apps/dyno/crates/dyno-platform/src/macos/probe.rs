//! The macOS sensor probe: kernel memory-pressure level, GPU load from IOKit's accelerator statistics (read through
//! `ioreg`, every few ticks and bounded by a timeout), tidier volumes and honest reasons for what Apple does not
//! expose to an unprivileged app (disk queue, fans on Apple silicon).

use super::volumes;
use crate::parse::ioreg::parse_gpu;
use crate::parse::mac_tools::pressure_from_level;
use crate::sensors::SensorProbe;
use dyno_core::metrics::{GpuSample, MemoryPressure, Reading, SystemSample};
use std::time::Duration;

/// The GPU is read every this many ticks (each read starts `ioreg`).
const GPU_EVERY: u64 = 5;
/// After a failed GPU read look again only after this many ticks.
const GPU_RETRY: u64 = 60;

/// Probe for macOS.
pub struct MacProbe {
    ticks: u64,
    gpu: Reading<GpuSample>,
    gpu_missing_until: u64,
    mem_total: u64,
}

impl MacProbe {
    /// A probe; the first tick reads the GPU.
    pub fn new() -> Self {
        MacProbe {
            ticks: 0,
            gpu: Reading::unavailable("Not read yet"),
            gpu_missing_until: 0,
            mem_total: 0,
        }
    }

    fn read_gpu(&mut self) -> Reading<GpuSample> {
        let mut cmd = crate::process::command("/usr/sbin/ioreg");
        cmd.args(["-r", "-d", "1", "-w0", "-c", "IOAccelerator"]);
        let out = match crate::process::output_text(cmd, Duration::from_secs(2)) {
            Ok(t) => t,
            Err(e) => {
                self.gpu_missing_until = self.ticks + GPU_RETRY;
                return Reading::unavailable(format!("GPU statistics could not be read: {e}"));
            }
        };
        match parse_gpu(&out) {
            Some(g) => Reading::Available(GpuSample {
                name: g.name,
                utilization: g.utilization,
                memory_used: g.memory_used,
                // Unified memory shares the system's RAM.
                memory_total: if g.memory_total > 0 { g.memory_total } else { self.mem_total },
            }),
            None => {
                self.gpu_missing_until = self.ticks + GPU_RETRY;
                Reading::unavailable("This Mac reports no GPU statistics")
            }
        }
    }
}

impl Default for MacProbe {
    fn default() -> Self {
        Self::new()
    }
}

/// The kernel's memory-pressure level.
pub fn memory_pressure() -> Option<MemoryPressure> {
    let name = c"kern.memorystatus_vm_pressure_level";
    let mut level: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    // SAFETY: `name` is a valid NUL-terminated string, `level` and `size` are valid for writes of the sizes given,
    // and no new value is set (null, 0).
    let r = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut level as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (r == 0).then(|| pressure_from_level(level)).flatten()
}

impl SensorProbe for MacProbe {
    fn enrich(&mut self, sample: &mut SystemSample) {
        self.mem_total = sample.memory.total;
        if let Some(p) = memory_pressure() {
            sample.memory.pressure = Reading::Available(p);
        }
        sample.memory.commit = Reading::unavailable("macOS has no commit limit: memory pressure and compression show how full it is");
        if self.ticks.is_multiple_of(GPU_EVERY) && self.ticks >= self.gpu_missing_until {
            self.gpu = self.read_gpu();
        }
        sample.gpu = self.gpu.clone();
        sample.disks = volumes::tidy(std::mem::take(&mut sample.disks));
        sample.disk_io.queue = Reading::unavailable("macOS does not report a disk queue length");
        sample.disk_io.busy = Reading::unavailable("macOS does not report disk busy time");
        if sample.fans.value().is_none() {
            sample.fans = Reading::unavailable("Apple silicon Macs report no fan speeds to apps without administrator rights");
        }
        self.ticks = self.ticks.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::SysinfoSensors;
    use dyno_core::ports::SystemSensors;
    use dyno_core::time::Timestamp;

    #[test]
    fn the_kernel_reports_a_pressure_level() {
        assert!(memory_pressure().is_some());
    }

    #[test]
    fn samples_this_mac_end_to_end() {
        let mut s = SysinfoSensors::new(Box::new(MacProbe::new()));
        s.sample(Timestamp(1));
        std::thread::sleep(Duration::from_millis(400));
        let x = s.sample(Timestamp(2));
        assert!(x.cpu.per_core.len() >= 2, "per-core CPU");
        assert!(x.memory.total > 1 << 30 && x.memory.used > 0 && x.memory.used <= x.memory.total);
        assert!(x.memory.pressure.value().is_some());
        assert!(x.memory.commit.value().is_none());
        assert!(
            x.disks.iter().any(|d| d.total > 0 && d.free <= d.total),
            "a volume with space: {:?}",
            x.disks
        );
        assert!(x
            .disks
            .iter()
            .all(|d| !d.mount.starts_with("/System/Volumes/") || d.mount.ends_with("/Data")));
        assert!(x.uptime_secs > 0);
        match &x.gpu {
            Reading::Available(g) => assert!((0.0..=100.0).contains(&g.utilization) && !g.name.is_empty()),
            Reading::Unavailable(why) => assert!(!why.is_empty()),
        }
        assert!(x.disk_io.queue.value().is_none());
    }
}
