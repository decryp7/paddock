//! "What is locking this file?" on Linux: the processes that hold the file open (descriptors under `/proc/*/fd`) or
//! hold an `flock`/`fcntl` lock on it (`/proc/locks`). The kernel shows descriptors only for the user's own
//! processes (all of them to root), so the answer says so when others were out of sight. No `lsof` needed.

use super::paths::{read_text, HostPaths};
use super::sys::effective_uid;
use crate::parse::linux_proc::{dev_major_minor, locks};
use crate::unix_proc::started_of;
use dyno_core::ports::{FileLocks, PlatformError};
use dyno_core::process::{Locker, ProcessRef};
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// Most descriptors examined in one question.
const MAX_FDS: usize = 2_000_000;
/// Most lockers reported.
const MAX_LOCKERS: usize = 100;

/// File locks for Linux.
pub struct LinuxLocks {
    paths: HostPaths,
    budget: Duration,
}

impl LinuxLocks {
    /// Locks read from `paths`, giving up after `budget`.
    pub fn new(paths: HostPaths, budget: Duration) -> Self {
        LinuxLocks { paths, budget }
    }
}

#[derive(Default)]
struct Found {
    open: bool,
    locks: Vec<String>,
}

impl FileLocks for LinuxLocks {
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| PlatformError::NotFound(format!("{} no longer exists.", path.display())))?;
        let meta =
            std::fs::metadata(&canonical).map_err(|e| PlatformError::Failed(format!("Could not read {}: {e}", canonical.display())))?;
        let wanted = canonical.to_string_lossy().into_owned();
        let started = Instant::now();
        let mut found: BTreeMap<u32, Found> = BTreeMap::new();
        let (mut hidden, mut fds, mut timed_out) = (0usize, 0usize, false);

        'procs: for entry in std::fs::read_dir(&self.paths.proc)
            .map_err(|e| PlatformError::Failed(format!("Could not read the process list: {e}")))?
            .flatten()
        {
            let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let dir = match std::fs::read_dir(entry.path().join("fd")) {
                Ok(d) => d,
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    hidden += 1;
                    continue;
                }
                Err(_) => continue,
            };
            for fd in dir.flatten() {
                fds += 1;
                if fds > MAX_FDS || (fds % 1024 == 0 && started.elapsed() > self.budget) {
                    timed_out = true;
                    break 'procs;
                }
                if let Ok(target) = std::fs::read_link(fd.path()) {
                    if target.as_os_str() == wanted.as_str() {
                        found.entry(pid).or_default().open = true;
                        break;
                    }
                }
            }
        }

        let (major, minor) = dev_major_minor(meta.dev());
        for l in read_text(&self.paths.proc.join("locks")).map(|t| locks(&t)).unwrap_or_default() {
            if l.inode == meta.ino() && l.major == major && l.minor == minor && l.pid != 0 {
                found.entry(l.pid).or_default().locks.push(l.kind);
            }
        }

        let mut out = Vec::new();
        for (pid, f) in found.into_iter().take(MAX_LOCKERS) {
            // A process that exited meanwhile has no start time and is not a locker any more.
            let Some(started) = started_of(pid) else { continue };
            let name = read_text(&self.paths.pid(pid).join("comm"))
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty());
            out.push(Locker {
                process: ProcessRef { pid, started },
                name: name.unwrap_or_else(|| format!("Process {pid}")),
                kind: Some(kind(&f)),
            });
        }
        if out.is_empty() && timed_out {
            return Err(PlatformError::Timeout(format!(
                "Looking through the open files took longer than {} s; try again when the computer is less busy.",
                self.budget.as_secs()
            )));
        }
        if out.is_empty() && hidden > 0 && effective_uid() != 0 {
            return Err(PlatformError::AccessDenied(format!(
                "No process of yours holds this file, but {hidden} processes of other users are out of sight. Run Dyno as root to see them all."
            )));
        }
        Ok(out)
    }
}

