//! `setup.log` (ADR 0053 section 2): one line per event, phases with start and duration, every process acted on, exit codes.
//! 1 MiB, one rotation. Written only after `check-dir` passed (the writer cannot be made without a [`CheckedDir`]). Lines
//! carry no user names, profile paths or command lines (ER-27); a failing log never fails the command.

use crate::checkdir::CheckedDir;
use std::io;

/// The log's size bound before it rotates.
pub const MAX_BYTES: u64 = 1 << 20;
/// A line is cut at this many characters.
pub const MAX_LINE: usize = 512;

/// Where lines go (the file in `updates\`, or a vector in tests).
pub trait LogFs {
    /// Current size in bytes.
    fn len(&self) -> u64;
    /// Whether the log is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Appends `text` (one line with its newline).
    fn append(&mut self, text: &str) -> io::Result<()>;
    /// Moves the log to `setup.log.1`, replacing an older one, and starts an empty log.
    fn rotate(&mut self) -> io::Result<()>;
}

/// Time source (a fake in tests).
pub trait Clock {
    /// Milliseconds since the Unix epoch, UTC.
    fn unix_ms(&self) -> u64;
}

/// What the commands log through (the log, or nothing).
pub trait Sink {
    /// Logs `phase` and a message.
    fn line(&mut self, phase: &str, message: &str);

    /// Logs a process acted on.
    fn process(&mut self, rec: &ProcessRecord) {
        self.line("process", &rec.describe());
    }
}

/// A sink that drops everything (before `check-dir` has passed).
#[derive(Debug, Default)]
pub struct NoLog;

impl Sink for NoLog {
    fn line(&mut self, _phase: &str, _message: &str) {}
}

/// A process acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRecord {
    /// The image file name (no path).
    pub image: String,
    /// The pid, when known.
    pub pid: Option<u32>,
    /// Start time, Unix milliseconds, when known.
    pub started_ms: Option<u64>,
    /// The session id, when known.
    pub session: Option<u32>,
    /// `restart-manager` or `by-handle`.
    pub method: String,
    /// `ended`, `skipped: another folder`, `still running`, ...
    pub outcome: String,
}

impl ProcessRecord {
    /// The log text: image, pid, start time, session, method, outcome (no path, no command line).
    pub fn describe(&self) -> String {
        format!(
            "image={} pid={} start={} session={} method={} outcome={}",
            clean(&self.image),
            self.pid.map_or_else(|| "?".to_string(), |p| p.to_string()),
            self.started_ms.map_or_else(|| "?".to_string(), format_utc),
            self.session
                .map_or_else(|| "?".to_string(), |s| s.to_string()),
            clean(&self.method),
            clean(&self.outcome)
        )
    }
}

/// An open phase.
#[derive(Debug, Clone, Copy)]
pub struct Phase {
    started_ms: u64,
}

/// The setup log.
pub struct SetupLog<F: LogFs, C: Clock> {
    fs: F,
    clock: C,
    dropped: u32,
}

impl<F: LogFs, C: Clock> SetupLog<F, C> {
    /// Opens the log for a folder that passed `check-dir`.
    pub fn open(_proof: &CheckedDir, fs: F, clock: C) -> Self {
        SetupLog {
            fs,
            clock,
            dropped: 0,
        }
    }

    /// Lines that could not be written.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Starts a phase (logs its start).
    pub fn begin(&mut self, phase: &str) -> Phase {
        self.line(phase, "start");
        Phase {
            started_ms: self.clock.unix_ms(),
        }
    }

    /// Ends a phase with its outcome and duration.
    pub fn end(&mut self, phase: &str, token: Phase, outcome: &str) {
        let duration = self.clock.unix_ms().saturating_sub(token.started_ms);
        self.line(
            phase,
            &format!("end outcome={outcome} duration_ms={duration}"),
        );
    }

    /// Logs the exit code and its meaning.
    pub fn exit(&mut self, code: i32, meaning: &str) {
        self.line("exit", &format!("code={code} {meaning}"));
    }

    /// The access to the file system (tests read it back).
    pub fn into_fs(self) -> F {
        self.fs
    }
}

impl<F: LogFs, C: Clock> Sink for SetupLog<F, C> {
    fn line(&mut self, phase: &str, message: &str) {
        let now = format_utc(self.clock.unix_ms());
        let text = format!("{now} phase={} {}\n", clean(phase), clean(message));
        let result = (|| {
            if self.fs.len() + text.len() as u64 > MAX_BYTES {
                self.fs.rotate()?;
            }
            self.fs.append(&text)
        })();
        if result.is_err() {
            self.dropped += 1;
        }
    }
}

