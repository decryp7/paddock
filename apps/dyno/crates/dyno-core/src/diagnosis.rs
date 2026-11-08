//! "Why is my computer slow right now?": names the cause in plain words, with the evidence, from the last minute of
//! samples and the current apps.
//!
//! Each detector scores its cause 0–1; causes under [`MIN_CONFIDENCE`] are dropped. With none left the answer is
//! "nothing is slowing it down", with the numbers that show it.

use crate::anomaly::Evidence;
use crate::grouping::{file_name, AppGroup};
use crate::hardware::{Confidence, OsThrottle};
use crate::metrics::{derive_pressure, MemoryPressure, SystemSample};
use crate::process::ProcessSample;
use crate::throttle::{self, ThrottleInput};
use crate::time::Timestamp;
use crate::units::{bytes, storage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The kinds of causes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CauseKind {
    /// Nothing found.
    NothingWrong,
    /// One app takes most of the CPU.
    CpuHog,
    /// Not enough memory.
    MemoryPressure,
    /// The disk can't keep up.
    DiskQueue,
    /// The CPU slowed itself down to cool off.
    ThermalThrottling,
    /// The OS is installing updates.
    OsUpdate,
    /// The antivirus is scanning.
    AntivirusScan,
    /// The search indexer is working.
    Indexing,
    /// The system disk is nearly full.
    LowDiskSpace,
    /// The GPU is maxed out.
    GpuBusy,
}

/// A page worth opening to look closer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HintLink {
    /// The task manager.
    TaskManager,
    /// The startup list.
    Startup,
}

/// Something worth knowing about a cause, in words, with a page to look closer at where one helps. Dyno changes
/// nothing on the computer, so a hint never carries an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    /// What to know or do (for the person).
    pub text: String,
    /// A page that shows more, if one does.
    pub link: Option<HintLink>,
}

impl Hint {
    fn text(text: &str) -> Hint {
        Hint {
            text: text.into(),
            link: None,
        }
    }

    fn open(text: &str, link: HintLink) -> Hint {
        Hint {
            text: text.into(),
            link: Some(link),
        }
    }
}

/// An app that is part of a cause, with its share of what was measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Involved {
    /// The app group's id.
    pub id: String,
    /// The app's name.
    pub name: String,
    /// Its share, formatted ("63% CPU", "1.2 GB", "40 MB/s").
    pub share: String,
}

/// Most apps named per cause.
pub const MAX_INVOLVED: usize = 3;

/// One cause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cause {
    /// Kind.
    pub kind: CauseKind,
    /// One line in plain words.
    pub title: String,
    /// What it means.
    pub explanation: String,
    /// 0–1.
    pub confidence: f32,
    /// The numbers.
    pub evidence: Vec<Evidence>,
    /// Hints.
    pub hints: Vec<Hint>,
    /// The busiest apps behind it (empty when the cause is not an app, e.g. heat).
    pub involved: Vec<Involved>,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    /// When.
    pub at: Timestamp,
    /// How many seconds of samples it looked at.
    pub window_secs: u32,
    /// The top cause's title.
    pub headline: String,
    /// Causes, most likely first (one `NothingWrong` when none).
    pub causes: Vec<Cause>,
}

/// Causes below this are not reported.
pub const MIN_CONFIDENCE: f32 = 0.3;

/// OS update processes (lower-case file names).
const UPDATE: &[&str] = &[
    "tiworker.exe",
    "trustedinstaller.exe",
    "mousocoreworker.exe",
    "wuauclt.exe",
    "musnotification.exe",
    "softwareupdated",
    "msiexec.exe",
];
/// Antivirus engines.
const ANTIVIRUS: &[&str] = &["msmpeng.exe", "mpdefendercoreservice.exe", "xprotect", "xprotectremediator"];
/// Search indexers.
const INDEXERS: &[&str] = &[
    "searchindexer.exe",
    "searchprotocolhost.exe",
    "searchfilterhost.exe",
    "mds_stores",
    "mdworker_shared",
    "mds",
];

/// What the diagnosis looks at.
pub struct Input<'a> {
    /// Now.
    pub now: Timestamp,
    /// The last minute or so of samples, oldest first (at least one).
    pub samples: &'a [SystemSample],
    /// Current apps (busiest first is not required).
    pub groups: &'a [AppGroup],
    /// Current processes.
    pub processes: &'a [ProcessSample],
    /// The volume the OS lives on (its id, e.g. "C:" or "/").
    pub system_disk: &'a str,
    /// What the OS says about speed limits, and whether a battery-saving mode is on (for the throttling cause).
    pub throttle: ThrottleContext,
}

