//! The one place a host step deletes (ER-14, ER-22): a path goes only when it lies strictly under an explicit root, and the root
//! is itself not somewhere a mistake would be a disaster. The shell scripts ran `rm -rf` on computed paths; a wrong variable
//! there could reach a home folder. Here the guard is checked on every call and refused with a message.
//!
//! A path that is not there is fine (the goal). Only the last component may be a link (a symlink, a Windows junction): it is
//! removed as a link and never followed. A link on the way between the root and the target refuses the removal. The deepest delete is `std::fs::remove_dir_all`, which does not follow links either.

use std::io;
use std::path::{Path, PathBuf};

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

/// How paths are read: Windows spelling (drive letters, `\` separators) and whether names compare without case. The real values
/// are the host's ([`Rules::host`]); tests give the other OS's so every OS checks both.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// Read `C:\x`, `C:/x` and `\\?\C:\x` as a drive and folders.
    pub windows: bool,
    /// Compare names (and drive letters) without case: Windows and macOS file systems are case-insensitive by default.
    pub fold: bool,
}

impl Rules {
    /// This machine's.
    pub const fn host() -> Rules {
        Rules {
            windows: cfg!(windows),
            fold: cfg!(any(windows, target_os = "macos")),
        }
    }
}

/// A path as its drive (empty on Unix and for a drive-less path) and its folder names; `.` dropped.
#[derive(Debug, PartialEq, Eq)]
struct Parts {
    drive: String,
    absolute: bool,
    names: Vec<String>,
}

/// The parts of `p`; `None` when it has `..` (a computed path is never allowed to climb).
fn split(p: &Path, rules: Rules) -> Option<Parts> {
    let text = p.to_string_lossy();
    let mut t: &str = &text;
    let mut drive = String::new();
    let mut absolute = false;
    if rules.windows {
        t = t.strip_prefix(r"\\?\").unwrap_or(t);
        let b = t.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            drive = t[..2].to_string();
            t = &t[2..];
            absolute = t.starts_with(['/', '\\']);
        }
    } else {
        absolute = t.starts_with('/');
    }
    let sep = |c: char| c == '/' || (rules.windows && c == '\\');
    let mut names = Vec::new();
    for part in t.split(sep) {
        match part {
            "" | "." => {}
            ".." => return None,
            n => names.push(n.to_string()),
        }
    }
    if rules.fold {
        drive = drive.to_lowercase();
    }
    Some(Parts { drive, absolute, names })
}

/// Whether two names are the same (case folded when `rules` say so).
fn same(a: &str, b: &str, rules: Rules) -> bool {
    if rules.fold { a.to_lowercase() == b.to_lowercase() } else { a == b }
}

/// Whether `inner`'s names start with all of `outer`'s, on the same drive.
fn starts_with(inner: &Parts, outer: &Parts, rules: Rules) -> bool {
    inner.drive == outer.drive
        && inner.names.len() >= outer.names.len()
        && outer.names.iter().zip(&inner.names).all(|(o, i)| same(o, i, rules))
}

/// Checks that `root` is a place a clean-up may work in: absolute, at least two folders deep and not the home folder or a folder
/// above it (`home` is the home folder as the job sees it, when known).
pub fn check_root(root: &Path, home: Option<&Path>) -> Result<(), String> {
    check_root_for(root, home, Rules::host())
}

/// [`check_root`] under explicit path rules.
pub fn check_root_for(root: &Path, home: Option<&Path>, rules: Rules) -> Result<(), String> {
    let parts = split(root, rules).ok_or_else(|| format!("the root {} has a `..` in it", root.display()))?;
    if !parts.absolute {
        return Err(format!("the root {} is not an absolute path", root.display()));
    }
    if parts.names.len() < 2 {
        return Err(format!("the root {} is a drive root or a top-level folder", root.display()));
    }
    if let Some(h) = home
        && let Some(hp) = split(h, rules)
        && !hp.names.is_empty()
        && starts_with(&hp, &parts, rules)
    {
        return Err(format!("the root {} is the home folder or above it", root.display()));
    }
    Ok(())
}

/// Whether `target` lies strictly under `root`, on the same drive, with `..` in neither.
pub fn strictly_under(root: &Path, target: &Path, rules: Rules) -> Result<bool, String> {
    let rp = split(root, rules).ok_or("the root has a `..`")?;
    let tp = split(target, rules).ok_or_else(|| format!("{} has a `..` in it", target.display()))?;
    Ok(tp.names.len() > rp.names.len() && starts_with(&tp, &rp, rules))
}

