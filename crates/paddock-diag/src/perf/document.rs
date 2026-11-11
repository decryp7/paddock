//! `performance.json` (versioned) and its readable summary, built from plain values: pure, no clock, no I/O.

use super::machine::MachineFacts;
use super::ring::{Phase, SCHEMA, Slow};
use serde::Serialize;

/// `performance.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// Version of this layout ([`SCHEMA`]).
    pub schema: u32,
    /// When it was captured, milliseconds since the Unix epoch.
    pub captured_at_ms: u64,
    /// The machine.
    pub machine: MachineFacts,
    /// The startup phases, in order.
    pub startup: Vec<Phase>,
    /// The slowest operations of the last 24 hours, slowest first.
    pub slowest: Vec<Slow>,
}

impl Document {
    /// A document over the parts.
    pub fn new(
        captured_at_ms: u64,
        machine: MachineFacts,
        startup: Vec<Phase>,
        slowest: Vec<Slow>,
    ) -> Self {
        Document {
            schema: SCHEMA,
            captured_at_ms,
            machine,
            startup,
            slowest,
        }
    }

    /// The document as JSON.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// The document as pretty-printed JSON text, ending in a newline.
    pub fn to_json_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    /// The short summary a person reads first.
    pub fn summary(&self) -> String {
        let m = &self.machine;
        let mut s = String::from("Performance summary\n===================\n\n");
        let cpu = if m.cpu_model.is_empty() {
            "processor not named"
        } else {
            m.cpu_model.as_str()
        };
        let cores = match m.cpu_physical_cores {
            Some(p) => format!("{} cores, {} threads", p, m.cpu_logical_cores),
            None => format!("{} threads", m.cpu_logical_cores),
        };
        s.push_str(&format!("Machine: {cpu}, {cores}\n"));
        s.push_str(&format!(
            "Memory:  {} total, {} available\n",
            bytes(m.memory_total_bytes),
            bytes(m.memory_free_bytes)
        ));
        match &m.data_disk {
            Some(d) => s.push_str(&format!(
                "Disk:    the data folder is on a {} of {}, {} free\n",
                d.kind.word(),
                bytes(d.total_bytes),
                bytes(d.free_bytes)
            )),
            None => s.push_str("Disk:    the data folder's volume could not be found\n"),
        }
        s.push_str(&format!(
            "System:  {}\n",
            if m.os.is_empty() { "not named" } else { &m.os }
        ));
        match m.load_average {
            Some([a, b, c]) => s.push_str(&format!(
                "Load:    {a:.2} (1 min), {b:.2} (5 min), {c:.2} (15 min)\n"
            )),
            None => s.push_str("Load:    not available on this system\n"),
        }
        s.push_str("\nStartup\n");
        if self.startup.is_empty() {
            s.push_str("  no phases were recorded\n");
        } else {
            let total = self.startup.last().map(|p| p.at_ms).unwrap_or(0);
            s.push_str(&format!(
                "  {} phases, {} until the last\n",
                self.startup.len(),
                ms(total)
            ));
            for p in &self.startup {
                s.push_str(&format!(
                    "  {:<28} {:>9}  (ended at {})\n",
                    p.name,
                    ms(p.duration_ms),
                    ms(p.at_ms)
                ));
            }
        }
        s.push_str("\nSlowest operations of the last 24 hours\n");
        if self.slowest.is_empty() {
            s.push_str("  none recorded\n");
        } else {
            for e in &self.slowest {
                let ago = self.captured_at_ms.saturating_sub(e.at_ms);
                s.push_str(&format!(
                    "  {:<28} {:<14} {:>9}  {} ago\n",
                    e.op,
                    e.phase,
                    ms(e.duration_ms),
                    ms_coarse(ago)
                ));
            }
        }
        s
    }
}

/// Bytes as GiB or MiB with one decimal.
pub fn bytes(n: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let f = n as f64;
    if f >= GIB {
        format!("{:.1} GiB", f / GIB)
    } else {
        format!("{:.0} MiB", f / MIB)
    }
}