/// Facts that are not part of a sample but help name why the CPU is slow.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThrottleContext {
    /// The OS's own speed limit, when it reports one.
    pub os: OsThrottle,
    /// A battery-saving mode is on, when known.
    pub power_saver: Option<bool>,
}

/// Samples the throttling check looks at.
const THROTTLE_WINDOW: usize = 10;

fn avg(samples: &[SystemSample], f: impl Fn(&SystemSample) -> f32) -> f32 {
    if samples.is_empty() {
        0.0
    } else {
        samples.iter().map(f).sum::<f32>() / samples.len() as f32
    }
}

fn app_kind(g: &AppGroup, by_pid: &HashMap<u32, &ProcessSample>) -> Option<CauseKind> {
    let names: Vec<String> = g
        .pids
        .iter()
        .filter_map(|p| by_pid.get(p))
        .map(|p| p.exe.as_deref().map(file_name).unwrap_or(&p.name).to_lowercase())
        .collect();
    let any = |list: &[&str]| names.iter().any(|n| list.contains(&n.as_str()));
    if any(UPDATE) {
        Some(CauseKind::OsUpdate)
    } else if any(ANTIVIRUS) {
        Some(CauseKind::AntivirusScan)
    } else if any(INDEXERS) {
        Some(CauseKind::Indexing)
    } else {
        None
    }
}

/// The busiest apps by `key` (those with a share above zero), at most [`MAX_INVOLVED`].
fn involved(groups: &[AppGroup], key: impl Fn(&AppGroup) -> f32, show: impl Fn(f32) -> String) -> Vec<Involved> {
    let mut ranked: Vec<(&AppGroup, f32)> = groups.iter().map(|g| (g, key(g))).filter(|(_, v)| *v > 0.0).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked
        .into_iter()
        .take(MAX_INVOLVED)
        .map(|(g, v)| Involved {
            id: g.id.clone(),
            name: g.name.clone(),
            share: show(v),
        })
        .collect()
}

fn pct(v: f32) -> String {
    format!("{v:.0}%")
}

/// Diagnoses the last minute.
pub fn diagnose(input: &Input<'_>) -> Diagnosis {
    let by_pid: HashMap<u32, &ProcessSample> = input.processes.iter().map(|p| (p.pid, p)).collect();
    let mut causes = Vec::new();
    cpu_causes(input, &by_pid, &mut causes);
    memory_cause(input, &mut causes);
    disk_cause(input, &mut causes);
    thermal_cause(input, &mut causes);
    space_cause(input, &mut causes);
    gpu_cause(input, &mut causes);
    causes.retain(|c| c.confidence >= MIN_CONFIDENCE);
    causes.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    if causes.is_empty() {
        let cpu = avg(input.samples, |s| s.cpu.total);
        let mem = avg(input.samples, |s| s.memory_percent());
        causes.push(Cause {
            kind: CauseKind::NothingWrong,
            title: "Nothing is slowing your computer down right now".into(),
            explanation: "CPU, memory and disk all have room to spare. If something felt slow, it may have been brief: \
                          the history charts show the last hour, and Dyno keeps watching."
                .into(),
            confidence: 1.0,
            evidence: vec![Evidence::new("CPU", pct(cpu)), Evidence::new("Memory in use", pct(mem))],
            hints: vec![],
            involved: vec![],
        });
    }
    let window_secs = match (input.samples.first(), input.samples.last()) {
        (Some(a), Some(b)) => (b.at.since(a.at) / 1000).clamp(0, i64::from(u32::MAX)) as u32,
        _ => 0,
    };
    Diagnosis {
        at: input.now,
        window_secs,
        headline: causes[0].title.clone(),
        causes,
    }
}

