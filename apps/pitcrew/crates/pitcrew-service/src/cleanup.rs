//! Disk housekeeping on disk: scanning a runner's folder into `pitcrew_core::housekeeping` entries and carrying a
//! cleanup plan out. The decisions (what, when) are the core's; this module only lists, measures and deletes.
//!
//! Deleting is defensive: on Windows paths are made verbatim (`\\?\`) so deep `node_modules`/`tool_cache` trees
//! beyond 260 characters can go, read-only attributes are cleared and the delete retried, and whatever still fails
//! (a file in use) is reported with a reason and tried again at the next cleanup — never an error for the whole
//! cleanup.

use pitcrew_core::config::RunnerConfig;
use pitcrew_core::housekeeping::{
    classify_binary_file, failure_reason, rotated_log_number, CleanupFailure, CleanupPlan, CleanupReport, DiskEntry, DiskScan, EntryKind,
    MAX_ENTRIES,
};
use pitcrew_core::layout::{self, LOG_FILE};
use pitcrew_core::time::Timestamp;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Most files one scan measures (beyond, sizes are lower bounds).
pub const MAX_FILES: usize = 500_000;
/// Longest a scan may take.
pub const SCAN_TIME: Duration = Duration::from_secs(30);

struct Budget {
    files: usize,
    until: Instant,
    truncated: bool,
}

impl Budget {
    fn spend(&mut self) -> bool {
        if self.files >= MAX_FILES || Instant::now() >= self.until {
            self.truncated = true;
            return false;
        }
        self.files += 1;
        true
    }
}

fn modified(meta: &std::fs::Metadata) -> Timestamp {
    let t = meta.modified().unwrap_or(UNIX_EPOCH);
    Timestamp(u64::try_from(t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(0))
}

/// Size and newest modification of a folder tree (symbolic links are not followed).
fn measure(path: &Path, budget: &mut Budget) -> (u64, Timestamp) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, Timestamp(0));
    };
    if !meta.is_dir() {
        return (meta.len(), modified(&meta));
    }
    let mut total = 0u64;
    let mut newest = modified(&meta);
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            if !budget.spend() {
                return (total, newest);
            }
            let Ok(m) = e.metadata() else { continue };
            newest = newest.max(modified(&m));
            if m.is_dir() && !m.file_type().is_symlink() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    (total, newest)
}

fn rel(dir: &Path, p: &Path) -> String {
    p.strip_prefix(dir).unwrap_or(p).display().to_string().replace('\\', "/")
}

/// Scans a runner's folder: work folders, cache entries, leftovers next to the binary and rotated logs.
pub fn scan(runner: &RunnerConfig, binary: &Path, free_bytes: Option<u64>) -> DiskScan {
    let mut budget = Budget {
        files: 0,
        until: Instant::now() + SCAN_TIME,
        truncated: false,
    };
    let dir = &runner.dir;
    let mut entries = vec![];
    let (work, cache) = layout::work_dirs(runner);
    for (folder, kind) in [(work, EntryKind::Work), (cache, EntryKind::Cache)] {
        if let Ok(list) = std::fs::read_dir(&folder) {
            for e in list.flatten() {
                if entries.len() >= MAX_ENTRIES {
                    budget.truncated = true;
                    break;
                }
                let (bytes, modified) = measure(&e.path(), &mut budget);
                entries.push(DiskEntry {
                    path: rel(dir, &e.path()),
                    kind,
                    bytes,
                    modified,
                });
            }
        }
    }
    let binary_name = binary.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut bin_dirs = vec![dir.clone()];
    if let Some(parent) = binary.parent() {
        if parent != dir.as_path() {
            bin_dirs.push(parent.to_path_buf());
        }
    }
    for folder in bin_dirs {
        if let Ok(list) = std::fs::read_dir(&folder) {
            for e in list.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(kind) = classify_binary_file(&name, &binary_name) {
                    let (bytes, modified) = measure(&e.path(), &mut budget);
                    entries.push(DiskEntry {
                        path: rel(dir, &e.path()),
                        kind,
                        bytes,
                        modified,
                    });
                }
            }
        }
    }
    if let Ok(list) = std::fs::read_dir(layout::log_dir(runner)) {
        for e in list.flatten() {
            if let Some(n) = rotated_log_number(&e.file_name().to_string_lossy(), LOG_FILE) {
                let (bytes, modified) = measure(&e.path(), &mut budget);
                entries.push(DiskEntry {
                    path: rel(dir, &e.path()),
                    kind: EntryKind::RotatedLog { n },
                    bytes,
                    modified,
                });
            }
        }
    }
    let (folder_bytes, _) = measure(dir, &mut budget);
    entries.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    DiskScan {
        entries,
        folder_bytes,
        free_bytes,
        truncated: budget.truncated,
    }
}

