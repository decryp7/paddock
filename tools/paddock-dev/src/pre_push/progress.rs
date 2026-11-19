//! What a running step has achieved, read from its output (WP-2 of docs/research/2026-11-19-gate-hang-detection.md): a counter that
//! grows when the step makes progress, and the one state that is waiting, not stalling.
//!
//! - Cargo build rows run with `--message-format=json-render-diagnostics`: progress is each finished unit (`compiler-artifact`) and
//!   each build script that ran (`build-script-executed`); for `cargo test --doc`, also each `Doc-tests` start and each `test ... ok`
//!   / `FAILED` line. Diagnostics stay on stderr, rendered, so the row's log and failure tail stay readable (the JSON lines are
//!   dropped from the tail, [`without_json`]).
//! - Every other row (nextest, npm, the Linux container, conformance): each non-empty output line is progress; the runners' own
//!   per-test limits do the per-test work.
//! - `Blocking waiting for file lock on ...` (cargo waiting for another cargo) is a wait state: it counts as no progress and no
//!   stall until the next progress event ([`Counter::waiting_on`]).
//!
//! CPU is never read here: tree CPU is blind to sccache (the compiles run in its server) and to the container row.
//! Pure: text in, numbers out; the executor feeds it the bytes it has not read yet ([`Counter::read_new`]).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How a step's progress is read from its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Progress {
    /// Each non-empty output line.
    #[default]
    Lines,
    /// Cargo's JSON messages (`--message-format=json-render-diagnostics`): finished units and ran build scripts, and the doctests.
    CargoJson,
}

/// The flag a [`Progress::CargoJson`] step's cargo is given.
pub const CARGO_JSON: &str = "--message-format=json-render-diagnostics";

/// `args` with [`CARGO_JSON`] added before a `--` (the arguments for the tool cargo runs), or at the end.
pub fn with_cargo_json(args: &[String]) -> Vec<String> {
    if args.iter().any(|a| a.starts_with("--message-format")) {
        return args.to_vec();
    }
    let at = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let mut out = args.to_vec();
    out.insert(at, CARGO_JSON.to_string());
    out
}

/// The text of a cargo wait for another cargo.
pub const LOCK_WAIT: &str = "Blocking waiting for file lock";

/// What one output line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// A step of progress.
    Progress,
    /// Cargo waits for another cargo's lock: a wait state.
    LockWait,
    /// Nothing that counts (a blank line, a diagnostic in JSON mode).
    Other,
}

/// Whether `line` is one of cargo's JSON messages that says a unit finished.
fn is_unit_event(line: &str) -> bool {
    line.starts_with('{') && (line.contains("\"reason\":\"compiler-artifact\"") || line.contains("\"reason\":\"build-script-executed\""))
}

/// Whether `line` is a JSON message of cargo (any reason).
fn is_json(line: &str) -> bool {
    line.starts_with("{\"reason\":")
}

/// The doctest lines that count as progress: the start of a package's doctests and each result.
fn is_doctest_event(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("Doc-tests ")
        || (t.starts_with("test ") && (t.ends_with(" ok") || t.ends_with("FAILED") || t.contains(" ... ok") || t.contains(" ... FAILED")))
}

/// Classifies one output line for a step read as `mode`.
pub fn classify(mode: Progress, line: &str) -> Line {
    if line.contains(LOCK_WAIT) {
        return Line::LockWait;
    }
    match mode {
        Progress::Lines => {
            if line.trim().is_empty() {
                Line::Other
            } else {
                Line::Progress
            }
        }
        Progress::CargoJson => {
            if is_unit_event(line) || is_doctest_event(line) {
                Line::Progress
            } else {
                Line::Other
            }
        }
    }
}

