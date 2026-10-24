use crate::error::{BenchError, Result};
use crate::kind::Kind;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The results file's format version.
pub const RESULTS_VERSION: u32 = 1;
/// Largest results or budgets file read.
pub const MAX_FILE_BYTES: u64 = 1 << 20;
/// Most measurements in one results file.
pub const MAX_MEASUREMENTS: usize = 1000;

/// One measured number. `value` is the median of `runs` runs, in the unit of the [`Kind`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    /// A stable name, the key a budget refers to (`import-clef`, `startup-cold`).
    pub name: String,
    /// What it measures.
    pub kind: Kind,
    /// The number.
    pub value: f64,
    /// How many runs it summarises.
    pub runs: u32,
}

/// The machine class the numbers come from (a result is only compared with budgets set on the same class).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    /// `macos`, `linux`, `windows`.
    pub os: String,
    /// `x86_64`, `aarch64`.
    pub arch: String,
    /// Logical cores, when known.
    pub cpus: Option<u32>,
}

impl Host {
    /// This machine.
    pub fn current() -> Host {
        Host {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            cpus: std::thread::available_parallelism()
                .ok()
                .and_then(|n| u32::try_from(n.get()).ok()),
        }
    }
}

/// One app's benchmark run, stored as `bench/<app>.results.json` (or wherever the app puts it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Results {
    /// Format version ([`RESULTS_VERSION`]).
    pub version: u32,
    /// The app (`debrief`).
    pub app: String,
    /// The app's version.
    pub app_version: String,
    /// The commit measured, when the caller knows it.
    pub commit: Option<String>,
    /// Where it ran.
    pub host: Host,
    /// The numbers, in the order they were recorded.
    pub measurements: Vec<Measurement>,
}

impl Results {
    /// An empty run on this machine.
    pub fn new(app: &str, app_version: &str) -> Results {
        Results {
            version: RESULTS_VERSION,
            app: app.into(),
            app_version: app_version.into(),
            commit: None,
            host: Host::current(),
            measurements: Vec::new(),
        }
    }

    /// The measurement named `name`.
    pub fn get(&self, name: &str) -> Option<&Measurement> {
        self.measurements.iter().find(|m| m.name == name)
    }

    /// Refuses what no benchmark can produce: no or duplicate names, negative or non-finite numbers, too many entries.
    pub fn validate(&self) -> Result<()> {
        if self.measurements.len() > MAX_MEASUREMENTS {
            return Err(BenchError::Invalid(format!(
                "more than {MAX_MEASUREMENTS} measurements; record fewer, larger ones."
            )));
        }
        for (i, m) in self.measurements.iter().enumerate() {
            if m.name.trim().is_empty() {
                return Err(BenchError::Invalid(
                    "a measurement has no name; give every measurement a stable name.".into(),
                ));
            }
            if !m.value.is_finite() || m.value < 0.0 {
                return Err(BenchError::Invalid(format!(
                    "measurement {} is {}; a measurement is a finite number, zero or more.",
                    m.name, m.value
                )));
            }
            if self.measurements[..i].iter().any(|o| o.name == m.name) {
                return Err(BenchError::Invalid(format!(
                    "measurement {} is recorded twice; names must be unique.",
                    m.name
                )));
            }
        }
        Ok(())
    }

    /// Reads a results file (bounded, validated, a newer version refused).
    pub fn load(path: &Path) -> Result<Results> {
        let text = read_bounded(path)?;
        Results::parse(&text, path)
    }

