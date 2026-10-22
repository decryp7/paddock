//! What only Linux can add to the common sensors: memory pressure (PSI) and cgroup limits, disk queue and busy time,
//! temperatures, fans, the GPU, CPU frequency from cpufreq, and cleaner volume and network lists.
//!
//! Containers: `/proc/meminfo` and `/proc/diskstats` describe the **host**, not the container. When the process runs
//! in a cgroup with a memory limit, the limit replaces the total; otherwise the host's figures are what is shown.

use super::gpu::GpuReader;
use super::hwmon::{self, Readings};
use super::paths::{read_text, read_trim, HostPaths};
use super::tools::Tools;
use crate::parse::linux_proc::{
    cgroup_limit, disk_load, diskstats, is_hidden_mount, is_virtual_interface, mean_khz_as_mhz, meminfo, pressure_level, psi_some_avg10,
    stat_value, DiskStat,
};
use crate::sensors::SensorProbe;
use dyno_core::metrics::{Fan, Reading, SystemSample, Temperature};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Temperatures, fans and the GPU are read every this many ticks (sysfs reads and `nvidia-smi` are not free).
const SLOW_EVERY: u64 = 5;
/// The reason for pressure when the kernel has no PSI.
pub const NO_PSI: &str = "Memory pressure needs the kernel's PSI, which is off or unreadable here (boot with psi=1)";
/// The reason for fans.
pub const NO_FAN_FILES: &str = "No fan sensors found under /sys/class/hwmon";
/// The reason for temperatures.
pub const NO_TEMPERATURE_FILES: &str = "No temperature sensors found under /sys/class/hwmon or /sys/class/thermal";

/// The cgroup files that describe this process's memory limit.
struct CgroupMemory {
    limit: u64,
    /// Bytes counted against the limit that the kernel can drop (inactive file cache).
    current: u64,
}

/// The Linux sensor probe.
pub struct LinuxProbe {
    paths: HostPaths,
    cgroup: PathBuf,
    gpu: GpuReader,
    ticks: u64,
    disks: Option<(Instant, Vec<DiskStat>)>,
    slow: Option<(Readings, Reading<dyno_core::metrics::GpuSample>)>,
}

impl LinuxProbe {
    /// A probe reading `paths`, with cgroups at `/sys/fs/cgroup`.
    pub fn new(paths: HostPaths, tools: Arc<dyn Tools>) -> Self {
        let cgroup = paths.sys.join("fs/cgroup");
        LinuxProbe {
            gpu: GpuReader::new(tools, SLOW_EVERY),
            paths,
            cgroup,
            ticks: 0,
            disks: None,
            slow: None,
        }
    }

