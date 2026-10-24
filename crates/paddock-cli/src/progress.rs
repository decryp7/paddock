use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Progress lines are written at most this often.
pub const LINE_EVERY: Duration = Duration::from_millis(500);

/// Phase lines on standard error ("Importing 12 of 40 files"), at most one per [`LINE_EVERY`], never the same line twice
/// in a row, and nothing at all with `--quiet`. The time is passed in (any monotonic duration since start), so tests need
/// no clock and no sleeps. A write error (a closed pipe) silently ends the output: progress is never worth failing for.
pub struct Progress<W: Write> {
    out: W,
    enabled: bool,
    every: Duration,
    last_line: String,
    last_at: Option<Duration>,
    broken: bool,
}

impl<W: Write> Progress<W> {
    /// Progress to `out`; `enabled` is `!--quiet`.
    pub fn new(out: W, enabled: bool) -> Progress<W> {
        Progress {
            out,
            enabled,
            every: LINE_EVERY,
            last_line: String::new(),
            last_at: None,
            broken: false,
        }
    }

    /// Another minimum gap between lines.
    pub fn every(mut self, every: Duration) -> Progress<W> {
        self.every = every;
        self
    }

    /// Offers a line at `now`; returns whether it was written. A changed phase (the text before the first digit) is
    /// written at once, so a short phase is never skipped.
    pub fn update(&mut self, now: Duration, line: &str) -> bool {
        if !self.enabled || self.broken || line.is_empty() || line == self.last_line {
            return false;
        }
        let new_phase = phase_of(line) != phase_of(&self.last_line);
        let due = self
            .last_at
            .is_none_or(|t| now.saturating_sub(t) >= self.every);
        if !(due || new_phase) {
            return false;
        }
        if writeln!(self.out, "{line}").is_err() {
            self.broken = true;
            return false;
        }
        self.last_line = line.to_string();
        self.last_at = Some(now);
        true
    }

    /// A note that is not progress (a cut result, a warning): always written unless quiet.
    pub fn note(&mut self, text: &str) {
        if self.enabled && !self.broken && writeln!(self.out, "{text}").is_err() {
            self.broken = true;
        }
    }

    /// Gives the writer back.
    pub fn into_inner(self) -> W {
        self.out
    }
}

fn phase_of(line: &str) -> &str {
    line.split(|c: char| c.is_ascii_digit())
        .next()
        .unwrap_or("")
}

/// The Ctrl+C / SIGTERM request: the app's signal handler calls [`StopFlag::request`], long loops poll
/// [`StopFlag::is_set`] and end with [`crate::Exit::Interrupted`]. Clones share the flag.
#[derive(Debug, Clone, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    /// A flag that is not set.
    pub fn new() -> StopFlag {
        StopFlag::default()
    }

    /// Asks the work to stop.
    pub fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether a stop was asked for.
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn throttles_dedupes_and_lets_a_new_phase_through() {
        let mut p = Progress::new(Vec::new(), true);
        assert!(p.update(ms(0), "Finding files: 1 found"));
        assert!(!p.update(ms(100), "Finding files: 2 found"), "too soon");
        assert!(p.update(ms(1200), "Finding files: 9 found"));
        assert!(!p.update(ms(1300), "Finding files: 9 found"), "same line");
        assert!(
            p.update(ms(1301), "Importing 0 of 4 files"),
            "a new phase is not throttled"
        );
        assert!(!p.update(ms(1302), ""), "empty lines are never written");
        let text = String::from_utf8(p.into_inner()).unwrap();
        assert!(
            text.contains("Finding files: 9 found\nImporting 0 of 4 files\n"),
            "{text}"
        );
        assert!(!text.contains("2 found"));
    }

    #[test]
    fn quiet_writes_nothing_and_a_closed_pipe_is_not_an_error() {
        let mut q = Progress::new(Vec::new(), false);
        assert!(!q.update(ms(0), "x"));
        q.note("n");
        assert!(q.into_inner().is_empty());

        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut c = Progress::new(Closed, true).every(ms(1));
        assert!(!c.update(ms(0), "a"));
        assert!(
            !c.update(ms(10), "b"),
            "stays silent after the first failure"
        );
        c.note("still silent");
    }

    #[test]
    fn notes_are_written_and_the_stop_flag_is_shared() {
        let mut p = Progress::new(Vec::new(), true);
        p.note("5 of 9 rows shown");
        assert_eq!(
            String::from_utf8(p.into_inner()).unwrap(),
            "5 of 9 rows shown\n"
        );
        let a = StopFlag::new();
        let b = a.clone();
        assert!(!b.is_set());
        a.request();
        assert!(b.is_set());
    }
}