/// The path to hand to the OS: verbatim on Windows, so long paths work.
fn os_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(c) = std::fs::canonicalize(p) {
            return c; // canonicalize returns a `\\?\` path on Windows
        }
    }
    p.to_path_buf()
}

fn clear_read_only(p: &Path) {
    let walk = |path: &Path| {
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(path, perms);
            }
        }
    };
    walk(p);
    let mut stack = vec![p.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for e in list.flatten() {
            let path = e.path();
            walk(&path);
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path);
            }
        }
    }
}

fn remove(p: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(p)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// Deletes one entry: once, then again after clearing read-only attributes.
pub fn delete(p: &Path) -> Result<(), String> {
    let path = os_path(p);
    match remove(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            clear_read_only(&path);
            remove(&path)
                .or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
                .map_err(|e| failure_reason(e.raw_os_error(), cfg!(windows), &e.to_string()))
        }
    }
}

/// Carries out `plan` in `runner`'s folder. Only paths inside the folder are touched.
pub fn execute(runner: &RunnerConfig, plan: &CleanupPlan, now: Timestamp) -> CleanupReport {
    let mut report = CleanupReport {
        trigger: plan.trigger,
        at: now,
        freed_bytes: 0,
        deleted: 0,
        failures: vec![],
    };
    for d in &plan.deletions {
        let rel_path = &d.entry.path;
        if rel_path.split('/').any(|part| part == ".." || part.is_empty()) || Path::new(rel_path).is_absolute() {
            report.failures.push(CleanupFailure {
                path: rel_path.clone(),
                reason: "the path is outside the runner's folder".into(),
            });
            continue;
        }
        match delete(&runner.dir.join(rel_path)) {
            Ok(()) => {
                report.freed_bytes += d.entry.bytes;
                report.deleted += 1;
            }
            Err(reason) => report.failures.push(CleanupFailure {
                path: rel_path.clone(),
                reason,
            }),
        }
    }
    report
}

