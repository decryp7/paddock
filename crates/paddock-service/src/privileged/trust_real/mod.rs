//! The real [`TrustPort`]: the facts of each path component are read from an object opened for that purpose and never
//! followed through a link (`O_NOFOLLOW` semantics on Unix, `FILE_FLAG_OPEN_REPARSE_POINT` on Windows).
//!
//! One module per OS family (`unix`, `windows`) behind this one; the pieces that do not depend on the OS (walking without
//! following links, moving aside, removing a tree) are here. The Windows module is native glue (crates/README.md) that only a
//! Windows runner can cover (its tests run in the crate job's Windows leg and on the Windows test VM); the Unix module is
//! covered by its own tempdir tests.
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
/// never following a link, and returns the names removed.
fn sweep_aside_free(parent: &str, keep: usize) -> io::Result<Vec<String>> {
    let mut found: Vec<((u64, u64), String)> = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let Some((_, suffix)) = name.rsplit_once(".untrusted-") else {
            continue;
        };
        let Some((secs, n)) = suffix.split_once('-') else {
            continue;
        };
        if let (Ok(secs), Ok(n)) = (secs.parse(), n.parse()) {
            found.push(((secs, n), name));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    let mut removed = Vec::new();
    for (_, name) in found.into_iter().skip(keep) {
        let sep = if cfg!(windows) { '\\' } else { '/' };
        remove_tree_no_follow(&format!("{}{sep}{name}", parent.trim_end_matches(sep)))?;
        removed.push(name);
    }
    Ok(removed)
}

/// `std::fs::remove_dir_all` removes a link inside the tree (and the root, if it is one) and never follows it, on Unix
/// and on Windows (it opens each entry with `FILE_FLAG_OPEN_REPARSE_POINT`); a root that is itself a link is removed as the
/// link it is.
fn remove_tree_no_follow(path: &str) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            // A directory symlink or junction is removed with remove_dir on Windows, a file symlink with remove_file.
            std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
        }
        Ok(_) => std::fs::remove_dir_all(path),
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
