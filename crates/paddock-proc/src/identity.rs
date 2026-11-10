//! Processes by identity, never by a bare pid (ADR 0033). An operating system hands a freed pid to the next process it
//! starts (Windows at once), so a pid read from a snapshot, a file or a parent link can name another program by the time it
//! is used. A [`ProcessId`] is the pid plus the time that process started; before anything acts on it, the start time of
//! whoever holds the pid now is compared ([`holder`]), and a mismatch is skipped with a warning instead of killed.

use crate::tree::Presence;
use std::fmt;

/// When a process started, in microseconds since the Unix epoch, as the OS reports it: the creation time on Windows,
/// `pbi_start_tv*` on macOS, the boot time plus the start tick on Linux (exact to a clock tick; the same process always
/// gives the same value). Compare start times of one machine only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StartTime(u64);

impl StartTime {
    /// A start time of `micros` microseconds since the Unix epoch (tests and values recorded earlier).
    pub const fn from_micros(micros: u64) -> StartTime {
        StartTime(micros)
    }

    /// Microseconds since the Unix epoch.
    pub const fn as_micros(self) -> u64 {
        self.0
    }
}

/// One process: its pid and when it started. Two processes that had the same pid one after the other are different
/// values, so a pid that was handed on is never mistaken for the process it used to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId {
    pid: u32,
    started: StartTime,
}

impl ProcessId {
    /// The process `pid` that started at `started`.
    pub const fn new(pid: u32, started: StartTime) -> ProcessId {
        ProcessId { pid, started }
    }

    /// The process that holds `pid` now, or `None` when there is none (or its start time cannot be read). Use it for a
    /// child this program started and has not reaped (its pid cannot be handed on before that) or right after a fresh look
    /// at the process table; never for a pid kept from earlier.
    pub fn of(pid: u32) -> Option<ProcessId> {
        crate::sys::start_time(pid).map(|s| ProcessId::new(pid, s))
    }

    /// The process that holds `pid` now, if it started no later than `written_ms` (ms since the Unix epoch): for a pid
    /// read from a file the process wrote about itself after it started (a discovery or lock file). A holder that started
    /// after the file was written got the pid when the writer had ended; it is `None` (the record is stale).
    pub fn of_recorded(pid: u32, written_ms: i64) -> Option<ProcessId> {
        ProcessId::of(pid).filter(|id| started_by(id.started, written_ms))
    }

    /// The process id.
    pub const fn pid(self) -> u32 {
        self.pid
    }

    /// When it started.
    pub const fn started(self) -> StartTime {
        self.started
    }

    /// Whether this process still runs: its pid is alive and still belongs to it. **Unknown counts as not alive** (the process
    /// table or its start time could not be read): right for code that ends or waits for a process, wrong for code that
    /// deletes a file, takes a lock over or starts a second copy on "gone". Those use [`ProcessId::presence`] or
    /// [`ProcessId::recorded_presence`] and treat [`Presence::Unknown`] as running (rule 53).
    pub fn is_alive(self) -> bool {
        holder(self, crate::sys::start_time(self.pid)) == Holder::Same
            && crate::tree::is_alive(self.pid)
    }
}

impl ProcessId {
    /// The three-valued [`ProcessId::is_alive`]: `Unknown` when the process table or the holder's start time cannot be read.
    pub fn presence(self) -> Presence {
        self.presence_given(
            crate::tree::presence(self.pid),
            crate::sys::start_time(self.pid),
        )
    }

    /// [`ProcessId::presence`] over what was read: the table's look at the pid and the start time of its holder.
    pub(crate) fn presence_given(self, table: Presence, now: Option<StartTime>) -> Presence {
        match (table, now) {
            (Presence::Unknown, _) => Presence::Unknown,
            (Presence::Gone, _) => Presence::Gone,
            (Presence::Running, None) => Presence::Unknown,
            (Presence::Running, Some(_)) => match holder(self, now) {
                Holder::Same => Presence::Running,
                Holder::Gone | Holder::Other(_) => Presence::Gone,
            },
        }
    }

