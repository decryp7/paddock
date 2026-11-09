//! What the sensors measure: one [`SystemSample`] per tick.
//!
//! Every reading the platform may not provide (GPU, temperatures, fans, disk queue, memory pressure, battery) is a
//! [`Reading`], so "not available on this machine" is a value with a reason, never a zero.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// A measurement that a platform may not provide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reading<T> {
    /// The value.
    Available(T),
    /// Not measurable here, and why (shown to the user as is), e.g. "Needs a vendor driver".
    Unavailable(String),
}

impl<T> Reading<T> {
    /// The value, if available.
    pub fn value(&self) -> Option<&T> {
        match self {
            Reading::Available(v) => Some(v),
            Reading::Unavailable(_) => None,
        }
    }

    /// Why there is no value, if there is none.
    pub fn unavailable_reason(&self) -> Option<&str> {
        match self {
            Reading::Available(_) => None,
            Reading::Unavailable(r) => Some(r),
        }
    }

    /// An unavailable reading with this reason.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Reading::Unavailable(reason.into())
    }

    /// Maps the value.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Reading<U> {
        match self {
            Reading::Available(v) => Reading::Available(f(v)),
            Reading::Unavailable(r) => Reading::Unavailable(r),
        }
    }
}

/// CPU load, 0–100 per core and in total.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuSample {
    /// Average load over all logical cores, 0–100.
    pub total: f32,
    /// Load per logical core, 0–100.
    pub per_core: Vec<f32>,
    /// Current frequency in MHz (average over cores), 0 when unknown.
    pub frequency_mhz: u32,
    /// Base (nominal) frequency in MHz, 0 when unknown. A current frequency far below it under load hints at
    /// thermal or power throttling.
    pub base_frequency_mhz: u32,
    /// Kind of each logical core, same length as `per_core` (or empty when the OS exposes no topology).
    #[serde(default)]
    pub core_kinds: Vec<CoreKind>,
    /// Current frequency of each logical core in MHz, same length as `per_core` (or empty when unknown).
    #[serde(default)]
    pub core_frequency_mhz: Vec<u32>,
}

/// What kind of core a logical core is on a hybrid CPU (Apple silicon P/E clusters, Intel hybrid P/E cores).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CoreKind {
    /// A performance core.
    Performance,
    /// An efficiency core.
    Efficiency,
    /// The OS does not say, or the CPU is uniform.
    #[default]
    Unknown,
}

/// Assigns kinds from counts: `efficiency_first` cores are Efficiency (Apple silicon numbers its E cores first),
/// the rest Performance. Zero of either, or counts that do not add up to `total`, give no topology (empty).
pub fn kinds_from_counts(total: usize, efficiency: usize, performance: usize, efficiency_first: bool) -> Vec<CoreKind> {
    if efficiency == 0 || performance == 0 || efficiency + performance != total {
        return Vec::new();
    }
    let (e, p) = (vec![CoreKind::Efficiency; efficiency], vec![CoreKind::Performance; performance]);
    if efficiency_first {
        [e, p].concat()
    } else {
        [p, e].concat()
    }
}

/// How hard the OS is working to find memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryPressure {
    /// Plenty of memory.
    Normal,
    /// The OS is compressing or paging noticeably.
    Elevated,
    /// The OS is short of memory: apps slow down or get terminated.
    Critical,
}

/// Physical memory, commit and swap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    /// Installed (visible) physical memory in bytes.
    pub total: u64,
    /// In use in bytes.
    pub used: u64,
    /// Available to start new work without paging, in bytes.
    pub available: u64,
    /// Committed bytes and the commit limit (Windows); unavailable elsewhere.
    pub commit: Reading<Commit>,
    /// Swap / page file in use and its size.
    pub swap_used: u64,
    /// Swap / page file size.
    pub swap_total: u64,
    /// The OS's own memory-pressure level where it reports one (macOS), else derived by the core.
    pub pressure: Reading<MemoryPressure>,
}

/// Commit charge: memory promised to processes (RAM + page file).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    /// Committed bytes.
    pub used: u64,
    /// The commit limit in bytes.
    pub limit: u64,
}

/// One volume or physical disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSample {
    /// Stable id (mount point or volume GUID).
    pub id: String,
    /// Display name, e.g. "C:" or "Macintosh HD".
    pub name: String,
    /// Mount point.
    pub mount: String,
    /// Capacity in bytes.
    pub total: u64,
    /// Free bytes.
    pub free: u64,
    /// Whether the volume is removable.
    pub removable: bool,
}

