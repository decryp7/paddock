//! "What is locking this file?" through `libproc` (macOS has no Restart Manager). Never `lsof -- <path>`: it asks the
//! kernel about every socket and pipe descriptor of every process (`dproc.c` `process_socket`), which can mark another
//! program's socket `SS_DRAINING` (ADR 0026). Here every process's descriptors are only listed from the table and
//! information is asked only for file (vnode) descriptors, whose file operations cannot drain (`fo_no_drain`).

use crate::unix_proc::started_of;
use dyno_core::ports::{FileLocks, PlatformError};
use dyno_core::process::{Locker, ProcessRef};
use std::path::Path;
use std::time::{Duration, Instant};

/// The longest one question may take on a loaded machine.
const LIMIT: Duration = Duration::from_secs(15);

/// `libproc`-backed file locks.
pub struct LsofLocks;

impl FileLocks for LsofLocks {
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError> {
        let path = path
            .canonicalize()
            .map_err(|_| PlatformError::NotFound(format!("{} does not exist.", path.display())))?;
        let want = path.to_string_lossy().into_owned();
        let me = std::process::id();
        let end = Instant::now() + LIMIT;
        let mut out = Vec::new();
        for pid in super::libproc::pids().into_iter().filter(|p| *p != me) {
            if Instant::now() > end {
                return Err(PlatformError::Timeout(
                    "Looking for the programs that hold the file took too long.".into(),
                ));
            }
            let holds = super::libproc::fds(pid)
                .into_iter()
                .filter(|(_, t)| super::libproc::is_vnode(*t))
                .any(|(fd, _)| super::libproc::vnode_path(pid, fd).is_some_and(|p| p == want));
            if !holds {
                continue;
            }
            let Some(started) = started_of(pid) else { continue };
            out.push(Locker {
                process: ProcessRef {
                    pid,
                    started: Some(started),
                },
                name: super::libproc::name(pid).unwrap_or_default(),
                kind: Some("Application".into()),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Stdio};

    /// Kills and reaps its child when dropped, so no assertion or `unwrap` can leave the dummy running.
    struct Reaped(Child);

    impl Drop for Reaped {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A loaded machine can run into the 15 s limit: only that (a `Timeout`) is asked again, a few times; any other
    /// error fails at once, and when the retries run out the last timeout is the failure message.
    fn lockers_retrying(path: &Path) -> Vec<Locker> {
        let mut last = None;
        for _ in 0..4 {
            match LsofLocks.lockers(path) {
                Ok(found) => return found,
                Err(e @ PlatformError::Timeout(_)) => last = Some(e),
                Err(e) => panic!("lockers for {}: {e:?}", path.display()),
            }
        }
        panic!("lockers for {} timed out 4 times, last: {last:?}", path.display());
    }

    #[test]
    fn finds_a_dummy_that_holds_a_temp_file_and_nobody_for_a_free_one() {
        let dir = tempfile::tempdir().unwrap();
        let held = dir.path().join("held.txt");
        std::fs::write(&held, b"x").unwrap();
        // The shell opens the file on descriptor 3, says so, and becomes `tail`, which keeps the descriptor: when "ready"
        // arrives the file is already held (no polling for it), and the pid is the one the lookup must name.
        let mut dummy = Reaped(
            crate::process::command("sh")
                .args(["-c", "exec 3< \"$1\"; echo ready; exec tail -f /dev/null", "sh"])
                .arg(&held)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        BufReader::new(dummy.0.stdout.take().unwrap()).read_line(&mut ready).unwrap();
        assert_eq!(ready.trim(), "ready");
        let found = lockers_retrying(&held);
        // The name is whatever the process is called at that moment (the shell announces "ready" just before it execs `tail`).
        assert!(
            found.iter().any(|l| l.process.pid == dummy.0.id() && !l.name.is_empty()),
            "{found:?}"
        );
        let free = dir.path().join("free.txt");
        std::fs::write(&free, b"y").unwrap();
        assert!(lockers_retrying(&free).is_empty());
        assert!(matches!(
            LsofLocks.lockers(&dir.path().join("missing")),
            Err(PlatformError::NotFound(_))
        ));
    }
}
