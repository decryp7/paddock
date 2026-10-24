//! Path grants: the proof that a path is one the user chose. A grant is recorded only on the Rust side, by the native file
//! dialogs, drag and drop onto a window, the command line or a second launch, a project the window opened, or a file the app
//! itself produced. A command asks [`PathGrants::check`] (through [`crate::tauri::WindowGrants`], resolved from the calling
//! window and never from the webview) for a [`GrantedPath`] to read or write, and gets the canonical path or a message that
//! tells the user to choose the file again.
//!
//! Grants are per window (a dialog or pop-out shares its host's), bounded ([`MAX_GRANTS`] per window, the oldest go first)
//! and expire after [`GRANT_TTL`] (a window's project file does not). Paths are compared canonically: `..`, `.` and links
//! resolve before the check, so no spelling reaches another file. A read grant on a folder covers what is inside it.

use crate::path::{Result, canonical, file_name, has_extension, output_target, parse};
use paddock_commands::CommandError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Grants kept per window; the oldest are dropped first.
pub const MAX_GRANTS: usize = 64;
/// How long a chosen path stays usable.
pub const GRANT_TTL: Duration = Duration::from_secs(60 * 60);

/// What a command does with a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read an existing file, or the files inside a folder.
    Read,
    /// Write a file (it may not exist yet).
    Write,
}

/// Where a grant came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantSource {
    /// A native open or save dialog.
    Dialog,
    /// Files dropped on the window.
    Drop,
    /// The command line, a second launch or the OS ("Open with").
    CommandLine,
    /// The window's project file (opened or saved): kept while the window lives, one per window.
    Project,
    /// An entry of a recent list ([`crate::Recents`]): recorded only after a granted open or save.
    Recent,
    /// A file or folder the app itself produced or chose (an unpacked bundle, a tool's output, the autosaved session).
    Output,
}

/// A path the user chose, checked: canonical (for a write, its folder is) and granted for `access`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantedPath {
    path: PathBuf,
    access: Access,
}

impl GrantedPath {
    /// The canonical path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What it was granted for.
    pub fn access(&self) -> Access {
        self.access
    }

