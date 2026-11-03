//! Benchmarks: what is measured, how a run is recorded and how two runs are compared. Pure types and functions; the
//! measuring itself runs out of process (`dyno-bench`) and is started by the agent only when the user clicks.

pub mod cpu;
pub mod disk;
pub mod estimate;
pub mod export;
pub mod gpu;
pub mod memory;
pub mod unit;
pub mod wire;

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use wire::{Liveness, PhaseProgress};

/// Largest scratch file the disk benchmark may write, in MiB. The agent refuses a bigger request.
pub const DISK_MAX_MIB: u32 = 4096;
/// Scratch file size when the user doesn't choose, in MiB.
pub const DISK_DEFAULT_MIB: u32 = disk::DEFAULT_MIB;
/// Smallest scratch file, in MiB (smaller numbers only measure the cache).
pub const DISK_MIN_MIB: u32 = 16;
/// Whether the disk test may run. It was off while the 1.0 method (reads from the page cache) stood (D10: that test is never
/// shipped); ADR 0030 WP-B3 replaced it, so it is on.
pub const DISK_TEST_ENABLED: bool = true;
/// What the person is told while [`DISK_TEST_ENABLED`] is false.
pub const DISK_TEST_OFF: &str =
    "The disk test is being rebuilt to measure the drive and not the memory cache; it returns in a later Dyno 1.1 update.";
/// Runs kept; the oldest are dropped first.
pub const MAX_RUNS: usize = 200;
/// Repetitions of a timed kernel (ADR 0030 section 3).
pub const REPETITIONS: u32 = 5;
/// The uncounted warm-up before the repetitions, ms.
pub const WARMUP_MILLIS: u64 = 1_000;
/// The pause between two repetitions, ms (heat carried from one into the next).
pub const GAP_MILLIS: u64 = 2_000;
/// The time budget of one timed section (single core, all cores, copy, latency) in one repetition, ms.
pub const SECTION_MILLIS: u64 = 1_000;
/// A metric whose repetitions vary by more than this fraction (coefficient of variation) is marked unsteady.
pub const UNSTEADY_CV: f64 = 0.10;
/// The method version of everything measured before 1.1 (single run, no warm-up): a run without a recorded version has this one.
pub const LEGACY_METHOD: u32 = 1;
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

    /// The version of the way this kind is measured. It changes whenever the method does (warm-up, repetitions, kernels, file
    /// handling), and only runs of the same version are compared (ADR 0030 section 3). Version 1 is the 1.0 method.
    pub fn method_version(self) -> u32 {
        match self {
            // 3 (1.1): a 1 s warm-up and five repetitions, median and spread; CPU has four kernels and a sustained run, memory the
            // four STREAM kernels and a latency sweep.
            BenchKind::Cpu | BenchKind::Memory => 3,
            // Disk 2: direct I/O, four patterns, ramps, three repetitions (WP-B3).
            BenchKind::Disk => 2,
            // GPU 2: wgpu compute, copy, fill rate and geometry (WP-B6); nothing was measured before.
            BenchKind::Gpu => 2,
        }
    }

    /// The plain-language name of the kind (sentence case), for job phases and messages.
    pub fn title(self) -> &'static str {
        match self {
            BenchKind::Cpu => "Processor",
            BenchKind::Memory => "Memory",
            BenchKind::Disk => "Disk",
            BenchKind::Gpu => "Graphics",
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
    /// The headline value: the median of the repetitions.
    pub value: f64,
    /// Unit shown after it, e.g. `MB/s`.
    pub unit: String,
    /// Whether a larger value is better (false for latency).
    pub higher_is_better: bool,
    /// Every repetition, in the order measured (one for a single measurement).
    #[serde(default)]
    pub samples: Vec<f64>,
    /// The smallest repetition.
    #[serde(default)]
    pub min: f64,
    /// The largest repetition.
    #[serde(default)]
    pub max: f64,
    /// The coefficient of variation of the repetitions (`None` where the mean is zero).
    #[serde(default)]
    pub cv: Option<f64>,
    /// Whether the repetitions varied by more than [`UNSTEADY_CV`].
    #[serde(default)]
    pub unsteady: bool,
}

