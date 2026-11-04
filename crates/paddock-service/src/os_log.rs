//! The OS log a service writes to from its first line (ADR 0054 section 2.3): a service has no console, and until its file
//! log is trusted there is nowhere else. One [`OsLog`] port, three adapters:
//!
//! | OS | Adapter | Where to read it |
//! |---|---|---|
//! | Windows | the Application event log, under the service's own source (`native::EventLog`) | Event Viewer, source = the service name |
//! | Linux | the systemd journal through standard error (`<n>` priority prefixes, [`JournalLog`]) | `journalctl -u <unit>` |
//! | macOS | the unified log through `syslog(3)` (`native::SysLog`) | `log show --predicate 'senderImagePath ENDSWITH "<program>"'` |
//!
//! The event source is registered by the install plan ([`crate::render::event_source_commands`]); the journal needs
//! `StandardError=journal` in the unit, which the rendered unit has. A line is never lost silently: when the OS log cannot be
//! opened, the lines go to standard error and the first one says why.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::spec::ServiceSpec;
use log::Level;
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

/// Where a service's most important lines go besides its own file.
pub trait OsLog: Send + Sync {
    /// Writes `text` at `level`. Never panics and never blocks the service for long; a line that cannot be written is
    /// dropped (there is nowhere left to report it).
    fn write(&self, level: Level, text: &str);
}

/// `text` cut to `max` characters (on a character boundary), with an ellipsis when it was cut.
pub fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The syslog priority of a `level` (RFC 5424: 3 error, 4 warning, 6 informational, 7 debug).
pub fn syslog_priority(level: Level) -> u8 {
    match level {
        Level::Error => 3,
        Level::Warn => 4,
        Level::Info => 6,
        Level::Debug | Level::Trace => 7,
    }
}

/// The lines journald reads from standard error for `text`: one per line of text, each with its `<priority>` prefix
/// (sd-daemon(3)); control characters read as spaces so one record stays one line.
pub fn journal_lines(level: Level, text: &str) -> Vec<String> {
    let priority = syslog_priority(level);
    text.lines()
        .map(|line| {
            line.chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>()
        })
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("<{priority}>{line}"))
        .collect()
}

/// A log that writes lines to a stream: standard error under journald (with the priority prefixes), plain elsewhere.
pub struct JournalLog<W: Write + Send> {
    out: Mutex<W>,
    prefixed: bool,
}

impl<W: Write + Send> JournalLog<W> {
    /// A log over `out`; `prefixed` adds journald's `<n>` prefixes.
    pub fn new(out: W, prefixed: bool) -> Self {
        Self {
            out: Mutex::new(out),
            prefixed,
        }
    }
}

impl<W: Write + Send> OsLog for JournalLog<W> {
    fn write(&self, level: Level, text: &str) {
        let lines = if self.prefixed {
            journal_lines(level, text)
        } else {
            text.lines().map(str::to_string).collect()
        };
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        for line in lines {
            if writeln!(out, "{line}").is_err() {
                // silent: standard error itself is closed, so there is no place left to report that the line was lost
                return;
            }
        }
    }
}

/// Whether standard error is the journal's stream (`JOURNAL_STREAM` is set by systemd for `StandardError=journal`).
pub fn stderr_is_journal(var: Option<&std::ffi::OsStr>) -> bool {
    var.is_some_and(|v| !v.is_empty())
}

/// A standard-error log that says first why the OS log could not be opened (`why`).
pub fn stderr_fallback(why: &str) -> Arc<dyn OsLog> {
    let log = JournalLog::new(std::io::stderr(), false);
    log.write(
        Level::Warn,
        &format!("the OS log could not be opened ({why}); lines go to standard error"),
    );
    Arc::new(log)
}

/// Opens the OS log of `spec` for this OS.
pub fn open(spec: &ServiceSpec) -> Arc<dyn OsLog> {
    #[cfg(windows)]
    {
        match crate::native::EventLog::open(spec.name()) {
            Ok(log) => Arc::new(log),
            Err(why) => stderr_fallback(&why),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match crate::native::SysLog::open(spec.label()) {
            Ok(log) => Arc::new(log),
            Err(why) => stderr_fallback(&why),
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = spec;
        let prefixed = stderr_is_journal(std::env::var_os("JOURNAL_STREAM").as_deref());
        Arc::new(JournalLog::new(std::io::stderr(), prefixed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn levels_map_to_syslog_priorities() {
        let all = [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ];
        assert_eq!(all.map(syslog_priority), [3, 4, 6, 7, 7]);
    }

    #[test]
    fn the_journal_gets_one_prefixed_line_per_line_of_text_without_control_characters() {
        assert_eq!(
            journal_lines(Level::Error, "first\tcolumn\nsecond\r\n\n   \nthird"),
            ["<3>first column", "<3>second", "<3>third"]
        );
        assert!(journal_lines(Level::Info, "").is_empty());
    }

    #[test]
    fn a_journal_log_writes_prefixed_lines_under_journald_and_plain_lines_elsewhere() {
        let sink = Sink::default();
        JournalLog::new(sink.clone(), true).write(Level::Warn, "a\nb");
        assert_eq!(
            String::from_utf8(sink.0.lock().unwrap().clone()).unwrap(),
            "<4>a\n<4>b\n"
        );
        let sink = Sink::default();
        JournalLog::new(sink.clone(), false).write(Level::Warn, "a\nb");
        assert_eq!(
            String::from_utf8(sink.0.lock().unwrap().clone()).unwrap(),
            "a\nb\n"
        );
    }

    #[test]
    fn a_closed_standard_error_drops_the_line_without_panicking() {
        JournalLog::new(Closed, true).write(Level::Error, "lost\nalso lost");
    }

    #[test]
    fn standard_error_is_the_journal_when_systemd_says_so() {
        assert!(stderr_is_journal(Some(std::ffi::OsStr::new("8:1234"))));
        assert!(!stderr_is_journal(Some(std::ffi::OsStr::new(""))));
        assert!(!stderr_is_journal(None));
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        assert_eq!(cut("short", 10), "short");
        let text = cut(&"é".repeat(50), 10);
        assert_eq!(text.chars().count(), 10);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn the_fallback_log_says_why_and_writes_to_standard_error() {
        stderr_fallback("probe").write(Level::Info, "to standard error");
    }
}
