//! Benchmarks: what is measured, how a run is recorded and how two runs are compared. Pure types and functions; the
//! measuring itself runs out of process (`dyno-bench`) and is started by the agent only when the user clicks.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Largest scratch file the disk benchmark may write, in MiB. The agent refuses a bigger request.
pub const DISK_MAX_MIB: u32 = 512;
/// Scratch file size when the user doesn't choose, in MiB.
pub const DISK_DEFAULT_MIB: u32 = 128;
/// Smallest scratch file, in MiB (smaller numbers only measure the cache).
pub const DISK_MIN_MIB: u32 = 16;
/// Runs kept; the oldest are dropped first.
pub const MAX_RUNS: usize = 50;
/// Longest label or group name, in characters.
pub const MAX_LABEL: usize = 80;
/// A change smaller than this many percent is noise, not better or worse.
pub const NOISE_PERCENT: f64 = 3.0;
/// Background load (CPU percentage points) above which a comparison warns that the runs are not alike.
pub const LOAD_WARNING_POINTS: f32 = 15.0;

/// What a benchmark measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum BenchKind {
    /// Integer work on one core and on all cores.
    Cpu,
    /// Copy bandwidth and random-access latency.
    Memory,
    /// Sequential and random I/O on a capped scratch file.
    Disk,
    /// GPU compute (optional; unavailable where no portable API is built in).
    Gpu,
}

impl BenchKind {
    /// Every kind, in the order the page shows them.
    pub const ALL: [BenchKind; 4] = [BenchKind::Cpu, BenchKind::Memory, BenchKind::Disk, BenchKind::Gpu];

    /// The name used on the command line of `dyno-bench` and in logs.
    pub fn name(self) -> &'static str {
        match self {
            BenchKind::Cpu => "cpu",
            BenchKind::Memory => "memory",
            BenchKind::Disk => "disk",
            BenchKind::Gpu => "gpu",
        }
    }

    /// Parses [`BenchKind::name`].
    pub fn parse(s: &str) -> Option<BenchKind> {
        BenchKind::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// One measured number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchMetric {
    /// Stable id, e.g. `cpu-single`.
    pub id: String,
    /// Plain-language name.
    pub label: String,
    /// The value.
    pub value: f64,
    /// Unit shown after it, e.g. `MB/s`.
    pub unit: String,
    /// Whether a larger value is better (false for latency).
    pub higher_is_better: bool,
}

impl BenchMetric {
    /// A metric where larger is better.
    pub fn higher(id: &str, label: &str, value: f64, unit: &str) -> BenchMetric {
        BenchMetric {
            id: id.into(),
            label: label.into(),
            value,
            unit: unit.into(),
            higher_is_better: true,
        }
    }

    /// A metric where smaller is better (latency).
    pub fn lower(id: &str, label: &str, value: f64, unit: &str) -> BenchMetric {
        BenchMetric {
            higher_is_better: false,
            ..BenchMetric::higher(id, label, value, unit)
        }
    }
}

/// How one kind ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum BenchStatus {
    /// Finished with these numbers.
    Done {
        /// The numbers.
        metrics: Vec<BenchMetric>,
    },
    /// Not possible here, with the reason and what to do.
    Unavailable {
        /// Why.
        reason: String,
    },
    /// Stopped before it finished (the timeout, or the user cancelled).
    Stopped {
        /// Why.
        reason: String,
    },
    /// The measuring process failed.
    Failed {
        /// What went wrong.
        reason: String,
    },
}

/// One kind's result inside a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindResult {
    /// What was measured.
    pub kind: BenchKind,
    /// How it ended.
    pub status: BenchStatus,
    /// How long it took, in milliseconds.
    pub millis: u64,
}

/// The machine's state when the run was started (before the benchmark itself added load).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoadAtRun {
    /// CPU in use, percent.
    pub cpu_percent: f32,
    /// Memory in use, percent.
    pub memory_percent: f32,
    /// Whether the machine ran on battery (`None` when unknown or no battery).
    pub on_battery: Option<bool>,
}

/// Which side of a before/after pair a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BenchPhase {
    /// Measured before a change.
    Before,
    /// Measured after the change.
    After,
}