fn kind(f: &Found) -> String {
    let mut parts = Vec::new();
    if f.open {
        parts.push("Has it open".to_string());
    }
    if !f.locks.is_empty() {
        let mut kinds = f.locks.clone();
        kinds.sort();
        kinds.dedup();
        parts.push(format!("Holds a {} lock", kinds.join("/").to_lowercase()));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn real() -> LinuxLocks {
        LinuxLocks::new(HostPaths::host(), Duration::from_secs(20))
    }

    #[test]
    fn finds_this_process_holding_a_file_open() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("held.log");
        let mut file = std::fs::File::create(&f).unwrap();
        writeln!(file, "x").unwrap();
        let lockers = real().lockers(&f).unwrap();
        let me = lockers
            .iter()
            .find(|l| l.process.pid == std::process::id())
            .expect("this process holds it");
        assert!(me.kind.as_deref().unwrap().contains("Has it open"));
        assert!(!me.name.is_empty());
        drop(file);
        let after = real().lockers(&f);
        assert!(after.map(|l| l.iter().all(|l| l.process.pid != std::process::id())).unwrap_or(true));
    }

    #[test]
    fn reports_flock_holders() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("locked.db");
        let file = std::fs::File::create(&f).unwrap();
        // An advisory flock, held until the file is closed.
        file.lock().unwrap();
        let lockers = real().lockers(&f).unwrap();
        let me = lockers.iter().find(|l| l.process.pid == std::process::id()).unwrap();
        let kind = me.kind.as_deref().unwrap();
        assert!(kind.contains("Has it open") && kind.to_lowercase().contains("lock"), "{kind}");
    }

    #[test]
    fn missing_files_are_not_found() {
        assert!(matches!(
            real().lockers(Path::new("/dyno/no/such/file")),
            Err(PlatformError::NotFound(_))
        ));
    }

    #[test]
    fn other_users_processes_are_explained_when_nothing_else_is_found() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x");
        std::fs::write(&f, "").unwrap();
        let proc = dir.path().join("proc");
        std::fs::create_dir_all(proc.join("500/fd")).unwrap();
        std::fs::set_permissions(proc.join("500/fd"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let mut l = LinuxLocks::new(HostPaths::under(dir.path()), Duration::from_secs(5));
        let r = l.lockers(&f);
        std::fs::set_permissions(proc.join("500/fd"), std::fs::Permissions::from_mode(0o755)).unwrap();
        if effective_uid() == 0 {
            assert_eq!(r, Ok(vec![]));
        } else {
            assert!(
                matches!(&r, Err(PlatformError::AccessDenied(m)) if m.contains("1 processes of other users")),
                "{r:?}"
            );
        }
    }

    #[test]
    fn a_fake_tree_with_a_lock_record_and_a_time_budget() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.db");
        std::fs::write(&f, "").unwrap();
        let meta = std::fs::metadata(&f).unwrap();
        let (major, minor) = dev_major_minor(meta.dev());
        let me = std::process::id();
        let proc = dir.path().join("proc");
        std::fs::create_dir_all(proc.join(me.to_string())).unwrap();
        std::fs::write(proc.join(me.to_string()).join("comm"), "myapp\n").unwrap();
        std::fs::write(
            proc.join("locks"),
            format!(
                "1: POSIX  ADVISORY  WRITE {me} {major:02x}:{minor:02x}:{} 0 EOF\n2: FLOCK ADVISORY WRITE {me} 00:01:1 0 EOF\n",
                meta.ino()
            ),
        )
        .unwrap();
        let got = LinuxLocks::new(HostPaths::under(dir.path()), Duration::from_secs(5))
            .lockers(&f)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            (got[0].name.as_str(), got[0].kind.as_deref()),
            ("myapp", Some("Holds a posix lock"))
        );
        // A budget of zero with many descriptors times out rather than hanging.
        let many = proc.join("7/fd");
        std::fs::create_dir_all(&many).unwrap();
        for i in 0..2100 {
            std::os::unix::fs::symlink("/nowhere", many.join(i.to_string())).unwrap();
        }
        std::fs::remove_file(proc.join("locks")).unwrap();
        let r = LinuxLocks::new(HostPaths::under(dir.path()), Duration::ZERO).lockers(&f);
        assert!(matches!(r, Err(PlatformError::Timeout(_))), "{r:?}");
    }
}