/// Removes `target` (a file, a folder with everything in it, or a link) if it lies strictly under `root` and no folder on the way
/// from `root` to `target` is a link (a link in the middle would send the delete somewhere else). The last component may be a
/// link: it is removed as a link and its target stays. `Ok(true)`: removed; `Ok(false)`: it was not there.
pub fn remove_under(root: &Path, target: &Path, home: Option<&Path>) -> Result<bool, RemoveError> {
    check_root(root, home).map_err(RemoveError::Refused)?;
    match strictly_under(root, target, Rules::host()) {
        Ok(true) => {}
        Ok(false) => {
            return Err(RemoveError::Refused(format!(
                "{} is not strictly under {}; it is not removed",
                target.display(),
                root.display()
            )));
        }
        Err(e) => return Err(RemoveError::Refused(format!("{e}; it is not removed"))),
    }
    // Every folder between the root and the target, as the target spells it: none may be a link.
    let mut at = target.parent();
    let mut middle = Vec::new();
    while let Some(d) = at {
        if strictly_under(root, d, Rules::host()) != Ok(true) {
            break;
        }
        middle.push(d);
        at = d.parent();
    }
    for d in middle {
        match std::fs::symlink_metadata(d) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(RemoveError::Refused(format!(
                    "{} passes through the link {}; it is not removed",
                    target.display(),
                    d.display()
                )));
            }
            Ok(_) => {}
            // Not there: nothing below it is either.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(RemoveError::Failed(e.to_string())),
        }
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
        let d = crate::ci_host::canonical_tempdir();
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
        std::os::unix::fs::symlink(&outside, root.join("l2")).unwrap();
        assert_eq!(remove_under(&root, &root.join("l2"), None), Ok(true));
        assert!(outside.join("keep").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_middle_of_the_path_refuses_the_removal_and_touches_nothing() {
        let (d, root) = tree();
        let outside = d.path().join("outside");
        std::fs::create_dir_all(outside.join("deep")).unwrap();
        std::fs::write(outside.join("deep/keep"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let r = remove_under(&root, &root.join("link/deep"), None);
        assert!(
            matches!(&r, Err(RemoveError::Refused(m)) if m.contains("passes through the link")),
            "{r:?}"
        );
        assert!(outside.join("deep/keep").exists(), "what the link points at is untouched");
        // A missing middle folder is just "not there".
        assert_eq!(remove_under(&root, &root.join("none/deeper"), None), Ok(false));
    }

    #[test]
    fn another_drive_is_not_under_the_root_and_names_compare_without_case_where_the_file_system_does() {
        let win = Rules { windows: true, fold: true };
        let (c, d) = (Path::new(r"C:\ci\home\x"), Path::new(r"D:\ci\home\x\y"));
        assert_eq!(
            strictly_under(c, d, win),
            Ok(false),
            "D:\\ is not under C:\\ even with the same folder names"
        );
        assert_eq!(
            strictly_under(c, Path::new(r"c:/CI/Home/x/y"), win),
            Ok(true),
            "drive letter, case and separators"
        );
        assert_eq!(
            strictly_under(c, Path::new(r"\\?\C:\ci\home\x\y"), win),
            Ok(true),
            "a verbatim prefix"
        );
        assert_eq!(strictly_under(c, c, win), Ok(false), "the root itself");
        assert!(strictly_under(c, Path::new(r"C:\ci\home\x\..\y"), win).is_err());
        let unix = Rules {
            windows: false,
            fold: false,
        };
        assert_eq!(
            strictly_under(Path::new("/ci/Home"), Path::new("/ci/home/x"), unix),
            Ok(false),
            "case counts on a case-sensitive file system"
        );
        assert_eq!(
            strictly_under(
                Path::new("/ci/Home"),
                Path::new("/ci/home/x"),
                Rules {
                    windows: false,
                    fold: true
                }
            ),
            Ok(true)
        );
    }

    #[test]
    fn a_home_folder_that_differs_only_in_case_is_still_the_home_folder_where_names_fold() {
        let fold = Rules {
            windows: false,
            fold: true,
        };
        let exact = Rules {
            windows: false,
            fold: false,
        };
        let (root, home) = (Path::new("/users/me/ci"), Path::new("/Users/Me"));
        assert!(
            check_root_for(Path::new("/users/me"), Some(home), fold)
                .unwrap_err()
                .contains("home folder")
        );
        assert!(
            check_root_for(Path::new("/users/me"), Some(home), exact).is_ok(),
            "another folder on a case-sensitive file system"
        );
        assert!(check_root_for(root, Some(home), fold).is_ok(), "below the home is fine");
        let win = Rules { windows: true, fold: true };
        assert!(
            check_root_for(Path::new(r"c:\users\me"), Some(Path::new(r"C:\Users\Me")), win)
                .unwrap_err()
                .contains("home folder")
        );
        assert!(
            check_root_for(Path::new(r"D:\users\me"), Some(Path::new(r"C:\Users\Me")), win).is_ok(),
            "another drive"
        );
        assert!(check_root_for(Path::new(r"C:\"), None, win).is_err(), "a drive root");
        assert!(
            check_root_for(Path::new(r"ci\home"), None, win)
                .unwrap_err()
                .contains("not an absolute path")
        );
    }

    #[test]
    fn a_file_goes_too() {
        let (_d, root) = tree();
        assert_eq!(remove_under(&root, &root.join("a/b/f"), None), Ok(true));
        assert!(root.join("a/b").exists());
    }
}
