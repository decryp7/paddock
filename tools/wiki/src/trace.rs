//! Troubleshooting log of a run: phases with durations, every git command with its exact arguments and exit code, every rules
//! source and whether it was found. With `--verbose` the lines also go to standard error; with `--log-file` they are appended to
//! a file as CLEF JSON lines (the format `paddock-log` writes: `@t`, `@l`, `@m` plus properties), so the same tools read them.
//! `paddock-log` itself installs a process-wide logger into the app log folder, which does not fit a short command; this keeps
//! its line format. Never file contents: only names, paths, counts, sizes, durations and exit codes.

use std::cell::RefCell;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use serde_json::{Map, Value, json};

/// The log of one run. Cheap when off.
pub struct Trace {
    verbose: bool,
    file: RefCell<Option<std::fs::File>>,
    lines: RefCell<Vec<String>>,
    start: Instant,
}

impl Trace {
    /// A trace that records nothing.
    pub fn off() -> Trace {
        Trace {
            verbose: false,
            file: RefCell::new(None),
            lines: RefCell::new(Vec::new()),
            start: Instant::now(),
        }
    }

    /// A trace; `log_file` is appended to (created if missing). The error names the file.
    pub fn new(verbose: bool, log_file: Option<&Path>) -> Result<Trace, String> {
        let file = match log_file {
            Some(p) => Some(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .map_err(|e| format!("cannot open the log file {}: {e}", p.display()))?,
            ),
            None => None,
        };
        Ok(Trace {
            verbose,
            file: RefCell::new(file),
            lines: RefCell::new(Vec::new()),
            start: Instant::now(),
        })
    }

    /// Records one event at `level` (`Information`, `Warning`, `Error`) with properties.
    pub fn event(&self, level: &str, message: &str, props: &[(&str, Value)]) {
        if !self.verbose && self.file.borrow().is_none() {
            return;
        }
        let mut obj = Map::new();
        obj.insert(
            "@t".into(),
            json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        );
        obj.insert("@l".into(), json!(level));
        obj.insert("@m".into(), json!(message));
        obj.insert("Tool".into(), json!("wiki"));
        for (k, v) in props {
            obj.insert((*k).to_string(), v.clone());
        }
        if let Some(f) = self.file.borrow_mut().as_mut() {
            let _ = writeln!(f, "{}", Value::Object(obj));
        }
        if self.verbose {
            let mut line = format!("[wiki +{}ms] {message}", self.start.elapsed().as_millis());
            for (k, v) in props {
                match v {
                    Value::String(s) => line.push_str(&format!(" {k}={s}")),
                    other => line.push_str(&format!(" {k}={other}")),
                }
            }
            self.lines.borrow_mut().push(line);
        }
    }

    /// An information event.
    pub fn info(&self, message: &str, props: &[(&str, Value)]) {
        self.event("Information", message, props);
    }

    /// Runs `f` as a named phase and records how long it took.
    pub fn phase<T>(&self, name: &str, f: impl FnOnce() -> T) -> T {
        self.info(&format!("phase {name} started"), &[]);
        let t = Instant::now();
        let out = f();
        let ms = u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.info(
            &format!("phase {name} done in {ms} ms"),
            &[("phase", json!(name)), ("ms", json!(ms))],
        );
        out
    }

    /// The verbose lines recorded so far, for the caller to print to standard error.
    pub fn take_lines(&self) -> Vec<String> {
        std::mem::take(&mut self.lines.borrow_mut())
    }
}