fn cpu_causes(input: &Input<'_>, by_pid: &HashMap<u32, &ProcessSample>, out: &mut Vec<Cause>) {
    let total = avg(input.samples, |s| s.cpu.total);
    let Some(top) = input.groups.iter().max_by(|a, b| a.cpu_read().total_cmp(&b.cpu_read())) else {
        return;
    };
    if total < 50.0 || top.cpu_read() < 20.0 {
        return;
    }
    let confidence = ((total - 40.0) / 50.0).clamp(0.0, 1.0) * (top.cpu_read() / 50.0).clamp(0.2, 1.0);
    let mut evidence = vec![
        Evidence::new("CPU (last minute)", pct(total)),
        Evidence::new(
            format!(
                "{} ({} process{})",
                top.name,
                top.pids.len(),
                if top.pids.len() == 1 { "" } else { "es" }
            ),
            pct(top.cpu_read()),
        ),
    ];
    if let Some(second) = input
        .groups
        .iter()
        .filter(|g| g.id != top.id)
        .max_by(|a, b| a.cpu_read().total_cmp(&b.cpu_read()))
    {
        evidence.push(Evidence::new(format!("Next: {}", second.name), pct(second.cpu_read())));
    }
    let (kind, title, explanation, mut hints) = match app_kind(top, by_pid) {
        Some(CauseKind::OsUpdate) => (
            CauseKind::OsUpdate,
            "Your system is installing updates".to_string(),
            format!(
                "{} is busy installing updates; this passes on its own. A restart may be needed afterwards.",
                top.name
            ),
            vec![Hint::text("Let it finish")],
        ),
        Some(CauseKind::AntivirusScan) => (
            CauseKind::AntivirusScan,
            "The antivirus is scanning".to_string(),
            format!(
                "{} is scanning files. Builds and big copies make it scan a lot; excluding your development folders \
                 from its scans helps.",
                top.name
            ),
            vec![Hint::text("Let it finish")],
        ),
        Some(CauseKind::Indexing) => (
            CauseKind::Indexing,
            "Search indexing is busy".to_string(),
            "The search indexer is catching up with changed files. Excluding build folders from indexing helps.".to_string(),
            vec![],
        ),
        _ => (
            CauseKind::CpuHog,
            format!("{} is using {} of your CPU", top.name, pct(top.cpu_read())),
            format!(
                "{} takes most of the processor, so everything else waits. Closing it or giving it less to do keeps the rest \
                 responsive.",
                top.name
            ),
            vec![],
        ),
    };
    hints.push(Hint::open("Open Processes", HintLink::TaskManager));
    out.push(Cause {
        kind,
        title,
        explanation,
        confidence,
        evidence,
        hints,
        involved: involved(input.groups, |g| g.cpu_read(), |v| format!("{} CPU", pct(v))),
    });
}

fn memory_cause(input: &Input<'_>, out: &mut Vec<Cause>) {
    let Some(last) = input.samples.last() else { return };
    let level = last
        .memory
        .pressure
        .value()
        .copied()
        .unwrap_or_else(|| derive_pressure(&last.memory));
    let confidence = match level {
        MemoryPressure::Critical => 0.9,
        MemoryPressure::Elevated => 0.55,
        MemoryPressure::Normal => return,
    };
    let mut evidence = vec![Evidence::new(
        "Memory in use",
        format!("{} of {}", bytes(last.memory.used), bytes(last.memory.total)),
    )];
    if let Some(c) = last.memory.commit.value() {
        evidence.push(Evidence::new("Committed", format!("{} of {}", bytes(c.used), bytes(c.limit))));
    }
    if last.memory.swap_used > 0 {
        evidence.push(Evidence::new("Swap / page file in use", bytes(last.memory.swap_used)));
    }
    let mut hints = vec![];
    if let Some(top) = input.groups.iter().max_by_key(|g| g.memory_read()) {
        evidence.push(Evidence::new(format!("Biggest: {}", top.name), bytes(top.memory_read())));
    }
    hints.push(Hint::open("Open Processes", HintLink::TaskManager));
    out.push(Cause {
        kind: CauseKind::MemoryPressure,
        title: if level == MemoryPressure::Critical {
            "Your computer is out of memory".into()
        } else {
            "Memory is running low".into()
        },
        explanation: "When memory runs out the system moves data to disk, and every app slows down. Closing apps you \
                      don't need (browser tabs count) frees memory at once."
            .into(),
        confidence,
        evidence,
        hints,
        involved: involved(input.groups, |g| g.memory_read() as f32, |v| bytes(v as u64)),
    });
}