/// `2023-11-14T22:13:20.000Z` for Unix milliseconds.
pub fn format_utc(unix_ms: u64) -> String {
    let ms = unix_ms % 1000;
    let secs = unix_ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Removes control characters and cuts at [`MAX_LINE`].
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_LINE)
        .collect()
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    pub struct MemFs {
        pub current: String,
        pub rotated: Option<String>,
        pub fail: bool,
    }
    impl LogFs for MemFs {
        fn len(&self) -> u64 {
            self.current.len() as u64
        }
        fn append(&mut self, text: &str) -> io::Result<()> {
            if self.fail {
                return Err(io::Error::other("disk full"));
            }
            self.current.push_str(text);
            Ok(())
        }
        fn rotate(&mut self) -> io::Result<()> {
            self.rotated = Some(std::mem::take(&mut self.current));
            Ok(())
        }
    }
    pub struct Tick(pub Cell<u64>);
    impl Clock for Tick {
        fn unix_ms(&self) -> u64 {
            let v = self.0.get();
            self.0.set(v + 250);
            v
        }
    }
    pub fn proof() -> CheckedDir {
        struct F;
        impl crate::checkdir::DirFacts for F {
            fn program_files(&self) -> Vec<String> {
                vec![r"C:\Program Files".into()]
            }
            fn inspect(&self, _: &str) -> Result<crate::checkdir::DirInspect, String> {
                Ok(crate::checkdir::DirInspect {
                    canonical: r"C:\Program Files\Dyno".into(),
                    exists: false,
                    reparse_component: false,
                    owner_sid: None,
                })
            }
            fn protection(&self, _: &crate::checkdir::DirInspect) -> Result<(), String> {
                Ok(())
            }
        }
        crate::checkdir::check(&F, "x").expect("proof")
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;
    use std::cell::Cell;

    fn log(start: u64) -> SetupLog<MemFs, Tick> {
        SetupLog::open(&proof(), MemFs::default(), Tick(Cell::new(start)))
    }

    #[test]
    fn utc_formatting() {
        assert_eq!(format_utc(1_700_000_000_000), "2023-11-14T22:13:20.000Z");
        assert_eq!(format_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_utc(1_709_164_800_123), "2024-02-29T00:00:00.123Z");
    }

    #[test]
    fn a_line_has_time_phase_and_message() {
        let mut l = log(1_700_000_000_000);
        l.line("guard", "no per-user copy");
        let fs = l.into_fs();
        assert_eq!(
            fs.current,
            "2023-11-14T22:13:20.000Z phase=guard no per-user copy\n"
        );
    }

    #[test]
    fn phases_log_start_and_duration() {
        let mut l = log(1_700_000_000_000);
        let t = l.begin("stop-all");
        l.end("stop-all", t, "ok");
        let text = l.into_fs().current;
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("phase=stop-all start"));
        assert!(lines[1].contains("phase=stop-all end outcome=ok duration_ms="));
        assert!(
            lines[1].ends_with("duration_ms=250") || lines[1].ends_with("duration_ms=500"),
            "{}",
            lines[1]
        );
    }

    #[test]
    fn process_records_name_image_pid_start_session_method_outcome() {
        let mut l = log(1_700_000_000_000);
        l.process(&ProcessRecord {
            image: "dyno.exe".into(),
            pid: Some(4242),
            started_ms: Some(1_700_000_000_000),
            session: Some(2),
            method: "restart-manager".into(),
            outcome: "ended".into(),
        });
        let text = l.into_fs().current;
        assert!(text.contains("phase=process image=dyno.exe pid=4242 start=2023-11-14T22:13:20.000Z session=2 method=restart-manager outcome=ended"), "{text}");
    }

    #[test]
    fn unknown_process_facts_are_question_marks() {
        let rec = ProcessRecord {
            image: "dyno.exe".into(),
            pid: None,
            started_ms: None,
            session: None,
            method: "by-handle".into(),
            outcome: "skipped: another folder".into(),
        };
        assert_eq!(
            rec.describe(),
            "image=dyno.exe pid=? start=? session=? method=by-handle outcome=skipped: another folder"
        );
    }

    #[test]
    fn exit_codes_are_logged() {
        let mut l = log(0);
        l.exit(11, "a program is still running");
        assert!(
            l.into_fs()
                .current
                .contains("phase=exit code=11 a program is still running")
        );
    }

    #[test]
    fn lines_are_cleaned_and_cut() {
        assert_eq!(clean("a\r\nb\tc\u{0}d"), "a  b c d");
        assert_eq!(clean(&"x".repeat(MAX_LINE + 50)).chars().count(), MAX_LINE);
        let mut l = log(0);
        l.line("p", "two\nlines");
        assert_eq!(l.into_fs().current.matches('\n').count(), 1);
    }

    #[test]
    fn rotates_once_at_the_bound() {
        let mut l = log(0);
        l.fs.current = "x".repeat(MAX_BYTES as usize - 10);
        l.line("p", "this line does not fit");
        let fs = l.into_fs();
        assert_eq!(
            fs.rotated.as_deref().map(str::len),
            Some(MAX_BYTES as usize - 10)
        );
        assert!(fs.current.contains("this line does not fit"));
        assert!(fs.current.len() < 200);
    }

    #[test]
    fn a_failing_disk_never_fails_the_command() {
        let mut l = log(0);
        l.fs.fail = true;
        l.line("p", "x");
        l.line("p", "y");
        assert_eq!(l.dropped(), 2);
    }
}