    /// Whether the process that wrote a record at `written_ms` (ms since the Unix epoch) under `pid` still runs: the holder
    /// of `pid` must have started no later than the record ([`ProcessId::of_recorded`]). `Unknown` when the process table
    /// or the holder's start time cannot be read, so a discovery or lock file is never judged stale on a failed read.
    pub fn recorded_presence(pid: u32, written_ms: i64) -> Presence {
        recorded_given(
            crate::tree::presence(pid),
            crate::sys::start_time(pid),
            written_ms,
        )
    }
}

/// [`ProcessId::recorded_presence`] over what was read.
pub(crate) fn recorded_given(table: Presence, now: Option<StartTime>, written_ms: i64) -> Presence {
    match (table, now) {
        (Presence::Unknown, _) => Presence::Unknown,
        (Presence::Gone, _) => Presence::Gone,
        (Presence::Running, None) => Presence::Unknown,
        (Presence::Running, Some(s)) if started_by(s, written_ms) => Presence::Running,
        (Presence::Running, Some(_)) => Presence::Gone,
    }
}

impl fmt::Display for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pid {} (started at {} µs)", self.pid, self.started.0)
    }
}

/// Who holds a process's pid now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// The process itself.
    Same,
    /// Nobody: the process is gone.
    Gone,
    /// Another process, which started at this time.
    Other(StartTime),
}

/// Who holds `id`'s pid, given the start time of the process that holds it now (`None`: no process has it).
pub fn holder(id: ProcessId, now: Option<StartTime>) -> Holder {
    match now {
        None => Holder::Gone,
        Some(s) if s == id.started => Holder::Same,
        Some(s) => Holder::Other(s),
    }
}

/// How far a start time may lie after a wall-clock time written in whole milliseconds and still be before it: Linux counts
/// start times from the boot time in whole seconds; elsewhere the record's millisecond is rounded down.
#[cfg(target_os = "linux")]
const CLOCK_SLACK_MICROS: u64 = 1_000_000;
#[cfg(not(target_os = "linux"))]
const CLOCK_SLACK_MICROS: u64 = 999;

/// Whether a process that started at `started` can have written a record at `written_ms` (ms since the Unix epoch).
pub(crate) fn started_by(started: StartTime, written_ms: i64) -> bool {
    u64::try_from(written_ms)
        .ok()
        .and_then(|ms| ms.checked_mul(1000))
        .is_some_and(|written| started.0 <= written.saturating_add(CLOCK_SLACK_MICROS))
}

/// What ending one process found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// It was ours and was told to end.
    Ended,
    /// It was gone already.
    Gone,
    /// Its pid belongs to another process now; nothing was done to it.
    NotOurs(StartTime),
    /// It is ours but the OS refused (no rights, or it ended meanwhile).
    Refused,
}

impl Ending {
    /// The ending for a process whose pid has `holder` and that was signalled (`sent`) when it was ours.
    pub(crate) fn of(holder: Holder, sent: bool) -> Ending {
        match holder {
            Holder::Same if sent => Ending::Ended,
            Holder::Same => Ending::Refused,
            Holder::Gone => Ending::Gone,
            Holder::Other(s) => Ending::NotOurs(s),
        }
    }
}

/// What ending a process found when the OS was asked to end it: one that is ours but had already exited when the kill
/// came (Windows answers ACCESS_DENIED to terminating an exited process) is gone, not refused.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn ending_after_kill(found: Holder, sent: bool, exited: bool) -> Ending {
    match found {
        Holder::Same if !sent && exited => Ending::Gone,
        h => Ending::of(h, sent),
    }
}

/// Now, as a start time: a process listed in a scan taken now started no later than this.
pub(crate) fn now() -> StartTime {
    let micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX));
    StartTime(micros)
}

/// Logs a skipped kill: the pid was handed on to another process since it was seen.
pub(crate) fn warn_not_ours(id: ProcessId, now: StartTime) {
    log::warn!(
        "Not ending {id}: the pid now belongs to another process (started at {} µs); it was handed on after the process ended",
        now.0
    );
}