fn disk_cause(input: &Input<'_>, out: &mut Vec<Cause>) {
    let queue = avg(input.samples, |s| s.disk_io.queue.value().copied().unwrap_or(0.0));
    let busy = avg(input.samples, |s| s.disk_io.busy.value().copied().unwrap_or(0.0));
    if queue < 2.0 && busy < 90.0 {
        return;
    }
    let confidence = ((queue / 6.0).max((busy - 70.0) / 30.0)).clamp(0.0, 0.95);
    let read = avg(input.samples, |s| s.disk_io.read_bps as f32);
    let write = avg(input.samples, |s| s.disk_io.write_bps as f32);
    let mut evidence = vec![
        Evidence::new("Disk queue", format!("{queue:.1}")),
        Evidence::new("Disk busy", pct(busy)),
        Evidence::new("Reading / writing", format!("{}/s / {}/s", bytes(read as u64), bytes(write as u64))),
    ];
    if let Some(top) = input
        .groups
        .iter()
        .filter(|g| g.disk_read() > 0.0)
        .max_by(|a, b| a.disk_read().total_cmp(&b.disk_read()))
    {
        evidence.push(Evidence::new(
            format!("Busiest: {}", top.name),
            format!("{}/s", bytes(top.disk_read() as u64)),
        ));
    }
    out.push(Cause {
        kind: CauseKind::DiskQueue,
        title: "The disk can't keep up".into(),
        explanation: "Requests are queuing at the disk, so apps wait for files. This is common on hard disks during \
                      updates, scans or big copies; it passes when the work is done."
            .into(),
        confidence,
        evidence,
        hints: vec![Hint::open("Open Processes", HintLink::TaskManager)],
        involved: involved(input.groups, |g| g.disk_read() as f32, |v| format!("{}/s", bytes(v as u64))),
    });
}

fn thermal_cause(input: &Input<'_>, out: &mut Vec<Cause>) {
    let window: Vec<ThrottleInput> = input
        .samples
        .iter()
        .rev()
        .take(THROTTLE_WINDOW)
        .rev()
        .map(|s| ThrottleInput::from_sample(s, input.throttle.power_saver, input.throttle.os))
        .collect();
    let reading = throttle::detect(&window);
    if !matches!(reading.verdict, throttle::Verdict::Throttled { .. }) {
        return;
    }
    out.push(Cause {
        kind: CauseKind::ThermalThrottling,
        title: reading.headline,
        explanation: format!("{} Confidence: {}.", reading.explanation, reading.confidence.word()),
        confidence: match reading.confidence {
            Confidence::High => 0.9,
            Confidence::Medium => 0.7,
            Confidence::Low => 0.5,
        },
        evidence: reading.evidence,
        hints: vec![],
        involved: vec![],
    });
}

fn space_cause(input: &Input<'_>, out: &mut Vec<Cause>) {
    let Some(last) = input.samples.last() else { return };
    let Some(d) = last.disks.iter().find(|d| d.id.eq_ignore_ascii_case(input.system_disk)) else {
        return;
    };
    let free_pct = crate::metrics::percent(d.free, d.total);
    if free_pct >= 5.0 && d.free >= 5 << 30 {
        return;
    }
    out.push(Cause {
        kind: CauseKind::LowDiskSpace,
        title: format!("{} is nearly full", d.name),
        explanation: "With little free space the system can't grow its page file or install updates, and apps fail to \
                      save."
            .into(),
        confidence: if d.free < 1 << 30 { 0.8 } else { 0.5 },
        evidence: vec![Evidence::new("Free", format!("{} ({})", storage(d.free), pct(free_pct)))],
        hints: vec![],
        involved: vec![],
    });
}

