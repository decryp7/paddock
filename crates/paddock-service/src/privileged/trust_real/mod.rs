//! The real [`TrustPort`]: the facts of each path component are read from an object opened for that purpose and never
//! followed through a link (`O_NOFOLLOW` semantics on Unix, `FILE_FLAG_OPEN_REPARSE_POINT` on Windows).
//!
//! One module per OS family (`unix`, `windows`) behind this one; the pieces that do not depend on the OS (walking without
//! following links, moving aside, removing a tree) are here. The Windows module is native glue (crates/README.md) that only a
//! Windows runner can cover (its tests run in the crate job's Windows leg and on the Windows test VM); the Unix module is
//! covered by its own tempdir tests.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]
#![allow(unsafe_code)]

use super::files::FileKind;
use super::trust::{Node, Style, TrustPort};
use std::io;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// The real machine.
#[derive(Debug, Default)]
pub struct RealTrust;

/// Every entry below `root` (a link is listed and not entered), at most `limit`.
fn walk_no_follow(
    port: &mut dyn TrustPort,
    root: &str,
    limit: usize,
) -> io::Result<Vec<(String, Node)>> {
    let sep = if port.style() == Style::Windows {
        '\\'
    } else {
        '/'
    };
    let mut out = Vec::new();
    let mut todo = vec![root.to_string()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            let path = format!("{}{sep}{name}", dir.trim_end_matches(sep));
            let Some(node) = port.node(&path)? else {
                continue;
            };
            if node.kind == FileKind::Dir {
                todo.push(path.clone());
            }
            out.push((path, node));
            if out.len() > limit {
                return Err(io::Error::other(format!("more than {limit} entries")));
            }
        }
    }
    Ok(out)
}

/// Renames `path` next to itself to a free name; the entry is never opened or followed.
fn rename_aside_free(path: &str) -> io::Result<String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for n in 0..100 {
        let aside = format!("{path}.untrusted-{secs}-{n}");
        if std::fs::symlink_metadata(&aside).is_ok() {
            continue;
        }
        std::fs::rename(path, &aside)?;
        return Ok(aside);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name to move the folder aside",
    ))
}

