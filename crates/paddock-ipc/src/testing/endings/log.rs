//! Log capture for the endpoint's end line (ADR 0072 §6). The endpoint writes one line per connection end, `key=value`
//! pairs after the marker [`END_MARKER`]; a test routes the product's log into a [`LogCapture`] and the harness finds it.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// The text an end line starts with (after any logger prefix).
pub const END_MARKER: &str = "connection ended";

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

    /// Whether nothing was logged.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
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