fn gpu_cause(input: &Input<'_>, out: &mut Vec<Cause>) {
    let util = avg(input.samples, |s| s.gpu.value().map_or(0.0, |g| g.utilization));
    if util < 90.0 {
        return;
    }
    out.push(Cause {
        kind: CauseKind::GpuBusy,
        title: "The graphics card is at its limit".into(),
        explanation: "The GPU is fully busy. In a game, lower the graphics settings; otherwise look for an app \
                      rendering or mining in the background."
            .into(),
        confidence: 0.4,
        evidence: vec![Evidence::new("GPU", pct(util))],
        hints: vec![],
        involved: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grouping::group;
    use crate::metrics::fixtures::sample;
    use crate::metrics::{Commit, GpuSample, Reading, Temperature};
    use crate::process::fixtures::proc;

    struct Case {
        samples: Vec<SystemSample>,
        procs: Vec<ProcessSample>,
        throttle: ThrottleContext,
    }

    impl Case {
        fn new() -> Case {
            Case {
                samples: (0..12).map(|i| sample(i * 5_000)).collect(),
                procs: vec![proc(10, None, "chrome.exe", 2.0, 400), proc(11, None, "code.exe", 1.0, 300)],
                throttle: ThrottleContext::default(),
            }
        }
        fn each(&mut self, f: impl Fn(&mut SystemSample)) {
            self.samples.iter_mut().for_each(f);
        }
        fn run(&self) -> Diagnosis {
            let groups = group(&self.procs);
            diagnose(&Input {
                now: Timestamp(60_000),
                samples: &self.samples,
                groups: &groups,
                processes: &self.procs,
                system_disk: "c:",
                throttle: self.throttle,
            })
        }
    }

    #[test]
    fn a_quiet_machine_is_fine() {
        let d = Case::new().run();
        assert_eq!(d.causes.len(), 1);
        assert_eq!(d.causes[0].kind, CauseKind::NothingWrong);
        assert_eq!(d.headline, "Nothing is slowing your computer down right now");
        let empty = Case {
            samples: vec![],
            procs: vec![],
            throttle: ThrottleContext::default(),
        };
        assert_eq!(empty.run().causes[0].kind, CauseKind::NothingWrong);
        assert_eq!(avg(&[], |s| s.cpu.total), 0.0);
    }

    #[test]
    fn a_cpu_hog_is_named_with_actions() {
        let mut c = Case::new();
        c.each(|s| s.cpu.total = 92.0);
        c.procs.push(proc(20, None, "rustc.exe", 63.0, 900));
        let d = c.run();
        let top = &d.causes[0];
        assert_eq!(top.kind, CauseKind::CpuHog);
        assert_eq!(d.headline, "Rust compiler is using 63% of your CPU");
        assert!(top.evidence.iter().any(|e| e.label.starts_with("Next: Google Chrome")));
        assert!(top.evidence.iter().any(|e| e.label == "Rust compiler (1 process)"));
        assert_eq!(top.hints.len(), 1);
        assert_eq!(top.hints[0].link, Some(HintLink::TaskManager));
        // Two processes: plural.
        c.procs.push(proc(21, Some(20), "rustc.exe", 1.0, 1));
        c.procs[3].exe = c.procs[2].exe.clone();
        let d = c.run();
        assert!(d.causes[0].evidence.iter().any(|e| e.label == "Rust compiler (2 processes)"));
    }

    #[test]
    fn a_cause_names_the_busiest_apps_and_the_window_looked_at() {
        let mut c = Case::new();
        c.each(|s| s.cpu.total = 92.0);
        c.procs.push(proc(20, None, "rustc.exe", 63.0, 900));
        let d = c.run();
        // 12 samples 5 s apart.
        assert_eq!(d.window_secs, 55);
        let inv = &d.causes[0].involved;
        assert_eq!(
            inv.first().map(|i| (i.name.as_str(), i.share.as_str())),
            Some(("Rust compiler", "63% CPU"))
        );
        assert!(inv.len() <= MAX_INVOLVED && inv.windows(2).all(|w| w[0].name != w[1].name));
        // Not an app: heat names nobody; a quiet machine has no window.
        let mut c = Case::new();
        c.each(|s| s.cpu.total = 99.0);
        assert!(c.run().causes[0].involved.is_empty());
        let empty = Case {
            samples: vec![],
            procs: vec![],
            throttle: ThrottleContext::default(),
        };
        assert_eq!(empty.run().window_secs, 0);
    }

    #[test]
    fn updates_antivirus_and_indexing_are_recognised() {
        for (name, kind) in [
            ("TiWorker.exe", CauseKind::OsUpdate),
            ("MsMpEng.exe", CauseKind::AntivirusScan),
            ("SearchIndexer.exe", CauseKind::Indexing),
        ] {
            let mut c = Case::new();
            c.each(|s| s.cpu.total = 85.0);
            c.procs.push(proc(30, None, name, 55.0, 100));
            assert_eq!(c.run().causes[0].kind, kind, "{name}");
        }
        let mut c = Case::new();
        c.each(|s| s.cpu.total = 30.0);
        c.procs.push(proc(30, None, "x.exe", 25.0, 1));
        assert_eq!(c.run().causes[0].kind, CauseKind::NothingWrong, "moderate load is fine");
        c.procs.clear();
        c.each(|s| s.cpu.total = 99.0);
        assert_eq!(c.run().causes[0].kind, CauseKind::NothingWrong, "no apps to blame");
    }

    #[test]
    fn memory_pressure_from_the_os_or_derived() {
        let mut c = Case::new();
        c.each(|s| {
            s.memory.pressure = Reading::Available(MemoryPressure::Critical);
            s.memory.swap_used = 3 << 30;
        });
        let d = c.run();
        assert_eq!(d.causes[0].kind, CauseKind::MemoryPressure);
        assert_eq!(d.causes[0].title, "Your computer is out of memory");
        assert!(d.causes[0].evidence.iter().any(|e| e.label == "Swap / page file in use"));
        assert_eq!(d.causes[0].hints[0].link, Some(HintLink::TaskManager));
        let mut c = Case::new();
        c.each(|s| {
            s.memory.pressure = Reading::unavailable("n/a");
            s.memory.available = s.memory.total / 10;
            s.memory.commit = Reading::unavailable("n/a");
        });
        c.procs.clear();
        let d = c.run();
        assert_eq!(d.causes[0].title, "Memory is running low");
        assert_eq!(d.causes[0].hints.len(), 1);
        let mut c = Case::new();
        c.each(|s| s.memory.commit = Reading::Available(Commit { used: 1, limit: 2 }));
        c.each(|s| s.memory.pressure = Reading::Available(MemoryPressure::Elevated));
        c.procs = vec![proc(3, None, "System", 0.0, 9000)];
        let d = c.run();
        assert_eq!(d.causes[0].hints.len(), 1);
    }

    #[test]
    fn disk_queue_thermal_space_and_gpu() {
        let mut c = Case::new();
        c.each(|s| {
            s.disk_io.queue = Reading::Available(4.2);
            s.disk_io.busy = Reading::Available(99.0);
        });
        c.procs[0].disk_write_bps = crate::process::Measured::Value(50e6);
        let d = c.run();
        assert_eq!(d.causes[0].kind, CauseKind::DiskQueue);
        assert!(d.causes[0].evidence.iter().any(|e| e.label == "Busiest: Google Chrome"));

        let mut c = Case::new();
        c.each(|s| {
            s.cpu.total = 60.0;
            s.cpu.frequency_mhz = 1_200;
            s.temperatures = Reading::Available(vec![Temperature {
                label: "CPU package".into(),
                celsius: 99.0,
                critical: Some(100.0),
            }]);
        });
        let d = c.run();
        let t = d.causes.iter().find(|c| c.kind == CauseKind::ThermalThrottling).unwrap();
        assert_eq!(t.confidence, 0.7, "hot and slow, but nothing from the system itself");
        assert_eq!(t.evidence.len(), 3, "load, clock, temperature");
        assert!(t.title.contains("heat") && t.explanation.contains("Confidence: medium"), "{t:?}");
        let mut c = Case::new();
        c.each(|s| {
            s.cpu.total = 60.0;
            s.cpu.frequency_mhz = 1_200;
        });
        let d = c.run();
        assert_eq!(d.causes[0].confidence, 0.5, "slow with no explanation is only a hint");
        assert!(
            !d.causes[0].title.contains("cool"),
            "no unsupported claim about heat: {}",
            d.causes[0].title
        );

        // The system saying so is the strongest evidence, and a cause is named.
        let mut c = Case::new();
        c.each(|s| {
            s.cpu.total = 80.0;
            s.cpu.frequency_mhz = 0;
            s.cpu.base_frequency_mhz = 0;
        });
        c.throttle.os = OsThrottle {
            speed_limit_percent: Some(60),
            thermal: Some(true),
            power: None,
        };
        assert_eq!(c.run().causes[0].confidence, 0.9);

        let mut c = Case::new();
        c.each(|s| s.disks[0].free = 512 << 20);
        let d = c.run();
        assert_eq!(d.causes[0].kind, CauseKind::LowDiskSpace);
        assert_eq!(d.causes[0].confidence, 0.8);
        c.each(|s| s.disks[0].free = 3 << 30);
        assert_eq!(c.run().causes[0].confidence, 0.5);

        let mut c = Case::new();
        c.each(|s| {
            s.gpu = Reading::Available(GpuSample {
                name: "RTX".into(),
                utilization: 97.0,
                memory_used: 0,
                memory_total: 0,
            })
        });
        assert_eq!(c.run().causes[0].kind, CauseKind::GpuBusy);
    }
}
