//! The OS problem logs behind `dyno_core::issues::IssueSource`: macOS crash reports and the unified log, the Windows
//! Event Log, the systemd journal and `coredumpctl`. Observe-only: every reader only reads what the OS already keeps,
//! external tools run with timeouts through [`crate::process`], and the text of an event is kept bounded and never
//! logged. The parsers are pure functions over text and compile on every OS (so they are tested everywhere); only the
//! choice of readers in [`os_sources`] depends on the OS.

pub mod linux;
pub mod macos;
pub mod windows;

use dyno_core::issues::{days_from_civil, IssueSource, ReadControl};
use dyno_core::ports::PlatformError;
use dyno_core::time::{Timestamp, DAY, HOUR, MINUTE, SECOND};
use std::time::Duration;

/// Runs an external program and returns its standard output. A seam: tests script it, the OS version spawns the tool with
/// a timeout and no console window.
pub trait ToolRunner: Send + Sync {
    /// Runs `program` with `args`; a non-zero exit is an error carrying the start of the tool's error output.
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<String, PlatformError>;
    /// Like [`run`](Self::run) with the output handed over line by line as it arrives, and the tool stopped at once when
    /// `control` is cancelled. The default runs the tool to the end and hands over its lines (no streaming, no early stop).
    fn run_lines(
        &self,
        program: &str,
        args: &[String],
        timeout: Duration,
        control: &dyn ReadControl,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<(), PlatformError> {
        let _ = control;
        self.run(program, args, timeout)?.lines().for_each(on_line);
        Ok(())
    }
}

/// Spawns the real tool.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsRunner;

impl ToolRunner for OsRunner {
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<String, PlatformError> {
        let mut cmd = crate::process::command(program);
        cmd.args(args);
        crate::process::output_text(cmd, timeout)
    }

