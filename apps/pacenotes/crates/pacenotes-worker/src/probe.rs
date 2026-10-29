//! The per-process probe the stall judge reads: run state and CPU time of one pid.

use crate::stall::RunState;

/// One reading of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcSample {
    /// Its run state.
    pub state: RunState,
    /// CPU time used so far (user and system), in microseconds.
    pub cpu_micros: u64,
}

/// Reads a process. `None` means the process is gone or the platform has no adapter.
pub trait ProcessProbe: Send + Sync {
    /// The state and CPU time of `pid` now.
    fn sample(&self, pid: u32) -> Option<ProcSample>;
}

/// The state letter of `ps` and `/proc/<pid>/stat`.
pub fn state_of(letter: char) -> Option<RunState> {
    match letter {
        'R' => Some(RunState::Runnable),
        'D' | 'U' => Some(RunState::IoWait),
        'S' | 'I' => Some(RunState::Blocked),
        'T' | 't' => Some(RunState::Stopped),
        _ => None,
    }
}

/// The text of `/proc/<pid>/stat` as a sample (`CLK_TCK` is taken as 100, as on every mainstream Linux).
pub fn parse_proc_stat(text: &str) -> Option<ProcSample> {
    let after = &text[text.rfind(')')? + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    let state = state_of(fields.first()?.chars().next()?)?;
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(ProcSample {
        state,
        cpu_micros: (utime + stime).saturating_mul(10_000),
    })
}

/// The output of `ps -o state= -o cputime= -p <pid>` (`S      0:01.23` or `R   1:02:03.45`) as a sample.
pub fn parse_ps(text: &str) -> Option<ProcSample> {
    let mut words = text.split_whitespace();
    let state = state_of(words.next()?.chars().next()?)?;
    let time = words.next()?;
    let mut micros = 0u64;
    for part in time.split(':') {
        let (whole, fraction) = part.split_once('.').unwrap_or((part, ""));
        let seconds: u64 = whole.parse().ok()?;
        micros = micros.checked_mul(60)?.checked_add(seconds.checked_mul(1_000_000)?)?;
        if !fraction.is_empty() {
            let digits: String = fraction.chars().take(6).collect();
            let scale = 10u64.pow(6 - u32::try_from(digits.len()).ok()?);
            micros = micros.checked_add(digits.parse::<u64>().ok()? * scale)?;
        }
    }
    Some(ProcSample { state, cpu_micros: micros })
}

/// The adapter for this machine: `/proc` on Linux, `ps` (through `paddock-proc`) on macOS, none on Windows (a gap listed in
/// ADR 0006: there the backstops are Cancel and the memory cap).
#[derive(Debug, Default, Clone, Copy)]
pub struct OsProbe;

impl ProcessProbe for OsProbe {
    #[cfg(target_os = "linux")]
    fn sample(&self, pid: u32) -> Option<ProcSample> {
        parse_proc_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
    }

    #[cfg(target_os = "macos")]
    fn sample(&self, pid: u32) -> Option<ProcSample> {
        let mut cmd = paddock_proc::command("ps");
        cmd.args(["-o", "state=", "-o", "cputime=", "-p", &pid.to_string()]);
        let out = paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(std::time::Duration::from_secs(5))).ok()?;
        parse_ps(&String::from_utf8_lossy(&out.stdout))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn sample(&self, _pid: u32) -> Option<ProcSample> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_letters_map_to_run_states() {
        assert_eq!(state_of('R'), Some(RunState::Runnable));
        assert_eq!(state_of('S'), Some(RunState::Blocked));
        assert_eq!(state_of('I'), Some(RunState::Blocked));
        assert_eq!(state_of('D'), Some(RunState::IoWait));
        assert_eq!(state_of('U'), Some(RunState::IoWait));
        assert_eq!(state_of('T'), Some(RunState::Stopped));
        assert_eq!(state_of('Z'), None);
    }

    #[test]
    fn a_proc_stat_line_with_a_hostile_name_is_read_from_after_the_last_paren() {
        let line = "1234 (evil) S (name) R 1 1234 1234 0 -1 4194304 100 0 0 0 250 50 0 0 20 0 1 0 100 1000 100 18446744073709551615";
        assert_eq!(
            parse_proc_stat(line),
            Some(ProcSample {
                state: RunState::Runnable,
                cpu_micros: 3_000_000
            })
        );
        assert_eq!(parse_proc_stat("1 (x) S 1 1 1"), None, "too few fields");
        assert_eq!(parse_proc_stat("garbage"), None);
        assert_eq!(parse_proc_stat("1 (x) Z 1"), None);
    }

    #[test]
    fn ps_output_with_minutes_and_hours_is_read() {
        assert_eq!(
            parse_ps("S      0:01.23\n"),
            Some(ProcSample {
                state: RunState::Blocked,
                cpu_micros: 1_230_000
            })
        );
        assert_eq!(
            parse_ps("R   1:02:03.45"),
            Some(ProcSample {
                state: RunState::Runnable,
                cpu_micros: 3_723_450_000
            })
        );
        assert_eq!(
            parse_ps("T 12:00.5"),
            Some(ProcSample {
                state: RunState::Stopped,
                cpu_micros: 720_500_000
            })
        );
        assert_eq!(
            parse_ps("U 0:00.000001"),
            Some(ProcSample {
                state: RunState::IoWait,
                cpu_micros: 1
            })
        );
        assert_eq!(parse_ps(""), None);
        assert_eq!(parse_ps("S"), None);
        assert_eq!(parse_ps("S abc"), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_os_probe_reads_this_very_process() {
        let sample = OsProbe.sample(std::process::id()).expect("this process can be read");
        assert!(matches!(sample.state, RunState::Runnable | RunState::Blocked | RunState::IoWait));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_os_probe_returns_none_for_a_process_that_is_gone() {
        assert_eq!(OsProbe.sample(u32::MAX - 1), None);
    }
}
