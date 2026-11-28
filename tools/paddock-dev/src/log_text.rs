//! A CI job log line without the runner's timestamp and colours: the one reader for the planner's and the run readers' logs; and the
//! one writer of a whole log line ([`write_line`], [`eline`], [`oline`]); and the one wording of a failure
//! line ([`error_line`]) and of how a process ended ([`describe_status`], [`describe_code`]), which every layer prints.

use regex::Regex;
use std::io::Write;
use std::process::ExitStatus;
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// Any ANSI escape sequence (colours, cursor moves).
static ANSI: LazyLock<Regex> = LazyLock::new(|| re(r"\x1b\[[0-9;]*[A-Za-z]"));
/// The runner's `2026-11-09T10:24:37.4427582Z ` stamp.
static STAMP: LazyLock<Regex> = LazyLock::new(|| re(r"^\d{4}-\d\d-\d\dT[0-9:.]+Z ?"));

/// A log line without the runner's timestamp and colours.
pub fn plain(line: &str) -> String {
    let no_colour = ANSI.replace_all(line.trim_end_matches('\r'), "");
    STAMP.replace(&no_colour, "").to_string()
}

/// Writes `line` and its newline in one `write_all`, so a log line or a workflow command (`::notice::`, `::warning::`) arrives whole.
/// `eprintln!` writes the text and the newline as two writes on unbuffered standard error, and the runner, which merges standard
/// output and standard error chunk by chunk, put a line from standard output between them: ci.yml run 8976, plan job 48889 printed
/// `...nothing else is needed.::notice::inactive: ...`, so the notice was not a notice.
pub fn write_line(w: &mut dyn Write, line: &str) -> std::io::Result<()> {
    let mut whole = String::with_capacity(line.len() + 1);
    whole.push_str(line);
    whole.push('\n');
    w.write_all(whole.as_bytes())?;
    w.flush()
}

/// A whole line on standard error (a command's log). A write that fails (a closed pipe) is dropped: the log is not the answer.
pub fn eline(line: &str) {
    if write_line(&mut std::io::stderr().lock(), line).is_err() {}
}

/// A whole line on standard output (a workflow command or an answer line).
pub fn oline(line: &str) {
    if write_line(&mut std::io::stdout().lock(), line).is_err() {}
}

/// `exit <n>` (with the NTSTATUS in hex for a Windows crash code) or `signal <n>`.
pub fn describe_status(s: ExitStatus) -> String {
    if let Some(code) = s.code() {
        return describe_code(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = s.signal() {
            return format!("signal {sig}");
        }
    }
    "ended without an exit code".to_string()
}

/// An exit code for a person: Windows reports a crash as an NTSTATUS (0xC0000005 is an access violation, 0xC0000409 a fast
/// fail), which reads as a huge or negative number in decimal.
pub fn describe_code(code: i32) -> String {
    let unsigned = code as u32;
    if unsigned >= 0xC000_0000 {
        format!("exit {unsigned} (0x{unsigned:08X}, an NTSTATUS crash code)")
    } else {
        format!("exit {code}")
    }
}

/// The one `::error::` line of a failure: the cause, then the next step.
pub fn error_line(cause: &str, next: &str) -> String {
    format!("::error::{cause} Next step: {next}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records each `write` call as one chunk, the unit in which the runner merges two streams.
    struct Calls(Vec<Vec<u8>>);
    impl Write for Calls {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.push(b.to_vec());
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Standard error's first chunk, then standard output's line, then the rest of standard error: the worst merge.
    fn merged(err: &[Vec<u8>], out: &[Vec<u8>]) -> String {
        let mut m = err[0].clone();
        m.extend(out.concat());
        m.extend(err[1..].concat());
        String::from_utf8_lossy(&m).into_owned()
    }

    #[test]
    fn a_line_is_one_write_so_a_notice_from_the_other_stream_starts_its_own_line() {
        let warning = "::warning::the selection record of this plan was not saved (x); Next: fix the cause above; nothing else is needed.";
        let notice = "::notice::inactive: debrief, dyno";
        let (mut err, mut out) = (Calls(Vec::new()), Calls(Vec::new()));
        write_line(&mut err, warning).unwrap();
        write_line(&mut out, notice).unwrap();
        assert_eq!(err.0.len(), 1, "one write per line");
        let text = merged(&err.0, &out.0);
        assert!(text.lines().any(|l| l == notice) && text.lines().any(|l| l == warning), "{text}");
        // Positive control: the old form (`writeln!` on an unbuffered writer, what `eprintln!` does) splits the line in two writes, and
        // the notice is glued to the warning, as in run 8976.
        let mut old = Calls(Vec::new());
        writeln!(old, "{warning}").unwrap();
        assert!(old.0.len() > 1, "the old form writes the newline apart");
        let text = merged(&old.0, &out.0);
        assert!(!text.lines().any(|l| l == notice), "{text}");
        assert!(text.contains("needed.::notice::inactive"), "{text}");
    }

    #[test]
    fn stamps_colours_and_carriage_returns_are_removed() {
        assert_eq!(plain("2026-11-09T10:24:37.4427582Z \x1b[32m  PASS\x1b[0m x\r"), "  PASS x");
        assert_eq!(plain("no stamp"), "no stamp");
    }

    #[test]
    fn windows_crash_codes_are_named_in_hex() {
        assert_eq!(describe_code(1), "exit 1");
        assert_eq!(describe_code(127), "exit 127");
        assert_eq!(
            describe_code(-1_073_741_819),
            "exit 3221225477 (0xC0000005, an NTSTATUS crash code)"
        );
    }

    #[test]
    fn an_error_line_has_the_cause_and_the_next_step() {
        assert_eq!(error_line("x failed.", "do y."), "::error::x failed. Next step: do y.");
    }
}
