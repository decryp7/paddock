//! The one place a host step deletes (ER-14, ER-22): a path goes only when it lies strictly under an explicit root, and the root
//! is itself not somewhere a mistake would be a disaster. The shell scripts ran `rm -rf` on computed paths; a wrong variable
//! there could reach a home folder. Here the guard is checked on every call and refused with a message.
//!
//! A link (a symlink, a Windows junction) is removed as a link and never followed. A path that is not there is fine (the goal).
//! The deepest delete is `std::fs::remove_dir_all`, which does not follow links either.

use std::io;
use std::path::{Component, Path, PathBuf};

/// Why a removal was refused, or what failed.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveError {
    /// The guard refused: the text says why. Nothing was touched.
    Refused(String),
    /// The OS refused: the text is its message.
    Failed(String),
}

impl std::fmt::Display for RemoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoveError::Refused(t) | RemoveError::Failed(t) => f.write_str(t),
        }
    }
}

/// The normal components of `p` with `.` dropped; `None` when it has `..` (a computed path is never allowed to climb).
fn normal_parts(p: &Path) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => parts.push(n.to_string_lossy().to_string()),
            Component::ParentDir => return None,
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
        }
    }
    Some(parts)
}

/// Checks that `root` is a place a clean-up may work in: absolute, at least two folders deep and not the home folder or a folder
/// above it (`home` is the home folder as the job sees it, when known).
pub fn check_root(root: &Path, home: Option<&Path>) -> Result<(), String> {
    if !root.has_root() && !root.is_absolute() {
        return Err(format!("the root {} is not an absolute path", root.display()));
    }
    let parts = normal_parts(root).ok_or_else(|| format!("the root {} has a `..` in it", root.display()))?;
    if parts.len() < 2 {
        return Err(format!("the root {} is a drive root or a top-level folder", root.display()));
    }
    if let Some(h) = home
        && let Some(hp) = normal_parts(h)
        && !hp.is_empty()
        && hp.starts_with(&parts)
    {
        return Err(format!("the root {} is the home folder or above it", root.display()));
    }
    Ok(())
}

/// Removes `target` (a file, a folder with everything in it, or a link) if it lies strictly under `root`.
/// `Ok(true)`: removed; `Ok(false)`: it was not there.
pub fn remove_under(root: &Path, target: &Path, home: Option<&Path>) -> Result<bool, RemoveError> {
    check_root(root, home).map_err(RemoveError::Refused)?;
    let rp = normal_parts(root).ok_or_else(|| RemoveError::Refused("the root has a `..`".into()))?;
    let tp =
        normal_parts(target).ok_or_else(|| RemoveError::Refused(format!("{} has a `..` in it; it is not removed", target.display())))?;
    if tp.len() <= rp.len() || !tp.starts_with(&rp) {
        return Err(RemoveError::Refused(format!(
            "{} is not strictly under {}; it is not removed",
            target.display(),
            root.display()
        )));
    }
    let meta = match std::fs::symlink_metadata(target) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(RemoveError::Failed(e.to_string())),
    };
    let done: io::Result<()> = if meta.file_type().is_symlink() {
        remove_link(target)
    } else if meta.is_dir() {
        // A Windows junction is a directory to `symlink_metadata`: remove_dir_all removes the junction, not its target.
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    };
    match done {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(RemoveError::Failed(e.to_string())),
    }
}

/// Removes a symlink: a file link on Unix, a file or directory link on Windows.
fn remove_link(p: &Path) -> io::Result<()> {
    std::fs::remove_file(p).or_else(|_| std::fs::remove_dir(p))
}

/// `root/child` for each name, for a clean-up that removes several fixed children of one root.
pub fn children(root: &Path, names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(|n| root.join(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ci").join("home");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/f"), b"x").unwrap();
        (d, root)
    }

    #[test]
    fn a_folder_under_the_root_goes_with_its_content() {
        let (_d, root) = tree();
        assert_eq!(remove_under(&root, &root.join("a"), None), Ok(true));
        assert!(!root.join("a").exists());
        assert_eq!(remove_under(&root, &root.join("a"), None), Ok(false), "not there is the goal");
    }

    #[test]
    fn the_root_itself_and_paths_outside_or_climbing_are_refused() {
        let (d, root) = tree();
        std::fs::create_dir(d.path().join("outside")).unwrap();
        for bad in [
            root.clone(),
            d.path().join("outside"),
            root.join("a/../../outside"),
            root.parent().unwrap().to_path_buf(),
        ] {
            assert!(
                matches!(remove_under(&root, &bad, None), Err(RemoveError::Refused(_))),
                "{}",
                bad.display()
            );
        }
        assert!(d.path().join("outside").exists() && root.join("a/b/f").exists());
    }

    #[test]
    fn a_root_that_is_shallow_or_a_home_folder_is_refused_before_anything_is_touched() {
        let (d, root) = tree();
        for bad in [Path::new("/"), Path::new("/Users"), Path::new("relative/dir")] {
            assert!(check_root(bad, None).is_err(), "{}", bad.display());
        }
        let home = d.path().join("ci");
        assert!(check_root(&home, Some(&home)).unwrap_err().contains("home folder"));
        assert!(
            check_root(d.path(), Some(&home)).unwrap_err().contains("home folder"),
            "above the home"
        );
        assert!(check_root(&root, Some(&home)).is_ok(), "below the home is fine");
        assert!(matches!(
            remove_under(Path::new("/"), Path::new("/tmp"), None),
            Err(RemoveError::Refused(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_removed_as_a_link_and_its_target_stays() {
        let (d, root) = tree();
        let outside = d.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        assert_eq!(remove_under(&root, &root.join("link"), None), Ok(true));
        assert!(!root.join("link").exists() && outside.join("keep").exists());
        // A path that goes through a link is a path under the root by name; its target is never reached by a recursive delete
        // because the last component is what is removed.
        std::os::unix::fs::symlink(&outside, root.join("l2")).unwrap();
        assert_eq!(remove_under(&root, &root.join("l2"), None), Ok(true));
        assert!(outside.join("keep").exists());
    }

    #[test]
    fn a_file_goes_too() {
        let (_d, root) = tree();
        assert_eq!(remove_under(&root, &root.join("a/b/f"), None), Ok(true));
        assert!(root.join("a/b").exists());
    }
}
