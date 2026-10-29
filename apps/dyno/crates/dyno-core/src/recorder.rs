//! The flight recorder's pure parts: the last ten minutes of the machine at one-second resolution in a hard-bounded
//! ring, the snapshot document saved when something goes wrong, and the policy that decides when the sampler may tick
//! at one second just for the recorder. The agent feeds the ring and stores the snapshots (ADR 0002: Dyno stays near
//! 1 % CPU, so the recorder backs off whenever that budget is at risk).

use crate::grouping::AppGroup;
use crate::metrics::SystemSample;
use crate::sampling::CPU_BUDGET;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// The snapshot document's schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// How much history the ring keeps, milliseconds.
pub const WINDOW_MS: i64 = 10 * 60 * 1_000;
/// The recorder's resolution, milliseconds.
pub const RESOLUTION_MS: i64 = 1_000;
/// Hard cap on samples in the ring (the window plus slack for jitter).
pub const MAX_SAMPLES: usize = 660;
/// Hard cap on the ring's estimated memory, bytes.
pub const MAX_RING_BYTES: usize = 2 * 1024 * 1024;
/// Top processes kept per sample.
pub const TOP_PROCESSES: usize = 5;
/// Per-core values kept per sample.
pub const MAX_CORES: usize = 256;
/// Longest process name kept, in characters.
pub const MAX_NAME: usize = 48;
/// Largest snapshot document, bytes; bigger captures are thinned (every second sample dropped) until they fit.
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
/// Most snapshots kept in the store.
pub const MAX_SNAPSHOTS: usize = 20;
/// Automatic captures are at least this far apart, milliseconds.
pub const MIN_AUTO_GAP_MS: i64 = 2 * 60 * 1_000;

/// One busy process at a moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderProcess {
    /// App name.
    pub name: String,
    /// CPU, percent of the machine.
    pub cpu: f32,
    /// Memory in bytes.
    pub memory: u64,
}

/// One second of the machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderSample {
    /// When.
    pub at: Timestamp,
    /// Total CPU, 0-100.
    pub cpu_total: f32,
    /// CPU per logical core, 0-100.
    pub cpu_cores: Vec<f32>,
    /// Memory in use, bytes.
    pub memory_used: u64,
    /// Installed memory, bytes.
    pub memory_total: u64,
    /// Disk read, bytes per second.
    pub disk_read_bps: f64,
    /// Disk write, bytes per second.
    pub disk_write_bps: f64,
    /// Network received, bytes per second, all interfaces.
    pub net_rx_bps: f64,
    /// Network sent, bytes per second, all interfaces.
    pub net_tx_bps: f64,
    /// The busiest apps by CPU as of the last process table.
    pub top: Vec<RecorderProcess>,
}

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

impl RecorderSample {
    /// The sample for `s`, with the busiest of `groups` (the latest process table; empty when none was taken yet).
    pub fn from_system(s: &SystemSample, groups: &[AppGroup]) -> RecorderSample {
        let mut top: Vec<&AppGroup> = groups.iter().collect();
        top.sort_by(|a, b| b.cpu_read().total_cmp(&a.cpu_read()));
        RecorderSample {
            at: s.at,
            cpu_total: s.cpu.total,
            cpu_cores: s.cpu.per_core.iter().take(MAX_CORES).copied().collect(),
            memory_used: s.memory.used,
            memory_total: s.memory.total,
            disk_read_bps: finite(s.disk_io.read_bps),
            disk_write_bps: finite(s.disk_io.write_bps),
            net_rx_bps: s.network.iter().map(|n| finite(n.rx_bps)).sum(),
            net_tx_bps: s.network.iter().map(|n| finite(n.tx_bps)).sum(),
            top: top
                .into_iter()
                .take(TOP_PROCESSES)
                .map(|g| RecorderProcess {
                    name: g.name.chars().take(MAX_NAME).collect(),
                    cpu: g.cpu_read(),
                    memory: g.memory_read(),
                })
                .collect(),
        }
    }

    /// An estimate of the sample's memory, bytes (used for the ring's hard bound).
    pub fn estimated_bytes(&self) -> usize {
        96 + self.cpu_cores.len() * 4 + self.top.iter().map(|p| 40 + p.name.len()).sum::<usize>()
    }
}

/// The ring of recent samples. Pushing evicts what is too old, too many or too big, so memory never grows.
#[derive(Debug, Clone, Default)]
pub struct Ring {
    samples: VecDeque<RecorderSample>,
    bytes: usize,
}

impl Ring {
    /// An empty ring.
    pub fn new() -> Ring {
        Ring::default()
    }

