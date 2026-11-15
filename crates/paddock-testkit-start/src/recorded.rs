//! A pid a process wrote about itself, read by a test (rule 53, ADR 0033): a pid file a stand-in wrote, or a line a child
//! printed. By the time the test looks, the code under test may have ended that process, and its pid may have been handed on
//! to another one. `ProcessId::of` cannot tell these apart from "not started yet": it gives `None` for a process that has
//! ended (a test that polls on it waits out its bound for a file that was there all along), and the identity of whoever holds
//! the pid now, which a failure path would then end (ER-14).
//!
//! - [`read_pid_file`] / [`read_pid_file_or`]: waits, with a bound, only until the file holds a complete record (a pid and a
//!   newline); then takes the identity once, through [`ProcessId::of_recorded`] with the file's modification time: a holder
//!   that started after the file was written got the pid when the writer had ended.
//! - [`printed`]: the identity of a pid a child printed, read after [`Stamp::now`] was taken right before the spawn: the
//!   holder must have started between that stamp and the read.
//!
//! - [`reported`]: the identity of a pid another program listed together with the start time it saw (a service's process
//!   list): the holder must have started within that time's resolution.
//!
//! - [`child_of_this_test`]: the identity of a pid that code inside the test process reported for a child it started (a
//!   supervisor's worker): verified to be a child of this process that started after it.
//!
//! All answer [`Recorded::Ended`] when no process that could have written the record holds the pid, and that is final: the
//! record was complete, so `Ended` never means "not yet".

use paddock_proc::ProcessId;
use paddock_testkit::HANG_GUARD;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Linux counts a start time from a boot time it reads in whole seconds, so a process can read up to about a second earlier than
/// a clock stamp taken before it started (the same slack as [`crate::Reaper`]'s creation time).
const START_SLACK_MS: i64 = 1_000;

/// What a recorded pid names when the test looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// The process that wrote the record, still there: act on it by this identity.
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
    let slack = START_SLACK_MS.unsigned_abs();
    let earliest = started_ms.saturating_sub(slack).saturating_mul(1000);
    let latest = started_ms
        .saturating_add(resolution_ms)
        .saturating_add(slack)
        .saturating_mul(1000);
    match ProcessId::of(pid) {
        Some(id) if (earliest..latest).contains(&id.started().as_micros()) => Recorded::Running(id),
        _ => Recorded::Ended { pid },
    }
}

