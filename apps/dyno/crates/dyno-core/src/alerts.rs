//! Alerts: low disk, overheating, memory pressure, crash loops and long high-CPU apps — each with a sustain time and a
//! cool-down so one problem is one notification, not one per tick.

use crate::anomaly::{Anomaly, AnomalyKind};
use crate::grouping::AppGroup;
use crate::health::Severity;
use crate::metrics::{derive_pressure, percent, MemoryPressure, SystemSample};
use crate::time::{Timestamp, HOUR, MINUTE, SECOND};
use crate::units::bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Kinds of alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlertKind {
    /// A disk is nearly full.
    LowDisk,
    /// A sensor is too hot.
    Overheating,
    /// Memory is critically short.
    MemoryPressure,
    /// An app keeps restarting.
    CrashLoop,
    /// An app used a lot of CPU for a long time.
    LongHighCpu,
}

/// One alert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// Id.
    pub id: String,
    /// When.
    pub at: Timestamp,
    /// Kind.
    pub kind: AlertKind,
    /// Severity.
    pub severity: Severity,
    /// One line.
    pub title: String,
    /// Detail.
    pub detail: String,
}

/// The user's alert settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertSettings {
    /// Alerts on.
    pub enabled: bool,
    /// Low disk below this free percentage (1–50).
    pub low_disk_percent: f32,
    /// Overheating at or above this temperature (60–110 °C).
    pub overheat_celsius: f32,
    /// An app above 50 % CPU for this many minutes (1–240).
    pub long_high_cpu_minutes: u32,
}

impl Default for AlertSettings {
    fn default() -> Self {
        AlertSettings {
            enabled: true,
            low_disk_percent: 10.0,
            overheat_celsius: 90.0,
            long_high_cpu_minutes: 15,
        }
    }
}

impl AlertSettings {
    /// Clamped to the allowed ranges.
    pub fn clamped(self) -> AlertSettings {
        AlertSettings {
            enabled: self.enabled,
            low_disk_percent: self.low_disk_percent.clamp(1.0, 50.0),
            overheat_celsius: self.overheat_celsius.clamp(60.0, 110.0),
            long_high_cpu_minutes: self.long_high_cpu_minutes.clamp(1, 240),
        }
    }
}

/// CPU share counted as "high" for [`AlertKind::LongHighCpu`].
pub const HIGH_CPU: f32 = 50.0;

/// Remembers what is building up and what fired recently.
#[derive(Debug, Default)]
pub struct Alerts {
    since: HashMap<String, Timestamp>,
    fired: HashMap<String, Timestamp>,
    counter: u64,
}

impl Alerts {
    /// A fresh evaluator.
    pub fn new() -> Alerts {
        Alerts::default()
    }

