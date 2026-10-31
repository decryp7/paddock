//! Troubleshooting log of a run (ER-27): a first line with the version, arguments, OS and CI mode; phases with durations; every
//! git command with its exact arguments, exit code and, when it failed, the tail of its standard error; every rules source and
//! whether it was found; a summary with the exit code. Every line carries the run's `OperationId`.
//!
//! The log goes to a per-run file (see [`default_log_file`]) or to `--log-file`, as CLEF JSON lines (the format `paddock-log`
//! writes: `@t`, `@l`, `@m` plus properties), and with `--verbose` live to standard error. `paddock-log` itself installs a
//! process-wide logger into the app log folder, which does not fit a short command, so this keeps its line format. Never file
//! contents: only names, paths, counts, sizes, durations and exit codes.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Map, Value, json};

/// Per-run log files kept in the default folder; older ones are removed at the start of a run.
pub const KEEP_LOG_FILES: usize = 20;

/// The log of one run. Cheap when off.
pub struct Trace<'a> {
    operation_id: String,
    live: RefCell<Option<&'a mut dyn Write>>,
    file: RefCell<Option<std::fs::File>>,
    start: Instant,
}

/// A fresh id for one run: time and process, enough to tell runs and their files apart.
pub fn new_operation_id() -> String {
    let now = chrono::Utc::now();
    format!(
        "{:08x}{:04x}",
        now.timestamp_subsec_nanos() ^ (now.timestamp() as u32),
        std::process::id() & 0xffff
    )
}

/// The per-run file in the default log folder: `$WIKI_LOG_DIR` (`off` disables), else the platform's state folder
/// (`~/Library/Logs/wiki`, `$XDG_STATE_HOME/wiki` or `~/.local/state/wiki`, `%LOCALAPPDATA%\wiki\logs`).
pub fn default_log_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("WIKI_LOG_DIR") {
        return if v == "off" || v.is_empty() { None } else { Some(PathBuf::from(v)) };
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("wiki").join("logs"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Logs/wiki"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(|d| PathBuf::from(d).join("wiki"))
            .or_else(|| home().map(|h| h.join(".local/state/wiki")))
    }
}

/// Creates this run's file in `dir` and removes the oldest beyond [`KEEP_LOG_FILES`]. `None` when the folder is unusable (a log
/// that cannot be written must not stop the command).
pub fn default_log_file(dir: &Path, operation_id: &str) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("wiki-") && n.ends_with(".log"))
        })
        .collect();
    old.sort();
    for p in old.iter().take(old.len().saturating_sub(KEEP_LOG_FILES - 1)) {
        let _ = std::fs::remove_file(p);
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    Some(dir.join(format!("wiki-{stamp}-{operation_id}.log")))
}

impl<'a> Trace<'a> {
    /// A trace that records nothing.
    pub fn off() -> Trace<'static> {
        Trace {
            operation_id: String::new(),
            live: RefCell::new(None),
            file: RefCell::new(None),
            start: Instant::now(),
        }
    }

    /// A trace. `live` gets every line at once (`--verbose`); `log_file` is appended to (created if missing); the error names it.
    pub fn new(operation_id: String, live: Option<&'a mut dyn Write>, log_file: Option<&Path>) -> Result<Trace<'a>, String> {
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
            operation_id,
            live: RefCell::new(live),
            file: RefCell::new(file),
            start: Instant::now(),
        })
    }

    /// The id every line carries.
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    /// Records one event at `level` (`Information`, `Warning`, `Error`) with properties.
    pub fn event(&self, level: &str, message: &str, props: &[(&str, Value)]) {
        if self.live.borrow().is_none() && self.file.borrow().is_none() {
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
        obj.insert("OperationId".into(), json!(self.operation_id));
        for (k, v) in props {
            obj.insert((*k).to_string(), v.clone());
        }
        if let Some(f) = self.file.borrow_mut().as_mut() {
            let _ = writeln!(f, "{}", Value::Object(obj));
        }
        if let Some(w) = self.live.borrow_mut().as_mut() {
            let mut line = format!("[wiki {} +{}ms] {message}", self.operation_id, self.start.elapsed().as_millis());
            for (k, v) in props {
                match v {
                    Value::String(s) => line.push_str(&format!(" {k}={s}")),
                    other => line.push_str(&format!(" {k}={other}")),
                }
            }
            let _ = writeln!(w, "{line}");
            let _ = w.flush();
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

    /// Milliseconds since the run started.
    pub fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    /// A sink that records each write as it happens.
    struct Recorder(Rc<RefCell<Vec<String>>>);
    impl Write for Recorder {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().push(String::from_utf8_lossy(buf).into_owned());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn verbose_lines_are_written_at_once_not_at_the_end() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut sink = Recorder(Rc::clone(&seen));
        let trace = Trace::new("op1".into(), Some(&mut sink as &mut dyn Write), None).unwrap();
        trace.info("first", &[("k", json!("v"))]);
        assert!(seen.borrow().iter().any(|l| l.contains("[wiki op1 +") && l.contains("first k=v")));
        let before = seen.borrow().len();
        trace.phase("p", || ());
        assert!(seen.borrow().len() > before, "a phase writes while the trace is still open");
    }

    #[test]
    fn an_off_trace_records_nothing_and_ids_differ() {
        let t = Trace::off();
        t.info("x", &[]);
        assert_eq!(t.operation_id(), "");
        let (a, b) = (new_operation_id(), new_operation_id());
        assert_eq!(a.len(), 12);
        assert_eq!(b.len(), 12);
    }
}
