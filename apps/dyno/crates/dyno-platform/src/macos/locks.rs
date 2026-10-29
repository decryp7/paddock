//! "What is locking this file?" through `lsof` (macOS has no Restart Manager).

use crate::parse::lsof::{parse_fields, processes};
use crate::unix_proc::started_of;
use dyno_core::ports::{FileLocks, PlatformError};
use dyno_core::process::{Locker, ProcessRef};
use std::path::Path;
use std::time::Duration;

/// `lsof`-backed file locks.
pub struct LsofLocks;

impl FileLocks for LsofLocks {
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError> {
        let path = path
            .canonicalize()
            .map_err(|_| PlatformError::NotFound(format!("{} does not exist.", path.display())))?;
        let mut cmd = crate::process::command("/usr/sbin/lsof");
        cmd.args(["-F", "pcfn", "--"]).arg(&path);
        let out = crate::process::run_with_timeout(cmd, Duration::from_secs(15))?;
        // lsof exits 1 when nothing has the file open.
        let text = String::from_utf8_lossy(&out.stdout);
        let me = std::process::id();
        Ok(processes(&parse_fields(&text))
            .into_iter()
            .filter(|(pid, _)| *pid != me)
            .filter_map(|(pid, name)| {
                let started = started_of(pid)?;
                Some(Locker {
                    process: ProcessRef {
                        pid,
                        started: Some(started),
                    },
                    name,
                    kind: Some("Application".into()),
                })
            })
            .collect())
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

    #[test]
    fn finds_a_dummy_that_holds_a_temp_file_and_nobody_for_a_free_one() {
        let dir = tempfile::tempdir().unwrap();
        let held = dir.path().join("held.txt");
        std::fs::write(&held, b"x").unwrap();
        // The shell opens the file on descriptor 3, says so, and becomes `tail`, which keeps the descriptor: when "ready"
        // arrives the file is already held (no polling for it), and the pid is the one `lsof` must name.
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
        // `lsof` can time out on a loaded machine (its own limit is 15 s): ask again, a few times, instead of failing on the first.
        let found = (0..4).find_map(|_| LsofLocks.lockers(&held).ok()).expect("lsof answered");
        assert!(found.iter().any(|l| l.process.pid == dummy.0.id() && l.name == "tail"), "{found:?}");
        let free = dir.path().join("free.txt");
        std::fs::write(&free, b"y").unwrap();
        assert!(LsofLocks.lockers(&free).unwrap().is_empty());
        assert!(matches!(
            LsofLocks.lockers(&dir.path().join("missing")),
            Err(PlatformError::NotFound(_))
        ));
    }
}
