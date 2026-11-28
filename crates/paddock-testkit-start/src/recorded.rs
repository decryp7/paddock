//! A pid a process wrote about itself, read by a test (rule 53, ADR 0033). The identity helpers live in `paddock_proc::recorded`
//! (re-exported here: [`Recorded`], [`Stamp`], [`printed`], [`reported`], [`child_of_this_test`], [`descendant_of`],
//! [`look_pid_file`]); this module adds the bounded wait for a pid file, which needs the kit's hang guard.

pub use paddock_proc::recorded::*;
use paddock_testkit::HANG_GUARD;
use std::path::Path;

/// Waits until `file` holds a complete pid record ([`look_pid_file`]) and returns what it names. A pid handed on within the
/// clock's slack after the write (about a second on Linux, under a millisecond elsewhere) cannot be told from the writer. Panics after
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

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_proc::ProcessId;
    use std::process::Stdio;
    use std::time::{Duration, SystemTime};

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

    /// A child that has ended and been waited for. Its `Child` is kept until the test ends: on Windows the open handle keeps
    /// the pid from being handed on (Windows reuses a freed pid at once), so an answer about the pid is about this child.
    struct Exited {
        _handle: std::process::Child,
        pid: u32,
    }

    fn ended() -> Exited {
        let mut c = paddock_proc::command(std::env::current_exe().unwrap());
        c.args(["--exact", "recorded::tests::no_such_test"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = c.spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        Exited {
            _handle: child,
            pid,
        }
    }

    /// `r` says the exited child is gone: `Ended` (Unix, where a collected child has no start time), or, where the OS still
    /// answers for an exited process whose handle is open (Windows), its identity, which has ended.
    #[track_caller]
    fn assert_gone(r: Recorded, gone: &Exited) {
        assert!(
            r.pid() == gone.pid && r.has_ended(),
            "{r:?} for the exited child {}",
            gone.pid
        );
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
        let gone = ended();
        std::fs::write(&file, format!("{}\n", gone.pid)).unwrap();
        let t = std::time::Instant::now();
        let r = read_pid_file(&file, "an ended writer");
        assert_gone(r, &gone);
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
        let later = spawned.after(Duration::from_secs(3600));
        assert_eq!(
            printed(child.id(), later),
            Recorded::Ended { pid: child.id() }
        );
        end(child);
        let gone = ended();
        assert_gone(printed(gone.pid, spawned), &gone);
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
        let gone = ended();
        assert_gone(reported(gone.pid, ms, 1000), &gone);
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
        let gone = ended();
        assert_gone(child_of_this_test(gone.pid), &gone);
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
