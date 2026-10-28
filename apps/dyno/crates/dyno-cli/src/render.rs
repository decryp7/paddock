//! Output. `--json` prints data (stable, camelCase, the same shapes as the app's contract); the human form is a few
//! aligned lines. Neither includes command lines or file contents: names of programs, numbers and counter paths only.

use crate::args::TopBy;
use crate::connection::AgentInfo;
use dyno_core::alarms::{Alarm, AlarmState};
use dyno_core::counters::{CounterHit, CounterObject, CounterObjectList, CounterReading, CounterUnit};
use dyno_core::diagnosis::Diagnosis;
use dyno_core::grouping::AppGroup;
use dyno_core::health::Severity;
use dyno_core::metrics::{MemoryPressure, Reading};
use dyno_core::protocol::LiveUpdate;
use dyno_core::units::{bytes, duration};
use serde::Serialize;

/// Pretty JSON for any serialisable value (the structures here cannot fail to serialise; a failure prints `null`).
pub fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| "null".into())
}

/// `status --json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusJson {
    /// The agent.
    pub agent: AgentJson,
    /// CPU load, percent of the machine.
    pub cpu_percent: f32,
    /// Memory in use, bytes.
    pub memory_used_bytes: u64,
    /// Installed memory, bytes.
    pub memory_total_bytes: u64,
    /// The OS's memory pressure, when it says.
    pub memory_pressure: Option<MemoryPressure>,
    /// Volumes.
    pub disks: Vec<DiskJson>,
    /// Network, bytes per second.
    pub network_down_bps: f64,
    /// Network, bytes per second.
    pub network_up_bps: f64,
    /// Seconds since boot.
    pub uptime_secs: u64,
    /// Dyno's own cost.
    pub dyno_cpu_percent: f32,
}

/// The agent in `status --json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentJson {
    /// Process id.
    pub pid: u32,
    /// Build.
    pub version: String,
    /// Protocol version.
    pub protocol: u32,
}

/// A volume in `status --json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskJson {
    /// Display name.
    pub name: String,
    /// Free bytes.
    pub free_bytes: u64,
    /// Capacity.
    pub total_bytes: u64,
}

/// An app in `top --json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopJson {
    /// Friendly name.
    pub name: String,
    /// How many processes.
    pub processes: usize,
    /// CPU, percent of the machine.
    pub cpu_percent: f32,
    /// Resident memory, bytes.
    pub memory_bytes: u64,
    /// Disk throughput, bytes per second.
    pub disk_bps: f64,
}

/// The status as data.
pub fn status_data(u: &LiveUpdate, agent: &AgentInfo) -> StatusJson {
    let s = &u.system;
    StatusJson {
        agent: AgentJson {
            pid: agent.pid,
            version: agent.version.clone(),
            protocol: agent.protocol,
        },
        cpu_percent: s.cpu.total,
        memory_used_bytes: s.memory.used,
        memory_total_bytes: s.memory.total,
        memory_pressure: s.memory.pressure.value().copied(),
        disks: s
            .disks
            .iter()
            .map(|d| DiskJson {
                name: d.name.clone(),
                free_bytes: d.free,
                total_bytes: d.total,
            })
            .collect(),
        network_down_bps: s.network.iter().map(|n| n.rx_bps).sum(),
        network_up_bps: s.network.iter().map(|n| n.tx_bps).sum(),
        uptime_secs: s.uptime_secs,
        dyno_cpu_percent: u.overhead.agent_cpu,
    }
}

fn rate(bps: f64) -> String {
    format!("{}/s", bytes(bps.max(0.0) as u64))
}

fn pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        "0%".into()
    } else {
        format!("{:.0}%", part as f64 * 100.0 / whole as f64)
    }
}

/// The status as lines.
pub fn status_text(u: &LiveUpdate, agent: &AgentInfo) -> String {
    let s = &u.system;
    let mut out = format!("Dyno {} (agent {}, protocol {})\n", agent.version, agent.pid, agent.protocol);
    out += &format!("CPU       {:.0}%\n", s.cpu.total);
    let pressure = match &s.memory.pressure {
        Reading::Available(MemoryPressure::Normal) => ", pressure normal",
        Reading::Available(MemoryPressure::Elevated) => ", pressure elevated",
        Reading::Available(MemoryPressure::Critical) => ", pressure critical",
        Reading::Unavailable(_) => "",
    };
    out += &format!(
        "Memory    {} of {} ({}){}\n",
        bytes(s.memory.used),
        bytes(s.memory.total),
        pct(s.memory.used, s.memory.total),
        pressure
    );
    if s.memory.swap_total > 0 {
        out += &format!("Swap      {} of {}\n", bytes(s.memory.swap_used), bytes(s.memory.swap_total));
    }
    for d in &s.disks {
        out += &format!("Disk      {}: {} free of {}\n", d.name, bytes(d.free), bytes(d.total));
    }
    let (down, up) = (
        s.network.iter().map(|n| n.rx_bps).sum::<f64>(),
        s.network.iter().map(|n| n.tx_bps).sum::<f64>(),
    );
    out += &format!("Network   down {}, up {}\n", rate(down), rate(up));
    if let Reading::Available(b) = &s.battery {
        out += &format!("Battery   {:.0}%\n", b.charge);
    }
    out += &format!("Uptime    {}\n", duration(s.uptime_secs));
    out += &format!("Dyno      {:.1}% CPU, {}\n", u.overhead.agent_cpu, bytes(u.overhead.agent_memory));
    out
}

