//! System sensors common to every OS, through `sysinfo`: CPU, memory, swap, volumes, disk and network rates, uptime
//! and temperatures where sysinfo finds sensors. An OS-specific [`SensorProbe`] adds what only that OS can tell
//! (Windows performance counters, macOS memory pressure).

use crate::battery::BatteryReader;
use dyno_core::metrics::{derive_pressure, CpuSample, DiskIo, DiskSample, MemorySample, NetworkSample, Reading, SystemSample, Temperature};
use dyno_core::ports::SystemSensors;
use dyno_core::time::Timestamp;
use std::time::Instant;
use sysinfo::{Components, CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, System};

/// Adds OS-specific readings to a sample that the common sensors filled in.
pub trait SensorProbe: Send {
    /// Fills in or overrides readings in `sample`.
    fn enrich(&mut self, sample: &mut SystemSample);
}

/// A probe that adds nothing.
pub struct NoProbe;

impl SensorProbe for NoProbe {
    fn enrich(&mut self, _sample: &mut SystemSample) {}
}

/// Splits RAM into (used, available). On macOS sysinfo's available memory counts active, inactive and free pages, so
/// it treats memory that apps are using as free and "used" came out as only wired plus compressed (17 % where Activity
/// Monitor said 54 %); there "used" is sysinfo's used memory (app memory, wired and compressed, as Activity Monitor
/// shows) and available is what is left. Elsewhere available is the OS' own figure (`MemAvailable` on Linux, available
/// physical memory on Windows) and used is the rest.
pub(crate) fn split_memory(macos: bool, total: u64, sys_available: u64, sys_used: u64) -> (u64, u64) {
    if macos {
        let used = sys_used.min(total);
        (used, total - used)
    } else {
        let available = sys_available.min(total);
        (total - available, available)
    }
}

/// Volumes are re-listed (new USB drives) every this many ticks; between, only their space and I/O are refreshed.
const RELIST_DISKS_EVERY: u64 = 5;
/// Volume free space is refreshed at most this often.
const STORAGE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);
/// Temperatures are read every this many ticks (on Windows each read is a WMI query).
const TEMPERATURES_EVERY: u64 = 5;
/// The battery is read every this many ticks.
const BATTERY_EVERY: u64 = 10;
/// When no temperature sensor was found, look again after this many ticks.
const RETRY_COMPONENTS_EVERY: u64 = 300;

/// The common sensors.
pub struct SysinfoSensors {
    sys: System,
    disks: Disks,
    networks: Networks,
    components: Option<Components>,
    battery: BatteryReader,
    probe: Box<dyn SensorProbe>,
    last: Option<Instant>,
    ticks: u64,
    last_storage: Option<Instant>,
    storage_refreshes: u64,
    temperatures: Reading<Vec<Temperature>>,
}

/// The reason given when no temperature sensor is visible.
pub const NO_TEMPERATURES: &str = "This computer reports no temperature sensors";
/// The reason given for fans (sysinfo reads none).
pub const NO_FANS: &str = "This computer reports no fan speeds";

