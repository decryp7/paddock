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
                    process: ProcessRef { pid, started },
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
    use std::process::Stdio;

    #[test]
    fn finds_a_dummy_that_holds_a_temp_file_and_nobody_for_a_free_one() {
        let dir = tempfile::tempdir().unwrap();
        let held = dir.path().join("held.txt");
        std::fs::write(&held, b"x").unwrap();
        // `tail -f` keeps the file open.
        let mut child = crate::process::command("tail")
            .arg("-f")
            .arg(&held)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut found = Vec::new();
        for _ in 0..50 {
            found = LsofLocks.lockers(&held).unwrap();
            if !found.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.iter().any(|l| l.process.pid == child.id() && l.name == "tail"), "{found:?}");
        let free = dir.path().join("free.txt");
        std::fs::write(&free, b"y").unwrap();
        assert!(LsofLocks.lockers(&free).unwrap().is_empty());
        assert!(matches!(
            LsofLocks.lockers(&dir.path().join("missing")),
            Err(PlatformError::NotFound(_))
        ));
    }
}