    fn run_lines(
        &self,
        program: &str,
        args: &[String],
        timeout: Duration,
        control: &dyn ReadControl,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<(), PlatformError> {
        let mut cmd = crate::process::command(program);
        cmd.args(args);
        crate::process::run_streaming(cmd, timeout, control, on_line)
    }
}

/// The readers that make sense on this OS.
pub fn os_sources() -> Vec<Box<dyn IssueSource>> {
    #[cfg(target_os = "macos")]
    {
        vec![
            Box::new(macos::MacReports::for_user()),
            Box::new(macos::MacLog::new(Box::new(OsRunner))),
        ]
    }
    #[cfg(windows)]
    {
        vec![Box::new(windows::WindowsEvents::new(Box::new(OsRunner)))]
    }
    #[cfg(target_os = "linux")]
    {
        vec![
            Box::new(linux::Journal::new(Box::new(OsRunner))),
            Box::new(linux::Coredumps::new(Box::new(OsRunner))),
        ]
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

/// How long ago, in whole hours rounded up (at least one), for tools that take `--last Nh`.
pub fn hours_back(since: Timestamp, until: Timestamp) -> u32 {
    let ms = until.since(since).max(0);
    u32::try_from((ms + HOUR - 1) / HOUR).unwrap_or(u32::MAX).max(1)
}

/// How long a tool may run for a window of `hours`: reading a long log on a busy machine is slow.
pub fn timeout_for(hours: u32) -> Duration {
    if hours > 6 {
        Duration::from_secs(120)
    } else {
        Duration::from_secs(60)
    }
}

/// Parses `2026-10-23 10:09:54.318304+0800`, `2026-10-22 20:48:57.00 +0800`, `2026-10-22T10:11:12.1234567Z` and
/// `2026-10-22T10:11:12+08:00`. A time without a zone is refused (it would be a guess).
pub fn parse_timestamp(text: &str) -> Option<Timestamp> {
    let t = text.trim();
    let b = t.as_bytes();
    if b.len() < 19 || !t.is_char_boundary(19) {
        return None;
    }
    let num = |a: usize, z: usize| t.get(a..z)?.parse::<i64>().ok();
    if b[4] != b'-' || b[7] != b'-' || !(b[10] == b' ' || b[10] == b'T') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let mut rest = &t[19..];
    let mut millis = 0i64;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).collect();
        let three: String = format!("{digits:0<3}").chars().take(3).collect();
        millis = three.parse().ok()?;
        rest = &frac[digits.len()..];
    }
    let rest = rest.trim_start();
    let offset_minutes: i64 = if rest == "Z" || rest == "z" {
        0
    } else {
        let sign = match rest.as_bytes().first()? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let z: String = rest[1..].chars().filter(|c| *c != ':').collect();
        if z.len() != 4 || !z.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        sign * (z[..2].parse::<i64>().ok()? * 60 + z[2..].parse::<i64>().ok()?)
    };
    let days = days_from_civil(y, mo as u32, d as u32);
    let ms = days * DAY + h * HOUR + mi * MINUTE + s * SECOND + millis - offset_minutes * MINUTE;
    Some(Timestamp(ms))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::Mutex;

    /// Answers each call with the next scripted result and records the calls (program and arguments).
    pub struct Scripted {
        pub answers: Mutex<Vec<Result<String, PlatformError>>>,
        pub calls: Mutex<Vec<(String, Vec<String>, Duration)>>,
    }

    impl Scripted {
        pub fn new(answers: Vec<Result<String, PlatformError>>) -> Scripted {
            Scripted {
                answers: Mutex::new(answers),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl ToolRunner for Scripted {
        fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<String, PlatformError> {
            self.calls.lock().unwrap().push((program.into(), args.to_vec(), timeout));
            let mut a = self.answers.lock().unwrap();
            if a.is_empty() {
                Err(PlatformError::Failed("no scripted answer".into()))
            } else {
                a.remove(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_with_zones_parse_to_utc() {
        // 2026-10-22 20:48:57 +0800 is 12:48:57 UTC.
        let utc = parse_timestamp("2026-10-22T12:48:57.000Z").unwrap();
        assert_eq!(parse_timestamp("2026-10-22 20:48:57.00 +0800"), Some(utc));
        assert_eq!(parse_timestamp("2026-10-22 20:48:57+0800"), Some(utc));
        assert_eq!(parse_timestamp("2026-10-22T20:48:57+08:00"), Some(utc));
        assert_eq!(parse_timestamp("2026-10-22 07:48:57.000000-0500"), Some(utc));
        assert_eq!(parse_timestamp("2026-10-23 10:09:54.318304+0800").unwrap().millis() % 1000, 318);
        assert_eq!(parse_timestamp("2026-10-22T12:48:57.1234567Z").unwrap().millis() % 1000, 123);
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(Timestamp(0)));
    }

    #[test]
    fn bad_timestamps_are_refused() {
        for bad in [
            "",
            "yesterday",
            "2026-10-22",
            "2026-10-22 20:48:57",
            "2026-13-22 20:48:57 +0800",
            "2026-10-22 25:48:57Z",
            "2026-10-22 20:48:57 +08",
            "2026-10-22 20:48:57 *0800",
            "2026-10-2é 20:48:57Z",
        ] {
            assert_eq!(parse_timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn windows_for_tools() {
        assert_eq!(hours_back(Timestamp(0), Timestamp(1)), 1);
        assert_eq!(hours_back(Timestamp(0), Timestamp(HOUR)), 1);
        assert_eq!(hours_back(Timestamp(0), Timestamp(HOUR + 1)), 2);
        assert_eq!(hours_back(Timestamp(10), Timestamp(0)), 1);
        assert_eq!(timeout_for(1), Duration::from_secs(60));
        assert_eq!(timeout_for(24), Duration::from_secs(120));
    }

    #[test]
    fn this_os_has_readers_or_none_at_all() {
        let n = os_sources().len();
        if cfg!(any(windows, target_os = "macos", target_os = "linux")) {
            assert!(n >= 1);
        }
    }

    #[test]
    fn a_missing_tool_is_unsupported_with_a_reason() {
        let e = OsRunner.run("/definitely/not/a/tool", &[], Duration::from_secs(2)).unwrap_err();
        assert!(matches!(e, PlatformError::Unsupported(_)), "{e}");
    }
}