/// A finished (or stopped) run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchRun {
    /// Unique id.
    pub id: String,
    /// When it started.
    pub started_at: Timestamp,
    /// The user's name for it (or the change it brackets).
    pub label: String,
    /// The change this run brackets, shared by its before and after runs.
    pub group: Option<String>,
    /// Before or after the change.
    pub phase: Option<BenchPhase>,
    /// Load when it started.
    pub load: LoadAtRun,
    /// Operating system and CPU, so runs from different machines are never mixed up.
    pub machine: String,
    /// Results, one per requested kind.
    pub results: Vec<KindResult>,
}

impl BenchRun {
    /// The metrics of every kind that finished.
    pub fn metrics(&self) -> impl Iterator<Item = (BenchKind, &BenchMetric)> {
        self.results
            .iter()
            .filter_map(|r| match &r.status {
                BenchStatus::Done { metrics } => Some(metrics.iter().map(move |m| (r.kind, m))),
                _ => None,
            })
            .flatten()
    }
}

/// What a request to start a run may ask, after parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// The kinds, in order, without repeats.
    pub kinds: Vec<BenchKind>,
    /// Scratch file size, MiB.
    pub disk_mib: u32,
    /// Label, trimmed.
    pub label: String,
    /// Group, trimmed.
    pub group: Option<String>,
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_LABEL)
        .collect()
}

/// Validates a start request: at least one kind, a bounded scratch file, short printable names.
pub fn plan(kinds: &[BenchKind], disk_mib: Option<u32>, label: &str, group: Option<&str>) -> Result<RunPlan, String> {
    let mut unique: Vec<BenchKind> = Vec::new();
    for k in kinds {
        if !unique.contains(k) {
            unique.push(*k);
        }
    }
    if unique.is_empty() {
        return Err("Choose at least one benchmark to run.".into());
    }
    let disk_mib = disk_mib.unwrap_or(DISK_DEFAULT_MIB);
    if !(DISK_MIN_MIB..=DISK_MAX_MIB).contains(&disk_mib) {
        return Err(format!("The disk test file must be between {DISK_MIN_MIB} and {DISK_MAX_MIB} MiB."));
    }
    let label = clean(label);
    let group = group.map(clean).filter(|g| !g.is_empty());
    Ok(RunPlan {
        kinds: unique,
        disk_mib,
        label: if label.is_empty() { "Benchmark".into() } else { label },
        group,
    })
}

/// Whether a change counts as an improvement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Better by more than the noise.
    Better,
    /// Worse by more than the noise.
    Worse,
    /// Within the noise.
    Same,
}

/// One metric in both runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricDelta {
    /// The kind it belongs to.
    pub kind: BenchKind,
    /// The metric id.
    pub id: String,
    /// Plain-language name.
    pub label: String,
    /// Unit.
    pub unit: String,
    /// Whether larger is better.
    pub higher_is_better: bool,
    /// Value before.
    pub before: f64,
    /// Value after.
    pub after: f64,
    /// Change in percent of the earlier value (positive = the number grew).
    pub change_percent: f64,
    /// Better, worse or the same.
    pub verdict: Verdict,
}

/// Two runs compared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// Id of the earlier run.
    pub before: String,
    /// Id of the later run.
    pub after: String,
    /// Metrics present in both.
    pub deltas: Vec<MetricDelta>,
    /// Why the comparison may mislead (different machine, different background load), empty when none.
    pub warnings: Vec<String>,
}