/// Disk activity over all physical disks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskIo {
    /// Bytes read per second.
    pub read_bps: f64,
    /// Bytes written per second.
    pub write_bps: f64,
    /// Average queue length (Windows performance counters); unavailable elsewhere.
    pub queue: Reading<f32>,
    /// Percentage of time the disks were busy, 0–100.
    pub busy: Reading<f32>,
}

/// One network interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSample {
    /// Interface name.
    pub name: String,
    /// Received bytes per second.
    pub rx_bps: f64,
    /// Sent bytes per second.
    pub tx_bps: f64,
}

/// The GPU's load and memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSample {
    /// Adapter name when known.
    pub name: String,
    /// Busiest engine's utilisation, 0–100.
    pub utilization: f32,
    /// Dedicated memory in use in bytes (0 when unknown).
    pub memory_used: u64,
    /// Dedicated memory in bytes (0 when unknown).
    pub memory_total: u64,
}

/// Whether the machine runs on battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerSource {
    /// Plugged in.
    Ac,
    /// On battery.
    Battery,
}

/// The battery's charge and wear.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatterySample {
    /// Charge, 0–100.
    pub charge: f32,
    /// Plugged in or on battery.
    pub source: PowerSource,
    /// Full-charge capacity as a percentage of the design capacity (wear), 0–100+.
    pub health: f32,
    /// Charge cycles, when reported.
    pub cycles: Option<u32>,
    /// Seconds to empty while discharging, when estimated.
    pub seconds_to_empty: Option<u64>,
}

/// One temperature sensor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Temperature {
    /// Sensor label, e.g. "CPU package".
    pub label: String,
    /// Degrees Celsius.
    pub celsius: f32,
    /// The sensor's critical temperature, when reported.
    pub critical: Option<f32>,
}

/// One fan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fan {
    /// Fan label.
    pub label: String,
    /// Revolutions per minute.
    pub rpm: u32,
}

/// Everything the sensors measured at one instant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSample {
    /// When it was taken.
    pub at: Timestamp,
    /// CPU.
    pub cpu: CpuSample,
    /// Memory.
    pub memory: MemorySample,
    /// Volumes with their free space.
    pub disks: Vec<DiskSample>,
    /// Disk throughput and queue.
    pub disk_io: DiskIo,
    /// Network interfaces with traffic.
    pub network: Vec<NetworkSample>,
    /// GPU.
    pub gpu: Reading<GpuSample>,
    /// Battery (unavailable on desktops: "No battery").
    pub battery: Reading<BatterySample>,
    /// Temperatures.
    pub temperatures: Reading<Vec<Temperature>>,
    /// Fans.
    pub fans: Reading<Vec<Fan>>,
    /// Seconds since boot.
    pub uptime_secs: u64,
}

impl SystemSample {
    /// Memory in use as a percentage of the total (0 when the total is unknown).
    pub fn memory_percent(&self) -> f32 {
        percent(self.memory.used, self.memory.total)
    }

    /// Total network traffic in bytes per second (received, sent).
    pub fn network_totals(&self) -> (f64, f64) {
        self.network.iter().fold((0.0, 0.0), |(rx, tx), n| (rx + n.rx_bps, tx + n.tx_bps))
    }

    /// The hottest temperature, if any.
    pub fn hottest(&self) -> Option<&Temperature> {
        self.temperatures.value()?.iter().max_by(|a, b| a.celsius.total_cmp(&b.celsius))
    }
}

/// `part` of `whole` in percent, 0 when `whole` is 0.
pub fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 * 100.0 / whole as f64) as f32
    }
}

