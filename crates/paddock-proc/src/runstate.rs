//! Whether a process is stopped (ADR 0031: a process the OS or an app such as App Tamer has stopped is waiting, not hung).
//! Windows is `Unknown`: `PROCESS_EXTENDED_BASIC_INFORMATION.IsFrozen` is deferred (ADR 0031 section 4).
//! Nothing else about the run state is read: the CPU time and the ready handshake say the rest.
//!
//! The reading is identity-checked (rule 53, ADR 0033): a pid that was handed on is [`RunState::Exited`], never the new
//! holder's state.

use crate::identity::{Holder, ProcessId, StartTime, holder};
use crate::usage::CpuSample;

/// What the OS says a process is doing, as far as a start judge needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// Running, sleeping or waiting for something: it can make progress.
    Running,
    /// Stopped by a signal (`SIGSTOP`, App Tamer, a debugger) and not resumed.
    Stopped,
    /// Gone, a zombie, or its pid now belongs to another process.
    Exited,
    /// The process may be there but this OS or this process's rights do not say (Windows, a process of another user, a
    /// `/proc` read that failed for a reason other than the process being gone).
    Unknown,
}

/// The run state of the process `id` (identity-checked: a pid that now names another process is `Exited`).
pub fn run_state(id: ProcessId) -> RunState {
    crate::sys::run_state(id)
}

/// The state field (3) of a Linux `/proc/<pid>/stat` line; the fields are counted after the last `)` of the command name.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_stat_state(stat: &str) -> Option<char> {
    stat[stat.rfind(')')? + 1..]
        .split_ascii_whitespace()
        .next()?
        .chars()
        .next()
}

/// Whether a Linux state letter is a stop (`T`: stopped by a signal, `t`: stopped by a tracer).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn is_stop_letter(state: char) -> bool {
    matches!(state, 'T' | 't')
}

/// The state from the text of `/proc/<pid>/stat`; `start` turns its start tick into a start time.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn from_stat(
    id: ProcessId,
    stat: &str,
    start: impl FnOnce(u64) -> Option<StartTime>,
) -> RunState {
    let (Some(ticks), Some(state)) = (
        crate::identity::parse_stat_start(stat),
        parse_stat_state(stat),
    ) else {
        return RunState::Unknown;
    };
    if holder(id, start(ticks)) != Holder::Same {
        return RunState::Exited;
    }
    match state {
        'Z' | 'X' | 'x' => RunState::Exited,
        s if is_stop_letter(s) => RunState::Stopped,
        _ => RunState::Running,
    }
}

/// The state from the read of `/proc/<pid>/stat`: only a process that is not there (`NotFound`, or `ESRCH` for one that
/// vanished mid-read) is `Exited`; any other failure (rights, a busy `/proc`) is `Unknown`, never a guess.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn from_read(
    id: ProcessId,
    read: std::io::Result<String>,
    start: impl FnOnce(u64) -> Option<StartTime>,
) -> RunState {
    match read {
        Ok(stat) => from_stat(id, &stat, start),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound || e.raw_os_error() == Some(3) => {
            RunState::Exited
        }
        Err(_) => RunState::Unknown,
    }
}