impl BenchMetric {
    /// A single measurement where larger is better.
    pub fn higher(id: &str, label: &str, value: f64, unit: &str) -> BenchMetric {
        BenchMetric {
            id: id.into(),
            label: label.into(),
            value,
            unit: unit.into(),
            higher_is_better: true,
            samples: vec![value],
            min: value,
            max: value,
            cv: Some(0.0),
            unsteady: false,
        }
    }

    /// A metric from its repetitions: the median is the value, the statistics come from `paddock-bench` (ER-22). Without a
    /// finite repetition the value is zero and the metric has no samples.
    pub fn measured(id: &str, label: &str, samples: &[f64], unit: &str, higher_is_better: bool) -> BenchMetric {
        let mut m = BenchMetric::higher(id, label, 0.0, unit);
        m.higher_is_better = higher_is_better;
        m.samples = samples.iter().copied().filter(|s| s.is_finite()).collect();
        m.fill_spread();
        m
    }

    /// Sets `value`, `min`, `max`, `cv` and `unsteady` from `samples` (without a finite sample the value stays and there is no spread).
    pub fn fill_spread(&mut self) {
        match paddock_bench::summarize(&self.samples) {
            Some(s) => {
                self.value = s.median;
                self.min = s.min;
                self.max = s.max;
                self.cv = s.cv();
                self.unsteady = s.cv().is_some_and(|cv| cv > UNSTEADY_CV);
            }
            None => {
                self.min = self.value;
                self.max = self.value;
                self.cv = None;
                self.unsteady = false;
            }
        }
    }

    /// The width of the repetitions, `max - min`.
    pub fn spread(&self) -> f64 {
        (self.max - self.min).max(0.0)
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
///
/// Tagged by `state` on the UI's JSON wire; a plain enum on the IPC (postcard cannot read self-describing enums).
#[derive(Debug, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state")]
enum StatusJson {
    Done { metrics: Vec<BenchMetric> },
    Unavailable { reason: String },
    Stopped { reason: String },
    Failed { reason: String },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum StatusBinary {
    Done { metrics: Vec<BenchMetric> },
    Unavailable { reason: String },
    Stopped { reason: String },
    Failed { reason: String },
}

macro_rules! status_from {
    ($($wire:ident),*) => {$(
        impl From<&BenchStatus> for $wire {
            fn from(s: &BenchStatus) -> Self {
                match s {
                    BenchStatus::Done { metrics } => $wire::Done { metrics: metrics.clone() },
                    BenchStatus::Unavailable { reason } => $wire::Unavailable { reason: reason.clone() },
                    BenchStatus::Stopped { reason } => $wire::Stopped { reason: reason.clone() },
                    BenchStatus::Failed { reason } => $wire::Failed { reason: reason.clone() },
                }
            }
        }
        impl From<$wire> for BenchStatus {
            fn from(s: $wire) -> Self {
                match s {
                    $wire::Done { metrics } => BenchStatus::Done { metrics },
                    $wire::Unavailable { reason } => BenchStatus::Unavailable { reason },
                    $wire::Stopped { reason } => BenchStatus::Stopped { reason },
                    $wire::Failed { reason } => BenchStatus::Failed { reason },
                }
            }
        }
    )*};
}
status_from!(StatusJson, StatusBinary);

impl Serialize for BenchStatus {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            StatusJson::from(self).serialize(s)
        } else {
            StatusBinary::from(self).serialize(s)
        }
    }
}

impl<'de> Deserialize<'de> for BenchStatus {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<BenchStatus, D::Error> {
        if d.is_human_readable() {
            StatusJson::deserialize(d).map(BenchStatus::from)
        } else {
            StatusBinary::deserialize(d).map(BenchStatus::from)
        }
    }
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
    /// The method version it was measured with ([`BenchKind::method_version`]); 1.0 runs have [`LEGACY_METHOD`].
    #[serde(default = "legacy_method")]
    pub method: u32,
    /// The sustained CPU run's trace, one point a second (empty for every other result).
    #[serde(default)]
    pub trace: Vec<cpu::TracePoint>,
    /// How this was measured, in words: the drive's file system and bus, whether the cache was bypassed, the vector width.
    #[serde(default)]
    pub details: Vec<String>,
    /// The drive's top is not writable: the page offers to choose a folder on it and run again.
    #[serde(default)]
    pub needs_folder: bool,
    /// The device measured, for a kind that runs once per device (the drive's name for the disk test); `None` otherwise.
    #[serde(default)]
    pub device: Option<String>,
}

fn legacy_method() -> u32 {
    LEGACY_METHOD
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
    /// Measured the 1.0 way (one run, no warm-up, the old disk test): kept, shown as "measured the old way" and never compared
    /// with a run of the current method (ADR 0030 D9).
    #[serde(default)]
    pub old_way: bool,
    /// Why the numbers are low confidence, when the run could not have full-confidence conditions (Linux without a systemd
    /// user manager: [`unit::LIMITED_NOTE`]).
    #[serde(default)]
    pub limited: Option<String>,
}

impl BenchRun {
    /// Fills the spread of every metric a 1.0 file saved without one.
    pub fn normalise(&mut self) {
        for r in &mut self.results {
            if let BenchStatus::Done { metrics } = &mut r.status {
                for m in metrics {
                    if m.samples.is_empty() && m.value.is_finite() {
                        // A metric saved by 1.0: one measurement.
                        m.samples = vec![m.value];
                    }
                    m.fill_spread();
                }
            }
        }
    }