/// The memory-pressure level the core derives when the OS reports none: from available memory and commit.
pub fn derive_pressure(memory: &MemorySample) -> MemoryPressure {
    let available = percent(memory.available, memory.total);
    let commit = memory.commit.value().map(|c| percent(c.used, c.limit)).unwrap_or(0.0);
    if memory.total > 0 && (available < 5.0 || commit > 95.0) {
        MemoryPressure::Critical
    } else if memory.total > 0 && (available < 15.0 || commit > 85.0) {
        MemoryPressure::Elevated
    } else {
        MemoryPressure::Normal
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A quiet 8-core machine with 16 GiB.
    pub fn sample(at: i64) -> SystemSample {
        SystemSample {
            at: Timestamp(at),
            cpu: CpuSample {
                total: 10.0,
                per_core: vec![10.0; 8],
                frequency_mhz: 3_000,
                base_frequency_mhz: 3_000,
                ..Default::default()
            },
            memory: MemorySample {
                total: 16 << 30,
                used: 8 << 30,
                available: 8 << 30,
                commit: Reading::Available(Commit {
                    used: 10 << 30,
                    limit: 24 << 30,
                }),
                swap_used: 0,
                swap_total: 8 << 30,
                pressure: Reading::Available(MemoryPressure::Normal),
            },
            disks: vec![DiskSample {
                id: "C:".into(),
                name: "C:".into(),
                mount: "C:\\".into(),
                total: 500 << 30,
                free: 200 << 30,
                removable: false,
            }],
            disk_io: DiskIo {
                read_bps: 1e6,
                write_bps: 1e6,
                queue: Reading::Available(0.1),
                busy: Reading::Available(3.0),
            },
            network: vec![NetworkSample {
                name: "Ethernet".into(),
                rx_bps: 1e5,
                tx_bps: 2e4,
            }],
            gpu: Reading::unavailable("No GPU counters"),
            battery: Reading::unavailable("No battery"),
            temperatures: Reading::unavailable("Needs a vendor driver"),
            fans: Reading::unavailable("Needs a vendor driver"),
            uptime_secs: 3_600,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::sample;
    use super::*;

    #[test]
    fn kinds_follow_counts_or_are_absent() {
        use CoreKind::*;
        assert_eq!(
            kinds_from_counts(6, 2, 4, true),
            vec![Efficiency, Efficiency, Performance, Performance, Performance, Performance]
        );
        assert_eq!(kinds_from_counts(3, 1, 2, false), vec![Performance, Performance, Efficiency]);
        assert!(kinds_from_counts(8, 0, 8, true).is_empty());
        assert!(kinds_from_counts(8, 2, 4, true).is_empty());
    }

    #[test]
    fn readings_carry_values_or_reasons() {
        let r: Reading<u32> = Reading::Available(3);
        assert_eq!(r.value(), Some(&3));
        assert_eq!(r.map(|v| v * 2), Reading::Available(6));
        let u: Reading<u32> = Reading::unavailable("no");
        assert_eq!(u.value(), None);
        assert_eq!(u.map(|v| v + 1), Reading::Unavailable("no".into()));
    }

    #[test]
    fn derived_values() {
        let mut s = sample(0);
        assert_eq!(s.memory_percent(), 50.0);
        assert_eq!(s.network_totals(), (1e5, 2e4));
        assert!(s.hottest().is_none());
        s.temperatures = Reading::Available(vec![
            Temperature {
                label: "a".into(),
                celsius: 50.0,
                critical: None,
            },
            Temperature {
                label: "b".into(),
                celsius: 80.0,
                critical: Some(100.0),
            },
        ]);
        assert_eq!(s.hottest().map(|t| t.label.as_str()), Some("b"));
        assert_eq!(percent(1, 0), 0.0);
    }

    #[test]
    fn pressure_from_available_memory_and_commit() {
        let mut m = sample(0).memory;
        assert_eq!(derive_pressure(&m), MemoryPressure::Normal);
        m.available = 2 << 30;
        assert_eq!(derive_pressure(&m), MemoryPressure::Elevated);
        m.available = 512 << 20;
        assert_eq!(derive_pressure(&m), MemoryPressure::Critical);
        m.available = 8 << 30;
        m.commit = Reading::Available(Commit { used: 97, limit: 100 });
        assert_eq!(derive_pressure(&m), MemoryPressure::Critical);
        m.commit = Reading::Available(Commit { used: 90, limit: 100 });
        assert_eq!(derive_pressure(&m), MemoryPressure::Elevated);
        m.commit = Reading::unavailable("n/a");
        assert_eq!(derive_pressure(&m), MemoryPressure::Normal);
        m.total = 0;
        assert_eq!(derive_pressure(&m), MemoryPressure::Normal);
    }

    #[test]
    fn json_shape_is_camel_case_and_externally_tagged() {
        let j = serde_json::to_value(sample(5)).unwrap();
        assert_eq!(j["cpu"]["perCore"].as_array().map(Vec::len), Some(8));
        assert_eq!(j["gpu"]["unavailable"], "No GPU counters");
        assert_eq!(j["memory"]["pressure"]["available"], "normal");
        let bytes = postcard::to_stdvec(&sample(5)).unwrap();
        let back: SystemSample = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, sample(5));
    }
}
