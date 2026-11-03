//! The helper's reader of who holds which socket or file on Linux (ADR 0027 WP-10): `/proc/<pid>/fd` for every process (root
//! only, which is why the helper does it) and `/proc/locks`. It never opens the file asked about: it compares the device and
//! inode that `stat` reports for each descriptor link. The `/proc` root is a parameter so tests use a temporary tree.

use crate::parse::linux_proc::{dev_major_minor, locks, socket_inode};
use dyno_core::helper::HelperHost;
use dyno_core::helper_holders::{HolderScan, HolderSource, RawHolder, MAX_SWEEP};
use dyno_core::helper_processes::classify_owner;
use dyno_core::helper_wire::HelperUser;
use dyno_core::ports::PlatformError;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The most descriptors looked at in one scan.
const MAX_FDS: usize = 4_000_000;

/// Reads `/proc` (or a copy of its shape).
pub struct ProcHolders {
    proc: PathBuf,
    budget: Duration,
}

impl ProcHolders {
    /// The real `/proc`, giving up after `budget`.
    pub fn new(budget: Duration) -> ProcHolders {
        ProcHolders {
            proc: "/proc".into(),
            budget,
        }
    }

    /// A tree elsewhere (tests).
    pub fn at(proc: impl Into<PathBuf>, budget: Duration) -> ProcHolders {
        ProcHolders { proc: proc.into(), budget }
    }

    fn pids(&self) -> Result<Vec<u32>, PlatformError> {
        let rd = std::fs::read_dir(&self.proc).map_err(|e| PlatformError::Failed(format!("The process list could not be read: {e}")))?;
        let mut pids: Vec<u32> = rd.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect();
        pids.sort_unstable();
        Ok(pids)
    }

    fn describe(&self, pid: u32, users: &sysinfo::Users) -> Option<RawHolder> {
        let dir = self.proc.join(pid.to_string());
        let uid = uid_of(&dir)?;
        let mut comm = String::new();
        let _ = std::fs::File::open(dir.join("comm")).and_then(|f| f.take(256).read_to_string(&mut comm));
        let name = comm.trim();
        let account = users.iter().find(|u| **u.id() == uid).map(|u| u.name().to_string());
        Some(RawHolder {
            pid,
            started: started_secs(pid),
            name: if name.is_empty() {
                format!("Process {pid}")
            } else {
                name.to_string()
            },
            owner: classify_owner(HelperHost::Linux, &HelperUser::Uid(uid), account.as_deref()),
            sockets: Vec::new(),
        })
    }
}

fn uid_of(dir: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(dir).ok().map(|m| m.uid())
}

fn started_secs(pid: u32) -> Option<i64> {
    let mut sys = sysinfo::System::new();
    let sp = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[sp]),
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    sys.process(sp).and_then(|p| i64::try_from(p.start_time()).ok()).filter(|s| *s > 0)
}

impl HolderSource for ProcHolders {
    fn sockets(&self) -> Result<HolderScan, PlatformError> {
        let started = Instant::now();
        let users = sysinfo::Users::new_with_refreshed_list();
        let (mut fds, mut truncated) = (0usize, false);
        let mut holders = Vec::new();
        for (n, pid) in self.pids()?.into_iter().enumerate() {
            if n >= MAX_SWEEP || started.elapsed() > self.budget {
                truncated = true;
                break;
            }
            let Ok(rd) = std::fs::read_dir(self.proc.join(pid.to_string()).join("fd")) else {
                continue;
            };
            let mut inodes = Vec::new();
            for fd in rd.flatten() {
                fds += 1;
                if fds > MAX_FDS {
                    truncated = true;
                    break;
                }
                if let Some(i) = std::fs::read_link(fd.path()).ok().and_then(|l| socket_inode(&l.to_string_lossy())) {
                    inodes.push(i);
                }
            }
            inodes.sort_unstable();
            inodes.dedup();
            if !inodes.is_empty() {
                if let Some(mut h) = self.describe(pid, &users) {
                    h.sockets = inodes;
                    holders.push(h);
                }
            }
            if truncated {
                break;
            }
        }
        Ok(HolderScan { holders, truncated })
    }