    /// The method version of `kind` in this run, if it was measured.
    pub fn method_of(&self, kind: BenchKind) -> Option<u32> {
        self.results.iter().find(|r| r.kind == kind).map(|r| r.method)
    }

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

/// A folder the person chose, through the shell's folder dialog, for the scratch file of a drive whose top is not writable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskFolder {
    /// The volume's id (its mount point), as in `disk_volumes`.
    pub volume: String,
    /// The folder, a path as the dialog gave it. The agent canonicalises it and checks it is on that volume.
    pub folder: String,
}

/// What a request to start a run may ask, after parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// The kinds, in order, without repeats.
    pub kinds: Vec<BenchKind>,
    /// Scratch file size, MiB.
    pub disk_mib: u32,
    /// The volumes to test (their ids, which are their mount points); empty means the system drive.
    pub drives: Vec<String>,
    /// Network drives may be tested in this run (the person agreed).
    pub allow_network: bool,
    /// Folders chosen for drives whose top is not writable.
    pub folders: Vec<DiskFolder>,
    /// The graphics adapter to measure, by index (`None`: the default).
    pub gpu_adapter: Option<u32>,
    /// Minutes of the sustained CPU run (`None`: the usual short CPU test).
    pub sustained_minutes: Option<u32>,
    /// The sustained disk write (past the drive's cache) was confirmed for this run, in GiB; `None` when it was not.
    pub sustained_write_gib: Option<u32>,
    /// Label, trimmed.
    pub label: String,
    /// Group, trimmed.
    pub group: Option<String>,
}

/// The most drives one run may test.
pub const MAX_DRIVES: usize = 16;

impl RunPlan {
    /// Sets the drives to test and whether network drives may be included. The ids are the volumes' mount points as the list
    /// gave them; the agent checks each against the list it has, so an id that names no volume is refused there. Here only the
    /// shape is checked: a bounded count, printable and not empty, without repeats.
    pub fn with_drives(mut self, drives: &[String], allow_network: bool) -> Result<RunPlan, String> {
        let mut unique: Vec<String> = Vec::new();
        for d in drives {
            if d.is_empty() || d.chars().count() > 512 || d.chars().any(char::is_control) {
                return Err("A drive to test has an unusable name.".into());
            }
            if !unique.contains(d) {
                unique.push(d.clone());
            }
        }
        if unique.len() > MAX_DRIVES {
            return Err(format!("Choose at most {MAX_DRIVES} drives."));
        }
        self.drives = unique;
        self.allow_network = allow_network;
        Ok(self)
    }
}