/// Removes the oldest `<name>.untrusted-<secs>-<n>` entries in `parent` beyond the newest `keep` (see [`rename_aside_free`]),
/// never following a link, and returns the names removed. Only an entry whose name is exactly that (digits only), that is a
/// real folder and not a link, and whose owner SYSTEM, Administrators or root holds, is touched: a folder a user made with a
/// similar name, or a link, stays.
fn sweep_aside_free(
    port: &mut dyn TrustPort,
    parent: &str,
    name: &str,
    keep: usize,
) -> io::Result<Vec<String>> {
    use crate::privileged::trust::Security;
    let prefix = format!("{name}.untrusted-");
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let mut found: Vec<((u64, u64), String)> = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let entry_name = entry?.file_name().to_string_lossy().into_owned();
        let Some(suffix) = entry_name.strip_prefix(&prefix) else {
            continue;
        };
        let Some((secs, n)) = suffix.split_once('-') else {
            continue;
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !digits(secs) || !digits(n) {
            continue;
        }
        let (Ok(secs), Ok(n)) = (secs.parse(), n.parse()) else {
            continue;
        };
        let path = format!("{}{sep}{entry_name}", parent.trim_end_matches(sep));
        let Some(node) = port.node(&path)? else {
            continue;
        };
        let trusted = match &node.security {
            Security::Windows(d) => crate::privileged::trust::is_trusted_sid(&d.owner),
            Security::Unix { owner, .. } => *owner == 0,
        };
        if node.kind == FileKind::Dir && trusted {
            found.push(((secs, n), entry_name));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    let mut removed = Vec::new();
    for (_, entry_name) in found.into_iter().skip(keep) {
        remove_tree_no_follow(&format!(
            "{}{sep}{entry_name}",
            parent.trim_end_matches(sep)
        ))?;
        removed.push(entry_name);
    }
    Ok(removed)
}

/// `std::fs::remove_dir_all` removes a link inside the tree (and the root, if it is one) and never follows it; a file or a link
/// given as the root is removed by its name, on Unix
/// and on Windows (it opens each entry with `FILE_FLAG_OPEN_REPARSE_POINT`); a root that is itself a link is removed as the
/// link it is.
fn remove_tree_no_follow(path: &str) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            // A directory symlink or junction is removed with remove_dir on Windows, a file symlink with remove_file.
            std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
        }
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        // A plain file (a planted one) goes by its name.
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// A program held open for a privileged run: opened without following a link, shown to be a regular file, and kept open
/// until it is dropped (after the process started), so the file that was checked is the file that runs.
///
/// - **Windows:** opened with `FILE_SHARE_READ` only (nobody can write to, rename or delete it while held) and with
///   `FILE_FLAG_OPEN_REPARSE_POINT`; [`final_path`](Self::final_path) is what `GetFinalPathNameByHandle` returns for this
///   handle, so a junction in the requested path shows as a different path.
/// - **Unix:** opened with `O_NOFOLLOW` after resolving the path, owned by root and not writable by others (checked on the
///   descriptor). On Linux it must be an ELF program and runs through `/proc/self/fd/<fd>` of this descriptor (the kernel's
///   `execve` of that path runs the open file); when `/proc/self/fd` is not available the fallback is logged and the checked
///   path runs (the protected folder chain is what prevents a swap). Elsewhere it runs by the resolved path.
#[derive(Debug)]
pub struct HeldImage {
    pub(super) _file: std::fs::File,
    pub(super) final_path: String,
    pub(super) spawn_path: String,
}

impl HeldImage {
    /// Holds `requested`.
    ///
    /// # Errors
    /// The path is missing, is a link or reparse point, is not a regular file, or (Unix) is not root's.
    pub fn open(requested: &str) -> io::Result<Self> {
        #[cfg(windows)]
        {
            windows::open_image(requested)
        }
        #[cfg(unix)]
        {
            unix::open_image(requested)
        }
    }

    /// The path the open object really has (links resolved).
    pub fn final_path(&self) -> &str {
        &self.final_path
    }

    /// What to run: the final path, or the descriptor's `/proc` path on Linux.
    pub fn spawn_path(&self) -> &str {
        &self.spawn_path
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::privileged::trust::Security;

    /// The real port, reporting every owner as the trusted one (the tests cannot make root-owned or Administrators-owned
    /// folders): for the tests of the sweep.
    pub(super) struct Trusting(pub RealTrust);

    impl TrustPort for Trusting {
        fn style(&self) -> Style {
            self.0.style()
        }
        fn node(&mut self, path: &str) -> io::Result<Option<Node>> {
            Ok(self.0.node(path)?.map(|mut n| {
                n.security = match n.security {
                    Security::Unix {
                        mode, write_acl, ..
                    } => Security::Unix {
                        owner: 0,
                        mode,
                        write_acl,
                    },
                    Security::Windows(mut d) => {
                        d.owner = crate::privileged::trust::SID_ADMINISTRATORS.into();
                        Security::Windows(d)
                    }
                };
                n
            }))
        }
        fn canonical(&mut self, p: &str) -> io::Result<String> {
            self.0.canonical(p)
        }
        fn walk(&mut self, p: &str, l: usize) -> io::Result<Vec<(String, Node)>> {
            self.0.walk(p, l)
        }
        fn create_dir(&mut self, p: &str) -> io::Result<()> {
            self.0.create_dir(p)
        }
        fn lock_dir(&mut self, p: &str) -> io::Result<()> {
            self.0.lock_dir(p)
        }
        fn rename_aside(&mut self, p: &str) -> io::Result<String> {
            self.0.rename_aside(p)
        }
        fn sweep_aside(
            &mut self,
            parent: &str,
            name: &str,
            keep: usize,
        ) -> io::Result<Vec<String>> {
            sweep_aside_free(self, parent, name, keep)
        }
        fn remove_tree(&mut self, p: &str) -> io::Result<()> {
            self.0.remove_tree(p)
        }
    }

    fn text(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// Round 5 (Pitcrew P2-3, P2-9): the real port tells which file a path is and how many names it has, never following a link.
    #[cfg(unix)]
    #[test]
    fn identity_tells_the_file_and_its_link_count_and_does_not_follow_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, "x").unwrap();
        let mut port = RealTrust;
        let first = port.identity(&text(&a)).unwrap().expect("exists");
        assert_eq!(first.links, 1);
        std::fs::hard_link(&a, &b).unwrap();
        let again = port.identity(&text(&a)).unwrap().unwrap();
        assert_eq!(again.links, 2, "a second name for the file");
        assert_eq!((again.volume, again.index), (first.volume, first.index));
        assert_eq!(
            port.identity(&text(&b)).unwrap().unwrap(),
            again,
            "the same file by either name"
        );
        let other = dir.path().join("c");
        std::fs::write(&other, "y").unwrap();
        assert_ne!(
            port.identity(&text(&other)).unwrap().unwrap().index,
            first.index
        );
        // A link is itself, not its target.
        let link = dir.path().join("l");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert_ne!(
            port.identity(&text(&link)).unwrap().unwrap().index,
            first.index
        );
        assert_eq!(
            port.identity(&text(&dir.path().join("missing"))).unwrap(),
            None
        );
    }

    #[test]
    fn sweep_aside_matches_only_the_exact_name_prefix_and_real_folders_and_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        for name in [
            "Dyno.untrusted-5-0",
            "Dyno.untrusted-9-1",
            "Dyno.untrusted-7-0",
            // Not the helper's names: another folder's prefix, no numbers, extra characters, a different case of the name.
            "Other.untrusted-1-0",
            "Dyno.untrusted-x-y",
            "Dyno.untrusted-1-2-3",
            "Dyno.untrusted-",
            "xDyno.untrusted-2-0",
            "other",
        ] {
            std::fs::create_dir(parent.join(name)).unwrap();
        }
        // A file with the right name is not a folder.
        std::fs::write(parent.join("Dyno.untrusted-3-0"), b"file").unwrap();
        let mut port = Trusting(RealTrust);
        let mut removed = port.sweep_aside(&text(&parent), "Dyno", 1).unwrap();
        removed.sort();
        assert_eq!(removed, vec!["Dyno.untrusted-5-0", "Dyno.untrusted-7-0"]);
        for kept in [
            "Dyno.untrusted-9-1",
            "Other.untrusted-1-0",
            "Dyno.untrusted-x-y",
            "Dyno.untrusted-1-2-3",
            "Dyno.untrusted-",
            "xDyno.untrusted-2-0",
            "other",
            "Dyno.untrusted-3-0",
        ] {
            assert!(
                parent.join(kept).exists(),
                "{kept} is not the helper's and stays"
            );
        }
        assert!(
            port.sweep_aside(&text(&parent), "Dyno", 5)
                .unwrap()
                .is_empty()
        );
        assert!(
            port.sweep_aside(&text(&parent.join("none")), "Dyno", 1)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn sweep_aside_never_follows_or_removes_a_link_and_skips_what_an_untrusted_user_owns() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
        let parent = dir.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        std::fs::create_dir(parent.join("Dyno.untrusted-9-0")).unwrap();
        std::os::unix::fs::symlink(&outside, parent.join("Dyno.untrusted-1-0")).unwrap();
        let removed = Trusting(RealTrust)
            .sweep_aside(&text(&parent), "Dyno", 0)
            .unwrap();
        assert_eq!(removed, vec!["Dyno.untrusted-9-0"]);
        assert!(
            std::fs::symlink_metadata(parent.join("Dyno.untrusted-1-0")).is_ok(),
            "the link stays"
        );
        assert!(outside.join("keep.txt").exists());
        // Owned by the user running the test (not root): not the helper's, so not touched.
        if unsafe { libc::getuid() } != 0 {
            std::fs::create_dir(parent.join("Dyno.untrusted-4-0")).unwrap();
            assert!(
                RealTrust
                    .sweep_aside(&text(&parent), "Dyno", 0)
                    .unwrap()
                    .is_empty()
            );
            assert!(parent.join("Dyno.untrusted-4-0").exists());
        }
    }
}