    /// Adds a sample. One closer than half a second to the previous one, or older than it, is ignored.
    pub fn push(&mut self, sample: RecorderSample) {
        if let Some(last) = self.samples.back() {
            if sample.at.since(last.at) < RESOLUTION_MS / 2 {
                return;
            }
        }
        self.bytes += sample.estimated_bytes();
        let now = sample.at;
        self.samples.push_back(sample);
        while let Some(front) = self.samples.front() {
            let too_old = now.since(front.at) > WINDOW_MS;
            if too_old || self.samples.len() > MAX_SAMPLES || self.bytes > MAX_RING_BYTES {
                self.bytes = self.bytes.saturating_sub(front.estimated_bytes());
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// How many samples are held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The estimated memory held, bytes.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The samples, oldest first.
    pub fn samples(&self) -> Vec<RecorderSample> {
        self.samples.iter().cloned().collect()
    }
}

/// Why a snapshot was taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CaptureReason {
    /// The user asked.
    Manual,
    /// An alarm fired.
    Alarm {
        /// The alarm.
        alarm_id: u64,
        /// Its title.
        title: String,
    },
}

impl CaptureReason {
    /// A short label for lists.
    pub fn label(&self) -> String {
        match self {
            CaptureReason::Manual => "Captured by you".into(),
            CaptureReason::Alarm { title, .. } => format!("Alarm: {title}"),
        }
    }
}

/// The saved document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderSnapshot {
    /// Schema version.
    pub version: u32,
    /// When it was taken.
    pub at: Timestamp,
    /// Why.
    pub reason: CaptureReason,
    /// The samples, oldest first.
    pub samples: Vec<RecorderSample>,
}

/// A snapshot in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderSnapshotInfo {
    /// Id.
    pub id: u64,
    /// When it was taken.
    pub at: Timestamp,
    /// Why.
    pub reason: CaptureReason,
    /// Size of the stored document, bytes.
    pub bytes: u32,
    /// Samples inside.
    pub samples: u32,
}

impl RecorderSnapshot {
    /// A snapshot of the ring.
    pub fn new(at: Timestamp, reason: CaptureReason, samples: Vec<RecorderSample>) -> RecorderSnapshot {
        RecorderSnapshot {
            version: SCHEMA_VERSION,
            at,
            reason,
            samples,
        }
    }

    /// The compact JSON document, thinned until it fits [`MAX_SNAPSHOT_BYTES`].
    pub fn to_bounded_json(mut self) -> Result<(String, RecorderSnapshot), serde_json::Error> {
        loop {
            let text = serde_json::to_string(&self)?;
            if text.len() <= MAX_SNAPSHOT_BYTES || self.samples.len() <= 1 {
                return Ok((text, self));
            }
            self.samples = self.samples.into_iter().step_by(2).collect();
        }
    }

    /// Reads a stored document; `None` for damage or a newer version.
    pub fn parse(text: &str) -> Option<RecorderSnapshot> {
        if text.len() > MAX_SNAPSHOT_BYTES * 2 {
            return None;
        }
        serde_json::from_str::<RecorderSnapshot>(text)
            .ok()
            .filter(|s| s.version <= SCHEMA_VERSION)
    }
}

/// The system sampling interval the recorder wants: one second while it is enabled and Dyno is comfortably inside
/// its CPU budget and the machine is on mains power, otherwise whatever the sampler would do anyway. It only ever
/// speeds the system sample up (the process table keeps its own, slower interval).
pub fn system_interval_ms(base_ms: u32, enabled: bool, on_battery: bool, own_cpu: f32) -> u32 {
    let cheap = enabled && !on_battery && own_cpu.is_finite() && own_cpu <= CPU_BUDGET * 0.5;
    if cheap {
        base_ms.min(RESOLUTION_MS as u32)
    } else {
        base_ms
    }
}