    /// The path as text (what the webview gets back), lossy for non-UTF-8 names.
    pub fn text(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

#[derive(Debug, Clone)]
struct Grant {
    /// Canonical; for a write, the canonical folder joined with the file name.
    path: PathBuf,
    access: Access,
    /// A folder opened for reading: what is inside it is covered too.
    tree: bool,
    expires: Option<Instant>,
}

impl Grant {
    fn covers(&self, path: &Path, access: Access, now: Instant) -> bool {
        if self.expires.is_some_and(|e| now >= e) || self.access != access {
            return false;
        }
        self.path == path || (self.tree && path.starts_with(&self.path))
    }
}

/// The grants of every window (cheap to clone; clones share the same grants).
#[derive(Clone, Default)]
pub struct PathGrants {
    windows: Arc<Mutex<HashMap<String, Vec<Grant>>>>,
}

impl PathGrants {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Vec<Grant>>> {
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Records that the user chose `path` in `window`. A read grant needs an existing file or folder; a write grant an
    /// existing folder. Fails (with a reason) for anything else.
    pub fn grant(
        &self,
        window: &str,
        path: &Path,
        access: Access,
        source: GrantSource,
    ) -> Result<GrantedPath> {
        self.grant_at(window, path, access, source, Instant::now())
    }

    pub(crate) fn grant_at(
        &self,
        window: &str,
        path: &Path,
        access: Access,
        source: GrantSource,
        now: Instant,
    ) -> Result<GrantedPath> {
        let (canonical, tree) = match access {
            Access::Read => {
                let c = canonical(path)?;
                let tree = c.is_dir();
                (c, tree)
            }
            Access::Write => (output_target(path)?, false),
        };
        let expires = (source != GrantSource::Project).then(|| now + GRANT_TTL);
        let mut all = self.lock();
        let list = all.entry(window.to_string()).or_default();
        list.retain(|g| {
            g.expires.is_none_or(|e| now < e) && !(g.path == canonical && g.access == access)
        });
        if source == GrantSource::Project {
            // One project file per window.
            list.retain(|g| g.expires.is_some());
        }
        list.push(Grant {
            path: canonical.clone(),
            access,
            tree,
            expires,
        });
        while list.len() > MAX_GRANTS {
            let oldest = list.iter().position(|g| g.expires.is_some()).unwrap_or(0);
            list.remove(oldest);
        }
        Ok(GrantedPath {
            path: canonical,
            access,
        })
    }

    /// Grants every path that resolves, for reading (dropped files, the command line); returns the granted paths.
    pub fn grant_all<S: AsRef<Path>>(
        &self,
        window: &str,
        paths: &[S],
        source: GrantSource,
    ) -> Vec<GrantedPath> {
        paths
            .iter()
            .filter_map(|p| self.grant(window, p.as_ref(), Access::Read, source).ok())
            .collect()
    }

    /// `raw` (from the webview) as a granted path of `window`, or why not. `extensions` (lower case, without the dot; empty:
    /// any) are what the command reads or writes.
    pub fn check(
        &self,
        window: &str,
        raw: &str,
        access: Access,
        extensions: &[&str],
    ) -> Result<GrantedPath> {
        self.check_at(window, raw, access, extensions, Instant::now())
    }

    pub(crate) fn check_at(
        &self,
        window: &str,
        raw: &str,
        access: Access,
        extensions: &[&str],
        now: Instant,
    ) -> Result<GrantedPath> {
        let path = parse(raw)?;
        let resolved = match access {
            Access::Read => canonical(path)?,
            Access::Write => output_target(path)?,
        };
        has_extension(&resolved, extensions)?;
        let granted = self
            .lock()
            .get(window)
            .is_some_and(|list| list.iter().any(|g| g.covers(&resolved, access, now)));
        if !granted {
            return Err(CommandError::not_granted(match access {
                Access::Read => format!(
                    "Only files you choose can be read: choose {} again with Open",
                    file_name(&resolved)
                ),
                Access::Write => format!(
                    "Files are written only where you choose: choose where to save {} again",
                    file_name(&resolved)
                ),
            })
            .hint("Choose it again in the dialog"));
        }
        Ok(GrantedPath {
            path: resolved,
            access,
        })
    }

    /// Drops a window's grants (it closed).
    pub fn forget(&self, window: &str) {
        self.lock().remove(window);
    }

    /// How many grants a window holds (expired ones included until the next grant prunes them).
    pub fn count(&self, window: &str) -> usize {
        self.lock().get(window).map_or(0, Vec::len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::MAX_PATH_LEN;
    use paddock_commands::ErrorCode;

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn only_granted_paths_pass_and_messages_say_what_to_do() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("in.json");
        std::fs::write(&file, "{}").unwrap();
        let g = PathGrants::default();
        let e = g
            .check("main", &s(&file), Access::Read, &["json"])
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::NotGranted);
        assert!(!e.retryable);
        assert!(
            e.message.contains("Only files you choose") && e.message.contains("in.json"),
            "{}",
            e.message
        );
        assert!(e.hint.is_some());
        let granted = g
            .grant("main", &file, Access::Read, GrantSource::Dialog)
            .unwrap();
        assert_eq!(granted.text(), s(&canonical(&file).unwrap()));
        let ok = g.check("main", &s(&file), Access::Read, &["json"]).unwrap();
        assert_eq!(ok.path(), canonical(&file).unwrap());
        assert_eq!(ok.access(), Access::Read);
        // Per window, per access, per extension.
        assert!(g.check("w-2", &s(&file), Access::Read, &[]).is_err());
        let e = g.check("main", &s(&file), Access::Write, &[]).unwrap_err();
        assert!(
            e.message.contains("written only where you choose"),
            "{}",
            e.message
        );
        assert!(
            g.check("main", &s(&file), Access::Read, &["toml"])
                .unwrap_err()
                .message
                .contains(".toml")
        );
        // A save grant for a file that does not exist yet.
        let out = t.path().join("out.zip");
        g.grant("main", &out, Access::Write, GrantSource::Dialog)
            .unwrap();
        assert!(g.check("main", &s(&out), Access::Write, &["zip"]).is_ok());
        assert!(
            g.check(
                "main",
                &s(&t.path().join("other.zip")),
                Access::Write,
                &["zip"]
            )
            .is_err()
        );
        assert!(
            g.check("main", &s(&out), Access::Read, &[]).is_err(),
            "a save grant does not read"
        );
        g.forget("main");
        assert_eq!(g.count("main"), 0);
        assert!(g.check("main", &s(&file), Access::Read, &[]).is_err());
    }

    #[test]
    fn other_spellings_and_links_cannot_escape_a_grant() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::fs::create_dir_all(r.join("chosen/sub")).unwrap();
        std::fs::create_dir_all(r.join("secret")).unwrap();
        std::fs::write(r.join("secret/key.txt"), "k").unwrap();
        std::fs::write(r.join("chosen/sub/a.log"), "a").unwrap();
        let g = PathGrants::default();
        g.grant("main", &r.join("chosen"), Access::Read, GrantSource::Dialog)
            .unwrap();
        // Inside the chosen folder, however spelled.
        assert!(
            g.check("main", &s(&r.join("chosen/sub/a.log")), Access::Read, &[])
                .is_ok()
        );
        assert!(
            g.check(
                "main",
                &s(&r.join("chosen/sub/../sub/./a.log")),
                Access::Read,
                &[]
            )
            .is_ok()
        );
        // `..` out of it is refused.
        assert!(
            g.check(
                "main",
                &s(&r.join("chosen/../secret/key.txt")),
                Access::Read,
                &[]
            )
            .is_err()
        );
        // A write grant is exact: `..` cannot point it at a neighbour.
        g.grant(
            "main",
            &r.join("chosen/out.csv"),
            Access::Write,
            GrantSource::Dialog,
        )
        .unwrap();
        assert!(
            g.check(
                "main",
                &s(&r.join("chosen/sub/../out.csv")),
                Access::Write,
                &[]
            )
            .is_ok()
        );
        assert!(
            g.check("main", &s(&r.join("chosen/../out.csv")), Access::Write, &[])
                .is_err()
        );
        assert!(
            g.check("main", &s(&r.join("chosen/sub/a.log")), Access::Write, &[])
                .is_err(),
            "a read tree does not write"
        );
        // A link inside the chosen folder that points outside resolves before the check.
        #[cfg(unix)]
        {
            let link = r.join("chosen/link");
            std::os::unix::fs::symlink(r.join("secret"), &link).unwrap();
            assert!(
                g.check("main", &s(&link.join("key.txt")), Access::Read, &[])
                    .is_err()
            );
            assert!(
                g.check("main", &s(&link.join("new.txt")), Access::Write, &[])
                    .is_err()
            );
        }
    }

    #[test]
    fn grants_are_bounded_expire_and_need_real_paths() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        let g = PathGrants::default();
        let now = Instant::now();
        let project = r.join("p.proj");
        g.grant_at("main", &project, Access::Write, GrantSource::Project, now)
            .unwrap();
        for i in 0..MAX_GRANTS + 10 {
            g.grant_at(
                "main",
                &r.join(format!("f{i}.csv")),
                Access::Write,
                GrantSource::Dialog,
                now,
            )
            .unwrap();
        }
        assert_eq!(g.count("main"), MAX_GRANTS);
        assert!(
            g.check_at("main", &s(&r.join("f0.csv")), Access::Write, &[], now)
                .is_err(),
            "the oldest went first"
        );
        let last = s(&r.join(format!("f{}.csv", MAX_GRANTS + 9)));
        assert!(g.check_at("main", &last, Access::Write, &[], now).is_ok());
        assert!(
            g.check_at("main", &last, Access::Write, &[], now + GRANT_TTL)
                .is_err(),
            "expired"
        );
        // The project file stays, and a second project replaces it.
        assert!(
            g.check_at(
                "main",
                &s(&project),
                Access::Write,
                &[],
                now + GRANT_TTL * 10
            )
            .is_ok()
        );
        let second = r.join("q.proj");
        g.grant_at("main", &second, Access::Write, GrantSource::Project, now)
            .unwrap();
        assert!(
            g.check_at("main", &s(&project), Access::Write, &[], now)
                .is_err(),
            "one project per window"
        );
        assert!(
            g.check_at("main", &s(&second), Access::Write, &[], now)
                .is_ok()
        );
        // Only real paths can be granted or checked.
        assert!(
            g.grant(
                "main",
                &r.join("missing.log"),
                Access::Read,
                GrantSource::Drop
            )
            .is_err()
        );
        assert!(
            g.grant(
                "main",
                &r.join("no/such/x.csv"),
                Access::Write,
                GrantSource::Dialog
            )
            .is_err()
        );
        let granted = g.grant_all(
            "main",
            &[r.join("missing"), r.to_path_buf()],
            GrantSource::Drop,
        );
        assert_eq!(granted.len(), 1);
        for bad in ["", "relative.csv", "a\0b", &"x".repeat(MAX_PATH_LEN + 1)] {
            assert!(g.check("main", bad, Access::Read, &[]).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn granting_again_replaces_and_a_poisoned_lock_recovers() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("a.log");
        std::fs::write(&file, "x").unwrap();
        let g = PathGrants::default();
        for _ in 0..5 {
            g.grant("main", &file, Access::Read, GrantSource::Output)
                .unwrap();
        }
        assert_eq!(g.count("main"), 1, "the same path and access is one grant");
        // Poisoned lock: the grants survive a panic in another thread.
        let g2 = g.clone();
        let _ = std::thread::spawn(move || {
            let _hold = g2.lock();
            panic!("poison");
        })
        .join();
        assert!(g.check("main", &s(&file), Access::Read, &[]).is_ok());
    }
}