/// Compares `after` with `before`: every metric present in both, with a verdict that ignores changes inside
/// [`NOISE_PERCENT`]. Warns when the machine differs or the background load differs by [`LOAD_WARNING_POINTS`].
pub fn compare(before: &BenchRun, after: &BenchRun) -> Comparison {
    let mut deltas = Vec::new();
    for (kind, b) in before.metrics() {
        let Some((_, a)) = after.metrics().find(|(k, m)| *k == kind && m.id == b.id) else {
            continue;
        };
        let change_percent = if b.value.abs() < f64::EPSILON {
            0.0
        } else {
            (a.value - b.value) / b.value * 100.0
        };
        let improved = if b.higher_is_better { change_percent } else { -change_percent };
        let verdict = if improved.abs() < NOISE_PERCENT {
            Verdict::Same
        } else if improved > 0.0 {
            Verdict::Better
        } else {
            Verdict::Worse
        };
        deltas.push(MetricDelta {
            kind,
            id: b.id.clone(),
            label: b.label.clone(),
            unit: b.unit.clone(),
            higher_is_better: b.higher_is_better,
            before: b.value,
            after: a.value,
            change_percent,
            verdict,
        });
    }
    let mut warnings = Vec::new();
    if before.machine != after.machine {
        warnings.push("The runs are from different machines; the comparison means little.".into());
    }
    if (before.load.cpu_percent - after.load.cpu_percent).abs() > LOAD_WARNING_POINTS {
        warnings.push(format!(
            "The background CPU load differed ({:.0}% before, {:.0}% after); repeat the runs on a quiet machine.",
            before.load.cpu_percent, after.load.cpu_percent
        ));
    }
    if before.load.on_battery != after.load.on_battery && before.load.on_battery.is_some() && after.load.on_battery.is_some() {
        warnings.push("One run was on battery and the other on mains power.".into());
    }
    Comparison {
        before: before.id.clone(),
        after: after.id.clone(),
        deltas,
        warnings,
    }
}

/// A run in progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchProgress {
    /// The run's id.
    pub run_id: String,
    /// When it started.
    pub started_at: Timestamp,
    /// The kinds it will run, in order.
    pub kinds: Vec<BenchKind>,
    /// The kind being measured now (`None` between kinds).
    pub current: Option<BenchKind>,
    /// Kinds already finished.
    pub done: Vec<BenchKind>,
}

/// What the benchmarks page shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BenchOverview {
    /// The run in progress, if any.
    pub running: Option<BenchProgress>,
    /// Saved runs, newest first.
    pub runs: Vec<BenchRun>,
}

/// The saved run history: versioned, bounded and tolerant of damage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchHistory {
    /// Schema version of the file.
    pub version: u32,
    /// Runs, newest first.
    pub runs: Vec<BenchRun>,
}

/// Current [`BenchHistory::version`].
pub const HISTORY_VERSION: u32 = 1;

impl Default for BenchHistory {
    fn default() -> Self {
        BenchHistory {
            version: HISTORY_VERSION,
            runs: Vec::new(),
        }
    }
}

impl BenchHistory {
    /// Parses a saved file; anything unreadable gives an empty history and a warning (never an error).
    pub fn parse(text: &str) -> (BenchHistory, Option<String>) {
        match serde_json::from_str::<BenchHistory>(text) {
            Ok(mut h) if h.version == HISTORY_VERSION => {
                h.runs.truncate(MAX_RUNS);
                (h, None)
            }
            Ok(h) => (
                BenchHistory::default(),
                Some(format!(
                    "The benchmark history is from a different version ({}); starting a new one.",
                    h.version
                )),
            ),
            Err(e) => (
                BenchHistory::default(),
                Some(format!("The benchmark history could not be read ({e}); starting a new one.")),
            ),
        }
    }

    /// Adds a run at the front and drops the oldest beyond [`MAX_RUNS`].
    pub fn push(&mut self, run: BenchRun) {
        self.runs.insert(0, run);
        self.runs.truncate(MAX_RUNS);
    }

    /// The run with this id.
    pub fn get(&self, id: &str) -> Option<&BenchRun> {
        self.runs.iter().find(|r| r.id == id)
    }

