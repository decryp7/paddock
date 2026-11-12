//! Paying the first run of a program before the start that matters (ADR 0071).
//!
//! [`warm_up`] runs `exe args` (`--version` by convention) once to its end, with no budget of its own, so the hold the OS puts
//! on the first run of a fresh binary on macOS (42 s on an idle Mac, over 240 s under I/O load, with no CPU used and no output)
//! is not charged to a start guard that follows. It is memoised per process by the file's identity, prints how long the first
//! start took (ER-27) and is never what makes a start correct: that is the wait on ready or exit ([`crate::StartWatch`]).
//!
//! macOS only in practice: on Windows a browser's `--version` starts the browser, so callers use it where the OS holds a first
//! run, not everywhere.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// How much of a failed first run's error output is kept (the end of it); the rest is read and dropped.
const STDERR_KEEP: usize = 4096;

/// What a program is: its canonical path, length and modification time, and on Unix its device and inode. A replaced file is a
/// new program (a rebuilt binary is checked again by the OS).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Identity {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    file: (u64, u64),
}

#[expect(
    clippy::disallowed_methods,
    reason = "a memo key compared only with other keys made here, never shown or passed to a program; paddock-proc has no paddock-env dependency yet: until WP-P2 (row 12) of the ADR 0021 amendment 2026-11-11"
)]
fn identity_of(exe: &OsStr) -> Option<Identity> {
    let path = std::fs::canonicalize(exe).ok()?;
    let meta = std::fs::metadata(&path).ok()?;
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    };
    Some(Identity {
        path,
        len: meta.len(),
        modified: meta.modified().ok(),
        #[cfg(unix)]
        file,
    })
}

fn memo() -> &'static Mutex<HashMap<Identity, Duration>> {
    static MEMO: OnceLock<Mutex<HashMap<Identity, Duration>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Runs `exe args` to its end, once per program identity in this process, and returns how long that first start took (a
/// repeat call returns the recorded time without starting it again). It prints `the first start of <exe> took <s> s` to
/// stderr each time it actually runs. There is no time limit here: the caller's own bound (a test runner's slow-timeout) is
/// the guard, because a process the OS holds before its first instruction can be told from a hang by neither CPU nor output.
/// Standard output is discarded and only the end of the error output is kept.
///
/// # Errors
/// A message naming the program when it could not be run, or its exit status and the tail of its error output when it failed:
/// a program that cannot print its version is broken, not slow. A failure is not memoised.
pub fn warm_up(exe: impl AsRef<OsStr>, args: &[&str]) -> Result<Duration, String> {
    let name = exe.as_ref().to_string_lossy().into_owned();
    let id = identity_of(exe.as_ref());
    if let Some(took) = id.as_ref().and_then(|i| {
        memo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(i)
            .copied()
    }) {
        return Ok(took);
    }
    let began = Instant::now();
    let mut child = crate::command(exe.as_ref())
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{name} could not be run for its first start: {e}"))?;
    let tail = child.stderr.take().map(|mut err| {
        let mut kept: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = err.read(&mut chunk) {
            if n == 0 {
                break;
            }
            kept.extend_from_slice(&chunk[..n]);
            if kept.len() > STDERR_KEEP {
                kept.drain(..kept.len() - STDERR_KEEP);
            }
        }
        String::from_utf8_lossy(&kept).trim().to_string()
    });
    let status = child
        .wait()
        .map_err(|e| format!("{name} could not be waited for: {e}"))?;
    let took = began.elapsed();
    if !status.success() {
        let tail = tail.unwrap_or_default();
        let tail: String = tail
            .chars()
            .rev()
            .take(400)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(format!(
            "{name}'s first run ended with {status} (its output: {})",
            if tail.is_empty() { "none" } else { &tail }
        ));
    }
    eprintln!("the first start of {name} took {:.1} s", took.as_secs_f64());
    if let Some(id) = id {
        memo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, took);
    }
    Ok(took)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn fifo(path: &Path) {
        let status = crate::command("mkfifo").arg(path).status().unwrap();
        assert!(status.success(), "mkfifo");
    }

    /// The first run is waited for to its end: the stand-in says it is running through one named pipe and then blocks reading
    /// another until the test releases it, so `warm_up` is seen not returning while the program is alive, however long that
    /// takes, with no sleep and no spin deciding anything (a read on a pipe blocks).
    #[test]
    fn warm_up_waits_for_a_program_that_answers_only_when_released() {
        let dir = tempfile::tempdir().unwrap();
        let (up, go) = (dir.path().join("up"), dir.path().join("go"));
        fifo(&up);
        fifo(&go);
        let exe = dir.path().join("slow.sh");
        script(
            &exe,
            &format!(
                "echo up > '{}'\ncat '{}' > /dev/null\necho 'Fake 1.0'\n",
                up.display(),
                go.display()
            ),
        );
        std::thread::scope(|s| {
            let run = s.spawn(|| warm_up(&exe, &["--version"]));
            let mut line = String::new();
            BufReader::new(std::fs::File::open(&up).unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(line.trim(), "up", "the program is running");
            assert!(
                !run.is_finished(),
                "warm_up returned while the program was still running"
            );
            std::fs::OpenOptions::new()
                .write(true)
                .open(&go)
                .unwrap()
                .write_all(b"go\n")
                .unwrap();
            assert!(run.join().unwrap().is_ok());
        });
    }

    /// A first run that fails names its status and the end of its output; one that cannot be run says why; neither is memoised.
    #[test]
    fn warm_up_names_a_failure_and_does_not_remember_it() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.sh");
        script(&bad, "echo 'bundle damaged' >&2\nexit 5\n");
        let e = warm_up(&bad, &["--version"]).unwrap_err();
        assert!(
            e.contains("exit status: 5") && e.contains("bundle damaged"),
            "{e}"
        );
        script(&bad, "echo 'fixed'\n");
        assert!(
            warm_up(&bad, &["--version"]).is_ok(),
            "a repaired program is started again"
        );
        let e = warm_up(dir.path().join("nope"), &[]).unwrap_err();
        assert!(e.contains("could not be run"), "{e}");
    }

    /// One start per program identity: a repeat costs nothing, and a replaced file is a new program. Each run appends a line to
    /// a file, so the count is the number of starts. Output beyond the kept tail is read and dropped without blocking the child.
    #[test]
    fn warm_up_starts_a_program_once_per_identity_and_again_after_it_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let runs = dir.path().join("runs");
        let exe = dir.path().join("count.sh");
        let body = format!(
            "echo run >> '{}'\nhead -c 300000 /dev/zero >&2\n",
            runs.display()
        );
        script(&exe, &body);
        let count = || std::fs::read_to_string(&runs).map_or(0, |t| t.lines().count());
        let first = warm_up(&exe, &["--version"]).unwrap();
        assert_eq!(count(), 1);
        assert_eq!(
            warm_up(&exe, &["--version"]).unwrap(),
            first,
            "the recorded time, no second start"
        );
        assert_eq!(count(), 1);
        script(&exe, &format!("{body}# replaced\n"));
        warm_up(&exe, &["--version"]).unwrap();
        assert_eq!(count(), 2, "a replaced file is a new program");
    }
}
