//! "What is locking this file?" through `libproc` (macOS has no Restart Manager). Never `lsof -- <path>`: it asks the
//! kernel about every socket and pipe descriptor of every process (`dproc.c` `process_socket`), which can mark another
//! program's socket `SS_DRAINING` (Dyno ADR 0026). Here every process's descriptors are only listed from the table and
//! information is asked only for file (vnode) descriptors, whose file operations cannot drain (`fo_no_drain`).

use crate::unix_proc::started_of;
use dyno_core::ports::{FileLocks, PlatformError};
use dyno_core::process::{Locker, ProcessRef};
use std::path::Path;

/// Most descriptors looked at in one question (bounded by work, not by a clock: ADR 0031, ER-19). A machine has some
/// tens of thousands of open files; this is far above that.
const MAX_LOOKED_AT: usize = 4 << 20;

/// `libproc`-backed file locks.
pub struct LsofLocks;

impl FileLocks for LsofLocks {
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError> {
        use std::os::unix::fs::MetadataExt;
        let meta = path
            .metadata()
            .map_err(|_| PlatformError::NotFound(format!("{} does not exist.", path.display())))?;
        // The file itself, not its name: a hard link or a renamed path is the same file (device and inode).
        let want = (meta.dev() as u32, meta.ino());
        let me = std::process::id();
        let mut looked_at = 0usize;
        let mut out = Vec::new();
        for pid in super::libproc::pids().into_iter().filter(|p| *p != me) {
            let mut holds = false;
            for (fd, _) in super::libproc::fds(pid).into_iter().filter(|(_, t)| super::libproc::is_vnode(*t)) {
                looked_at += 1;
                if looked_at > MAX_LOOKED_AT {
                    return Err(PlatformError::Failed("There are too many open files to look through.".into()));
                }
                if super::libproc::vnode_info(pid, fd).is_some_and(|v| (v.dev, v.ino) == want) {
                    holds = true;
                    break;
                }
            }
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
        let found = LsofLocks.lockers(&held).unwrap();
        // The name is whatever the process is called at that moment (the shell announces "ready" just before it execs `tail`).
        assert!(
            found.iter().any(|l| l.process.pid == dummy.0.id() && !l.name.is_empty()),
            "{found:?}"
        );
        // The same file under another name (a hard link) has the same lockers: the match is by device and inode.
        let link = dir.path().join("link.txt");
        std::fs::hard_link(&held, &link).unwrap();
        let by_link = LsofLocks.lockers(&link).unwrap();
        assert!(by_link.iter().any(|l| l.process.pid == dummy.0.id()), "{by_link:?}");
        let free = dir.path().join("free.txt");
        std::fs::write(&free, b"y").unwrap();
        assert!(LsofLocks.lockers(&free).unwrap().is_empty());
        assert!(matches!(
            LsofLocks.lockers(&dir.path().join("missing")),
            Err(PlatformError::NotFound(_))
        ));
    }
}
