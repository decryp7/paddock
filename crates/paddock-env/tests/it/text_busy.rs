//! The `ETXTBSY` retry (CI run 8174): its bound, its one error, and the two real refusals it exists for.

use paddock_env::text_busy::{self, MAX_RETRIES, is_text_busy, retry};
use std::io;

fn busy() -> io::Error {
    io::Error::from(io::ErrorKind::ExecutableFileBusy)
}

#[test]
fn a_busy_file_is_tried_again_until_it_is_free() {
    let mut calls = 0;
    let got = retry("busy-then-free", || {
        calls += 1;
        if calls < 4 { Err(busy()) } else { Ok(calls) }
    });
    assert_eq!(got.unwrap(), 4);
    assert_eq!(text_busy::retries_for("busy-then-free"), 3);
}

#[test]
fn any_other_error_returns_at_once() {
    let mut calls = 0;
    let got: io::Result<()> = retry("test", || {
        calls += 1;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    });
    assert_eq!(got.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(calls, 1);
}

#[test]
fn a_file_that_stays_busy_fails_after_the_bound_with_the_same_error() {
    let mut calls = 0;
    let got: io::Result<()> = retry("test", || {
        calls += 1;
        Err(busy())
    });
    assert!(is_text_busy(&got.unwrap_err()));
    assert_eq!(calls, MAX_RETRIES + 1);
}

// Linux only: macOS lets a file that is open for writing be executed and a running program be written (the probes below
// would fail there, which is how this was found).
#[cfg(target_os = "linux")]
mod real {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::process::Command;
    use std::sync::mpsc;

    fn script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("prog.sh");
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o755)
            .open(&path)
            .unwrap();
        f.write_all(format!("#!/bin/sh\n{body}\n").as_bytes())
            .unwrap();
        drop(f);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The first refusal: executing a file that something holds open for writing (what a child forked in the middle of a write
    /// holds). The write handle is let go only after the first attempt failed, so the retry is what makes the start succeed.
    #[test]
    fn a_program_whose_write_handle_is_still_held_starts_after_the_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = script(dir.path(), "exit 0");
        let writer = OpenOptions::new().write(true).open(&path).unwrap();
        // The precondition (ER-33): without the retry this start really is refused.
        let raw = Command::new(&path).spawn();
        assert!(
            raw.as_ref().is_err_and(is_text_busy),
            "the held write handle must make the start fail with ETXTBSY, got {raw:?}"
        );
        let (failed_tx, failed_rx) = mpsc::channel();
        let releaser = std::thread::spawn(move || {
            failed_rx.recv().unwrap();
            drop(writer);
        });
        let mut cmd = Command::new(&path);
        let mut child = retry("start the script", || {
            let r = cmd.spawn();
            if r.as_ref().is_err_and(is_text_busy) {
                failed_tx.send(()).ok();
            }
            r
        })
        .unwrap();
        releaser.join().unwrap();
        assert!(child.wait().unwrap().success());
    }

    /// Ends its child on every path out of a test, a failed assertion included (no zombie).
    struct Reap(Option<std::process::Child>);

    impl Reap {
        fn end(&mut self) {
            if let Some(mut c) = self.0.take() {
                c.kill().unwrap();
                c.wait().unwrap();
            }
        }
    }

    impl Drop for Reap {
        fn drop(&mut self) {
            if let Some(mut c) = self.0.take() {
                drop(c.kill());
                drop(c.wait());
            }
        }
    }

    /// The second refusal: writing a file whose program is running (a copy of `sleep`: only a binary the kernel maps is busy,
    /// a script is only read by its shell). The program is ended only after the first write failed, so what the retry waits
    /// out is the end of the program.
    #[test]
    fn a_program_that_still_runs_can_be_written_again_once_it_has_ended() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prog");
        std::fs::copy("/bin/sleep", &path).unwrap();
        let mut cmd = Command::new(&path);
        cmd.arg("600");
        let mut child = Reap(Some(retry("start the program", || cmd.spawn()).unwrap()));
        let raw = std::fs::write(&path, b"#!/bin/sh\nexit 0\n");
        assert!(
            raw.as_ref().is_err_and(is_text_busy),
            "a running program must refuse a write with ETXTBSY, got {raw:?}"
        );
        let mut ended = false;
        retry("replace the program", || {
            let r = std::fs::write(&path, b"#!/bin/sh\nexit 0\n");
            if r.as_ref().is_err_and(is_text_busy) && !ended {
                child.end();
                ended = true;
            }
            r
        })
        .unwrap();
        assert!(
            ended,
            "the first retried write must have met the running program"
        );
    }
}