impl RunPlan {
    /// Sets the folders chosen for drives that are in the plan; one that names a drive not in it, or an unusable path, is refused.
    pub fn with_folders(mut self, folders: &[DiskFolder]) -> Result<RunPlan, String> {
        for f in folders {
            if f.folder.is_empty() || f.folder.chars().count() > 1024 || f.folder.chars().any(char::is_control) {
                return Err("A chosen folder has an unusable path.".into());
            }
            if !self.drives.contains(&f.volume) {
                return Err("A chosen folder belongs to a drive that is not in the run.".into());
            }
        }
        self.folders = folders.to_vec();
        Ok(self)
    }

    /// Sets the sustained CPU run (minutes, one of the offered lengths; the sensor limit is checked when the run starts, where the
    /// sensors are known) and the sustained disk write (GiB per drive, bounded by [`disk::SUSTAINED_WRITE_MAX_GIB`]).
    pub fn with_sustained(mut self, minutes: Option<u32>, write_gib: Option<u32>) -> Result<RunPlan, String> {
        if let Some(m) = minutes {
            if !cpu::SUSTAINED_MINUTES.contains(&m) {
                return Err(format!("The sustained run lasts 2, 5, 10 or 20 minutes, not {m}."));
            }
        }
        if let Some(g) = write_gib {
            if !(1..=disk::SUSTAINED_WRITE_MAX_GIB).contains(&g) {
                return Err(format!(
                    "The sustained write must be between 1 and {} GiB.",
                    disk::SUSTAINED_WRITE_MAX_GIB
                ));
            }
        }
        self.sustained_minutes = minutes;
        self.sustained_write_gib = write_gib;
        Ok(self)
    }
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
    plan_with(kinds, disk_mib, label, group, DISK_TEST_ENABLED)
}

/// [`plan`] with the disk switch passed in, so the rule is testable whichever way the build flag is set.
pub fn plan_with(
    kinds: &[BenchKind],
    disk_mib: Option<u32>,
    label: &str,
    group: Option<&str>,
    disk_enabled: bool,
) -> Result<RunPlan, String> {
    if !disk_enabled && kinds.contains(&BenchKind::Disk) {
        return Err(DISK_TEST_OFF.into());
    }
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
        drives: Vec::new(),
        allow_network: false,
        folders: Vec::new(),
        gpu_adapter: None,
        sustained_minutes: None,
        sustained_write_gib: None,
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
    /// How widely the repetitions of the earlier run varied (`max - min`).
    pub before_spread: f64,
    /// How widely the repetitions of the later run varied.
    pub after_spread: f64,
    /// Whether either run's repetitions varied by more than [`UNSTEADY_CV`].
    pub unsteady: bool,
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

/// Compares `after` with `before`: every metric present in both and measured with the same method version. A change counts
/// as better or worse only when it is larger than the noise band ([`NOISE_PERCENT`] of the earlier value) and than the two
/// runs' spreads together (`max - min` of each); otherwise the verdict is "same". Kinds measured with different method versions
/// (a 1.0 run against a 1.1 run) are not compared, with a warning. Warns when the machine differs or the background load
/// differs by [`LOAD_WARNING_POINTS`].
pub fn compare(before: &BenchRun, after: &BenchRun) -> Comparison {
    let mut deltas = Vec::new();
    let mut skipped: Vec<BenchKind> = Vec::new();
    for (kind, b) in before.metrics() {
        if before.method_of(kind) != after.method_of(kind) {
            if !skipped.contains(&kind) {
                skipped.push(kind);
            }
            continue;
        }
        let Some((_, a)) = after.metrics().find(|(k, m)| *k == kind && m.id == b.id) else {
            continue;
        };
        let change_percent = if b.value.abs() < f64::EPSILON {
            0.0
        } else {
            (a.value - b.value) / b.value * 100.0
        };
        let improved = if b.higher_is_better { change_percent } else { -change_percent };
        let diff = (a.value - b.value).abs();
        let noise = b.value.abs() * NOISE_PERCENT / 100.0;
        let verdict = if diff <= noise || diff <= b.spread() + a.spread() {
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
            before_spread: b.spread(),
            after_spread: a.spread(),
            unsteady: b.unsteady || a.unsteady,
            verdict,
        });
    }
    let mut warnings = Vec::new();
    for kind in skipped {
        let old = if before.old_way || after.old_way {
            " (one of them was measured the old way, with Dyno 1.0)"
        } else {
            ""
        };
        warnings.push(format!(
            "The {} results were measured with different methods{old}, so they are not compared.",
            kind.title()
        ));
    }
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
    /// The phase being measured now, as the measuring program reports it (`None` between phases and kinds).
    pub phase: Option<PhaseProgress>,
    /// How alive the run looks. Only the person's Cancel (or a named safety stop) ends a slow or quiet run.
    pub liveness: Liveness,
    /// Set when the run could not have full-confidence conditions (see [`BenchRun::limited`]).
    pub limited: Option<String>,
    /// The sustained CPU run's points so far (the page draws them live); empty otherwise.
    #[serde(default)]
    pub trace: Vec<cpu::TracePoint>,
}

