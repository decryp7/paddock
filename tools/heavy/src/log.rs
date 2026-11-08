//! Built-in troubleshooting log (rule 56 / ER-27). One CLEF JSON line per event (the format of `paddock-log`, ADR 0036: `@t`,
//! `@l` omitted for Information, `@m`, `SourceContext`, `Application`, `Process`, `Version`, `ProcessId`, `Os`, `SessionId`),
//! written to `<lock folder>/heavy-logs/heavy.<day>.log` or `--log-file`. `SessionId` is the correlation id of one run.
//! `paddock-log` itself (tracing, chrono, serde, the app environment) is too large for a lock CLI, so only the line format is shared.

use crate::clock::Clock;
use serde_json::{Map, Value, json};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Event severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Detail shown with `--verbose` on stderr (still always in the file).
    Debug,
    /// Normal events (no `@l` in CLEF).
    Info,
    /// Something to look at: a break, a stale removal, a refusal.
    Warn,
    /// A failure.
    Error,
}

/// Lines kept in memory (for tests and the failure summary); older ones are dropped.
const MEM_LINES: usize = 4000;

/// The log of one run.
pub struct Logger {
    run_id: String,
    pid: u32,
    clock: Arc<dyn Clock>,
    file: Option<PathBuf>,
    verbose: bool,
    mem: Mutex<Vec<String>>,
}

impl Logger {
    /// A logger for run `run_id`. `file` is the CLEF file (None: no file); `verbose` echoes every event to stderr.
    pub fn new(run_id: String, pid: u32, clock: Arc<dyn Clock>, file: Option<PathBuf>, verbose: bool) -> Logger {
        Logger {
            run_id,
            pid,
            clock,
            file,
            verbose,
            mem: Mutex::new(Vec::new()),
        }
    }

    /// The correlation id of this run.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The CLEF file, if any.
    pub fn file(&self) -> Option<&PathBuf> {
        self.file.as_ref()
    }

    /// Records one event. Never fails and never panics: a log that cannot be written is skipped.
    pub fn event(&self, level: Level, context: &str, msg: &str, props: &[(&str, Value)]) {
        let mut m = Map::new();
        m.insert("@t".into(), json!(crate::timefmt::rfc3339_micros(self.clock.now_micros())));
        match level {
            Level::Debug => m.insert("@l".into(), json!("Debug")),
            Level::Info => None,
            Level::Warn => m.insert("@l".into(), json!("Warning")),
            Level::Error => m.insert("@l".into(), json!("Error")),
        };
        m.insert("@m".into(), json!(msg));
        m.insert("SourceContext".into(), json!(context));
        m.insert("Application".into(), json!("heavy"));
        m.insert("Process".into(), json!("cli"));
        m.insert("Version".into(), json!(env!("CARGO_PKG_VERSION")));
        m.insert("ProcessId".into(), json!(self.pid));
        m.insert("Os".into(), json!(std::env::consts::OS));
        m.insert("SessionId".into(), json!(self.run_id));
        for (k, v) in props {
            let key = if k.starts_with('@') { format!("@{k}") } else { (*k).to_string() };
            m.insert(key, v.clone());
        }
        let line = Value::Object(m).to_string();
        if let Some(path) = &self.file {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{line}");
            }
        }
        if self.verbose {
            let extra: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
            eprintln!("heavy[{}] {msg} {}", self.run_id, extra.join(" "));
        }
        if let Ok(mut mem) = self.mem.lock() {
            if mem.len() >= MEM_LINES {
                mem.drain(..MEM_LINES / 2);
            }
            mem.push(line);
        }
    }

    /// Shorthand: an Info event.
    pub fn info(&self, context: &str, msg: &str, props: &[(&str, Value)]) {
        self.event(Level::Info, context, msg, props);
    }

    /// Shorthand: a Warning event.
    pub fn warn(&self, context: &str, msg: &str, props: &[(&str, Value)]) {
        self.event(Level::Warn, context, msg, props);
    }

    /// Every line recorded by this logger so far.
    pub fn lines(&self) -> Vec<String> {
        self.mem.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// Whether some recorded line contains `needle`.
    pub fn has(&self, needle: &str) -> bool {
        self.lines().iter().any(|l| l.contains(needle))
    }
}