    /// Reads results from text; `path` names the source in errors.
    pub fn parse(text: &str, path: &Path) -> Result<Results> {
        let parse = |message: String| BenchError::Parse {
            path: path.into(),
            message,
        };
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| parse(e.to_string()))?;
        let found = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if found > u64::from(RESULTS_VERSION) {
            return Err(BenchError::NewerVersion {
                path: path.into(),
                found: u32::try_from(found).unwrap_or(u32::MAX),
            });
        }
        let results: Results = serde_json::from_value(value).map_err(|e| parse(e.to_string()))?;
        results.validate()?;
        Ok(results)
    }

    /// Writes the file atomically (a crash leaves the old results or the new ones).
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let mut text =
            serde_json::to_string_pretty(self).map_err(|e| BenchError::Write(e.to_string()))?;
        text.push('\n');
        paddock_env::write_atomic(path, text.as_bytes(), false)
            .map_err(|e| BenchError::Write(e.to_string()))
    }
}

/// Reads a text file of at most [`MAX_FILE_BYTES`].
pub(crate) fn read_bounded(path: &Path) -> Result<String> {
    let io = |op, source| BenchError::Io {
        op,
        path: path.into(),
        source,
    };
    let meta = std::fs::metadata(path).map_err(|e| io("read", e))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(BenchError::TooLarge {
            path: path.into(),
            limit: MAX_FILE_BYTES,
        });
    }
    std::fs::read_to_string(path).map_err(|e| io("read", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Results {
        let mut r = Results::new("demo", "1.2.3");
        r.commit = Some("abc123".into());
        r.measurements.push(Measurement {
            name: "import".into(),
            kind: Kind::Throughput,
            value: 1000.0,
            runs: 5,
        });
        r.measurements.push(Measurement {
            name: "startup".into(),
            kind: Kind::Startup,
            value: 420.5,
            runs: 3,
        });
        r
    }

    #[test]
    fn saves_and_loads_the_same_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bench").join("demo.results.json");
        sample().save(&path).unwrap();
        let back = Results::load(&path).unwrap();
        assert_eq!(back, sample());
        assert_eq!(back.get("startup").unwrap().value, 420.5);
        assert!(back.get("nope").is_none());
        assert!(!Host::current().os.is_empty());
    }

    #[test]
    fn refuses_what_no_benchmark_produces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.json");
        let mut r = sample();
        r.measurements[0].value = -1.0;
        assert!(matches!(r.save(&p), Err(BenchError::Invalid(m)) if m.contains("import")));
        assert!(!p.exists(), "nothing is written for an invalid run");
        let mut r = sample();
        r.measurements[0].value = f64::NAN;
        assert!(r.validate().is_err());
        let mut r = sample();
        r.measurements[1].name = "import".into();
        assert!(matches!(r.validate(), Err(BenchError::Invalid(m)) if m.contains("twice")));
        let mut r = sample();
        r.measurements[1].name = "  ".into();
        assert!(matches!(r.validate(), Err(BenchError::Invalid(m)) if m.contains("no name")));
        let mut r = sample();
        r.measurements = (0..=MAX_MEASUREMENTS)
            .map(|i| Measurement {
                name: i.to_string(),
                kind: Kind::Memory,
                value: 1.0,
                runs: 1,
            })
            .collect();
        assert!(r.validate().is_err());
    }

    #[test]
    fn damaged_newer_oversized_and_missing_files_have_clear_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(matches!(Results::load(&p), Err(BenchError::Parse { .. })));
        std::fs::write(&p, r#"{"version": 9, "app": "x"}"#).unwrap();
        assert!(matches!(
            Results::load(&p),
            Err(BenchError::NewerVersion { found: 9, .. })
        ));
        std::fs::write(&p, r#"{"app": "x"}"#).unwrap();
        assert!(
            matches!(Results::load(&p), Err(BenchError::Parse { .. })),
            "missing fields"
        );
        std::fs::write(&p, vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert!(matches!(
            Results::load(&p),
            Err(BenchError::TooLarge { .. })
        ));
        assert!(matches!(
            Results::load(&dir.path().join("none.json")),
            Err(BenchError::Io { .. })
        ));
        for e in [
            Results::load(&p).unwrap_err(),
            BenchError::NewerVersion {
                path: p.clone(),
                found: 2,
            },
            BenchError::Write("disk full".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
