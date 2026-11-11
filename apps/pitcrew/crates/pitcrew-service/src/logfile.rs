//! The runner's log file. Pitcrew owns the runner's standard output and error (pipes it reads), so it writes the
//! file itself: every line also goes to the watchdog, Pitcrew adds its own `supervisor:` lines, and it rotates the
//! file between two lines without stopping the runner (the PowerShell set-up redirected through `cmd >>`, which kept
//! the file locked against other writers and could only rotate before a start).

use pitcrew_core::logs::{RotationPolicy, RotationStep};
use pitcrew_core::time::{format_rfc3339, Timestamp};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Whether the log's owner must be SYSTEM or Administrators: only the Windows service running as LocalSystem, the one writer that a
/// user's hard link or planted file could turn against the machine. An engine in a user's session writes the user's own files.
fn require_trusted_owner() -> bool {
    #[cfg(windows)]
    {
        crate::platform::win_session::is_local_system()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Opens a log for appending without ever following a link, and only if the file that was opened is acceptable (one name, a trusted
/// owner where that is asked: [`paddock_service::privileged::trust_real::open_append_no_follow`], checked on the handle). A file that
/// is not acceptable (a hard link a user holds, a planted file) is moved aside to a fresh `<name>.untrusted-<secs>-<n>` name and a new
/// file is made, so what the service writes never reaches the other name; if it cannot be moved, nothing is written (round 6 P1-B).
fn open_log(path: &Path) -> std::io::Result<File> {
    let owner = require_trusted_owner();
    match paddock_service::privileged::trust_real::open_append_no_follow(path, owner) {
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            let secs = crate::cleanup::now().0;
            for n in 0..100 {
                let mut aside = path.as_os_str().to_os_string();
                aside.push(format!(".untrusted-{secs}-{n}"));
                let aside = PathBuf::from(aside);
                if std::fs::symlink_metadata(&aside).is_ok() {
                    continue;
                }
                std::fs::rename(path, &aside)
                    .map_err(|e2| std::io::Error::new(e2.kind(), format!("{e}; it could not be moved aside ({e2})")))?;
                return paddock_service::privileged::trust_real::open_append_no_follow(path, owner);
            }
            Err(e)
        }
        other => other,
    }
}

/// Appends lines to `<dir>/<name>` and rotates it to `<name>.1` … `<name>.<keep>`.
#[derive(Debug)]
pub struct LogWriter {
    path: PathBuf,
    policy: RotationPolicy,
    file: Option<File>,
    size: u64,
    failed: bool,
    /// Writes nothing anywhere (see [`LogWriter::discarding`]).
    discard: bool,
}

impl LogWriter {
    /// A writer for `dir/name`; the folder is created when needed. Failing to open the file is not fatal: lines
    /// are then only kept in memory (and the failure is reported by [`LogWriter::failed`]).
    pub fn open(dir: &Path, name: &str, policy: RotationPolicy) -> Self {
        let path = dir.join(name);
        let mut w = LogWriter {
            path,
            policy,
            file: None,
            size: 0,
            failed: false,
            discard: false,
        };
        w.reopen();
        w
    }

    /// A writer that keeps nothing and touches no file or folder: the log of a runner whose folder the service may not touch
    /// (ADR 0004, second amendment). Its lines go to the service's own log instead.
    pub fn discarding() -> Self {
        LogWriter {
            path: PathBuf::new(),
            policy: RotationPolicy::default(),
            file: None,
            size: 0,
            failed: false,
            discard: true,
        }
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the file could not be written.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Changes the rotation policy.
    pub fn set_policy(&mut self, policy: RotationPolicy) {
        self.policy = policy;
    }

    fn reopen(&mut self) {
        if self.discard {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match open_log(&self.path) {
            Ok(f) => {
                self.size = f.metadata().map(|m| m.len()).unwrap_or(0);
                self.file = Some(f);
                self.failed = false;
            }
            Err(_) => {
                self.file = None;
                self.failed = true;
            }
        }
    }

    /// Appends one line (a newline is added).
    pub fn write_line(&mut self, line: &str) {
        if self.discard {
            return;
        }
        // Rotate first, so the newest line is always in the live file.
        if !self.policy.plan(self.size).is_empty() {
            self.rotate();
        }
        if self.file.is_none() {
            self.reopen();
        }
        if let Some(f) = &mut self.file {
            if f.write_all(line.as_bytes()).and_then(|_| f.write_all(b"\n")).is_ok() {
                self.size += line.len() as u64 + 1;
                self.failed = false;
            } else {
                self.failed = true;
                self.file = None;
            }
        }
    }

    /// Appends one of Pitcrew's own lines: `<time> supervisor: <text>`.
    pub fn supervisor(&mut self, now: Timestamp, text: &str) {
        self.write_line(&format!("{} supervisor: {text}", format_rfc3339(now)));
    }

    /// Rotates now, following the core's plan.
    pub fn rotate(&mut self) {
        if self.discard {
            return;
        }
        let steps = self.policy.plan(self.size.max(self.policy.max_bytes + 1));
        self.file = None;
        let name = |n: u32| -> PathBuf {
            if n == 0 {
                self.path.clone()
            } else {
                let mut s = self.path.as_os_str().to_os_string();
                s.push(format!(".{n}"));
                PathBuf::from(s)
            }
        };
        for step in steps {
            match step {
                RotationStep::Delete(n) => {
                    let _ = std::fs::remove_file(name(n));
                }
                RotationStep::Rename { from, to } => {
                    let src = name(from);
                    if src.exists() {
                        let _ = std::fs::rename(&src, name(to));
                    }
                }
            }
        }
        self.reopen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pitcrew-it-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn writes_and_rotates_without_losing_lines() {
        let d = temp("rotate");
        let mut w = LogWriter::open(&d, "runner.log", RotationPolicy { max_bytes: 100, keep: 2 });
        assert!(!w.failed());
        for i in 0..30 {
            w.write_line(&format!("line {i:02} ......"));
        }
        w.supervisor(Timestamp(0), "runner exited with code 1; restarting in 10 s");
        let live = std::fs::read_to_string(w.path()).unwrap();
        assert!(live.contains("1970-01-01T00:00:00.000Z supervisor: runner exited"));
        assert!(d.join("runner.log.1").exists());
        assert!(d.join("runner.log.2").exists());
        assert!(!d.join("runner.log.3").exists(), "only `keep` files are kept");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unwritable_folder_is_reported_not_fatal() {
        let d = temp("blocked");
        std::fs::create_dir_all(d.parent().unwrap()).unwrap();
        // A file where the folder should be.
        std::fs::write(&d, b"x").unwrap();
        let mut w = LogWriter::open(&d, "runner.log", RotationPolicy::default());
        assert!(w.failed());
        w.write_line("still fine");
        w.set_policy(RotationPolicy { max_bytes: 1, keep: 0 });
        w.rotate();
        assert!(w.failed());
        let _ = std::fs::remove_file(&d);
    }

    #[test]
    fn a_discarding_writer_touches_no_file_and_is_not_a_failure() {
        let mut w = LogWriter::discarding();
        w.write_line("kept nowhere");
        w.supervisor(Timestamp(0), "kept nowhere either");
        w.set_policy(RotationPolicy { max_bytes: 1, keep: 0 });
        w.rotate();
        assert!(!w.failed());
        assert_eq!(w.path(), Path::new(""));
        assert!(!Path::new("kept nowhere").exists());
    }

    /// Round 6 P1-B: a hard link a user holds to the log made the privileged writer append to the user's file's name too (and to the file
    /// it pointed at). Now the file is moved aside and a new one is started; the other name keeps only what it had.
    #[cfg(unix)]
    #[test]
    fn a_log_with_a_second_name_is_moved_aside_and_the_other_name_is_never_written() {
        let d = temp("hardlink");
        std::fs::create_dir_all(&d).unwrap();
        let log = d.join("service.log");
        std::fs::write(&log, "old line\n").unwrap();
        let held = d.join("held.txt");
        std::fs::hard_link(&log, &held).unwrap();
        let mut w = LogWriter::open(&d, "service.log", RotationPolicy::default());
        w.write_line("new line");
        assert!(!w.failed());
        assert_eq!(
            std::fs::read_to_string(&held).unwrap(),
            "old line\n",
            "the other name was written to"
        );
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "new line\n");
        let asides = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("service.log.untrusted-"))
            .count();
        assert_eq!(asides, 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A link where the log should be is never followed: nothing is written to its target.
    #[cfg(unix)]
    #[test]
    fn a_link_where_the_log_should_be_is_not_written_through() {
        let d = temp("symlink");
        std::fs::create_dir_all(&d).unwrap();
        let target = d.join("target.txt");
        std::fs::write(&target, "mine").unwrap();
        std::os::unix::fs::symlink(&target, d.join("service.log")).unwrap();
        let mut w = LogWriter::open(&d, "service.log", RotationPolicy::default());
        w.write_line("a line");
        assert!(w.failed(), "a link is not a log");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "mine");
        let _ = std::fs::remove_dir_all(&d);
    }
}