impl SysinfoSensors {
    /// Sensors with an OS probe.
    pub fn new(probe: Box<dyn SensorProbe>) -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_list(CpuRefreshKind::nothing());
        SysinfoSensors {
            sys,
            disks: Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage().with_io_usage()),
            networks: Networks::new_with_refreshed_list(),
            components: None,
            battery: BatteryReader::new(BATTERY_EVERY),
            probe,
            last: None,
            ticks: 0,
            last_storage: None,
            storage_refreshes: 0,
            temperatures: Reading::unavailable(NO_TEMPERATURES),
        }
    }

    fn cpu(&mut self) -> CpuSample {
        self.sys
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
        let cpus = self.sys.cpus();
        let per_core: Vec<f32> = cpus.iter().map(|c| clamp_pct(c.cpu_usage())).collect();
        let freq = if cpus.is_empty() {
            0
        } else {
            cpus.iter().map(|c| c.frequency()).sum::<u64>() / cpus.len() as u64
        };
        CpuSample {
            total: clamp_pct(self.sys.global_cpu_usage()),
            per_core,
            frequency_mhz: u32::try_from(freq).unwrap_or(0),
            base_frequency_mhz: 0,
            core_kinds: Vec::new(),
            core_frequency_mhz: cpus.iter().map(|c| u32::try_from(c.frequency()).unwrap_or(0)).collect(),
        }
    }

    fn memory(&mut self) -> MemorySample {
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram().with_swap());
        let total = self.sys.total_memory();
        let (used, available) = split_memory(
            cfg!(target_os = "macos"),
            total,
            self.sys.available_memory(),
            self.sys.used_memory(),
        );
        let mut m = MemorySample {
            total,
            used,
            available,
            commit: Reading::unavailable("Commit charge is a Windows measure"),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            pressure: Reading::unavailable("Not measured"),
        };
        m.pressure = Reading::Available(derive_pressure(&m));
        m
    }

    fn disks(&mut self, secs: f64) -> (Vec<DiskSample>, DiskIo) {
        // Free space is polled rarely: calling the OS' volume-space API often can stall Finder on macOS (an iStat Menus
        // lesson). I/O counters are cheap and refresh every tick.
        let storage_due = self.last_storage.is_none_or(|t| t.elapsed() >= STORAGE_EVERY);
        let mut kind = DiskRefreshKind::nothing().with_io_usage();
        if storage_due {
            kind = kind.with_storage();
            self.last_storage = Some(Instant::now());
        }
        // New volumes (a USB drive) are looked for on every fifth space refresh.
        let relist = storage_due && self.storage_refreshes.is_multiple_of(RELIST_DISKS_EVERY);
        if storage_due {
            self.storage_refreshes += 1;
        }
        self.disks.refresh_specifics(relist, kind);
        let (mut read, mut written) = (0u64, 0u64);
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for d in self.disks.list() {
            let mount = d.mount_point().to_string_lossy().into_owned();
            let usage = d.usage();
            read = read.saturating_add(usage.read_bytes);
            written = written.saturating_add(usage.written_bytes);
            if seen.contains(&mount) {
                continue;
            }
            seen.push(mount.clone());
            let name = volume_name(&mount, &d.name().to_string_lossy());
            out.push(DiskSample {
                id: mount.clone(),
                name,
                mount,
                total: d.total_space(),
                free: d.available_space().min(d.total_space()),
                removable: d.is_removable(),
            });
        }
        let io = DiskIo {
            read_bps: rate(read, secs),
            write_bps: rate(written, secs),
            queue: Reading::unavailable("This OS reports no disk queue length"),
            busy: Reading::unavailable("This OS reports no disk busy time"),
        };
        (out, io)
    }

    fn network(&mut self, secs: f64) -> Vec<NetworkSample> {
        self.networks.refresh(true);
        let mut out: Vec<NetworkSample> = self
            .networks
            .list()
            .iter()
            .filter(|(_, n)| n.total_received() > 0 || n.total_transmitted() > 0)
            .map(|(name, n)| NetworkSample {
                name: name.clone(),
                rx_bps: rate(n.received(), secs),
                tx_bps: rate(n.transmitted(), secs),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn temperatures(&mut self) -> Reading<Vec<Temperature>> {
        let due = self.ticks.is_multiple_of(TEMPERATURES_EVERY);
        let retry = self.ticks.is_multiple_of(RETRY_COMPONENTS_EVERY);
        if !due {
            return self.temperatures.clone();
        }
        match &mut self.components {
            Some(c) => c.refresh(false),
            None if retry => self.components = Some(Components::new_with_refreshed_list()),
            None => {}
        }
        let temps: Vec<Temperature> = self
            .components
            .iter()
            .flat_map(|c| c.list())
            .filter_map(|c| {
                let t = c.temperature().filter(|t| t.is_finite() && *t > -50.0 && *t < 200.0)?;
                Some(Temperature {
                    label: c.label().to_string(),
                    celsius: t,
                    critical: c.critical().filter(|v| v.is_finite() && *v > 0.0),
                })
            })
            .collect();
        if temps.is_empty() {
            if retry {
                // Nothing found: drop the (possibly WMI-backed) list until the next retry.
                self.components = None;
            }
            self.temperatures = Reading::unavailable(NO_TEMPERATURES);
        } else {
            self.temperatures = Reading::Available(temps);
        }
        self.temperatures.clone()
    }
}

impl SystemSensors for SysinfoSensors {
    fn sample(&mut self, now: Timestamp) -> SystemSample {
        let instant = Instant::now();
        let secs = self.last.map(|l| instant.duration_since(l).as_secs_f64()).unwrap_or(0.0);
        self.last = Some(instant);
        let cpu = self.cpu();
        let memory = self.memory();
        let (disks, disk_io) = self.disks(secs);
        let network = self.network(secs);
        let temperatures = self.temperatures();
        let mut sample = SystemSample {
            at: now,
            cpu,
            memory,
            disks,
            disk_io,
            network,
            gpu: Reading::unavailable("This OS reports no GPU load"),
            battery: self.battery.read(),
            temperatures,
            fans: Reading::unavailable(NO_FANS),
            uptime_secs: System::uptime(),
        };
        self.probe.enrich(&mut sample);
        self.ticks = self.ticks.wrapping_add(1);
        sample
    }
}

/// Bytes over `secs` as a rate (0 for the first sample).
pub(crate) fn rate(bytes: u64, secs: f64) -> f64 {
    if secs > 0.0 {
        bytes as f64 / secs
    } else {
        0.0
    }
}

/// A percentage kept in 0–100 (NaN → 0).
pub(crate) fn clamp_pct(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// A display name for a volume: "C:" for a Windows drive, else the volume label or the mount point.
fn volume_name(mount: &str, label: &str) -> String {
    let b = mount.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return mount[..2].to_ascii_uppercase();
    }
    if !label.is_empty() {
        return label.to_string();
    }
    mount.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(rate(1000, 2.0), 500.0);
        assert_eq!(rate(1000, 0.0), 0.0);
        assert_eq!(clamp_pct(f32::NAN), 0.0);
        assert_eq!(clamp_pct(150.0), 100.0);
        assert_eq!(clamp_pct(-1.0), 0.0);
        assert_eq!(volume_name("c:\\", "Windows"), "C:");
        assert_eq!(volume_name("/", "Macintosh HD"), "Macintosh HD");
        assert_eq!(volume_name("/mnt/x", ""), "/mnt/x");
    }

    struct Mark;
    impl SensorProbe for Mark {
        fn enrich(&mut self, sample: &mut SystemSample) {
            sample.fans = Reading::unavailable("probe ran");
        }
    }

    #[test]
    fn macos_used_is_app_wired_and_compressed_not_total_minus_active_free() {
        const GIB: u64 = 1 << 30;
        // The reported case: 64 GiB, sysinfo "available" 53 GiB (active + inactive + free), used memory 34.8 GiB.
        let (used, available) = split_memory(true, 64 * GIB, 53 * GIB, 35 * GIB);
        assert_eq!((used, available), (35 * GIB, 29 * GIB));
        // Other OSs keep total minus the OS' available figure, and nothing exceeds the total.
        assert_eq!(split_memory(false, 64 * GIB, 53 * GIB, 35 * GIB), (11 * GIB, 53 * GIB));
        assert_eq!(split_memory(true, 8 * GIB, 0, 9 * GIB), (8 * GIB, 0));
        assert_eq!(split_memory(false, 8 * GIB, 9 * GIB, 0), (0, 8 * GIB));
    }

    #[test]
    fn samples_this_machine() {
        let mut s = SysinfoSensors::new(Box::new(Mark));
        let first = s.sample(Timestamp(1));
        std::thread::sleep(std::time::Duration::from_millis(250));
        let second = s.sample(Timestamp(2));
        assert_eq!(second.at, Timestamp(2));
        assert!(!second.cpu.per_core.is_empty());
        assert!(second.cpu.total >= 0.0 && second.cpu.total <= 100.0);
        assert!(second.memory.total > 0 && second.memory.used <= second.memory.total);
        assert!(second.memory.pressure.value().is_some());
        assert!(!second.disks.is_empty(), "no volumes found");
        assert!(second.disks.iter().all(|d| d.free <= d.total));
        assert_eq!(first.disk_io.read_bps, 0.0);
        assert_eq!(second.fans, Reading::unavailable("probe ran"));
        assert!(second.uptime_secs > 0);
        let mut quiet = SysinfoSensors::new(Box::new(NoProbe));
        assert_eq!(quiet.sample(Timestamp(0)).fans, Reading::unavailable(NO_FANS));
    }
}
