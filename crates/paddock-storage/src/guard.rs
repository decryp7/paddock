//! The re-check before every delete: the preview is only what the user saw; before each removal the item must still exist, still be
//! a plain file or folder of the kind the preview saw, resolve (through its real parent, so a link in the chain cannot redirect
//! it) inside its category's folder, be one the preview listed, and not have changed since. Nothing is trusted from the preview but
//! which item the user saw.

use crate::walk::{Budget, Size, size_of};
use paddock_files::path::confined;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What the preview saw of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    /// It was a folder.
    pub folder: bool,
    /// The newest modification time inside it.
    pub newest: Option<SystemTime>,
}

/// What the live file system shows now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Live {
    /// Its size now.
    pub size: Size,
}

/// Checks `path` against the live file system. `Ok(None)`: it is gone already (nothing to do). `Ok(Some(real))`: remove the real
/// path (the parent resolved). `Err(why)`: leave it alone, in words for the user. `root` must be canonical.
pub fn recheck(
    path: &Path,
    root: &Path,
    seen: Option<&Seen>,
    budget: &mut Budget,
) -> Result<Option<(PathBuf, Live)>, String> {
    if !path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(format!("{} is not a plain full path.", path.display()));
    }
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("It cannot be read: {e}")),
    };
    let Some(seen) = seen else {
        return Err("It appeared after the preview; measure again to see it.".into());
    };
    if meta.file_type().is_symlink() {
        return Err("It is now a link; links are never followed or deleted.".into());
    }
    if meta.is_dir() != seen.folder {
        return Err("It changed from a file to a folder (or back) since the preview.".into());
    }
    let real = confined(root, path)
        .map_err(|_| "It is outside the folder this category is limited to.".to_string())?;
    let size = size_of(&real, budget, &());
    if size.capped {
        return Err("It is too large to check again safely; clear fewer things at once.".into());
    }
    if size.newest > seen.newest {
        return Err("It changed after the preview; measure again to see it.".into());
    }
    Ok(Some((real, Live { size })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::tests::{standard, write};
    use std::time::Duration;

    fn seen_of(path: &Path) -> Seen {
        let size = size_of(path, &mut standard(), &());
        Seen {
            folder: path.is_dir(),
            newest: size.newest,
        }
    }

    fn root() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let r = paddock_files::path::canonical(d.path()).unwrap();
        (d, r)
    }

    #[test]
    fn an_untouched_item_inside_its_root_passes_and_gives_the_real_path() {
        let (_d, root) = root();
        write(&root.join("x/a"), 5);
        let seen = seen_of(&root.join("x"));
        let (real, live) = recheck(&root.join("x"), &root, Some(&seen), &mut standard())
            .unwrap()
            .unwrap();
        assert_eq!(real, root.join("x"));
        assert_eq!(live.size.bytes, 5);
    }

    #[test]
    fn what_is_gone_needs_nothing() {
        let (_d, root) = root();
        assert_eq!(
            recheck(&root.join("gone"), &root, None, &mut standard()),
            Ok(None)
        );
    }

    #[test]
    fn everything_that_changed_since_the_preview_is_refused_with_words() {
        let (_d, root) = root();
        write(&root.join("x/a"), 5);
        write(&root.join("f"), 5);
        let seen = seen_of(&root.join("x"));
        let check = |p: &Path, s: Option<&Seen>| recheck(p, &root, s, &mut standard()).unwrap_err();
        // Not listed by the preview.
        assert!(check(&root.join("x"), None).contains("appeared after the preview"));
        // Newer content.
        let old = Seen {
            newest: seen.newest.map(|t| t - Duration::from_secs(60)),
            ..seen
        };
        assert!(check(&root.join("x"), Some(&old)).contains("changed after the preview"));
        // A file where a folder was.
        assert!(check(&root.join("f"), Some(&seen)).contains("file to a folder"));
        // Not a full plain path.
        assert!(check(Path::new("relative"), Some(&seen)).contains("not a plain full path"));
        assert!(check(&root.join("x/../x"), Some(&seen)).contains("not a plain full path"));
        // Too large to check again.
        let mut tiny = Budget::new(
            1,
            Duration::from_secs(60),
            std::sync::Arc::new(paddock_jobs::time::SystemClock),
        );
        write(&root.join("big/a"), 1);
        write(&root.join("big/b"), 1);
        let big = seen_of(&root.join("big"));
        assert!(
            recheck(&root.join("big"), &root, Some(&big), &mut tiny)
                .unwrap_err()
                .contains("too large")
        );
    }

    #[cfg(unix)]
    #[test]
    fn what_cannot_be_read_is_refused_in_words() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, root) = root();
        write(&root.join("closed/x"), 1);
        std::fs::set_permissions(root.join("closed"), std::fs::Permissions::from_mode(0o000))
            .unwrap();
        // The superuser (the CI container) reads a closed folder: nothing to refuse there.
        let superuser = std::fs::read_dir(root.join("closed")).is_ok();
        let e = recheck(&root.join("closed/x"), &root, None, &mut standard());
        std::fs::set_permissions(root.join("closed"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        if !superuser && let Err(why) = e {
            assert!(why.starts_with("It cannot be read"), "{why}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_place_of_an_item_or_in_its_chain_is_never_followed() {
        let (_d, root) = root();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("victim/v"), 1);
        write(&root.join("x/a"), 1);
        let seen = seen_of(&root.join("x"));
        std::fs::remove_dir_all(root.join("x")).unwrap();
        assert!(
            crate::walk::tests::link_dir(outside.path(), &root.join("x")),
            "links are available here"
        );
        assert!(
            recheck(&root.join("x"), &root, Some(&seen), &mut standard())
                .unwrap_err()
                .contains("now a link")
        );
        // A link in the chain: the item's real parent is outside the root.
        let victim = root.join("x/victim");
        let victim_seen = Seen {
            folder: true,
            newest: None,
        };
        let e = recheck(&victim, &root, Some(&victim_seen), &mut standard()).unwrap_err();
        assert!(e.contains("limited to"), "{e}");
        assert!(outside.path().join("victim/v").exists());
    }
}
