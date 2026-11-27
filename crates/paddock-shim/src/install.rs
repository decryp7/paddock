//! `throttle shims install <dir>`: the links (Windows: copies) that make `throttle` run as `cargo`, `npm`, ... when called by those
//! names. It writes only the names it is asked for, and never replaces a file it did not write.

use std::path::{Path, PathBuf};

/// What happened to one name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// The link was made.
    Made(PathBuf),
    /// The link was already there and points at this program.
    Present(PathBuf),
    /// Left alone, and why (a file that is not ours is never replaced).
    Refused(String),
}

/// Installs one shim per name in `names` into `dir` (created if missing), pointing at `exe`: the result of each, in order (bulk: one call,
/// one result per item).
pub fn install(dir: &Path, exe: &Path, names: &[&str]) -> Vec<(String, Installed)> {
    if let Err(e) = std::fs::create_dir_all(dir) {
        return names
            .iter()
            .map(|n| {
                (
                    (*n).to_string(),
                    Installed::Refused(format!("cannot create {}: {e}", dir.display())),
                )
            })
            .collect();
    }
    names
        .iter()
        .map(|n| ((*n).to_string(), one(dir, exe, n)))
        .collect()
}

fn file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

#[cfg(unix)]
fn one(dir: &Path, exe: &Path, name: &str) -> Installed {
    let link = dir.join(file_name(name));
    if let Ok(meta) = std::fs::symlink_metadata(&link) {
        let ours =
            meta.file_type().is_symlink() && std::fs::read_link(&link).is_ok_and(|t| t == exe);
        return if ours {
            Installed::Present(link)
        } else {
            Installed::Refused(format!(
                "{} exists and is not a link to {}",
                link.display(),
                exe.display()
            ))
        };
    }
    match std::os::unix::fs::symlink(exe, &link) {
        Ok(()) => Installed::Made(link),
        Err(e) => Installed::Refused(format!("cannot link {}: {e}", link.display())),
    }
}

#[cfg(not(unix))]
fn one(dir: &Path, exe: &Path, name: &str) -> Installed {
    let link = dir.join(file_name(name));
    if link.exists() {
        let same = std::fs::read(&link).ok() == std::fs::read(exe).ok();
        return if same {
            Installed::Present(link)
        } else {
            Installed::Refused(format!(
                "{} exists and is not a copy of {}",
                link.display(),
                exe.display()
            ))
        };
    }
    match std::fs::hard_link(exe, &link).or_else(|_| std::fs::copy(exe, &link).map(|_| ())) {
        Ok(()) => Installed::Made(link),
        Err(e) => Installed::Refused(format!("cannot write {}: {e}", link.display())),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn links_are_made_once_and_a_foreign_file_is_never_replaced() {
        let t = tempfile::tempdir().unwrap();
        let exe = t.path().join("throttle");
        std::fs::write(&exe, "x").unwrap();
        let dir = t.path().join("shims");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("npm"), "mine").unwrap();
        let first = install(&dir, &exe, &["cargo", "npm"]);
        assert!(matches!(first[0].1, Installed::Made(_)), "{first:?}");
        assert!(matches!(first[1].1, Installed::Refused(_)), "{first:?}");
        assert_eq!(std::fs::read_to_string(dir.join("npm")).unwrap(), "mine");
        let again = install(&dir, &exe, &["cargo"]);
        assert!(matches!(again[0].1, Installed::Present(_)), "{again:?}");
        assert_eq!(std::fs::read_link(dir.join("cargo")).unwrap(), exe);
    }
}