/// The busiest apps, sorted and cut to `count`.
pub fn top_rows(u: &LiveUpdate, by: TopBy, count: usize) -> Vec<TopJson> {
    let list: &[AppGroup] = match by {
        TopBy::Cpu => &u.top_cpu,
        TopBy::Memory => &u.top_memory,
    };
    list.iter()
        .take(count)
        .map(|g| TopJson {
            name: g.name.clone(),
            processes: g.pids.len(),
            cpu_percent: g.cpu,
            memory_bytes: g.memory,
            disk_bps: g.disk_bps,
        })
        .collect()
}

/// A table of the busiest apps.
pub fn top_text(rows: &[TopJson]) -> String {
    if rows.is_empty() {
        return "Nothing is using noticeable resources right now.\n".into();
    }
    let w = rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(4).clamp(4, 40);
    let mut out = format!("{:<w$}  {:>6}  {:>10}  {:>4}\n", "App", "CPU", "Memory", "Procs");
    for r in rows {
        let name: String = r.name.chars().take(w).collect();
        out += &format!(
            "{name:<w$}  {:>5.1}%  {:>10}  {:>4}\n",
            r.cpu_percent,
            bytes(r.memory_bytes),
            r.processes
        );
    }
    out
}

/// The diagnosis in words.
pub fn why_slow_text(d: &Diagnosis) -> String {
    let mut out = format!("{}\n", d.headline);
    for c in &d.causes {
        out += &format!("\n{} ({:.0}% sure)\n  {}\n", c.title, c.confidence * 100.0, c.explanation);
        if !c.involved.is_empty() {
            let who: Vec<String> = c.involved.iter().map(|i| format!("{} ({})", i.name, i.share)).collect();
            out += &format!("  Mostly: {}\n", who.join(", "));
        }
        for s in &c.suggestions {
            out += &format!("  You could: {}\n", s.label);
        }
    }
    out
}

fn severity(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Critical => "critical",
    }
}

/// Alarms as a list.
pub fn alarms_text(alarms: &[Alarm]) -> String {
    if alarms.is_empty() {
        return "No alarms need attention.\n".into();
    }
    let mut out = String::new();
    for a in alarms {
        let state = match a.state {
            AlarmState::Active => "active",
            AlarmState::Acknowledged => "acknowledged",
            AlarmState::Cleared => "cleared",
            AlarmState::ClearedUnacknowledged => "cleared, not acknowledged",
        };
        out += &format!("#{:<5} {:<9} {} ({state})\n", a.id, severity(a.severity), a.title);
    }
    out
}

fn unit(u: CounterUnit) -> &'static str {
    match u {
        CounterUnit::Percent => "%",
        CounterUnit::Bytes => "B",
        CounterUnit::BytesPerSec => "B/s",
        CounterUnit::PerSec => "/s",
        CounterUnit::Milliseconds => "ms",
        CounterUnit::Seconds => "s",
        CounterUnit::Count | CounterUnit::Number => "",
    }
}

/// The counter objects.
pub fn objects_text(l: &CounterObjectList) -> String {
    let w = l.objects.iter().map(|o| o.name.chars().count()).max().unwrap_or(6).clamp(6, 50);
    let mut out = format!("{:<w$}  {:>8}  {:>9}\n", "Object", "Counters", "Instances");
    for o in &l.objects {
        out += &format!("{:<w$}  {:>8}  {:>9}\n", o.name, o.counters, o.instances_total);
    }
    if l.truncated {
        out += "(The list was cut to keep it small; search by name to find the rest.)\n";
    }
    out
}

/// One object's counters and instances.
pub fn object_text(o: &CounterObject) -> String {
    let mut out = format!("{}\n", o.name);
    for c in &o.counters {
        let u = unit(c.unit);
        out += &format!("  \\{}\\{}  [{}]\n", o.name, c.name, if u.is_empty() { "number" } else { u });
    }
    if !o.instances.is_empty() {
        out += &format!("Instances ({}): {}\n", o.instances_total, o.instances.join(", "));
    }
    out
}

