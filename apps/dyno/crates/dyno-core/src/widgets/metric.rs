//! What a dashboard widget or a menu-bar item measures: the metric, and the rule for which widgets can show which.

use serde::{Deserialize, Serialize};

/// Longest process name, in characters.
pub const MAX_TEXT: usize = 64;

/// What a widget measures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    /// Total CPU load.
    CpuTotal,
    /// Load per core.
    CpuPerCore,
    /// Memory in use.
    Memory,
    /// Memory pressure.
    MemoryPressure,
    /// Swap / page file in use.
    Swap,
    /// Disk activity.
    DiskActivity,
    /// Free space on the system disk.
    DiskFree,
    /// Network upload.
    NetworkUp,
    /// Network download.
    NetworkDown,
    /// Network upload and download together: the menu bar draws both rates stacked; elsewhere its value is the total.
    NetworkBoth,
    /// GPU load.
    GpuLoad,
    /// The hottest temperature sensor.
    Temperature,
    /// Fan speed.
    Fan,
    /// Battery charge.
    Battery,
    /// One app's CPU, by process name.
    Process(String),
    /// A Windows performance counter, by path.
    PerfCounter(String),
}

impl Metric {
    /// Whether the value is a 0-100 percentage (what a ring or a meter can show).
    pub fn is_percent(&self) -> bool {
        matches!(
            self,
            Metric::CpuTotal
                | Metric::Memory
                | Metric::MemoryPressure
                | Metric::Swap
                | Metric::DiskActivity
                | Metric::DiskFree
                | Metric::GpuLoad
                | Metric::Battery
                | Metric::Process(_)
        )
    }

    /// The metrics a picker offers, in display order (the two parameterised ones, a process and a counter, are chosen
    /// another way).
    pub const CHOICES: [Metric; 14] = [
        Metric::CpuTotal,
        Metric::CpuPerCore,
        Metric::Memory,
        Metric::MemoryPressure,
        Metric::Swap,
        Metric::DiskActivity,
        Metric::DiskFree,
        Metric::NetworkDown,
        Metric::NetworkUp,
        Metric::NetworkBoth,
        Metric::GpuLoad,
        Metric::Temperature,
        Metric::Fan,
        Metric::Battery,
    ];

    /// The metric's name in sentence case, as a picker and a tooltip say it ("CPU per core", "Process chrome").
    pub fn title(&self) -> String {
        match self {
            Metric::CpuTotal => "CPU".into(),
            Metric::CpuPerCore => "CPU per core".into(),
            Metric::Memory => "Memory".into(),
            Metric::MemoryPressure => "Memory pressure".into(),
            Metric::Swap => "Swap".into(),
            Metric::DiskActivity => "Disk activity".into(),
            Metric::DiskFree => "Disk free".into(),
            Metric::NetworkDown => "Network down".into(),
            Metric::NetworkUp => "Network up".into(),
            Metric::NetworkBoth => "Network (up and down)".into(),
            Metric::GpuLoad => "GPU".into(),
            Metric::Temperature => "Temperature".into(),
            Metric::Fan => "Fan".into(),
            Metric::Battery => "Battery".into(),
            Metric::Process(n) => format!("Process {n}"),
            Metric::PerfCounter(p) => format!("Counter {p}"),
        }
    }

    /// The metric as a sentence part: "process chrome", "Memory".
    pub fn label(&self) -> String {
        match self {
            Metric::Process(n) => format!("process {n}"),
            Metric::PerfCounter(p) => format!("counter {p}"),
            other => format!("{other:?}"),
        }
    }

    /// Checks a parameterised metric's argument; the message says what to change.
    pub fn validate(&self) -> Result<(), String> {
        const BAD: &str = "A process name is 1 to 64 characters; a counter is written \\Object(Instance)\\Counter.";
        match self {
            Metric::Process(s) if s.trim().is_empty() || s.chars().count() > MAX_TEXT => Err(BAD.into()),
            // A counter is a real path (`\Object(Instance)\Counter`, up to the counters' own limit), not free text.
            Metric::PerfCounter(s) if s.parse::<crate::counters::CounterPath>().is_err() => Err(BAD.into()),
            _ => Ok(()),
        }
    }
}