    /// This process's cgroup directory: its own path under the cgroup root when that exists, else the root (which
    /// inside a container is the container's own cgroup).
    fn own_cgroup(&self) -> PathBuf {
        let own = read_text(&self.paths.proc.join("self/cgroup"))
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("0::").map(str::to_string)))
            .map(|p| self.cgroup.join(p.trim_start_matches('/')));
        own.filter(|p| p.is_dir()).unwrap_or_else(|| self.cgroup.clone())
    }

    fn cgroup_memory(&self) -> Option<CgroupMemory> {
        let dir = self.own_cgroup();
        let read = |names: &[&str]| names.iter().find_map(|n| read_trim(&dir.join(n)));
        let (limit, current, stat) = match read(&["memory.max"]) {
            Some(v2) => (
                cgroup_limit(&v2)?,
                read_trim(&dir.join("memory.current"))?,
                read_text(&dir.join("memory.stat")),
            ),
            None => {
                // cgroup v1: the memory controller is its own hierarchy.
                let v1 = self.cgroup.join("memory");
                (
                    cgroup_limit(&read_trim(&v1.join("memory.limit_in_bytes"))?)?,
                    read_trim(&v1.join("memory.usage_in_bytes"))?,
                    read_text(&v1.join("memory.stat")),
                )
            }
        };
        let inactive = stat
            .as_deref()
            .and_then(|s| stat_value(s, "inactive_file").or_else(|| stat_value(s, "total_inactive_file")))
            .unwrap_or(0);
        Some(CgroupMemory {
            limit,
            current: current.trim().parse::<u64>().ok()?.saturating_sub(inactive),
        })
    }

    fn memory(&self, sample: &mut SystemSample) {
        let host = read_text(&self.paths.proc.join("meminfo")).and_then(|t| meminfo(&t));
        let limited = self.cgroup_memory().filter(|c| host.is_none_or(|h| c.limit < h.total));
        if let Some(c) = &limited {
            let m = &mut sample.memory;
            m.total = c.limit;
            m.used = c.current.min(c.limit);
            m.available = c.limit - m.used;
        }
        // A limited cgroup has its own pressure file; the host-wide one says little about it.
        let psi = limited
            .as_ref()
            .and_then(|_| read_text(&self.own_cgroup().join("memory.pressure")))
            .or_else(|| read_text(&self.paths.proc.join("pressure/memory")))
            .and_then(|t| psi_some_avg10(&t));
        sample.memory.pressure = match psi {
            Some(p) => Reading::Available(pressure_level(p, sample.memory.available, sample.memory.total)),
            None => Reading::unavailable(NO_PSI),
        };
    }

    fn disk_io(&mut self, sample: &mut SystemSample) {
        let now = read_text(&self.paths.proc.join("diskstats")).map(|t| diskstats(&t));
        let at = Instant::now();
        let (queue, busy) = match (&now, &self.disks) {
            (None, _) => (Reading::unavailable("Disk statistics (/proc/diskstats) can't be read"), None),
            (Some(n), _) if n.is_empty() => (
                Reading::unavailable("No physical disk found (only virtual or network storage)"),
                None,
            ),
            (Some(n), Some((then, prev))) => {
                match disk_load(prev, n, u64::try_from(at.duration_since(*then).as_millis()).unwrap_or(u64::MAX)) {
                    Some(l) => (Reading::Available(l.queue), Some(l.busy)),
                    None => (Reading::unavailable("Measuring (needs two samples)"), None),
                }
            }
            (Some(_), None) => (Reading::unavailable("Measuring (needs two samples)"), None),
        };
        sample.disk_io.queue = queue;
        sample.disk_io.busy = match busy {
            Some(b) => Reading::Available(b),
            None => sample.disk_io.queue.clone().map(|_| 0.0),
        };
        if let Some(n) = now {
            self.disks = Some((at, n));
        }
    }

    fn volumes(sample: &mut SystemSample) {
        sample.disks.retain(|d| !is_hidden_mount(&d.name, &d.mount));
        // The same device mounted several times (btrfs subvolumes, bind mounts): keep the shortest mount point.
        let all = std::mem::take(&mut sample.disks);
        for d in all {
            match sample.disks.iter_mut().find(|k| k.name == d.name && k.total == d.total) {
                Some(kept) if d.mount.len() < kept.mount.len() => *kept = d,
                Some(_) => {}
                None => sample.disks.push(d),
            }
        }
    }

    fn cpu_frequency(&self, sample: &mut SystemSample) {
        let cpufreq = |cpu: usize, file: &str| read_trim(&self.paths.sys.join(format!("devices/system/cpu/cpu{cpu}/cpufreq/{file}")));
        if sample.cpu.frequency_mhz == 0 {
            let n = sample.cpu.per_core.len().clamp(1, 256);
            let cur: Vec<String> = (0..n).filter_map(|c| cpufreq(c, "scaling_cur_freq")).collect();
            sample.cpu.frequency_mhz = mean_khz_as_mhz(&cur).unwrap_or(0);
        }
        if sample.cpu.base_frequency_mhz == 0 {
            // Only the hardware-reported base: the maximum is the boost clock and would make every load look throttled.
            sample.cpu.base_frequency_mhz = cpufreq(0, "base_frequency").and_then(|v| mean_khz_as_mhz(&[v])).unwrap_or(0);
        }
    }

    fn hardware(&mut self, sample: &mut SystemSample) {
        if self.slow.is_none() || self.ticks.is_multiple_of(SLOW_EVERY) {
            let readings = hwmon::read(&self.paths.sys);
            let gpu = self.gpu.read(&self.paths.sys);
            self.slow = Some((readings, gpu));
        }
        let Some((readings, gpu)) = &self.slow else { return };
        sample.gpu = gpu.clone();
        sample.fans = if readings.fans.is_empty() {
            Reading::unavailable(NO_FAN_FILES)
        } else {
            Reading::Available(
                readings
                    .fans
                    .iter()
                    .map(|f| Fan {
                        label: f.label.clone(),
                        rpm: f.rpm,
                    })
                    .collect(),
            )
        };
        if !readings.temperatures.is_empty() {
            sample.temperatures = Reading::Available(readings.temperatures.iter().map(copy_temperature).collect());
        } else if sample.temperatures.value().is_none() {
            sample.temperatures = Reading::unavailable(NO_TEMPERATURE_FILES);
        }
    }
}