/// `text` without cargo's JSON message lines: what a failure's tail shows of a [`Progress::CargoJson`] step.
pub fn without_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for l in text.lines().filter(|l| !is_json(l)) {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// The most one [`Counter::read_new`] reads: a build can write megabytes between two looks; the rest is read at the next one.
const READ_AT_MOST: u64 = 4 * 1024 * 1024;

/// The progress of one step so far: the counter, the wait state and the last event's line.
#[derive(Debug, Clone)]
pub struct Counter {
    mode: Progress,
    pos: u64,
    carry: String,
    events: u64,
    waiting: Option<String>,
    last_event: String,
}

impl Counter {
    /// Nothing read yet.
    pub fn new(mode: Progress) -> Counter {
        Counter {
            mode,
            pos: 0,
            carry: String::new(),
            events: 0,
            waiting: None,
            last_event: String::new(),
        }
    }

    /// How many progress events so far: grows, never falls.
    pub fn events(&self) -> u64 {
        self.events
    }

    /// The lock wait the step is in (the cargo line), until its next progress event.
    pub fn waiting_on(&self) -> Option<&str> {
        self.waiting.as_deref()
    }

    /// The last progress event's line (cut to a readable length).
    pub fn last_event(&self) -> &str {
        &self.last_event
    }

    /// Takes new output text: complete lines are classified, a cut line waits for its rest.
    pub fn feed(&mut self, text: &str) {
        self.carry.push_str(text);
        let Some(end) = self.carry.rfind('\n') else { return };
        let done: String = self.carry.drain(..=end).collect();
        for line in done.lines() {
            match classify(self.mode, line) {
                Line::Progress => {
                    self.events += 1;
                    self.waiting = None;
                    self.last_event = line.chars().take(160).collect();
                }
                Line::LockWait => self.waiting = Some(line.trim().chars().take(200).collect()),
                Line::Other => {}
            }
        }
    }

    /// Reads what `path` holds beyond what was read before (at most [`READ_AT_MOST`]) and feeds it. A file that is shorter than the
    /// position (truncated) is read from its start.
    pub fn read_new(&mut self, path: &Path) {
        let Ok(mut f) = std::fs::File::open(path) else { return };
        let len = f.metadata().map_or(0, |m| m.len());
        if len < self.pos {
            self.pos = 0;
            self.carry.clear();
        }
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        let Ok(n) = f.take(READ_AT_MOST).read_to_end(&mut buf) else {
            return;
        };
        self.pos += n as u64;
        // A multi-byte character cut at the end of this read is completed by the next one: only whole lines are classified.
        self.feed(&String::from_utf8_lossy(&buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: &str = r#"{"reason":"compiler-artifact","package_id":"x 0.1.0","target":{"name":"x"},"fresh":false}"#;
    const SCRIPT: &str = r#"{"reason":"build-script-executed","package_id":"x 0.1.0"}"#;

    #[test]
    fn a_cargo_build_counts_finished_units_and_ran_build_scripts_only() {
        let mut c = Counter::new(Progress::CargoJson);
        c.feed(&format!(
            "{UNIT}\n{SCRIPT}\n{{\"reason\":\"compiler-message\",\"message\":{{}}}}\nwarning: unused import\n\n"
        ));
        assert_eq!(c.events(), 2, "a diagnostic and a blank line are not progress");
        c.feed("   Doc-tests foo\ntest src/lib.rs - bar (line 3) ... ok\ntest src/lib.rs - baz (line 9) ... FAILED\n");
        assert_eq!(c.events(), 5);
        assert!(c.last_event().contains("FAILED"));
    }

    #[test]
    fn other_rows_count_every_non_empty_line() {
        let mut c = Counter::new(Progress::Lines);
        c.feed("PASS [ 0.1s] a\n\n   \nPASS [ 0.2s] b\n");
        assert_eq!(c.events(), 2);
    }

    #[test]
    fn a_lock_wait_is_a_wait_state_until_the_next_progress_event_and_never_progress() {
        for mode in [Progress::CargoJson, Progress::Lines] {
            let mut c = Counter::new(mode);
            c.feed(&format!("{UNIT}\n"));
            let before = c.events();
            c.feed("    Blocking waiting for file lock on build directory\n");
            assert_eq!(c.events(), before, "{mode:?}: waiting is not progress");
            assert!(c.waiting_on().is_some_and(|w| w.contains("build directory")), "{mode:?}");
            c.feed(&format!("{UNIT}\n"));
            assert_eq!((c.events(), c.waiting_on()), (before + 1, None), "{mode:?}: an event ends the wait");
        }
    }

    #[test]
    fn a_line_cut_between_two_reads_is_counted_once_when_it_is_complete() {
        let mut c = Counter::new(Progress::CargoJson);
        let (a, b) = UNIT.split_at(20);
        c.feed(a);
        assert_eq!(c.events(), 0);
        c.feed(&format!("{b}\n"));
        assert_eq!(c.events(), 1);
    }

    #[test]
    fn reading_a_file_takes_only_the_new_bytes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("log");
        std::fs::write(&p, format!("{UNIT}\n")).unwrap();
        let mut c = Counter::new(Progress::CargoJson);
        c.read_new(&p);
        c.read_new(&p);
        assert_eq!(c.events(), 1, "the same bytes are not read twice");
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        std::io::Write::write_all(&mut f, format!("{SCRIPT}\n").as_bytes()).unwrap();
        c.read_new(&p);
        assert_eq!(c.events(), 2);
    }

    /// The positive control (ER-33): with the parser taken out (a mode that counts nothing), a step that is making progress reads as
    /// no progress at all, which is what a stall judge would call stuck.
    #[test]
    fn the_progress_parser_is_what_tells_a_busy_build_from_a_silent_one() {
        let busy: String = (0..50).map(|_| format!("{UNIT}\n")).collect();
        let mut with = Counter::new(Progress::CargoJson);
        with.feed(&busy);
        assert_eq!(with.events(), 50);
        // The same output read as something that is not cargo JSON in a step that counts only JSON: nothing counts.
        let mut without = Counter::new(Progress::CargoJson);
        without.feed(&busy.replace("compiler-artifact", "x").replace("\"reason\"", "\"why\""));
        assert_eq!(without.events(), 0);
    }

    #[test]
    fn the_json_flag_goes_before_the_tool_arguments_and_only_once() {
        let a: Vec<String> = ["clippy", "--workspace", "--", "-D", "warnings"].map(String::from).to_vec();
        assert_eq!(
            with_cargo_json(&a).join(" "),
            format!("clippy --workspace {CARGO_JSON} -- -D warnings")
        );
        let b: Vec<String> = ["test", "--doc"].map(String::from).to_vec();
        assert_eq!(with_cargo_json(&b).join(" "), format!("test --doc {CARGO_JSON}"));
        let c = with_cargo_json(&a);
        assert_eq!(with_cargo_json(&c), c);
    }

    #[test]
    fn a_failure_tail_drops_the_json_lines_and_keeps_the_rendered_diagnostics() {
        let text = format!("{UNIT}\nerror: this is the lint\n  --> src/a.rs:1:1\n{SCRIPT}\n");
        assert_eq!(without_json(&text), "error: this is the lint\n  --> src/a.rs:1:1\n");
    }
}