/// The state from a CPU sample read with the identity (macOS: both come from one `proc_pidinfo` call); `now` reads the start
/// time of the pid's holder when the sample carried none or there is no sample.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn from_sample(
    id: ProcessId,
    sample: Option<CpuSample>,
    now: impl FnOnce() -> Option<StartTime>,
) -> RunState {
    let Some(sample) = sample else {
        return match holder(id, now()) {
            Holder::Same => RunState::Unknown,
            _ => RunState::Exited,
        };
    };
    if holder(id, sample.started.or_else(now)) != Holder::Same {
        return RunState::Exited;
    }
    match sample.stopped {
        Some(true) => RunState::Stopped,
        Some(false) => RunState::Running,
        None => RunState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(started: u64) -> ProcessId {
        ProcessId::new(42, StartTime::from_micros(started))
    }
    fn stat(state: char, start: u64) -> String {
        format!(
            "42 (a (weird) name) {state} 1 42 42 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 1 0 {start} 1000000 50"
        )
    }
    fn at(ticks: u64) -> Option<StartTime> {
        Some(StartTime::from_micros(ticks * 10))
    }

    #[test]
    fn a_stat_line_gives_the_state_letter_after_the_last_parenthesis() {
        assert_eq!(parse_stat_state(&stat('T', 7)), Some('T'));
        assert_eq!(parse_stat_state("no parenthesis"), None);
        assert_eq!(parse_stat_state("1 (x)"), None);
    }

    #[test]
    fn only_t_and_lower_t_are_stops() {
        for (letter, stop) in [
            ('T', true),
            ('t', true),
            ('S', false),
            ('R', false),
            ('D', false),
            ('Z', false),
        ] {
            assert_eq!(is_stop_letter(letter), stop, "{letter}");
        }
    }

    #[test]
    fn stat_states_map_to_run_states_and_a_handed_on_pid_is_exited() {
        let me = id(70);
        for (letter, want) in [
            ('T', RunState::Stopped),
            ('t', RunState::Stopped),
            ('S', RunState::Running),
            ('R', RunState::Running),
            ('D', RunState::Running),
            ('Z', RunState::Exited),
            ('X', RunState::Exited),
        ] {
            assert_eq!(from_stat(me, &stat(letter, 7), at), want, "{letter}");
        }
        assert_eq!(
            from_stat(me, &stat('T', 8), at),
            RunState::Exited,
            "another start time: not this process"
        );
        assert_eq!(
            from_stat(me, &stat('T', 7), |_| None),
            RunState::Exited,
            "start time unreadable"
        );
        assert_eq!(from_stat(me, "garbage", at), RunState::Unknown);
    }

    #[test]
    fn only_a_missing_process_is_exited_any_other_read_error_is_unknown() {
        use std::io::{Error, ErrorKind};
        let me = id(70);
        let read = |e: Error| from_read(me, Err(e), at);
        assert_eq!(read(Error::from(ErrorKind::NotFound)), RunState::Exited);
        assert_eq!(read(Error::from_raw_os_error(3)), RunState::Exited, "ESRCH");
        assert_eq!(
            read(Error::from(ErrorKind::PermissionDenied)),
            RunState::Unknown
        );
        assert_eq!(read(Error::from(ErrorKind::Other)), RunState::Unknown);
        assert_eq!(from_read(me, Ok(stat('T', 7)), at), RunState::Stopped);
    }

    #[test]
    fn a_sample_with_its_identity_gives_the_state_and_one_without_is_looked_up() {
        let me = id(5);
        let sample = |started: Option<u64>, stopped| CpuSample {
            micros: 1,
            started: started.map(StartTime::from_micros),
            stopped,
        };
        let never = || -> Option<StartTime> { panic!("the sample carried the identity") };
        assert_eq!(
            from_sample(me, Some(sample(Some(5), Some(true))), never),
            RunState::Stopped
        );
        assert_eq!(
            from_sample(me, Some(sample(Some(5), Some(false))), never),
            RunState::Running
        );
        assert_eq!(
            from_sample(me, Some(sample(Some(5), None)), never),
            RunState::Unknown
        );
        assert_eq!(
            from_sample(me, Some(sample(Some(6), Some(true))), never),
            RunState::Exited
        );
        let same = || Some(StartTime::from_micros(5));
        assert_eq!(
            from_sample(me, Some(sample(None, Some(true))), same),
            RunState::Stopped
        );
        assert_eq!(
            from_sample(me, Some(sample(None, Some(true))), || None),
            RunState::Exited
        );
        assert_eq!(
            from_sample(me, None, same),
            RunState::Unknown,
            "there but unreadable"
        );
        assert_eq!(from_sample(me, None, || None), RunState::Exited);
        assert_eq!(
            from_sample(me, None, || Some(StartTime::from_micros(9))),
            RunState::Exited
        );
    }
}