/// Whether an automatic capture is allowed: enough time has passed since the last one.
pub fn auto_capture_allowed(last: Option<Timestamp>, now: Timestamp) -> bool {
    last.is_none_or(|l| now.since(l) >= MIN_AUTO_GAP_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixtures::sample;
    use crate::metrics::NetworkSample;

    fn group(name: &str, cpu: f32) -> AppGroup {
        AppGroup {
            id: format!("exe:{name}"),
            name: name.into(),
            exe: None,
            pids: vec![1],
            cpu: crate::process::Measured::Value(cpu),
            memory: crate::process::Measured::Value(1 << 20),
            disk_bps: crate::process::Measured::Value(0.0),
            partial: false,
        }
    }

    fn at(ms: i64) -> RecorderSample {
        let mut s = sample(ms);
        s.at = Timestamp(ms);
        RecorderSample::from_system(&s, &[])
    }

    #[test]
    fn a_sample_keeps_the_busiest_apps_and_sums_the_network() {
        let mut s = sample(5);
        s.network = vec![
            NetworkSample {
                name: "a".into(),
                rx_bps: 10.0,
                tx_bps: 1.0,
            },
            NetworkSample {
                name: "b".into(),
                rx_bps: f64::NAN,
                tx_bps: -4.0,
            },
        ];
        s.disk_io.read_bps = f64::INFINITY;
        let groups: Vec<AppGroup> = (0..8).map(|i| group(&format!("{}{i}", "n".repeat(80)), i as f32)).collect();
        let r = RecorderSample::from_system(&s, &groups);
        assert_eq!((r.net_rx_bps, r.net_tx_bps, r.disk_read_bps), (10.0, 1.0, 0.0));
        assert_eq!(r.top.len(), TOP_PROCESSES);
        assert_eq!(r.top[0].cpu, 7.0);
        assert_eq!(r.top[0].name.chars().count(), MAX_NAME);
        assert_eq!(r.cpu_cores.len(), s.cpu.per_core.len());
        assert!(r.estimated_bytes() > 96);
    }

    #[test]
    fn the_ring_keeps_ten_minutes_at_one_second_and_never_more() {
        let mut ring = Ring::new();
        assert!(ring.is_empty());
        for i in 0..2_000 {
            ring.push(at(i * 1_000));
        }
        assert!(ring.len() <= MAX_SAMPLES);
        assert_eq!(ring.len(), 601, "ten minutes plus the current second");
        let s = ring.samples();
        assert_eq!(s.last().unwrap().at, Timestamp(1_999_000));
        assert_eq!(s[0].at, Timestamp(1_999_000 - WINDOW_MS));
        assert!(ring.bytes() > 0 && ring.bytes() < MAX_RING_BYTES);
        // Samples closer than half a second, or going backwards, are ignored.
        let n = ring.len();
        ring.push(at(1_999_200));
        ring.push(at(5));
        assert_eq!(ring.len(), n);
    }

    #[test]
    fn the_ring_has_a_hard_memory_bound() {
        let mut ring = Ring::new();
        for i in 0..MAX_SAMPLES as i64 {
            let mut s = at(i * 1_000);
            s.cpu_cores = vec![1.0; 100_000];
            ring.push(s);
        }
        assert!(ring.bytes() <= MAX_RING_BYTES + 400_000, "bounded: {}", ring.bytes());
        assert!(ring.len() < 10);
    }

    #[test]
    fn snapshots_are_bounded_and_round_trip() {
        let samples: Vec<RecorderSample> = (0..600)
            .map(|i| {
                let mut s = at(i * 1_000);
                s.cpu_cores = vec![12.345678; 250];
                s
            })
            .collect();
        let snap = RecorderSnapshot::new(
            Timestamp(9),
            CaptureReason::Alarm {
                alarm_id: 3,
                title: "Disk".into(),
            },
            samples,
        );
        let (text, kept) = snap.clone().to_bounded_json().unwrap();
        assert!(text.len() <= MAX_SNAPSHOT_BYTES);
        assert!(kept.samples.len() < 600 && kept.samples.len() > 100);
        assert_eq!(RecorderSnapshot::parse(&text).unwrap(), kept);
        let small = RecorderSnapshot::new(Timestamp(1), CaptureReason::Manual, vec![at(0)]);
        let (t, k) = small.clone().to_bounded_json().unwrap();
        assert_eq!(k, small);
        assert!(t.contains("\"reason\":\"manual\""), "{t}");
        assert!(RecorderSnapshot::parse("{").is_none());
        assert!(RecorderSnapshot::parse(&t.replace("\"version\":1", "\"version\":9")).is_none());
        assert!(RecorderSnapshot::parse(&" ".repeat(MAX_SNAPSHOT_BYTES * 2 + 1)).is_none());
        assert_eq!(CaptureReason::Manual.label(), "Captured by you");
        assert_eq!(snap.reason.label(), "Alarm: Disk");
        let j = serde_json::to_value(&snap.reason).unwrap();
        assert_eq!(j, serde_json::json!({"alarm": {"alarmId": 3, "title": "Disk"}}));
        let info = RecorderSnapshotInfo {
            id: 1,
            at: Timestamp(2),
            reason: CaptureReason::Manual,
            bytes: 3,
            samples: 4,
        };
        assert_eq!(
            postcard::from_bytes::<RecorderSnapshotInfo>(&postcard::to_stdvec(&info).unwrap()).unwrap(),
            info
        );
    }

    #[test]
    fn the_recorder_only_speeds_up_when_it_is_cheap() {
        assert_eq!(system_interval_ms(2_000, true, false, 0.1), 1_000);
        assert_eq!(system_interval_ms(500, true, false, 0.1), 500, "never slows a faster tick");
        assert_eq!(system_interval_ms(2_000, false, false, 0.1), 2_000, "disabled");
        assert_eq!(system_interval_ms(2_000, true, true, 0.1), 2_000, "battery");
        assert_eq!(system_interval_ms(2_000, true, false, 0.9), 2_000, "over half the budget");
        assert_eq!(system_interval_ms(2_000, true, false, f32::NAN), 2_000);
        assert!(auto_capture_allowed(None, Timestamp(0)));
        assert!(!auto_capture_allowed(Some(Timestamp(0)), Timestamp(MIN_AUTO_GAP_MS - 1)));
        assert!(auto_capture_allowed(Some(Timestamp(0)), Timestamp(MIN_AUTO_GAP_MS)));
    }
}
