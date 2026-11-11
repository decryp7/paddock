//! Log capture for the endpoint's end line (ADR 0072 §6). The endpoint writes one line per connection end, `key=value`
//! pairs after the marker [`END_MARKER`]; a test routes the product's log into a [`LogCapture`] and the harness finds it.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// The text an end line starts with (after any logger prefix).
pub const END_MARKER: &str = crate::endlog::MARKER;

/// The lines a product logged, shared between the product's logger and the test.
#[derive(Clone, Default)]
pub struct LogCapture(Arc<(Mutex<Vec<String>>, Condvar)>);

impl LogCapture {
    /// An empty capture.
    pub fn new() -> LogCapture {
        LogCapture::default()
    }

    /// Records one line.
    pub fn push(&self, line: impl Into<String>) {
        self.0
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(line.into());
        self.0.1.notify_all();
    }

    /// The lines so far.
    pub fn lines(&self) -> Vec<String> {
        self.0.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The lines from index `from` on (the lines of one scenario).
    pub fn since(&self, from: usize) -> Vec<String> {
        self.lines().into_iter().skip(from).collect()
    }

    /// How many lines there are.
    pub fn len(&self) -> usize {
        self.0.0.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Blocks until at least `n` lines were logged; false if that did not happen within `guard` (a hang guard).
    pub fn wait_for_len(&self, n: usize, guard: Duration) -> bool {
        let g = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        let (g, _) = self
            .0
            .1
            .wait_timeout_while(g, guard, |l| l.len() < n)
            .unwrap_or_else(|p| p.into_inner());
        g.len() >= n
    }

    /// Blocks until an end line was logged; false if that did not happen within `guard` (a hang guard).
    pub fn wait_for_end_line(&self, guard: Duration) -> bool {
        let g = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        let (g, _) = self
            .0
            .1
            .wait_timeout_while(g, guard, |l| !l.iter().any(|x| x.contains(END_MARKER)))
            .unwrap_or_else(|p| p.into_inner());
        g.iter().any(|x| x.contains(END_MARKER))
    }

    /// Blocks until an end line was logged at index `from` or later; false after `guard` (a hang guard).
    pub fn wait_for_end_line_since(&self, from: usize, guard: Duration) -> bool {
        self.wait_for_end_lines_since(from, 1, guard)
    }

    /// Blocks until `n` end lines were logged at index `from` or later; false after `guard` (a hang guard).
    pub fn wait_for_end_lines_since(&self, from: usize, n: usize, guard: Duration) -> bool {
        let seen = |l: &Vec<String>| {
            l.iter()
                .skip(from)
                .filter(|x| x.contains(END_MARKER))
                .count()
                >= n
        };
        let g = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        let (g, _) = self
            .0
            .1
            .wait_timeout_while(g, guard, |l| !seen(l))
            .unwrap_or_else(|p| p.into_inner());
        seen(&g)
    }

    /// Waits for `n` end lines at index `from` or later, then until no further end line arrives for `quiet` (the window of the
    /// scenario is then over: a connection that ended twice, or one the rig did not count, shows up as an extra line). Returns
    /// false when the `n` lines did not come within `guard`. The quiet part can only miss a very late extra, never invent one.
    pub fn wait_settled(&self, from: usize, n: usize, guard: Duration, quiet: Duration) -> bool {
        if !self.wait_for_end_lines_since(from, n, guard) {
            return false;
        }
        let count = |l: &Vec<String>| {
            l.iter()
                .skip(from)
                .filter(|x| x.contains(END_MARKER))
                .count()
        };
        let mut g = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            let seen = count(&g);
            let (next, timeout) = self
                .0
                .1
                .wait_timeout_while(g, quiet, |l| count(l) == seen)
                .unwrap_or_else(|p| p.into_inner());
            g = next;
            if timeout.timed_out() {
                return true;
            }
        }
    }

    /// Whether nothing was logged.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Every end line in `lines`, in order.
pub fn end_lines(lines: &[String]) -> Vec<&String> {
    lines.iter().filter(|l| l.contains(END_MARKER)).collect()
}

/// The first end line in `lines`.
pub fn find_end_line(lines: &[String]) -> Option<String> {
    lines.iter().find(|l| l.contains(END_MARKER)).cloned()
}

/// The value of `key` in an end line (`key=value` separated by spaces).
pub fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let after = line.split(END_MARKER).nth(1)?;
    after
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
}

struct Forward(LogCapture);

impl log::Log for Forward {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            self.0.push(format!("{} {}", r.level(), r.args()));
        }
    }

    fn flush(&self) {}
}

/// The capture of this process's `log` records (Info and up), installing the logger on first use. An error when another
/// logger already owns the process: a rig then cannot see the endpoint's lines and says so instead of passing quietly.
/// Under `cargo test` the tests of one binary share the process, so a neighbour's lines can appear in a window; nextest runs
/// each test in its own process.
pub fn process_capture() -> Result<LogCapture, String> {
    static CAPTURE: std::sync::OnceLock<Result<LogCapture, String>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = LogCapture::new();
            log::set_logger(Box::leak(Box::new(Forward(capture.clone()))))
                .map_err(|_| "another logger is installed in this process; the rig cannot capture the end lines".to_string())?;
            log::set_max_level(log::LevelFilter::Info);
            Ok(capture)
        })
        .clone()
}