/// Microseconds since the Unix epoch from a Windows `FILETIME` (100 ns intervals since 1601), `None` before 1970.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn micros_from_filetime(filetime: u64) -> Option<u64> {
    const EPOCH_DIFF_MICROS: u64 = 11_644_473_600_000_000;
    (filetime / 10).checked_sub(EPOCH_DIFF_MICROS)
}

/// The start tick (field 22, `starttime`) of a Linux `/proc/<pid>/stat` line. The command name (field 2) may contain
/// spaces and parentheses, so the fields are counted after its last `)`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_stat_start(stat: &str) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    // After the name: state is field 3, so starttime (22) is the 20th field here.
    rest.split_ascii_whitespace().nth(19)?.parse().ok()
}

/// The boot time in seconds since the Unix epoch from the text of Linux `/proc/stat` (the `btime` line).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_btime(proc_stat: &str) -> Option<u64> {
    proc_stat
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()
}

/// Microseconds since the Unix epoch of a Linux start tick, given the boot time and the clock ticks per second.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn micros_from_ticks(boot_secs: u64, ticks: u64, hz: u64) -> Option<u64> {
    if hz == 0 {
        return None;
    }
    let since_boot = u64::try_from(u128::from(ticks) * 1_000_000 / u128::from(hz)).ok()?;
    boot_secs.checked_mul(1_000_000)?.checked_add(since_boot)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn at(us: u64) -> StartTime {
        StartTime::from_micros(us)
    }

    #[test]
    fn an_unreadable_table_or_start_time_is_unknown_never_gone() {
        use crate::tree::Presence::{Gone, Running, Unknown};
        let id = ProcessId::new(4242, at(1_000));
        assert_eq!(id.presence_given(Unknown, Some(at(1_000))), Unknown);
        assert_eq!(id.presence_given(Unknown, None), Unknown);
        assert_eq!(id.presence_given(Gone, None), Gone);
        assert_eq!(id.presence_given(Running, Some(at(1_000))), Running);
        assert_eq!(
            id.presence_given(Running, Some(at(9_000))),
            Gone,
            "a pid handed on"
        );
        assert_eq!(
            id.presence_given(Running, None),
            Unknown,
            "running, but its start time cannot be read"
        );
        assert!(
            !id.is_alive(),
            "is_alive stays false for an unknown or absent process"
        );
    }

    #[test]
    fn a_recorded_pid_is_judged_the_same_three_ways() {
        use crate::tree::Presence::{Gone, Running, Unknown};
        let written_ms = 10;
        assert_eq!(recorded_given(Unknown, None, written_ms), Unknown);
        assert_eq!(recorded_given(Gone, None, written_ms), Gone);
        assert_eq!(
            recorded_given(Running, Some(at(5_000)), written_ms),
            Running
        );
        assert_eq!(
            recorded_given(Running, Some(at(9_000_000)), written_ms),
            Gone,
            "started after the record"
        );
        assert_eq!(recorded_given(Running, None, written_ms), Unknown);
        assert_eq!(
            ProcessId::recorded_presence(std::process::id(), i64::MAX / 4000),
            Running
        );
    }

    #[test]
    fn a_pid_handed_on_is_another_process() {
        let id = ProcessId::new(4242, at(1_000));
        assert_eq!(holder(id, Some(at(1_000))), Holder::Same);
        assert_eq!(holder(id, None), Holder::Gone);
        assert_eq!(holder(id, Some(at(9_000))), Holder::Other(at(9_000)));
        assert_ne!(id, ProcessId::new(4242, at(9_000)));
        assert_eq!((id.pid(), id.started().as_micros()), (4242, 1_000));
        assert_eq!(id.to_string(), "pid 4242 (started at 1000 µs)");
    }

    #[test]
    fn a_kill_of_an_exited_process_is_gone_not_refused() {
        assert_eq!(ending_after_kill(Holder::Same, false, true), Ending::Gone);
        assert_eq!(
            ending_after_kill(Holder::Same, false, false),
            Ending::Refused
        );
        assert_eq!(ending_after_kill(Holder::Same, true, false), Ending::Ended);
        assert_eq!(
            ending_after_kill(Holder::Other(at(3)), false, true),
            Ending::NotOurs(at(3))
        );
        assert!(now() > at(1_700_000_000_000_000), "after 2023");
    }

    #[test]
    fn an_ending_says_what_was_found() {
        assert_eq!(Ending::of(Holder::Same, true), Ending::Ended);
        assert_eq!(Ending::of(Holder::Same, false), Ending::Refused);
        assert_eq!(Ending::of(Holder::Gone, false), Ending::Gone);
        assert_eq!(
            Ending::of(Holder::Other(at(5)), false),
            Ending::NotOurs(at(5))
        );
        warn_not_ours(ProcessId::new(1, at(1)), at(2));
    }

    #[test]
    fn this_process_has_a_stable_start_time_before_now() {
        let me = ProcessId::of(std::process::id()).expect("this process has a start time");
        assert_eq!(
            ProcessId::of(me.pid()),
            Some(me),
            "the same process always gives the same start time"
        );
        assert!(me.is_alive());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros();
        // Linux counts from the boot time in whole seconds.
        assert!(
            u128::from(me.started().as_micros()) <= now + 1_000_000,
            "{me} starts in the future"
        );
        assert_eq!(ProcessId::of(u32::MAX - 1), None);
        assert!(
            !ProcessId::new(
                me.pid(),
                StartTime::from_micros(me.started().as_micros() + 1)
            )
            .is_alive()
        );
    }

    #[test]
    fn a_record_is_written_by_a_process_that_started_before_it() {
        assert!(started_by(at(5_000_000), 5_000));
        assert!(
            started_by(at(5_000_999), 5_000),
            "within the record's millisecond"
        );
        assert_eq!(
            started_by(at(5_900_000), 5_000),
            cfg!(target_os = "linux"),
            "Linux allows the boot time's second"
        );
        assert!(
            !started_by(at(6_000_001), 5_000),
            "started after the record was written"
        );
        assert!(!started_by(at(1), -1), "a record from before 1970");
        assert!(!started_by(at(1), i64::MAX), "a time that overflows");
        let me = ProcessId::of(std::process::id()).unwrap();
        let now_ms = i64::try_from(me.started().as_micros() / 1000).unwrap() + 10;
        assert_eq!(ProcessId::of_recorded(me.pid(), now_ms), Some(me));
        assert_eq!(
            ProcessId::of_recorded(me.pid(), 0),
            None,
            "a record from before this process started"
        );
        assert_eq!(ProcessId::of_recorded(u32::MAX - 1, now_ms), None);
    }

    #[test]
    fn filetimes_convert_to_unix_micros() {
        // 2001-09-09T01:46:40Z is 1_000_000_000 s after the Unix epoch.
        assert_eq!(
            micros_from_filetime((11_644_473_600 + 1_000_000_000) * 10_000_000),
            Some(1_000_000_000_000_000)
        );
        assert_eq!(micros_from_filetime(0), None, "before 1970");
    }

    #[test]
    fn the_start_tick_is_read_after_the_last_parenthesis() {
        let stat = "1234 (a (weird) name) S 1 1234 1234 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 1 0 98765 1000000 50 18446744073709551615";
        assert_eq!(parse_stat_start(stat), Some(98_765));
        assert_eq!(parse_stat_start("1234 (x) S 1 2"), None, "too few fields");
        assert_eq!(parse_stat_start("no parenthesis"), None);
        assert_eq!(
            parse_stat_start("1 (x) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 lots 1"),
            None
        );
    }

    #[test]
    fn the_boot_time_is_read_from_proc_stat() {
        assert_eq!(
            parse_btime("cpu 1 2 3\nbtime 1700000000\nprocesses 5\n"),
            Some(1_700_000_000)
        );
        assert_eq!(parse_btime("cpu 1 2 3\n"), None);
        assert_eq!(parse_btime("btime soon\n"), None);
    }

    #[test]
    fn ticks_become_micros() {
        assert_eq!(micros_from_ticks(10, 250, 100), Some(12_500_000));
        assert_eq!(micros_from_ticks(10, 250, 0), None);
        assert_eq!(micros_from_ticks(u64::MAX, 1, 100), None);
    }
}
