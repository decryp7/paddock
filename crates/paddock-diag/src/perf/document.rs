//! `performance.json` (versioned) and its readable summary, built from plain values: pure, no clock, no I/O.

use super::machine::MachineFacts;
use super::ring::{Phase, SCHEMA, Slow};
use serde::Serialize;

/// One slow operation as the document lists it: how long ago instead of a clock time, so that no number of 13 digits is
/// written (the text redactor masks such a run as a card number about one time in ten and breaks the JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// What ran.
    pub op: String,
    /// The phase that took the time.
    pub phase: String,
    /// How long, milliseconds.
    pub duration_ms: u64,
    /// How long before the capture it ended, seconds.
    pub age_seconds: u64,
}

/// `performance.json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// Version of this layout ([`SCHEMA`]).
    pub schema: u32,
    /// When it was captured, RFC 3339 (UTC).
    pub captured_at: String,
    /// The machine.
    pub machine: MachineFacts,
    /// The startup phases, in order.
    pub startup: Vec<Phase>,
    /// The slowest operations of the last 24 hours, slowest first.
    pub slowest: Vec<Row>,
}

impl Document {
    /// A document over the parts; `captured_at_ms` is milliseconds since the Unix epoch.
    pub fn new(
        captured_at_ms: u64,
        machine: MachineFacts,
        startup: Vec<Phase>,
        slowest: Vec<Slow>,
    ) -> Self {
        let captured_at = i64::try_from(captured_at_ms)
            .ok()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            .unwrap_or_default();
        Document {
            schema: SCHEMA,
            captured_at,
            machine,
            startup,
            slowest: slowest
                .into_iter()
                .map(|e| Row {
                    op: e.op,
                    phase: e.phase,
                    duration_ms: e.duration_ms,
                    age_seconds: captured_at_ms.saturating_sub(e.at_ms) / 1000,
                })
                .collect(),
        }
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
        s.push_str(&format!("Captured: {}\n", self.captured_at));
        s.push_str(&format!("Machine: {cpu}, {cores}\n"));
        s.push_str(&format!(
            "Memory:  {} total, {} available\n",
            mib_text(m.memory_total_mib),
            mib_text(m.memory_free_mib)
        ));
        match &m.data_disk {
            Some(d) => s.push_str(&format!(
                "Disk:    the data folder is on a {} of {}, {} free\n",
                d.kind.word(),
                mib_text(d.total_mib),
                mib_text(d.free_mib)
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
                s.push_str(&format!(
                    "  {:<28} {:<14} {:>9}  {} ago\n",
                    e.op,
                    e.phase,
                    ms(e.duration_ms),
                    ms_coarse(e.age_seconds.saturating_mul(1000))
                ));
            }
        }
        s
    }
}

/// MiB as GiB or MiB with one decimal.
pub fn mib_text(n: u64) -> String {
    if n >= 1024 {
        format!("{:.1} GiB", n as f64 / 1024.0)
    } else {
        format!("{n} MiB")
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
            memory_total_mib: 32 * 1024,
            memory_free_mib: 2560,
            data_disk: Some(DataDisk {
                kind: DiskKind::Ssd,
                total_mib: 512 * 1024,
                free_mib: 100 * 1024,
            }),
            os: "TestOS 1.2".into(),
            load_average: Some([1.0, 2.5, 3.0]),
        }
    }

    const CAPTURED: u64 = 1_794_362_400_000; // 2026-11-11T02:00:00Z

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
                at_ms: CAPTURED - 7_200_000,
            },
            Slow {
                op: "query".into(),
                phase: "total".into(),
                duration_ms: 850,
                at_ms: CAPTURED - 1000,
            },
        ];
        Document::new(CAPTURED, machine(), startup, slowest)
    }

    #[test]
    fn the_json_is_versioned_and_camel_case() {
        let v: serde_json::Value = serde_json::from_str(&doc().to_json_text()).unwrap();
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["capturedAt"], "2026-11-11T02:00:00Z");
        assert_eq!(v["machine"]["cpuLogicalCores"], 16);
        assert_eq!(v["machine"]["memoryTotalMib"], 32 * 1024);
        assert_eq!(v["machine"]["dataDisk"]["kind"], "ssd");
        assert_eq!(v["machine"]["dataDisk"]["freeMib"], 100 * 1024);
        assert_eq!(v["startup"][1]["durationMs"], 1800);
        assert_eq!(v["slowest"][0]["op"], "import-files");
        assert_eq!(v["slowest"][0]["ageSeconds"], 7200);
        assert_eq!(v["slowest"][1]["ageSeconds"], 1);
        assert!(v["slowest"][0].get("atMs").is_none());
    }

    #[test]
    fn a_time_that_cannot_be_a_date_gives_an_empty_capture_time_not_a_panic() {
        let d = Document::new(u64::MAX, machine(), vec![], vec![]);
        assert_eq!(d.captured_at, "");
    }

    /// P1-1: the text redactor masks a run of 13 to 19 digits that passes the Luhn check (a card number) as an unquoted
    /// `[redacted]`, which breaks the JSON. About one 13-digit time in ten passes, so no number of 13 digits may be written.
    #[test]
    fn the_json_survives_the_redactor_for_any_time_and_size() {
        let redactor = crate::Redactor::new();
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..3000 {
            let at = 1_700_000_000_000 + next() % 400_000_000_000;
            let mut m = machine();
            m.memory_total_mib = (next() % (64 << 40)) >> 20;
            m.memory_free_mib = (next() % (64 << 40)) >> 20;
            m.data_disk = Some(DataDisk {
                kind: DiskKind::Hdd,
                total_mib: (next() % (64 << 40)) >> 20,
                free_mib: (next() % (64 << 40)) >> 20,
            });
            m.load_average = Some([(next() % 100_000) as f64 / 100.0, 0.0, 0.0]);
            let slowest = (0..5)
                .map(|i| Slow::new("op", "p", next(), at - i * (next() % 86_400_000)))
                .collect();
            let startup = vec![Phase {
                name: "p".into(),
                at_ms: next() % 100_000_000,
                duration_ms: next() % 100_000_000,
            }];
            let text = Document::new(at, m, startup, slowest).to_json_text();
            let (masked, counts) = redactor.text(&text);
            assert_eq!(
                counts,
                Default::default(),
                "the redactor changed the document:\n{masked}"
            );
            assert!(
                serde_json::from_str::<serde_json::Value>(&masked).is_ok(),
                "{masked}"
            );
        }
    }

    #[test]
    fn the_summary_reads_in_words() {
        let s = doc().summary();
        for want in [
            "Captured: 2026-11-11T02:00:00Z",
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
        assert_eq!(
            (mib_text(3), mib_text(1023), mib_text(3 * 1024)),
            ("3 MiB".into(), "1023 MiB".into(), "3.0 GiB".into())
        );
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
