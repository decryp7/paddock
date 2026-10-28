//! Who owns the files an agent with administrator rights creates.
//!
//! **The answer (macOS and Linux):** an elevated agent runs as root but must never use root's home or leave root-owned
//! files behind. The app therefore (1) makes the session folder itself, as the logged-in user, inside the user's own Dyno
//! data folder, (2) passes that folder explicitly (`run --elevated <folder> <user> <uid> <gid>`; nothing is resolved from
//! root's `HOME`), so `agent.json` is where the app and the `dyno` command line look, and (3) the agent hands everything
//! it creates in that folder to `uid:gid` ([`hand_over`]): at start, every few seconds while it runs and once more when it
//! ends. Files the user's own agent opens later are therefore the user's, and the next normal start finds nothing it cannot
//! open. Existing files are only re-owned, never rewritten. Symbolic links are never followed.
//!
//! **Windows:** the elevated agent is the same account with an elevated token, so the data lives in the same user profile;
//! the folder is passed explicitly as well. (If a standard user types an administrator's credentials at the prompt, the
//! process runs as that account, which is why nothing is resolved from the environment.)

use std::io;
use std::path::Path;

/// Most entries looked at in one pass (the session folder holds a handful of files).
const MAX_ENTRIES: usize = 10_000;
/// Deepest folder level followed.
const MAX_DEPTH: usize = 8;

/// The two file-system calls the hand-over needs; the tests inject fakes.
pub trait Owners {
    /// The `(uid, gid)` that owns `path` (not following a symbolic link).
    fn owner(&self, path: &Path) -> io::Result<(u32, u32)>;
    /// Makes `uid:gid` the owner of `path` (not following a symbolic link).
    fn chown(&self, path: &Path, uid: u32, gid: u32) -> io::Result<()>;
}

/// Hands `root` and everything below it to `uid:gid`. Returns how many entries were changed. Entries that cannot be
/// read or changed are skipped: a failed hand-over must never stop the agent.
pub fn hand_over(root: &Path, uid: u32, gid: u32, owners: &dyn Owners) -> usize {
    let mut changed = 0;
    let mut seen = 0;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((path, depth)) = stack.pop() {
        seen += 1;
        if seen > MAX_ENTRIES {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if owners.owner(&path).is_ok_and(|o| o != (uid, gid)) && owners.chown(&path, uid, gid).is_ok() {
            changed += 1;
        }
        if meta.is_dir() && depth < MAX_DEPTH {
            if let Ok(entries) = std::fs::read_dir(&path) {
                stack.extend(entries.flatten().map(|e| (e.path(), depth + 1)));
            }
        }
    }
    changed
}

/// The real file system.
#[cfg(unix)]
pub struct OsOwners;

#[cfg(unix)]
impl Owners for OsOwners {
    fn owner(&self, path: &Path) -> io::Result<(u32, u32)> {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).map(|m| (m.uid(), m.gid()))
    }

    #[allow(unsafe_code)]
    fn chown(&self, path: &Path, uid: u32, gid: u32) -> io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
        // SAFETY: `c` is a NUL-terminated path that lives through the call; lchown does not follow a final symbolic link.
        if unsafe { libc::lchown(c.as_ptr(), uid, gid) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;

    /// Everything is owned by root (0:0) until chowned; records the calls.
    #[derive(Default)]
    struct Fake {
        done: RefCell<HashSet<std::path::PathBuf>>,
        calls: RefCell<Vec<(std::path::PathBuf, u32, u32)>>,
        refuse: bool,
    }

    impl Owners for Fake {
        fn owner(&self, path: &Path) -> io::Result<(u32, u32)> {
            Ok(if self.done.borrow().contains(path) { (501, 20) } else { (0, 0) })
        }
        fn chown(&self, path: &Path, uid: u32, gid: u32) -> io::Result<()> {
            if self.refuse {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            self.done.borrow_mut().insert(path.to_path_buf());
            self.calls.borrow_mut().push((path.to_path_buf(), uid, gid));
            Ok(())
        }
    }

    #[test]
    fn everything_below_the_folder_is_handed_to_the_user_once() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("logs")).unwrap();
        for f in ["dyno.db", "dyno.db-wal", "dyno.db-shm", "agent.json", "logs/agent.log"] {
            std::fs::write(tmp.path().join(f), b"x").unwrap();
        }
        let fake = Fake::default();
        let n = hand_over(tmp.path(), 501, 20, &fake);
        assert_eq!(n, 7, "the folder, the logs folder and five files");
        assert!(fake.calls.borrow().iter().all(|(_, u, g)| (*u, *g) == (501, 20)));
        assert_eq!(hand_over(tmp.path(), 501, 20, &fake), 0, "files already theirs are left alone");
        // A file created later is picked up by the next pass.
        std::fs::write(tmp.path().join("settings.json"), b"{}").unwrap();
        assert_eq!(hand_over(tmp.path(), 501, 20, &fake), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_never_followed_and_failures_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();
        let fake = Fake::default();
        assert_eq!(hand_over(tmp.path(), 501, 20, &fake), 1, "only the folder itself");
        assert!(fake
            .calls
            .borrow()
            .iter()
            .all(|(p, _, _)| !p.starts_with(outside.path()) && !p.ends_with("link")));
        let refusing = Fake {
            refuse: true,
            ..Fake::default()
        };
        assert_eq!(hand_over(tmp.path(), 501, 20, &refusing), 0);
        assert_eq!(hand_over(&tmp.path().join("missing"), 501, 20, &fake), 0);
    }

    #[cfg(unix)]
    #[test]
    fn the_real_calls_report_this_users_ownership_and_a_no_op_chown_works() {
        use std::os::unix::fs::MetadataExt;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        let meta = std::fs::metadata(&f).unwrap();
        let (uid, gid) = (meta.uid(), meta.gid());
        assert_eq!(OsOwners.owner(&f).unwrap(), (uid, gid));
        OsOwners.chown(&f, uid, gid).unwrap();
        assert_eq!(hand_over(tmp.path(), uid, gid, &OsOwners), 0, "already the user's");
        assert!(OsOwners.chown(&tmp.path().join("missing"), uid, gid).is_err());
    }
}