    /// Evaluates one tick.
    pub fn tick(&mut self, settings: &AlertSettings, s: &SystemSample, groups: &[AppGroup], anomalies: &[Anomaly]) -> Vec<Alert> {
        let st = settings.clamped();
        if !st.enabled {
            self.since.clear();
            return Vec::new();
        }
        let now = s.at;
        let mut out = Vec::new();
        let mut live: Vec<String> = Vec::new();

        for d in s.disks.iter().filter(|d| !d.removable && d.total > 0) {
            let free = percent(d.free, d.total);
            if free < st.low_disk_percent {
                let key = format!("lowDisk:{}", d.id);
                live.push(key.clone());
                let sev = if free < 3.0 { Severity::Critical } else { Severity::Warning };
                self.maybe(
                    &mut out,
                    key,
                    now,
                    0,
                    6 * HOUR,
                    AlertKind::LowDisk,
                    sev,
                    format!("{} is almost full", d.name),
                    format!("{} free ({free:.0}%).", bytes(d.free)),
                );
            }
        }
        if let Some(t) = s.hottest().filter(|t| t.celsius >= st.overheat_celsius) {
            live.push("heat".into());
            self.maybe(
                &mut out,
                "heat".into(),
                now,
                MINUTE,
                30 * MINUTE,
                AlertKind::Overheating,
                Severity::Warning,
                format!("{} is overheating", t.label),
                format!("{:.0} °C for over a minute. Check the vents and what is running.", t.celsius),
            );
        }
        let pressure = s.memory.pressure.value().copied().unwrap_or_else(|| derive_pressure(&s.memory));
        if pressure == MemoryPressure::Critical {
            live.push("memory".into());
            self.maybe(
                &mut out,
                "memory".into(),
                now,
                MINUTE,
                30 * MINUTE,
                AlertKind::MemoryPressure,
                Severity::Warning,
                "Your PC is out of memory".into(),
                format!(
                    "{} of {} in use. Close apps you don't need.",
                    bytes(s.memory.used),
                    bytes(s.memory.total)
                ),
            );
        }
        for a in anomalies.iter().filter(|a| a.kind == AnomalyKind::RestartLoop) {
            let key = format!("crash:{}", a.app);
            live.push(key.clone());
            self.maybe(
                &mut out,
                key,
                now,
                0,
                HOUR,
                AlertKind::CrashLoop,
                Severity::Warning,
                a.title.clone(),
                a.explanation.clone(),
            );
        }
        let sustain = i64::from(st.long_high_cpu_minutes) * MINUTE;
        for g in groups.iter().filter(|g| g.cpu > HIGH_CPU) {
            let key = format!("cpu:{}", g.id);
            live.push(key.clone());
            self.maybe(
                &mut out,
                key,
                now,
                sustain,
                HOUR,
                AlertKind::LongHighCpu,
                Severity::Info,
                format!(
                    "{} has used over {HIGH_CPU:.0}% CPU for {} minutes",
                    g.name, st.long_high_cpu_minutes
                ),
                "Check the task manager to see what it is doing.".into(),
            );
        }
        self.since.retain(|k, _| live.contains(k));
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn maybe(
        &mut self,
        out: &mut Vec<Alert>,
        key: String,
        now: Timestamp,
        sustain: i64,
        cooldown: i64,
        kind: AlertKind,
        severity: Severity,
        title: String,
        detail: String,
    ) {
        let since = *self.since.entry(key.clone()).or_insert(now);
        if now.since(since) < sustain.max(0) {
            return;
        }
        if self.fired.get(&key).is_some_and(|f| now.since(*f) < cooldown) {
            return;
        }
        self.fired.insert(key, now);
        self.counter += 1;
        out.push(Alert {
            id: format!("alert-{}-{}", now.millis() / SECOND, self.counter),
            at: now,
            kind,
            severity,
            title,
            detail,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixtures::sample;
    use crate::metrics::{Reading, Temperature};

    fn group(cpu: f32) -> AppGroup {
        AppGroup {
            id: "exe:hog".into(),
            name: "Hog".into(),
            exe: None,
            pids: vec![1],
            cpu,
            memory: 0,
            disk_bps: 0.0,
        }
    }

    #[test]
    fn low_disk_fires_once_per_cooldown() {
        let mut a = Alerts::new();
        let st = AlertSettings::default();
        let mut s = sample(0);
        s.disks[0].free = 10 << 30; // 2%
        let first = a.tick(&st, &s, &[], &[]);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].severity, Severity::Critical);
        s.at = Timestamp(HOUR);
        assert!(a.tick(&st, &s, &[], &[]).is_empty());
        s.at = Timestamp(7 * HOUR);
        s.disks[0].free = 30 << 30; // 6%
        let again = a.tick(&st, &s, &[], &[]);
        assert_eq!(again[0].severity, Severity::Warning);
        assert_ne!(again[0].id, first[0].id);
        s.disks[0].removable = true;
        s.at = Timestamp(20 * HOUR);
        assert!(a.tick(&st, &s, &[], &[]).is_empty(), "removable disks don't alert");
    }

    #[test]
    fn heat_and_memory_need_a_minute() {
        let mut a = Alerts::new();
        let st = AlertSettings::default();
        let mut s = sample(0);
        s.temperatures = Reading::Available(vec![Temperature {
            label: "CPU".into(),
            celsius: 95.0,
            critical: None,
        }]);
        s.memory.pressure = Reading::Available(MemoryPressure::Critical);
        assert!(a.tick(&st, &s, &[], &[]).is_empty());
        s.at = Timestamp(MINUTE);
        let fired = a.tick(&st, &s, &[], &[]);
        let kinds: Vec<AlertKind> = fired.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, [AlertKind::Overheating, AlertKind::MemoryPressure]);
        // Cooling down resets the build-up.
        s.temperatures = Reading::unavailable("x");
        s.at = Timestamp(2 * MINUTE);
        a.tick(&st, &s, &[], &[]);
        assert!(!a.since.contains_key("heat"));
    }

    #[test]
    fn long_high_cpu_and_crash_loops() {
        let mut a = Alerts::new();
        let st = AlertSettings {
            long_high_cpu_minutes: 2,
            ..Default::default()
        };
        let s0 = sample(0);
        assert!(a.tick(&st, &s0, &[group(80.0)], &[]).is_empty());
        let mut s = sample(2 * MINUTE);
        s.at = Timestamp(2 * MINUTE);
        let fired = a.tick(&st, &s, &[group(80.0)], &[]);
        assert_eq!(fired[0].kind, AlertKind::LongHighCpu);
        assert!(fired[0].title.contains("for 2 minutes"));
        let loop_ = Anomaly {
            id: "restartLoop:X".into(),
            kind: AnomalyKind::RestartLoop,
            app: "X".into(),
            since: Timestamp(0),
            title: "X restarted".into(),
            explanation: "e".into(),
            evidence: vec![],
        };
        let mut leak = loop_.clone();
        leak.kind = AnomalyKind::MemoryGrowth;
        let fired = a.tick(&st, &s, &[], &[loop_, leak]);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].kind, AlertKind::CrashLoop);
    }

    #[test]
    fn disabled_and_clamped() {
        let mut a = Alerts::new();
        let off = AlertSettings {
            enabled: false,
            ..Default::default()
        };
        let mut s = sample(0);
        s.disks[0].free = 0;
        assert!(a.tick(&off, &s, &[], &[]).is_empty());
        let c = AlertSettings {
            enabled: true,
            low_disk_percent: 99.0,
            overheat_celsius: 0.0,
            long_high_cpu_minutes: 0,
        }
        .clamped();
        assert_eq!((c.low_disk_percent, c.overheat_celsius, c.long_high_cpu_minutes), (50.0, 60.0, 1));
    }
}