    /// Removes a run; false when there was none.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.runs.len();
        self.runs.retain(|r| r.id != id);
        self.runs.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, cpu_load: f32, values: &[(&str, f64, bool)]) -> BenchRun {
        BenchRun {
            id: id.into(),
            started_at: Timestamp(1),
            label: "x".into(),
            group: None,
            phase: None,
            load: LoadAtRun {
                cpu_percent: cpu_load,
                memory_percent: 40.0,
                on_battery: None,
            },
            machine: "m".into(),
            results: vec![KindResult {
                kind: BenchKind::Cpu,
                millis: 10,
                status: BenchStatus::Done {
                    metrics: values
                        .iter()
                        .map(|(n, v, h)| {
                            if *h {
                                BenchMetric::higher(n, n, *v, "u")
                            } else {
                                BenchMetric::lower(n, n, *v, "u")
                            }
                        })
                        .collect(),
                },
            }],
        }
    }

    #[test]
    fn kinds_round_trip_their_names() {
        for k in BenchKind::ALL {
            assert_eq!(BenchKind::parse(k.name()), Some(k));
        }
        assert_eq!(BenchKind::parse("nope"), None);
    }

    #[test]
    fn a_plan_is_validated_and_cleaned() {
        let p = plan(
            &[BenchKind::Disk, BenchKind::Disk, BenchKind::Cpu],
            None,
            "  Power\u{7}  plan ",
            Some("  "),
        )
        .unwrap();
        assert_eq!(p.kinds, vec![BenchKind::Disk, BenchKind::Cpu]);
        assert_eq!(p.disk_mib, DISK_DEFAULT_MIB);
        assert_eq!(p.label, "Power  plan");
        assert_eq!(p.group, None);
        assert!(plan(&[], None, "", None).is_err());
        assert!(plan(&[BenchKind::Disk], Some(DISK_MAX_MIB + 1), "", None).is_err());
        assert!(plan(&[BenchKind::Disk], Some(1), "", None).is_err());
        assert_eq!(plan(&[BenchKind::Cpu], None, "", None).unwrap().label, "Benchmark");
        let long = "a".repeat(500);
        assert_eq!(
            plan(&[BenchKind::Cpu], None, &long, Some(&long)).unwrap().label.chars().count(),
            MAX_LABEL
        );
    }

    #[test]
    fn comparison_uses_the_direction_and_ignores_noise() {
        let before = run(
            "a",
            5.0,
            &[
                ("score", 100.0, true),
                ("lat", 100.0, false),
                ("flat", 100.0, true),
                ("only-before", 1.0, true),
            ],
        );
        let after = run("b", 5.0, &[("score", 120.0, true), ("lat", 120.0, false), ("flat", 101.0, true)]);
        let c = compare(&before, &after);
        let by = |id: &str| c.deltas.iter().find(|d| d.id == id).unwrap();
        assert_eq!(by("score").verdict, Verdict::Better);
        assert_eq!(by("lat").verdict, Verdict::Worse);
        assert_eq!(by("flat").verdict, Verdict::Same);
        assert_eq!(c.deltas.len(), 3);
        assert!((by("score").change_percent - 20.0).abs() < 1e-9);
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn comparison_warns_about_unlike_conditions_and_survives_zero() {
        let mut a = run("a", 5.0, &[("z", 0.0, true)]);
        let mut b = run("b", 60.0, &[("z", 4.0, true)]);
        a.load.on_battery = Some(false);
        b.load.on_battery = Some(true);
        b.machine = "other".into();
        let c = compare(&a, &b);
        assert_eq!(c.deltas[0].change_percent, 0.0);
        assert_eq!(c.warnings.len(), 3);
    }

    #[test]
    fn non_finished_kinds_have_no_metrics() {
        let mut r = run("a", 0.0, &[("x", 1.0, true)]);
        r.results.push(KindResult {
            kind: BenchKind::Gpu,
            millis: 0,
            status: BenchStatus::Unavailable { reason: "none".into() },
        });
        assert_eq!(r.metrics().count(), 1);
    }

    #[test]
    fn history_is_bounded_versioned_and_tolerant() {
        let mut h = BenchHistory::default();
        for i in 0..(MAX_RUNS + 5) {
            h.push(run(&i.to_string(), 0.0, &[]));
        }
        assert_eq!(h.runs.len(), MAX_RUNS);
        assert_eq!(h.runs[0].id, (MAX_RUNS + 4).to_string());
        assert!(h.get("30").is_some());
        assert!(h.remove("30"));
        assert!(!h.remove("30"));
        let text = serde_json::to_string(&h).unwrap();
        let (back, warn) = BenchHistory::parse(&text);
        assert_eq!(back, h);
        assert!(warn.is_none());
        let (empty, warn) = BenchHistory::parse("{not json");
        assert!(empty.runs.is_empty());
        assert!(warn.is_some());
        let (empty, warn) = BenchHistory::parse(r#"{"version":99,"runs":[]}"#);
        assert!(empty.runs.is_empty() && warn.is_some());
    }
}