/// What the benchmarks page shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BenchOverview {
    /// The run in progress, if any.
    pub running: Option<BenchProgress>,
    /// Saved runs, newest first.
    pub runs: Vec<BenchRun>,
    /// Whether the disk test can be chosen ([`DISK_TEST_ENABLED`]); the page shows it as off otherwise.
    pub disk_test_enabled: bool,
    /// What the picker shows before a run: about how long each kind takes and what the disk test writes per drive.
    #[serde(default)]
    pub estimates: estimate::Estimates,
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
/// (2: repetitions with spread and method versions; version 1 files are read and their runs marked as measured the old way.)
pub const HISTORY_VERSION: u32 = 2;

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
                h.runs.iter_mut().for_each(BenchRun::normalise);
                h.runs.truncate(MAX_RUNS);
                (h, None)
            }
            Ok(mut h) if h.version == 1 => {
                // Dyno 1.0's runs are kept and marked (ADR 0030 D9); they are never compared with the current method.
                for r in &mut h.runs {
                    r.old_way = true;
                    r.normalise();
                }
                h.version = HISTORY_VERSION;
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
            old_way: false,
            limited: None,
            results: vec![KindResult {
                kind: BenchKind::Cpu,
                millis: 10,
                method: 2,
                device: None,
                trace: Vec::new(),
                details: Vec::new(),
                needs_folder: false,
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
    fn chosen_folders_must_belong_to_drives_in_the_plan() {
        let base = plan_with(&[BenchKind::Disk], None, "", None, true)
            .unwrap()
            .with_drives(&["/Volumes/A".into()], false)
            .unwrap();
        let ok = DiskFolder {
            volume: "/Volumes/A".into(),
            folder: "/Volumes/A/Bench".into(),
        };
        assert_eq!(
            base.clone().with_folders(std::slice::from_ref(&ok)).unwrap().folders,
            vec![ok.clone()]
        );
        let stray = DiskFolder {
            volume: "/Volumes/B".into(),
            ..ok.clone()
        };
        assert!(base.clone().with_folders(&[stray]).is_err());
        assert!(base
            .with_folders(&[DiskFolder {
                folder: "a\nb".to_string(),
                ..ok
            }])
            .is_err());
    }

    #[test]
    fn sustained_choices_are_validated() {
        let base = plan_with(&[BenchKind::Cpu], None, "", None, true).unwrap();
        assert_eq!(base.clone().with_sustained(Some(5), Some(8)).unwrap().sustained_minutes, Some(5));
        assert!(base.clone().with_sustained(Some(7), None).is_err());
        assert!(base.clone().with_sustained(None, Some(0)).is_err());
        assert!(base.with_sustained(None, Some(disk::SUSTAINED_WRITE_MAX_GIB + 1)).is_err());
    }

    #[test]
    fn drives_are_bounded_unique_and_printable() {
        let base = plan_with(&[BenchKind::Disk], None, "", None, true).unwrap();
        assert!(base.drives.is_empty(), "no drive chosen means the system drive");
        let p = base
            .clone()
            .with_drives(&["/Volumes/A".into(), "/Volumes/A".into(), "/Volumes/B".into()], true)
            .unwrap();
        assert_eq!(p.drives, vec!["/Volumes/A", "/Volumes/B"]);
        assert!(p.allow_network);
        assert!(base.clone().with_drives(&["".into()], false).is_err());
        assert!(base.clone().with_drives(&["a\nb".to_string()], false).is_err());
        let many: Vec<String> = (0..=MAX_DRIVES).map(|i| format!("/v{i}")).collect();
        assert!(base.with_drives(&many, false).is_err());
    }

    #[test]
    fn a_plan_is_validated_and_cleaned() {
        let p = plan_with(
            &[BenchKind::Disk, BenchKind::Disk, BenchKind::Cpu],
            None,
            "  Power\u{7}  plan ",
            Some("  "),
            true,
        )
        .unwrap();
        assert_eq!(p.kinds, vec![BenchKind::Disk, BenchKind::Cpu]);
        assert_eq!(p.disk_mib, DISK_DEFAULT_MIB);
        assert_eq!(p.label, "Power  plan");
        assert_eq!(p.group, None);
        assert!(plan(&[], None, "", None).is_err());
        assert!(plan_with(&[BenchKind::Disk], Some(DISK_MAX_MIB + 1), "", None, true).is_err());
        assert!(plan_with(&[BenchKind::Disk], Some(1), "", None, true).is_err());
        assert_eq!(plan(&[BenchKind::Cpu], None, "", None).unwrap().label, "Benchmark");
        let long = "a".repeat(500);
        assert_eq!(
            plan(&[BenchKind::Cpu], None, &long, Some(&long)).unwrap().label.chars().count(),
            MAX_LABEL
        );
    }

    #[test]
    fn the_disk_test_is_refused_while_it_is_off_and_the_default_build_has_it_off() {
        // The build flag decides; while it is off (until WP-B3, ADR 0030 D10) a plan with Disk is refused with the plain words.
        let with_disk = plan(&[BenchKind::Cpu, BenchKind::Disk], None, "", None);
        assert_eq!(with_disk.is_err(), !DISK_TEST_ENABLED);
        assert_eq!(
            plan_with(&[BenchKind::Cpu, BenchKind::Disk], None, "", None, false).unwrap_err(),
            DISK_TEST_OFF
        );
        assert!(plan(&[BenchKind::Cpu, BenchKind::Memory], None, "", None).is_ok());
        assert!(plan_with(&[BenchKind::Disk], None, "", None, true).is_ok());
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
            method: 1,
            device: None,
            trace: Vec::new(),
            details: Vec::new(),
            needs_folder: false,
            status: BenchStatus::Unavailable { reason: "none".into() },
        });
        assert_eq!(r.metrics().count(), 1);
    }

    fn measured_run(id: &str, method: u32, samples: &[f64]) -> BenchRun {
        let mut r = run(id, 0.0, &[]);
        r.results = vec![KindResult {
            kind: BenchKind::Cpu,
            millis: 1,
            method,
            device: None,
            trace: Vec::new(),
            details: Vec::new(),
            needs_folder: false,
            status: BenchStatus::Done {
                metrics: vec![BenchMetric::measured("cpu-single", "Single core", samples, "u", true)],
            },
        }];
        r
    }

    #[test]
    fn a_metric_keeps_every_repetition_with_the_median_as_its_value_and_marks_unsteady() {
        let m = BenchMetric::measured("x", "X", &[100.0, 102.0, 98.0, 101.0, 99.0], "u", true);
        assert_eq!((m.value, m.min, m.max), (100.0, 98.0, 102.0));
        assert_eq!(m.samples.len(), 5);
        assert!(!m.unsteady && m.cv.unwrap() < 0.05);
        assert_eq!(m.spread(), 4.0);
        let u = BenchMetric::measured("x", "X", &[100.0, 140.0, 60.0, 100.0, 100.0], "u", true);
        assert!(u.unsteady, "cv {:?}", u.cv);
        let none = BenchMetric::measured("x", "X", &[f64::NAN], "u", false);
        assert_eq!(
            (none.value, none.samples.len(), none.cv, none.unsteady, none.higher_is_better),
            (0.0, 0, None, false, false)
        );
    }

    #[test]
    fn a_change_inside_the_spreads_is_about_the_same_and_one_outside_counts() {
        // 10 % better, but the repetitions of each run span 12 points: not a real change.
        let c = compare(
            &measured_run("a", 2, &[94.0, 100.0, 106.0]),
            &measured_run("b", 2, &[104.0, 110.0, 116.0]),
        );
        assert_eq!(c.deltas[0].verdict, Verdict::Same);
        assert_eq!((c.deltas[0].before_spread, c.deltas[0].after_spread), (12.0, 12.0));
        // The same 10 % with tight repetitions is better.
        let c = compare(
            &measured_run("a", 2, &[99.0, 100.0, 101.0]),
            &measured_run("b", 2, &[109.0, 110.0, 111.0]),
        );
        assert_eq!(c.deltas[0].verdict, Verdict::Better);
        assert!(!c.deltas[0].unsteady);
        let c = compare(
            &measured_run("a", 2, &[100.0, 100.0, 100.0]),
            &measured_run("b", 2, &[100.0, 150.0, 60.0]),
        );
        assert!(c.deltas[0].unsteady);
    }

    #[test]
    fn runs_of_different_method_versions_are_not_compared() {
        let mut old = measured_run("a", 1, &[100.0]);
        old.old_way = true;
        let c = compare(&old, &measured_run("b", 2, &[150.0, 150.0, 150.0]));
        assert!(c.deltas.is_empty());
        assert!(
            c.warnings.iter().any(|w| w.contains("Processor") && w.contains("old way")),
            "{:?}",
            c.warnings
        );
    }

    #[test]
    fn a_1_0_history_is_kept_and_marked_the_old_way() {
        let v1 = r#"{"version":1,"runs":[{"id":"1","startedAt":5,"label":"Baseline","group":null,"phase":null,
            "load":{"cpuPercent":3.0,"memoryPercent":40.0,"onBattery":null},"machine":"m",
            "results":[{"kind":"cpu","millis":900,"status":{"state":"done","metrics":[
            {"id":"cpu-single","label":"Single core","value":612.0,"unit":"M rounds/s","higherIsBetter":true}]}}]}]}"#;
        let (h, warn) = BenchHistory::parse(v1);
        assert!(warn.is_none());
        assert_eq!(h.version, HISTORY_VERSION);
        let r = &h.runs[0];
        assert!(r.old_way);
        assert_eq!(r.method_of(BenchKind::Cpu), Some(LEGACY_METHOD));
        let (_, m) = r.metrics().next().unwrap();
        assert_eq!((m.value, m.samples.clone(), m.min, m.max), (612.0, vec![612.0], 612.0, 612.0));
        assert!(!m.unsteady);
        // A 1.0 run is never compared with a current one.
        let c = compare(r, &measured_run("n", BenchKind::Cpu.method_version(), &[600.0, 610.0, 605.0]));
        assert!(c.deltas.is_empty());
        // New runs are not marked.
        assert!(!measured_run("n", 2, &[1.0]).old_way);
        assert!(BenchKind::Cpu.method_version() > LEGACY_METHOD && BenchKind::Memory.method_version() > LEGACY_METHOD);
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

    #[test]
    fn every_status_crosses_the_ipc_and_the_ui_wire() {
        let all = [
            BenchStatus::Done {
                metrics: vec![BenchMetric::higher("a", "A", 1.0, "x")],
            },
            BenchStatus::Unavailable { reason: "u".into() },
            BenchStatus::Stopped { reason: "s".into() },
            BenchStatus::Failed { reason: "f".into() },
        ];
        for st in all {
            let bytes = postcard::to_stdvec(&st).unwrap();
            assert_eq!(postcard::from_bytes::<BenchStatus>(&bytes).unwrap(), st, "postcard");
            let json = serde_json::to_value(&st).unwrap();
            assert!(json["state"].is_string(), "the UI shape is tagged by state");
            assert_eq!(serde_json::from_value::<BenchStatus>(json).unwrap(), st);
        }
    }
}