/// The identity of `pid`, which code running inside this test process reported for a child it started (a supervisor's
/// worker, read from its status): the holder must be a child of this process that started after this process did. A pid
/// whose child was reaped and handed on to an unrelated process is `Ended`, so a test that ends "the worker" never ends a
/// stranger (ER-14). (A pid handed on to another child of this process is that child, which the test started too.)
pub fn child_of_this_test(pid: u32) -> Recorded {
    let me = ProcessId::of(std::process::id());
    let verified = paddock_proc::own::look(None, &[])
        .into_iter()
        .find(|row| row.id.pid() == pid)
        .filter(|row| {
            row.parent == Some(std::process::id())
                && !row.zombie
                && me.is_some_and(|me| row.id.started() >= me.started())
        });
    match verified {
        // The row's identity was read before its parent: confirm the pid still names it.
        Some(row) if ProcessId::of(pid) == Some(row.id) => Recorded::Running(row.id),
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

/// Waits until `file` holds a complete pid record ([`look_pid_file`]) and returns what it names. Panics after
/// [`HANG_GUARD`] with the last look (a writer that never writes is a hang, not slow).
pub fn read_pid_file(file: &Path, what: &str) -> Recorded {
    match read_pid_file_or(file, what, || false) {
        Ok(r) => r,
        Err(last) => unreachable!("a wait that is never stopped stopped: {last}"),
    }
}

/// [`read_pid_file`], asking `stop` between looks: when it says true before the record is complete (the code under test has
/// returned, so the record will never come), returns the last look as `Err`, for the test to report with the code's own error.
pub fn read_pid_file_or(
    file: &Path,
    what: &str,
    mut stop: impl FnMut() -> bool,
) -> Result<Recorded, String> {
    let end = std::time::Instant::now() + HANG_GUARD;
    let mut pause = paddock_testkit::wait::FIRST;
    loop {
        let last = match look_pid_file(file) {
            Ok(r) => return Ok(r),
            Err(last) => last,
        };
        if stop() {
            return Err(last);
        }
        assert!(
            std::time::Instant::now() < end,
            "{what}: no complete pid record in {} after {:.0} s; last look: {last}",
            file.display(),
            HANG_GUARD.as_secs_f64()
        );
        std::thread::sleep(pause); // conformance: hang-guard — a poll for a file, bounded by HANG_GUARD
        pause = (pause * 2).min(paddock_testkit::wait::LAST);
    }
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    /// A child that runs until its stdin closes (`cat` reading nothing), started after the caller's stamp.
    fn holder() -> std::process::Child {
        let mut c = paddock_proc::command(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "recorded::tests::hold_child",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(HOLD, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        c.spawn().unwrap()
    }

    const HOLD: &str = "PADDOCK_TESTKIT_RECORDED_HOLD";

    /// Not a test: the held child, when this binary runs as one (it reads stdin to its end, so it ends with the test).
    #[test]
    fn hold_child() {
        if std::env::var_os(HOLD).is_some() {
            let mut rest = Vec::new();
            drop(std::io::Read::read_to_end(&mut std::io::stdin(), &mut rest));
            std::process::exit(0);
        }
    }

    fn end(mut child: std::process::Child) {
        drop(child.stdin.take());
        child.wait().unwrap();
    }

    /// The pid of a child that has ended and been collected.
    fn ended_pid() -> u32 {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args(["--exact", "recorded::tests::no_such_test"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = c.spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn a_complete_record_of_a_running_writer_is_its_identity() {
        let d = tempfile::tempdir().unwrap();
        let child = holder();
        let file = d.path().join("pid");
        std::fs::write(&file, format!("{}\n", child.id())).unwrap();
        let r = read_pid_file(&file, "the holder");
        assert_eq!(r.running().map(ProcessId::pid), Some(child.id()), "{r:?}");
        end(child);
    }

    /// The race of run 8118: the pid file is complete, but its writer was ended before the test looked. The answer is `Ended` at
    /// the first look, not a wait for the bound.
    #[test]
    fn a_record_whose_writer_has_ended_is_ended_at_once() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("pid");
        let pid = ended_pid();
        std::fs::write(&file, format!("{pid}\n")).unwrap();
        let t = std::time::Instant::now();
        let r = read_pid_file(&file, "an ended writer");
        assert_eq!(r, Recorded::Ended { pid });
        assert!(r.has_ended());
        assert!(t.elapsed() < HANG_GUARD / 2, "{:?}", t.elapsed());
    }

    /// A pid handed on: the process that holds the recorded pid now started after the record was written, so it is not the
    /// writer, and a failure path must not end it (ER-14).
    #[test]
    fn a_record_older_than_the_pid_holder_names_no_one() {
        let d = tempfile::tempdir().unwrap();
        let child = holder();
        let file = d.path().join("pid");
        std::fs::write(&file, format!("{}\n", child.id())).unwrap();
        let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(an_hour_ago)
            .unwrap();
        let r = read_pid_file(&file, "a handed-on pid");
        assert_eq!(r, Recorded::Ended { pid: child.id() });
        end(child);
    }

    /// A record being written is "not yet": the wait goes on until the line is complete.
    #[test]
    fn a_record_without_its_newline_is_not_yet_and_the_wait_goes_on() {
        let d = tempfile::tempdir().unwrap();
        let child = holder();
        let file = d.path().join("pid");
        let pid = child.id().to_string();
        // A torn write of a longer pid: the first digit only.
        std::fs::write(&file, &pid[..1]).unwrap();
        assert!(
            look_pid_file(&file)
                .unwrap_err()
                .contains("not yet a complete line")
        );
        let mut looks = 0;
        let r = read_pid_file_or(&file, "the holder", || {
            looks += 1;
            std::fs::write(&file, format!("{pid}\n")).unwrap();
            false
        })
        .unwrap();
        assert_eq!(looks, 1);
        assert_eq!(r.running().map(ProcessId::pid), Some(child.id()));
        end(child);
    }

    /// A wait whose code under test has returned stops with what it saw, for the test to report the code's error.
    #[test]
    fn a_stopped_wait_returns_its_last_look() {
        let d = tempfile::tempdir().unwrap();
        let e = read_pid_file_or(&d.path().join("pid"), "nothing", || true).unwrap_err();
        assert!(e.starts_with("no pid file yet"), "{e}");
    }

    #[test]
    fn a_printed_pid_is_its_writer_only_if_it_started_after_the_stamp() {
        let spawned = Stamp::now();
        let child = holder();
        assert_eq!(
            printed(child.id(), spawned).running().map(ProcessId::pid),
            Some(child.id())
        );
        // Started before the stamp (an hour earlier than any process this test started): not the child that printed it.
        let later = Stamp(spawned.0 + 3_600_000);
        assert_eq!(
            printed(child.id(), later),
            Recorded::Ended { pid: child.id() }
        );
        end(child);
        let pid = ended_pid();
        assert_eq!(printed(pid, spawned), Recorded::Ended { pid });
    }

    /// A service's process list names the pid with the start time it saw; a holder that started at another time (the pid was
    /// handed on) is not that process, and a failure path must not end it (ER-14).
    #[test]
    fn a_reported_pid_is_its_process_only_if_the_reported_start_matches() {
        let child = holder();
        let id = ProcessId::of(child.id()).unwrap();
        let ms = id.started().as_micros() / 1000;
        assert_eq!(reported(child.id(), ms, 1).running(), Some(id));
        // Whole seconds, as sysinfo reports them.
        assert_eq!(
            reported(child.id(), ms / 1000 * 1000, 1000).running(),
            Some(id)
        );
        // Reported an hour earlier: the process holding the pid now is another one.
        assert_eq!(
            reported(child.id(), ms - 3_600_000, 1000),
            Recorded::Ended { pid: child.id() }
        );
        end(child);
        let pid = ended_pid();
        assert_eq!(reported(pid, ms, 1000), Recorded::Ended { pid });
    }

    #[test]
    fn a_child_of_this_test_is_verified_by_its_parent() {
        let child = holder();
        assert_eq!(
            child_of_this_test(child.id()).running().map(ProcessId::pid),
            Some(child.id())
        );
        // This process's parent and pid 1 are not children of this test.
        let parent = paddock_proc::own::look(None, &[])
            .into_iter()
            .find(|r| r.id.pid() == std::process::id())
            .and_then(|r| r.parent)
            .unwrap();
        assert_eq!(child_of_this_test(parent), Recorded::Ended { pid: parent });
        assert_eq!(child_of_this_test(1), Recorded::Ended { pid: 1 });
        end(child);
        let pid = ended_pid();
        assert_eq!(child_of_this_test(pid), Recorded::Ended { pid });
    }

    /// Whatever a pid file holds (torn, binary, huge numbers, signs), one look answers or says "not yet"; it never panics.
    #[test]
    fn a_pid_file_with_any_bytes_never_panics_a_look() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("pid");
        let seeds: [&[u8]; 5] = [
            b"4242\n",
            b"-1\n",
            b"99999999999999999999\n",
            b" 7 \r\n",
            b"\xff\xfe0\n",
        ];
        paddock_testkit::assert_never_panics(&seeds, |bytes| {
            std::fs::write(&file, bytes).unwrap();
            let _ = look_pid_file(&file);
        });
    }

    #[test]
    #[should_panic(
        expected = "has no identity: it ended (or its pid was handed on), or its start time is unreadable"
    )]
    fn an_ended_record_says_why_it_has_no_identity() {
        Recorded::Ended { pid: 7 }.expect_running("the stand-in");
    }
}