/// Milliseconds as `850 ms`, `12.4 s` or `3 min 05 s`.
pub fn ms(n: u64) -> String {
    if n < 1000 {
        format!("{n} ms")
    } else if n < 60_000 {
        format!("{:.1} s", n as f64 / 1000.0)
    } else {
        format!("{} min {:02} s", n / 60_000, (n % 60_000) / 1000)
    }
}

/// An age as `5 min` or `3 h`.
fn ms_coarse(n: u64) -> String {
    match n {
        0..=59_999 => "under 1 min".into(),
        60_000..=3_599_999 => format!("{} min", n / 60_000),
        _ => format!("{} h", n / 3_600_000),
    }
}

#[cfg(test)]
mod tests {
    use super::super::machine::{DataDisk, DiskKind};
    use super::*;

    fn machine() -> MachineFacts {
        MachineFacts {
            cpu_model: "Test CPU 9000".into(),
            cpu_logical_cores: 16,
            cpu_physical_cores: Some(8),
            memory_total_bytes: 32 * 1024 * 1024 * 1024,
            memory_free_bytes: 5 * 1024 * 1024 * 1024 / 2,
            data_disk: Some(DataDisk {
                kind: DiskKind::Ssd,
                total_bytes: 512 * 1024 * 1024 * 1024,
                free_bytes: 100 * 1024 * 1024 * 1024,
            }),
            os: "TestOS 1.2".into(),
            load_average: Some([1.0, 2.5, 3.0]),
        }
    }

    fn doc() -> Document {
        let startup = vec![
            Phase {
                name: "logging".into(),
                at_ms: 40,
                duration_ms: 40,
            },
            Phase {
                name: "window-ready".into(),
                at_ms: 1840,
                duration_ms: 1800,
            },
        ];
        let slowest = vec![
            Slow {
                op: "import-files".into(),
                phase: "parse".into(),
                duration_ms: 125_000,
                at_ms: 2_800_000,
            },
            Slow {
                op: "query".into(),
                phase: "total".into(),
                duration_ms: 850,
                at_ms: 999_000,
            },
        ];
        Document::new(10_000_000, machine(), startup, slowest)
    }

    #[test]
    fn the_json_is_versioned_and_camel_case() {
        let v = doc().to_json();
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["capturedAtMs"], 10_000_000);
        assert_eq!(v["machine"]["cpuLogicalCores"], 16);
        assert_eq!(v["machine"]["dataDisk"]["kind"], "ssd");
        assert_eq!(v["startup"][1]["durationMs"], 1800);
        assert_eq!(v["slowest"][0]["op"], "import-files");
    }

    #[test]
    fn the_summary_reads_in_words() {
        let s = doc().summary();
        for want in [
            "Test CPU 9000, 8 cores, 16 threads",
            "32.0 GiB total, 2.5 GiB available",
            "SSD of 512.0 GiB, 100.0 GiB free",
            "TestOS 1.2",
            "1.00 (1 min), 2.50 (5 min), 3.00 (15 min)",
            "2 phases, 1.8 s until the last",
            "window-ready",
            "import-files",
            "2 min 05 s",
            "850 ms",
            "2 h ago",
        ] {
            assert!(s.contains(want), "{want}\n{s}");
        }
    }

    #[test]
    fn the_summary_says_what_is_missing() {
        let mut m = machine();
        m.cpu_model.clear();
        m.cpu_physical_cores = None;
        m.data_disk = None;
        m.os.clear();
        m.load_average = None;
        let s = Document::new(1, m, vec![], vec![]).summary();
        for want in [
            "processor not named, 16 threads",
            "could not be found",
            "not named",
            "not available on this system",
            "no phases were recorded",
            "none recorded",
        ] {
            assert!(s.contains(want), "{want}\n{s}");
        }
    }

    #[test]
    fn units_scale() {
        assert_eq!(bytes(3 * 1024 * 1024), "3 MiB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
        assert_eq!(
            (ms(999), ms(1000), ms(61_000)),
            ("999 ms".into(), "1.0 s".into(), "1 min 01 s".into())
        );
        assert_eq!(
            (ms_coarse(10), ms_coarse(120_000), ms_coarse(7_300_000)),
            ("under 1 min".into(), "2 min".into(), "2 h".into())
        );
    }
}
