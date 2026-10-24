//! Extract into a staging folder beside the destination, then swap it in.

use crate::control::Control;
use crate::error::{ArchiveError, Result, io};
use crate::options::Options;
use crate::readers::{Format, read_into};
use crate::writer::Report;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A staging name for `dest`, unique per process and call: `.<name>.paddock-<kind>-<pid>-<n>` beside it.
fn sibling(dest: &Path, kind: &str) -> PathBuf {
    let name = dest
        .file_name()
        .map_or_else(|| "out".to_string(), |n| n.to_string_lossy().into_owned());
    let parent = parent_of(dest);
    parent.join(format!(
        ".{name}.paddock-{kind}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn parent_of(dest: &Path) -> PathBuf {
    match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Removes a folder or file when dropped, unless disarmed: a failure or a cancel leaves no staging behind.
struct Cleanup(Option<PathBuf>);

impl Cleanup {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_dir_all(&p);
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// Unpacks `archive` into the folder `dest`, all or nothing: the entries are written to a staging folder beside `dest`
/// (same disk, so the final step is a rename), and only when every entry was written is `dest` replaced by it. A failure, a
/// limit or a cancel removes the staging folder and leaves an existing `dest` exactly as it was (rule 11: build new data,
/// then swap). The format is recognised from the file's content.
pub fn extract(archive: &Path, dest: &Path, opts: &Options, ctl: &dyn Control) -> Result<Report> {
    let format = Format::sniff(archive)?;
    let parent = parent_of(dest);
    std::fs::create_dir_all(&parent).map_err(io("create folder", &parent))?;
    let staging = sibling(dest, "staging");
    std::fs::create_dir(&staging).map_err(io("create folder", &staging))?;
    let mut cleanup = Cleanup(Some(staging.clone()));
    let report = read_into(
        archive,
        format,
        &staging,
        &sibling(dest, "expand"),
        opts,
        ctl,
    )?;
    ctl.check()?;
    swap(&staging, dest)?;
    cleanup.disarm();
    Ok(report)
}

/// Replaces `dest` by `staging` (a rename); an existing `dest` is moved aside first and removed afterwards, and put back if the
/// second rename fails.
fn swap(staging: &Path, dest: &Path) -> Result<()> {
    if !dest.exists() {
        return std::fs::rename(staging, dest).map_err(io("move into place", dest));
    }
    if !dest.is_dir() {
        return Err(ArchiveError::Io {
            op: "replace",
            path: dest.to_path_buf(),
            source: std::io::Error::other("it is a file, not a folder"),
        });
    }
    let old = sibling(dest, "old");
    std::fs::rename(dest, &old).map_err(io("move aside", dest))?;
    if let Err(e) = std::fs::rename(staging, dest) {
        let _ = std::fs::rename(&old, dest);
        return Err(ArchiveError::Io {
            op: "move into place",
            path: dest.to_path_buf(),
            source: e,
        });
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Removes staging, expansion and moved-aside leftovers of `dest` that a crash left in its parent folder and that are older
/// than `older_than` (so a run in another process is not touched). Returns how many were removed.
pub fn sweep_stale(dest: &Path, older_than: Duration) -> usize {
    let name = dest
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let prefixes = [
        format!(".{name}.paddock-staging-"),
        format!(".{name}.paddock-expand-"),
        format!(".{name}.paddock-old-"),
    ];
    let Ok(read) = std::fs::read_dir(parent_of(dest)) else {
        return 0;
    };
    let mut removed = 0;
    for entry in read.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !prefixes.iter().any(|p| file_name.starts_with(p)) {
            continue;
        }
        let old_enough = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age >= older_than);
        if old_enough {
            let p = entry.path();
            if std::fs::remove_dir_all(&p)
                .or_else(|_| std::fs::remove_file(&p))
                .is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_file_name_is_staged_in_the_current_folder() {
        assert_eq!(parent_of(Path::new("rt")), PathBuf::from("."));
        assert_eq!(parent_of(Path::new("a/rt")), PathBuf::from("a"));
        assert!(sibling(Path::new("rt"), "staging").starts_with("."));
    }

    #[test]
    fn a_failed_second_rename_puts_the_old_folder_back() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("rt");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("old"), "o").unwrap();
        let e = swap(&d.path().join("no-such-staging"), &dest).unwrap_err();
        assert!(matches!(
            e,
            ArchiveError::Io {
                op: "move into place",
                ..
            }
        ));
        assert_eq!(std::fs::read_to_string(dest.join("old")).unwrap(), "o");
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            1,
            "no moved-aside folder is left"
        );
    }
}
