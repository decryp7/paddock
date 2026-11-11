//! Deleting inside a runner's folder without ever following a link (Windows: a junction or symbolic link; Unix: a symlink).
//!
//! The service deletes with its own rights (a cleanup, a cache trim). Since ADR 0004's second amendment it works only in folders
//! only administrators can change (the guard refuses any other before a request gets here), so a user cannot plant a junction
//! where `work` or a cache folder should be. This is the second layer: [`remove_below`] walks from the folder down one component
//! at a time, refuses a link in any directory position, removes a link in the last position as a link (never entered), and
//! resolves nothing before it is looked at (no `canonicalize`).

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path};

fn refuse(p: &Path, what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{} is {what}; Pitcrew does not follow it and deleted nothing there", p.display()),
    )
}

/// A symbolic link, a junction or any other reparse point.
fn is_link(m: &std::fs::Metadata) -> bool {
    if m.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn real_dir(p: &Path) -> io::Result<()> {
    let m = std::fs::symlink_metadata(p)?;
    if is_link(&m) {
        Err(refuse(p, "a link or junction"))
    } else if !m.is_dir() {
        Err(refuse(p, "not a folder"))
    } else {
        Ok(())
    }
}

/// Removes the entry `rel` below `root` (a folder). Every directory on the way, `root` included, must be a real folder; the entry
/// itself may be a file, a folder (removed with what is in it, links inside removed as links) or a link (removed as a link). A
/// missing entry is success.
///
/// # Errors
/// [`io::ErrorKind::InvalidInput`] when `rel` is not made of plain names or a link is in the way (nothing was deleted); otherwise
/// what the OS said.
pub fn remove_below(root: &Path, rel: &Path) -> io::Result<()> {
    let mut names: Vec<&OsStr> = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(n) => names.push(n),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} is not a plain path inside the folder", rel.display()),
                ))
            }
        }
    }
    if names.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "there is nothing to delete"));
    }
    real_dir(root)?;
    let mut cur = root.to_path_buf();
    for (i, name) in names.iter().enumerate() {
        cur.push(name);
        if i + 1 < names.len() {
            match real_dir(&cur) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                other => other?,
            }
        }
    }
    match std::fs::symlink_metadata(&cur) {
        Ok(m) if is_link(&m) => std::fs::remove_file(&cur).or_else(|_| std::fs::remove_dir(&cur)),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(&cur),
        Ok(_) => std::fs::remove_file(&cur),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn scratch(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-safedel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A folder link: a symlink on Unix, a junction on Windows (no privilege needed, unlike a symbolic link).
    #[allow(
        clippy::disallowed_methods,
        reason = "a test makes a junction with the OS's own tool in its scratch folder"
    )]
    fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        }
    }

    #[test]
    fn a_plain_entry_is_removed_with_what_is_in_it() {
        let root = scratch("plain");
        std::fs::create_dir_all(root.join("work/job/deep")).unwrap();
        std::fs::write(root.join("work/job/deep/f"), b"x").unwrap();
        remove_below(&root, Path::new("work/job")).unwrap();
        assert!(root.join("work").exists() && !root.join("work/job").exists());
        remove_below(&root, Path::new("work/missing")).unwrap();
        remove_below(&root, Path::new("nothing/at/all")).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_link_in_the_way_is_never_followed_under_work() {
        let root = scratch("under-work");
        let victim = scratch("victim-work");
        std::fs::write(victim.join("precious"), b"keep").unwrap();
        std::fs::create_dir_all(victim.join("job")).unwrap();
        // `work` is a link to a folder the user wants SYSTEM to empty.
        link_dir(&victim, &root.join("work"));
        let err = remove_below(&root, Path::new("work/job")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(err.to_string().contains("does not follow"), "{err}");
        assert!(
            victim.join("precious").exists() && victim.join("job").exists(),
            "the target was not touched"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&victim);
    }

    #[test]
    fn a_link_in_the_way_is_never_followed_under_the_cache_folder() {
        let cache = scratch("under-cache");
        let victim = scratch("victim-cache");
        std::fs::write(victim.join("precious"), b"keep").unwrap();
        // A cache entry that is itself a link: removed as a link, its target stays.
        link_dir(&victim, &cache.join("entry"));
        remove_below(&cache, Path::new("entry")).unwrap();
        assert!(
            !cache.join("entry").exists() && victim.join("precious").exists(),
            "only the link went"
        );
        // The cache folder itself being a link is refused.
        let linked = scratch("linked-cache-parent").join("cache");
        link_dir(&victim, &linked);
        assert_eq!(
            remove_below(&linked, Path::new("precious")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(victim.join("precious").exists());
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&victim);
    }

    #[test]
    fn a_link_inside_the_removed_folder_is_removed_as_a_link() {
        let root = scratch("inside");
        let victim = scratch("victim-inside");
        std::fs::write(victim.join("precious"), b"keep").unwrap();
        std::fs::create_dir_all(root.join("work/job")).unwrap();
        link_dir(&victim, &root.join("work/job/trap"));
        remove_below(&root, Path::new("work/job")).unwrap();
        assert!(!root.join("work/job").exists());
        assert!(
            victim.join("precious").exists(),
            "the standard library's remove_dir_all does not enter links"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&victim);
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        let root = scratch("leave");
        for bad in ["../x", "/abs", "a/../b", ""] {
            assert_eq!(
                remove_below(&root, Path::new(bad)).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{bad:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