/// Search results.
pub fn hits_text(h: &[CounterHit]) -> String {
    if h.is_empty() {
        return "No counter matches those words.\n".into();
    }
    h.iter().map(|x| format!("{}\n", x.path)).collect()
}

/// Counter readings.
pub fn values_text(v: &[CounterReading]) -> String {
    v.iter()
        .map(|r| match &r.value {
            Reading::Available(x) => format!(
                "{}  {}{}{}\n",
                r.path,
                trim_number(*x),
                if unit(r.unit).is_empty() { "" } else { " " },
                unit(r.unit)
            ),
            Reading::Unavailable(why) => format!("{}  not available: {why}\n", r.path),
        })
        .collect()
}

fn trim_number(x: f64) -> String {
    if x.abs() >= 100.0 || x.fract() == 0.0 {
        format!("{x:.0}")
    } else {
        format!("{x:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::counters::{CounterInfo, CounterKind, CounterObjectSummary};
    use dyno_core::protocol::Overhead;
    use dyno_platform::fake::quiet_sample as sample;

    fn live() -> LiveUpdate {
        let mut system = sample(dyno_core::time::Timestamp(1));
        system.cpu.total = 34.4;
        system.memory.total = 8 << 30;
        system.memory.used = 2 << 30;
        LiveUpdate {
            system,
            pressure: MemoryPressure::Normal,
            top_cpu: vec![AppGroup {
                id: "a".into(),
                name: "Compiler".into(),
                exe: None,
                pids: vec![1, 2],
                cpu: 40.0,
                memory: 1 << 30,
                disk_bps: 0.0,
            }],
            top_memory: vec![],
            overhead: Overhead::default(),
            processes: None,
            groups: None,
        }
    }

    fn info() -> AgentInfo {
        AgentInfo {
            pid: 42,
            version: "0.1.0".into(),
            protocol: 1,
        }
    }

    #[test]
    fn status_in_words_and_as_stable_json() {
        let text = status_text(&live(), &info());
        assert!(text.contains("CPU       34%"));
        assert!(text.contains("Memory    2.0 GB of 8.0 GB (25%)"));
        let j: serde_json::Value = serde_json::from_str(&json(&status_data(&live(), &info()))).unwrap();
        assert_eq!(j["agent"]["pid"], 42);
        assert_eq!(j["memoryTotalBytes"], 8u64 << 30);
        assert!(j["cpuPercent"].is_number() && j["disks"].is_array());
    }

    #[test]
    fn top_sorts_by_the_chosen_key_and_cuts() {
        let rows = top_rows(&live(), TopBy::Cpu, 5);
        assert_eq!((rows[0].name.as_str(), rows[0].processes), ("Compiler", 2));
        assert!(top_rows(&live(), TopBy::Memory, 5).is_empty());
        assert!(top_text(&rows).contains("Compiler"));
        assert!(top_text(&[]).contains("Nothing"));
        assert_eq!(top_rows(&live(), TopBy::Cpu, 0).len(), 0);
    }

    #[test]
    fn alarms_and_counters_read_as_lines() {
        assert!(alarms_text(&[]).contains("No alarms"));
        let objects = CounterObjectList {
            objects: vec![CounterObjectSummary {
                name: "Memory".into(),
                counters: 4,
                instances_total: 0,
            }],
            truncated: true,
        };
        let t = objects_text(&objects);
        assert!(t.contains("Memory") && t.contains("cut"));
        let o = CounterObject {
            name: "Disk".into(),
            counters: vec![CounterInfo {
                name: "Reads".into(),
                unit: CounterUnit::PerSec,
                kind: CounterKind::Cumulative,
                help: None,
            }],
            instances: vec!["sda".into()],
            instances_total: 1,
        };
        let t = object_text(&o);
        assert!(t.contains("\\Disk\\Reads") && t.contains("sda"));
        assert!(hits_text(&[]).contains("No counter"));
        let vals = values_text(&[
            CounterReading {
                path: "\\M\\F".into(),
                unit: CounterUnit::Bytes,
                value: Reading::Available(1234.5),
            },
            CounterReading {
                path: "\\M\\G".into(),
                unit: CounterUnit::Percent,
                value: Reading::Unavailable("gone".into()),
            },
            CounterReading {
                path: "\\M\\H".into(),
                unit: CounterUnit::Count,
                value: Reading::Available(0.5),
            },
        ]);
        assert!(vals.contains("\\M\\F  1234 B"));
        assert!(vals.contains("not available: gone"));
        assert!(vals.contains("\\M\\H  0.50"));
    }
}