    fn file(&self, dev: u64, ino: u64) -> Result<HolderScan, PlatformError> {
        use std::os::unix::fs::MetadataExt;
        let started = Instant::now();
        let users = sysinfo::Users::new_with_refreshed_list();
        let (mut fds, mut truncated) = (0usize, false);
        let mut found: BTreeMap<u32, ()> = BTreeMap::new();
        'procs: for (n, pid) in self.pids()?.into_iter().enumerate() {
            if n >= MAX_SWEEP || started.elapsed() > self.budget {
                truncated = true;
                break;
            }
            let Ok(rd) = std::fs::read_dir(self.proc.join(pid.to_string()).join("fd")) else {
                continue;
            };
            for fd in rd.flatten() {
                fds += 1;
                if fds > MAX_FDS {
                    truncated = true;
                    break 'procs;
                }
                // `stat` through the descriptor link: the file itself is never opened.
                if let Ok(m) = std::fs::metadata(fd.path()) {
                    if m.dev() == dev && m.ino() == ino {
                        found.insert(pid, ());
                        break;
                    }
                }
            }
        }
        let (major, minor) = dev_major_minor(dev);
        let text = std::fs::read_to_string(self.proc.join("locks")).unwrap_or_default();
        for l in locks(&text) {
            if l.inode == ino && l.major == major && l.minor == minor && l.pid != 0 {
                found.insert(l.pid, ());
            }
        }
        // A process that exited meanwhile is not a holder any more.
        let holders = found.keys().filter_map(|pid| self.describe(*pid, &users)).collect();
        Ok(HolderScan { holders, truncated })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, MetadataExt};

    /// A `/proc` shaped tree: process `pid` of this account with the given descriptor links.
    fn tree(root: &Path, pid: u32, links: &[(&str, &Path)]) {
        let p = root.join(pid.to_string());
        std::fs::create_dir_all(p.join("fd")).unwrap();
        std::fs::write(p.join("comm"), "tool\n").unwrap();
        for (fd, target) in links {
            symlink(target, p.join("fd").join(fd)).unwrap();
        }
    }

    #[test]
    fn socket_links_give_inodes_per_program_and_other_links_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.txt");
        std::fs::write(&plain, "x").unwrap();
        tree(
            dir.path(),
            4242,
            &[
                ("3", Path::new("socket:[111]")),
                ("4", Path::new("socket:[111]")),
                ("5", Path::new("socket:[222]")),
                ("6", &plain),
            ],
        );
        tree(dir.path(), 4343, &[("3", &plain)]);
        let scan = ProcHolders::at(dir.path(), Duration::from_secs(5)).sockets().unwrap();
        assert_eq!(scan.holders.len(), 1);
        assert_eq!(
            (scan.holders[0].pid, scan.holders[0].sockets.clone(), scan.holders[0].name.as_str()),
            (4242, vec![111, 222], "tool")
        );
        assert!(!scan.truncated);
    }

    #[test]
    fn a_file_is_found_by_device_and_inode_through_descriptors_and_locks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("data.db");
        let other = dir.path().join("other.db");
        std::fs::write(&target, "x").unwrap();
        std::fs::write(&other, "y").unwrap();
        let m = std::fs::metadata(&target).unwrap();
        tree(dir.path(), 5001, &[("7", &target)]);
        tree(dir.path(), 5002, &[("7", &other)]);
        tree(dir.path(), 5003, &[]);
        let (major, minor) = dev_major_minor(m.dev());
        std::fs::write(
            dir.path().join("locks"),
            format!("1: POSIX  ADVISORY  WRITE 5003 {major:02x}:{minor:02x}:{} 0 EOF\n", m.ino()),
        )
        .unwrap();
        let scan = ProcHolders::at(dir.path(), Duration::from_secs(5)).file(m.dev(), m.ino()).unwrap();
        assert_eq!(scan.holders.iter().map(|h| h.pid).collect::<Vec<_>>(), [5001, 5003]);
        let none = ProcHolders::at(dir.path(), Duration::from_secs(5))
            .file(m.dev(), m.ino() + 9_999)
            .unwrap();
        assert!(none.holders.is_empty());
    }

    #[test]
    fn an_unreadable_process_list_is_an_error_in_words() {
        let e = ProcHolders::at("/definitely/not/here", Duration::from_secs(1))
            .sockets()
            .unwrap_err();
        assert!(e.to_string().contains("could not be read"));
    }
}
