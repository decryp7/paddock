//! The home guard (ER-14, ER-24): a build folder must not land inside the home folder (the internal disk). One implementation
//! for `heavy` and `paddock-dev`: a path is made absolute, its deepest existing folder is resolved through symlinks, `..` and `.`
//! are folded, and then it is compared with the home folder resolved the same way.

use crate::winpath;
use std::path::{Component, Path, PathBuf};

/// The absolute, symlink-resolved form of `p`: `~` and `~/x` read as the home folder, a relative path is joined to `cwd`, the
/// deepest existing ancestor is resolved ([`winpath::real`]: no verbatim prefix) and the rest is appended as written, with `..`
/// and `.` folded lexically.
pub fn abs_path(p: &str, home: Option<&Path>, cwd: &Path) -> PathBuf {
    let mut path = if p == "~" {
        home.map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(p))
    } else if let (Some(rest), Some(h)) = (p.strip_prefix("~/"), home) {
        h.join(rest)
    } else {
        PathBuf::from(p)
    };
    if path.is_relative() {
        path = cwd.join(path);
    }
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut base = path.clone();
    while !base.is_dir() {
        match (
            base.file_name().map(ToOwned::to_owned),
            base.parent().map(Path::to_path_buf),
        ) {
            (Some(n), Some(parent)) => {
                rest.push(n);
                base = parent;
            }
            _ => break,
        }
    }
    let mut out = winpath::real(&base).unwrap_or(base);
    for n in rest.iter().rev() {
        out.push(n);
    }
    let mut norm = PathBuf::new();
    for c in out.components() {
        match c {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            other => norm.push(other.as_os_str()),
        }
    }
    norm
}

/// Whether `dir` (as written: relative to `cwd`, `~` allowed, symlinks followed) is the home folder or inside it.
pub fn inside_home(dir: &str, home: &Path, cwd: &Path) -> bool {
    let h = abs_path(&home.to_string_lossy(), Some(home), cwd);
    abs_path(dir, Some(home), cwd).starts_with(&h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_inside_the_home_is_found_however_it_is_spelled() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        std::fs::create_dir_all(home.join("x")).unwrap();
        let cwd = d.path().join("elsewhere");
        std::fs::create_dir_all(&cwd).unwrap();
        for inside in [
            "~",
            "~/builds",
            "x/../y",
            &home.join("x").display().to_string(),
        ] {
            let cwd = if inside == "x/../y" {
                home.as_path()
            } else {
                cwd.as_path()
            };
            assert!(inside_home(inside, &home, cwd), "{inside}");
        }
        assert!(!inside_home(
            &d.path().join("d7").display().to_string(),
            &home,
            &cwd
        ));
        assert!(
            !inside_home("../d7", &home, &cwd),
            "a relative folder beside the home"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_into_the_home_does_not_hide_a_folder() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::os::unix::fs::symlink(&home, d.path().join("link")).unwrap();
        assert!(inside_home(
            &d.path().join("link/new").display().to_string(),
            &home,
            d.path()
        ));
    }
}