/// Now, as a timestamp.
pub fn now() -> Timestamp {
    Timestamp(u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::config::SessionMode;
    use pitcrew_core::distribution::RunnerKind;
    use pitcrew_core::housekeeping::{plan, CleanupPolicy, CleanupTrigger, DeleteReason, Deletion};
    use pitcrew_core::names::RunnerName;
    use pitcrew_core::session::LoggedOffMode;

    pub(crate) fn runner_in(dir: &Path) -> RunnerConfig {
        RunnerConfig {
            name: RunnerName::parse("t").unwrap(),
            kind: RunnerKind::GiteaRunner,
            dir: dir.to_path_buf(),
            config_file: None,
            labels: vec![],
            session: SessionMode::Interactive,
            logged_off: LoggedOffMode::Wait,
            server_url: None,
            enabled: true,
            binary_path: None,
            update: None,
            watchdog: None,
            cleanup: None,
            processes: None,
            env: Default::default(),
        }
    }

    #[test]
    fn scans_and_cleans_a_runner_folder_including_read_only_and_deep_files() {
        let d = std::env::temp_dir().join(format!("pitcrew-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let deep = d
            .join("work/abc")
            .join("x".repeat(60))
            .join("y".repeat(60))
            .join("z".repeat(60))
            .join("w".repeat(60));
        std::fs::create_dir_all(&deep).unwrap();
        let ro = deep.join("locked.msi");
        std::fs::write(&ro, vec![1u8; 2048]).unwrap();
        let mut perms = std::fs::metadata(&ro).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&ro, perms).unwrap();
        std::fs::create_dir_all(d.join("cache/entry")).unwrap();
        std::fs::write(d.join("cache/entry/blob"), vec![2u8; 100]).unwrap();
        std::fs::create_dir_all(d.join("bin")).unwrap();
        std::fs::write(d.join("bin/gitea-runner.exe"), b"bin").unwrap();
        std::fs::write(d.join("bin/gitea-runner.exe.download"), b"partial").unwrap();
        std::fs::write(d.join("bin/gitea-runner.exe.previous"), b"prev").unwrap();
        std::fs::write(d.join("act_runner.exe"), b"old").unwrap();
        std::fs::create_dir_all(d.join("logs")).unwrap();
        std::fs::write(d.join("logs/runner.log"), b"live").unwrap();
        std::fs::write(d.join("logs/runner.log.9"), b"old log").unwrap();

        let r = runner_in(&d);
        let s = scan(&r, &d.join("bin/gitea-runner.exe"), Some(u64::MAX));
        let kinds: Vec<_> = s.entries.iter().map(|e| (e.path.as_str(), e.kind)).collect();
        assert!(kinds.contains(&("work/abc", EntryKind::Work)), "{kinds:?}");
        assert!(kinds.contains(&("cache/entry", EntryKind::Cache)));
        assert!(kinds.contains(&("bin/gitea-runner.exe.download", EntryKind::Download)));
        assert!(kinds.contains(&("bin/gitea-runner.exe.previous", EntryKind::Previous)));
        assert!(kinds.contains(&("act_runner.exe", EntryKind::OldBinary)));
        assert!(kinds.contains(&("logs/runner.log.9", EntryKind::RotatedLog { n: 9 })));
        assert_eq!(s.entries[0].path, "work/abc", "the biggest first");
        assert!(s.folder_bytes >= 2048 + 100);
        assert!(!s.truncated);

        // An hour from now: the folders just written are past the grace period.
        let p = plan(
            &CleanupPolicy::default(),
            &s,
            CleanupTrigger::Manual,
            5,
            Timestamp(now().0 + 3_600_000),
        );
        let report = execute(&r, &p, now());
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert!(!d.join("work/abc").exists(), "deep, read-only files are deleted");
        assert!(d.join("bin/gitea-runner.exe.previous").exists(), "the roll-back binary is kept");
        assert!(d.join("bin/gitea-runner.exe").exists());
        assert!(d.join("logs/runner.log").exists());
        assert_eq!(report.deleted as usize, p.deletions.len());

        // Paths outside the folder are refused; missing ones count as deleted.
        let evil = CleanupPlan {
            trigger: CleanupTrigger::Manual,
            deletions: vec![
                Deletion {
                    entry: DiskEntry {
                        path: "../outside".into(),
                        kind: EntryKind::Work,
                        bytes: 1,
                        modified: Timestamp(0),
                    },
                    reason: DeleteReason::Pressure,
                },
                Deletion {
                    entry: DiskEntry {
                        path: "work/gone".into(),
                        kind: EntryKind::Work,
                        bytes: 1,
                        modified: Timestamp(0),
                    },
                    reason: DeleteReason::Pressure,
                },
            ],
            freed_bytes: 2,
            still_needed_bytes: 0,
        };
        let report = execute(&r, &evil, now());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.deleted, 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_in_use_is_reported_with_a_reason() {
        let d = std::env::temp_dir().join(format!("pitcrew-inuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("work/job")).unwrap();
        let f = d.join("work/job/open.bin");
        std::fs::write(&f, b"x").unwrap();
        // Windows refuses to delete a file opened without FILE_SHARE_DELETE.
        use std::os::windows::fs::OpenOptionsExt;
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&f).unwrap();
        let err = delete(&d.join("work/job")).unwrap_err();
        assert!(err.contains("tries again"), "{err}");
        drop(held);
        assert!(delete(&d.join("work/job")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }
}