fn copy_temperature(t: &Temperature) -> Temperature {
    Temperature {
        label: t.label.clone(),
        celsius: t.celsius,
        critical: t.critical,
    }
}

impl SensorProbe for LinuxProbe {
    fn enrich(&mut self, sample: &mut SystemSample) {
        self.memory(sample);
        self.disk_io(sample);
        Self::volumes(sample);
        sample.network.retain(|n| !is_virtual_interface(&n.name));
        self.cpu_frequency(sample);
        self.hardware(sample);
        self.ticks = self.ticks.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools::fake::FakeTools;
    use super::*;
    use dyno_core::metrics::{CpuSample, DiskIo, DiskSample, MemoryPressure, MemorySample, NetworkSample};
    use dyno_core::time::Timestamp;
    use std::path::Path;

    fn w(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn base_sample() -> SystemSample {
        SystemSample {
            at: Timestamp(0),
            cpu: CpuSample {
                total: 1.0,
                per_core: vec![1.0, 1.0],
                frequency_mhz: 0,
                base_frequency_mhz: 0,
            },
            memory: MemorySample {
                total: 16 << 30,
                used: 8 << 30,
                available: 8 << 30,
                commit: Reading::unavailable("Commit charge is a Windows measure"),
                swap_used: 0,
                swap_total: 0,
                pressure: Reading::unavailable("Not measured"),
            },
            disks: vec![],
            disk_io: DiskIo {
                read_bps: 0.0,
                write_bps: 0.0,
                queue: Reading::unavailable("x"),
                busy: Reading::unavailable("x"),
            },
            network: vec![],
            gpu: Reading::unavailable("x"),
            battery: Reading::unavailable("No battery"),
            temperatures: Reading::unavailable("none"),
            fans: Reading::unavailable("none"),
            uptime_secs: 1,
        }
    }

    fn probe(root: &Path) -> LinuxProbe {
        LinuxProbe::new(HostPaths::under(root), Arc::new(FakeTools::default()))
    }

    #[test]
    fn pressure_from_psi_and_free_memory() {
        let d = tempfile::tempdir().unwrap();
        let mut p = probe(d.path());
        let mut s = base_sample();
        p.enrich(&mut s);
        assert_eq!(s.memory.pressure, Reading::unavailable(NO_PSI));
        w(
            d.path(),
            "proc/pressure/memory",
            "some avg10=0.50 avg60=0 avg300=0 total=1\nfull avg10=0 avg60=0 avg300=0 total=0\n",
        );
        let mut s = base_sample();
        p.enrich(&mut s);
        assert_eq!(s.memory.pressure, Reading::Available(MemoryPressure::Normal));
        w(d.path(), "proc/pressure/memory", "some avg10=25.00 avg60=0 avg300=0 total=1\n");
        let mut s = base_sample();
        p.enrich(&mut s);
        assert_eq!(s.memory.pressure, Reading::Available(MemoryPressure::Critical));
    }

    #[test]
    fn a_cgroup_limit_replaces_the_hosts_total() {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "proc/meminfo", "MemTotal: 16777216 kB\nMemAvailable: 8388608 kB\n");
        w(d.path(), "sys/fs/cgroup/memory.max", "1073741824\n");
        w(d.path(), "sys/fs/cgroup/memory.current", "536870912\n");
        w(d.path(), "sys/fs/cgroup/memory.stat", "anon 1\ninactive_file 134217728\n");
        w(
            d.path(),
            "sys/fs/cgroup/memory.pressure",
            "some avg10=30.00 avg60=0 avg300=0 total=1\n",
        );
        w(d.path(), "proc/pressure/memory", "some avg10=0.00 avg60=0 avg300=0 total=1\n");
        let mut s = base_sample();
        probe(d.path()).enrich(&mut s);
        assert_eq!(s.memory.total, 1 << 30);
        assert_eq!(s.memory.used, (512 - 128) << 20);
        assert_eq!(s.memory.available, (1024 - 384) << 20);
        assert_eq!(
            s.memory.pressure,
            Reading::Available(MemoryPressure::Critical),
            "the cgroup's own pressure counts"
        );
        // No limit ("max"): the host's figures stay.
        w(d.path(), "sys/fs/cgroup/memory.max", "max\n");
        let mut s = base_sample();
        probe(d.path()).enrich(&mut s);
        assert_eq!(s.memory.total, 16 << 30);
        assert_eq!(s.memory.pressure, Reading::Available(MemoryPressure::Normal));
        // A limit above the host's memory is no limit.
        w(d.path(), "sys/fs/cgroup/memory.max", "99999999999\n");
        let mut s = base_sample();
        probe(d.path()).enrich(&mut s);
        assert_eq!(s.memory.total, 16 << 30);
    }

    #[test]
    fn cgroup_v1_and_own_path() {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "sys/fs/cgroup/memory/memory.limit_in_bytes", "268435456\n");
        w(d.path(), "sys/fs/cgroup/memory/memory.usage_in_bytes", "134217728\n");
        let mut s = base_sample();
        probe(d.path()).enrich(&mut s);
        assert_eq!((s.memory.total, s.memory.used), (256 << 20, 128 << 20));
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "proc/self/cgroup", "0::/system.slice/dyno.service\n");
        w(d.path(), "sys/fs/cgroup/system.slice/dyno.service/memory.max", "2147483648\n");
        w(d.path(), "sys/fs/cgroup/system.slice/dyno.service/memory.current", "1048576\n");
        let mut s = base_sample();
        probe(d.path()).enrich(&mut s);
        assert_eq!(s.memory.total, 2 << 30);
    }

    #[test]
    fn disk_queue_and_busy_come_from_two_samples() {
        let d = tempfile::tempdir().unwrap();
        let mut p = probe(d.path());
        let mut s = base_sample();
        p.enrich(&mut s);
        assert!(matches!(&s.disk_io.queue, Reading::Unavailable(m) if m.contains("diskstats")));
        w(
            d.path(),
            "proc/diskstats",
            "8 0 sda 1 0 1 1 1 0 1 1 0 100 100 0 0 0 0\n8 1 sda1 1 0 1 1 1 0 1 1 0 100 100 0 0 0 0\n",
        );
        let mut s = base_sample();
        p.enrich(&mut s);
        assert!(
            matches!(&s.disk_io.queue, Reading::Unavailable(m) if m.contains("Measuring")),
            "{:?}",
            s.disk_io
        );
        std::thread::sleep(std::time::Duration::from_millis(60));
        w(d.path(), "proc/diskstats", "8 0 sda 1 0 1 1 1 0 1 1 0 130 160 0 0 0 0\n");
        let mut s = base_sample();
        p.enrich(&mut s);
        let busy = *s.disk_io.busy.value().unwrap();
        let queue = *s.disk_io.queue.value().unwrap();
        assert!(busy > 0.0 && busy <= 100.0 && queue > 0.0, "{busy} {queue}");
        w(d.path(), "proc/diskstats", "7 0 loop0 1 0 1 1 1 0 1 1 0 1 1 0 0 0 0\n");
        let mut s = base_sample();
        p.enrich(&mut s);
        assert!(matches!(&s.disk_io.queue, Reading::Unavailable(m) if m.contains("No physical disk")));
    }

    #[test]
    fn volumes_and_interfaces_are_cleaned() {
        let d = tempfile::tempdir().unwrap();
        let disk = |name: &str, mount: &str, total: u64| DiskSample {
            id: mount.into(),
            name: name.into(),
            mount: mount.into(),
            total,
            free: 1,
            removable: false,
        };
        let mut s = base_sample();
        s.disks = vec![
            disk("/dev/nvme0n1p2", "/", 100),
            disk("/dev/nvme0n1p2", "/home", 100),
            disk("/dev/nvme0n1p2", "/var/lib/docker/btrfs", 100),
            disk("tmpfs", "/tmp", 5),
            disk("/dev/loop2", "/snap/core/1", 5),
            disk("/dev/sdb1", "/mnt/usb", 50),
            disk("/dev/vda1", "/etc/hosts", 100),
        ];
        let net = |n: &str| NetworkSample {
            name: n.into(),
            rx_bps: 1.0,
            tx_bps: 1.0,
        };
        s.network = vec![net("eth0"), net("lo"), net("docker0"), net("veth1"), net("wlan0")];
        probe(d.path()).enrich(&mut s);
        let mounts: Vec<&str> = s.disks.iter().map(|d| d.mount.as_str()).collect();
        assert_eq!(mounts, ["/", "/mnt/usb"]);
        let nets: Vec<&str> = s.network.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(nets, ["eth0", "wlan0"]);
        assert_eq!(s.network_totals(), (2.0, 2.0));
    }

    #[test]
    fn frequency_hardware_and_gpu_are_filled_and_cached() {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq", "2000000\n");
        w(d.path(), "sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq", "3000000\n");
        w(d.path(), "sys/devices/system/cpu/cpu0/cpufreq/base_frequency", "1800000\n");
        w(d.path(), "sys/class/hwmon/hwmon0/name", "coretemp\n");
        w(d.path(), "sys/class/hwmon/hwmon0/temp1_input", "50000\n");
        w(d.path(), "sys/class/drm/card0/device/gpu_busy_percent", "40\n");
        let mut p = probe(d.path());
        let mut s = base_sample();
        p.enrich(&mut s);
        assert_eq!((s.cpu.frequency_mhz, s.cpu.base_frequency_mhz), (2500, 1800));
        assert_eq!(s.temperatures.value().map(|t| t[0].celsius), Some(50.0));
        assert_eq!(s.fans, Reading::unavailable(NO_FAN_FILES));
        assert_eq!(s.gpu.value().map(|g| g.utilization), Some(40.0));
        // Between slow reads the cached values are served.
        w(d.path(), "sys/class/hwmon/hwmon0/temp1_input", "90000\n");
        let mut s = base_sample();
        s.cpu.frequency_mhz = 1234;
        p.enrich(&mut s);
        assert_eq!(s.cpu.frequency_mhz, 1234, "sysinfo's frequency is kept");
        assert_eq!(s.temperatures.value().map(|t| t[0].celsius), Some(50.0));
        // Nothing found: honest reasons, and sysinfo's temperatures survive.
        let e = tempfile::tempdir().unwrap();
        let mut s = base_sample();
        probe(e.path()).enrich(&mut s);
        assert_eq!(s.temperatures, Reading::unavailable(NO_TEMPERATURE_FILES));
        assert_eq!(s.gpu, Reading::unavailable(super::super::gpu::NO_GPU));
        let mut s = base_sample();
        s.temperatures = Reading::Available(vec![Temperature {
            label: "sysinfo".into(),
            celsius: 33.0,
            critical: None,
        }]);
        probe(e.path()).enrich(&mut s);
        assert_eq!(s.temperatures.value().map(|t| t[0].label.as_str()), Some("sysinfo"));
    }

    #[test]
    fn reads_this_machine_without_panicking() {
        let mut s = base_sample();
        let mut p = LinuxProbe::new(HostPaths::host(), Arc::new(super::super::tools::SystemTools));
        p.enrich(&mut s);
        p.enrich(&mut s);
        assert!(s.memory.total > 0);
    }
}
