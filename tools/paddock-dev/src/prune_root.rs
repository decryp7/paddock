//! The root guard of `dev host prune` (ER-14, ER-24): the folders a prune never touches. Pure: whether two paths are the
//! same folder is asked of a comparator, so the rules run the same on every OS and [`crate::prune_fs::same_file`] supplies the
//! real one (file identity where the platform has it).

use crate::prune::PruneError;
use std::path::{Component, Path};

/// The folders of the home that hold the user's own files.
pub const USER_FOLDERS: [&str; 3] = ["Documents", "Desktop", "Downloads"];

/// Component-wise, ASCII case-insensitive equality: the fallback comparison where file identity is not available (a missing
/// folder, Windows) and what the pure tests use. Windows and macOS file systems ignore case, so it errs towards "same".
pub fn same_path_ci(a: &Path, b: &Path) -> bool {
    let (mut x, mut y) = (a.components(), b.components());
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if p.as_os_str().eq_ignore_ascii_case(q.as_os_str()) => {}
            _ => return false,
        }
    }
}

/// Refuses the roots a prune must never touch. `root` is absolute (resolved where it exists), `home` is the resolved home
/// folder, and `same` says whether two paths are the same folder.
///
/// Refused: a relative or empty root, a `..`, the top of a disk or one level below it, the home folder or a folder holding it,
/// and anything inside the home's `Documents`, `Desktop` or `Downloads`. The CI home (`~/.paddock-ci`) and
/// `~/.<app>-target-*` are inside the home on purpose and are allowed.
pub fn check_root(root: &Path, home: &Path, same: &dyn Fn(&Path, &Path) -> bool) -> Result<(), PruneError> {
    let shown = root.display().to_string();
    if shown.is_empty() || !root.is_absolute() {
        return Err(PruneError::NotAbsolute(shown));
    }
    if root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(PruneError::DotDot(shown));
    }
    if root.components().filter(|c| matches!(c, Component::Normal(_))).count() < 2 {
        return Err(PruneError::TooShallow(shown));
    }
    if home.ancestors().any(|a| same(a, root)) {
        return Err(PruneError::ContainsHome(shown));
    }
    for name in USER_FOLDERS {
        let folder = home.join(name);
        if root.ancestors().any(|a| same(a, &folder)) {
            return Err(PruneError::UserFolder(shown));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_in_the_users_documents_or_holding_the_home_are_refused() {
        let home = Path::new("/Users/me");
        let ok = |p: &str| check_root(Path::new(p), home, &same_path_ci);
        assert_eq!(ok("/Users/me/.paddock-ci/target-a-s1"), Ok(()));
        assert_eq!(ok("/Volumes/work/build"), Ok(()));
        assert_eq!(ok("/Users/me/Documentsx/build"), Ok(()), "only the exact folder names");
        assert!(matches!(ok("/Users/me/Documents/build"), Err(PruneError::UserFolder(_))));
        assert!(
            matches!(ok("/Users/me/downloads/x"), Err(PruneError::UserFolder(_))),
            "case-insensitive"
        );
        assert!(
            matches!(ok("/USERS/ME/DESKTOP"), Err(PruneError::UserFolder(_))),
            "a case variant of the whole path"
        );
        assert!(matches!(ok("/Users/me"), Err(PruneError::ContainsHome(_))));
        assert!(
            matches!(ok("/users/ME"), Err(PruneError::ContainsHome(_))),
            "a case variant of the home"
        );
        assert!(matches!(ok("/Users"), Err(PruneError::TooShallow(_))), "one level below the top");
        assert!(matches!(ok("/"), Err(PruneError::TooShallow(_))));
        assert!(matches!(ok("relative/dir"), Err(PruneError::NotAbsolute(_))));
        assert!(matches!(ok(""), Err(PruneError::NotAbsolute(_))));
        assert!(matches!(ok("/Users/me/x/../Documents"), Err(PruneError::DotDot(_))));
        assert!(matches!(
            check_root(Path::new("/a/b/c"), Path::new("/a/b/c/d"), &same_path_ci),
            Err(PruneError::ContainsHome(_))
        ));
    }

    #[test]
    fn a_comparator_of_file_identity_decides_what_is_the_same_folder() {
        // /alias is the home by identity although the names differ.
        let same = |a: &Path, b: &Path| a == b || (a == Path::new("/alias") && b == Path::new("/Users/me/Documents"));
        assert!(matches!(
            check_root(Path::new("/alias/x"), Path::new("/Users/me"), &same),
            Err(PruneError::UserFolder(_))
        ));
        assert_eq!(check_root(Path::new("/other/x"), Path::new("/Users/me"), &same), Ok(()));
    }

    #[test]
    fn path_comparison_is_per_component() {
        assert!(same_path_ci(Path::new("/A/b"), Path::new("/a/B")));
        assert!(!same_path_ci(Path::new("/a/b"), Path::new("/a")));
        assert!(!same_path_ci(Path::new("/a"), Path::new("/a/b")));
        assert!(!same_path_ci(Path::new("/a/b"), Path::new("/a/c")));
    }
}
