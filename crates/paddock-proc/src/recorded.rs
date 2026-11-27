//! A pid a process wrote about itself, read by a test (rule 53, ADR 0033): a pid file a stand-in wrote, or a line a child
//! printed. By the time the test looks, the code under test may have ended that process, and its pid may have been handed on
//! to another one. `ProcessId::of` cannot tell these apart from "not started yet": it gives `None` for a process that has
//! ended (a test that polls on it waits out its bound for a file that was there all along), and the identity of whoever holds
//! the pid now, which a failure path would then end (ER-14).
//!
//! - [`look_pid_file`]: one look at a pid file: when it holds a complete record (a pid and a newline) the identity is taken once,
//!   through [`ProcessId::of_recorded`] with the file's modification time: a holder that started after the file was written got
//!   the pid when the writer had ended. (`paddock_testkit_start::recorded::read_pid_file` waits for the record, with a bound.)
//! - [`printed`]: the identity of a pid a child printed, read after [`Stamp::now`] was taken right before the spawn: the
//!   holder must have started between that stamp and the read.
//!
//! - [`reported`]: the identity of a pid another program listed together with the start time it saw (a service's process
//!   list): the holder must have started within that time's resolution.
//!
//! - [`child_of_this_test`] / [`descendant_of`]: the identity of a pid that code inside the test process reported for a child
//!   it started (a supervisor's worker), or that a process list names: verified to be below this process (or `root`).
//!
//! (Lives in paddock-proc behind `test-support` so its own tests use it too, with no dependency cycle; `paddock_testkit_start::recorded` re-exports it
//! and adds the pid-file wait, which needs the kit's hang guard.)
//!
//! All answer [`Recorded::Ended`] when no process that could have written the record holds the pid, and that is final: the
//! record was complete, so `Ended` never means "not yet".

use crate::ProcessId;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Linux counts a start time from a boot time it reads in whole seconds, so a process can read up to about a second earlier than
/// a clock stamp taken before it started (the same slack as the kit's `Reaper` creation time).
const START_SLACK_MS: i64 = 1_000;

/// What a recorded pid names when the test looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// The process that wrote the record, still there: act on it by this identity. (Windows still answers for an exited process
    /// whose handle someone holds open: ask [`Recorded::has_ended`], never read `Running` as "it runs".)
    Running(ProcessId),
    /// No process that could have written the record holds the pid: it has ended (or the pid was handed on to another
    /// process), or its start time cannot be read. Never "not yet".
    Ended {
        /// The pid the record named.
        pid: u32,
    },
}

impl Recorded {
    /// The identity, when the writer was still there.
    pub fn running(self) -> Option<ProcessId> {
        match self {
            Recorded::Running(id) => Some(id),
            Recorded::Ended { .. } => None,
        }
    }

    /// The pid the record named.
    pub fn pid(self) -> u32 {
        match self {
            Recorded::Running(id) => id.pid(),
            Recorded::Ended { pid } => pid,
        }
    }

    /// The identity, or a panic that says the recorded process has none.
    pub fn expect_running(self, what: &str) -> ProcessId {
        match self {
            Recorded::Running(id) => id,
            Recorded::Ended { pid } => panic!(
                "{what} (pid {pid}) has no identity: it ended (or its pid was handed on), or its start time is unreadable"
            ),
        }
    }

    /// Whether the recorded process is gone: [`Recorded::Ended`], or an identity that [has ended](ProcessId::has_ended).
    pub fn has_ended(self) -> bool {
        match self {
            Recorded::Running(id) => id.has_ended(),
            Recorded::Ended { .. } => true,
        }
    }
}

/// A wall-clock time in milliseconds since the Unix epoch, taken right before a spawn whose output will name a pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(i64);

impl Stamp {
    /// Now.
    pub fn now() -> Stamp {
        Stamp(millis(SystemTime::now()))
    }

    /// This stamp moved `by` later (a test of a holder that started before the stamp).
    #[must_use]
    pub fn after(self, by: std::time::Duration) -> Stamp {
        Stamp(
            self.0
                .saturating_add(i64::try_from(by.as_millis()).unwrap_or(i64::MAX)),
        )
    }
}

/// The identity of `pid`, which a child started after `spawned` printed and the test has just read: the holder must have started
/// no earlier than `spawned` and no later than now. A pid handed on between the child printing it and this call cannot be told
/// apart from the writer, so call it as soon as the line is read.
pub fn printed(pid: u32, spawned: Stamp) -> Recorded {
    let earliest = u64::try_from(spawned.0.saturating_sub(START_SLACK_MS))
        .unwrap_or(0)
        .saturating_mul(1000);
    match ProcessId::of_recorded(pid, millis(SystemTime::now())) {
        Some(id) if id.started().as_micros() >= earliest => Recorded::Running(id),
        _ => Recorded::Ended { pid },
    }
}

/// The identity of `pid`, which another program reported together with the start time it read, `started_ms` (ms since the
/// Unix epoch) at a resolution of `resolution_ms` (1000 for a start time in whole seconds): the holder must have started in
/// `[started_ms, started_ms + resolution_ms)`, with the clock's slack on both sides. A pid handed on within the same window
/// cannot be told apart, so keep the resolution as fine as the reporter's.
pub fn reported(pid: u32, started_ms: u64, resolution_ms: u64) -> Recorded {
    match ProcessId::of_reported(pid, started_ms, resolution_ms) {
        Some(id) => Recorded::Running(id),
        None => Recorded::Ended { pid },
    }
}

/// The identity of `pid`, which code running inside this test process reported for a child it started (a supervisor's
/// worker, read from its status): the holder must be a descendant of this process ([`descendant_of`]). A pid whose child was
/// reaped and handed on to an unrelated process is `Ended`, so a test that ends "the worker" never ends a stranger (ER-14). (A
/// pid handed on to another descendant of this process is that descendant, which the test started too.)
pub fn child_of_this_test(pid: u32) -> Recorded {
    match ProcessId::of(std::process::id()) {
        Some(me) => descendant_of(me, pid),
        None => Recorded::Ended { pid },
    }
}

/// The identity of `pid`, read from a process list or a status (a `ps` row, a supervisor's report), verified to be a
/// descendant of `root` in a fresh look at the process table: started after its parent, below `root` (`paddock_proc::
/// descendants_of`, which reads the start times and parents once and is far cheaper than a full table read with command lines).
pub fn descendant_of(root: ProcessId, pid: u32) -> Recorded {
    match crate::descendants_of(root)
        .into_iter()
        .find(|id| id.pid() == pid)
    {
        // The table was read before this call returned: confirm the pid still names that process.
        Some(id) if ProcessId::of(pid) == Some(id) => Recorded::Running(id),
        _ => Recorded::Ended { pid },
    }
}

/// One look at a pid file: the record when it is complete, or what was found instead.
///
/// A complete record is a pid followed by a newline (what `echo $$ > file` writes in one go), so a file that is being written
/// is "not yet", never a shorter pid.
pub fn look_pid_file(file: &Path) -> Result<Recorded, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("no pid file yet ({e})"))?;
    let Some(line) = text.strip_suffix('\n') else {
        return Err(format!(
            "the pid file holds {text:?}, not yet a complete line"
        ));
    };
    let pid = line
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("the pid file holds {text:?}, not a pid"))?;
    let written = std::fs::metadata(file)
        .and_then(|m| m.modified())
        .map_err(|e| format!("the pid file's modification time cannot be read ({e})"))?;
    Ok(match ProcessId::of_recorded(pid, millis(written)) {
        Some(id) => Recorded::Running(id),
        None => Recorded::Ended { pid },
    })
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
