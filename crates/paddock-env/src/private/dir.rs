//! [`PrivateDir`] and [`PrivateFile`]: the one way a trust-bearing folder or file is made and used (ADR 0092 section 1).
//!
//! The first step opens the folder by path; every later step is relative to the held handle, so an ancestor renamed, replaced or
//! turned into a link afterwards changes which path names the folder, not what the handle refers to. Folders and files are
//! owner-only from the moment they exist (no narrowing call afterwards), the folder and its parent are checked from handles, and
//! an existing folder open to others is refused with a message that says what to fix: it is never tightened, because tightening
//! cannot evict what was planted while it was open.
//!
//! **Why not cap-std (ADR 0092 section 1 asked for a one-day spike first).** The spike was skipped on reading: `cap-std`'s
//! `DirBuilder` has no Windows option for a security descriptor, so it cannot create a Windows folder owner-only in the
//! creating call, which is the point of the type; and its Unix side is the same handful of `openat` calls used here. The
//! type therefore calls `libc` (`mkdirat`, `openat`, `renameat`, `fchmod`) and `NtCreateFile` itself; `windows-sys` is
//! already in the workspace hack.
//!
//! **Logging.** [`PrivateError`]'s text carries the path (and the fix-it sentence). Adopters log
//! [`PrivateError::reason`], which names no path, and show the full text to the user (ER-27).

use super::error::{Fail, PrivateError, PrivateResult, at};
use super::sys::{self, Dir};
use crate::folder::Links;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Who owns what is trusted: a user id on Unix, a SID in text form on Windows. Only the test seams
/// (`create_all_as`, `open_as`) take one.
#[cfg(any(test, feature = "test-support"))]
pub type AccountId = sys::Account;

/// A held, checked, owner-only folder.
#[derive(Debug)]
pub struct PrivateDir {
    dir: Dir,
    path: PathBuf,
    who: sys::Account,
}

/// A held, checked, owner-only file, opened through its [`PrivateDir`].
#[derive(Debug)]
pub struct PrivateFile {
    file: File,
    path: PathBuf,
}

/// One plain name: not empty, not `.` or `..`, no separator or NUL.
fn plain(name: &str) -> PrivateResult<&OsStr> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || (cfg!(windows) && name.contains([':', '*', '?', '"', '<', '>', '|']));
    if bad {
        Err(PrivateError::BadName(name.to_owned()))
    } else {
        Ok(OsStr::new(name))
    }
}

fn io_err(op: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> PrivateError {
    let path = path.to_path_buf();
    move |source| PrivateError::Io { op, path, source }
}

fn is_missing(e: &super::error::Fail) -> bool {
    matches!(e, super::error::Fail::Io(e) if e.kind() == std::io::ErrorKind::NotFound)
}

/// The folder `name` in `parent`: opened if it exists, made owner-only if it does not. When another creator makes it between
/// the two steps, the make reports "already exists" and the folder is opened once more, to be checked like any existing one.
fn open_or_make(parent: &Dir, name: &OsStr, path: &Path) -> PrivateResult<Dir> {
    match parent.open_at(name) {
        Ok(existing) => Ok(existing),
        Err(e) if is_missing(&e) => match parent.mkdir_at(name) {
            Ok(made) => Ok(made),
            Err(Fail::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                parent.open_at(name).map_err(at("open the folder", path))
            }
            Err(e) => Err(at("create folder", path)(e)),
        },
        Err(e) => Err(at("open the folder", path)(e)),
    }
}

impl PrivateDir {
    /// Makes `path` and any missing parents owner-only and returns the held folder. The deepest existing ancestor is opened by path
    /// (links in it are followed: `/var` may be a link); every missing folder is made relative to the one before, owner-only at
    /// creation. An existing final folder that passes the check is reused as it is; a link in its place, another owner, a write
    /// right for others and an untrusted parent are refused and nothing is changed. Two callers making the same new folder at
    /// once both succeed: the loser of the race opens what the winner made and checks it like any existing folder.
    ///
    /// # Errors
    /// [`PrivateError::Refused`] for an untrusted folder or parent, [`PrivateError::Io`] when a step fails,
    /// [`PrivateError::NotAbsolute`] for a relative path.
    pub fn create_all(path: &Path) -> PrivateResult<PrivateDir> {
        let who = sys::current_account().map_err(io_err("find the account", path))?;
        Self::create_all_for(path, who)
    }

    /// [`PrivateDir::create_all`] for the account `who` (tests name another account to meet a folder it does not own).
    ///
    /// # Errors
    /// As [`PrivateDir::create_all`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn create_all_as(path: &Path, who: AccountId) -> PrivateResult<PrivateDir> {
        Self::create_all_for(path, who)
    }

    fn create_all_for(path: &Path, who: sys::Account) -> PrivateResult<PrivateDir> {
        if !path.is_absolute() {
            return Err(PrivateError::NotAbsolute(path.to_path_buf()));
        }
        // The deepest existing proper ancestor, by path; the names below it are made relative to it.
        let mut missing: Vec<&OsStr> = Vec::new();
        let mut cur = path;
        let mut dir = loop {
            let Some(parent) = cur.parent() else {
                return Err(io_err("find an existing folder above", path)(
                    std::io::ErrorKind::NotFound.into(),
                ));
            };
            if let Some(name) = cur.file_name() {
                missing.push(name);
            }
            cur = parent;
            match Dir::open_path(cur, true) {
                Ok(d) => break d,
                Err(e) if is_missing(&e) => {}
                Err(e) => return Err(at("open the folder", cur)(e)),
            }
        };
        let mut here = cur.to_path_buf();
        for name in missing.into_iter().rev() {
            here.push(name);
            dir = open_or_make(&dir, name, &here)?;
        }
        let made = PrivateDir {
            dir,
            path: path.to_path_buf(),
            who,
        };
        made.verify()?;
        Ok(made)
    }

    /// Opens an existing folder by path and checks it and its parent from the handle. With [`Links::Follow`] a link in the last
    /// place is followed and the folder it leads to is checked; with [`Links::Refuse`] it is refused.
    ///
    /// # Errors
    /// [`PrivateError::Refused`] when the folder or its parent cannot be trusted, [`PrivateError::Io`] when it cannot be opened.
    pub fn open(path: &Path, links: Links) -> PrivateResult<PrivateDir> {
        let who = sys::current_account().map_err(io_err("find the account", path))?;
        Self::open_for(path, links, who)
    }

    /// [`PrivateDir::open`] for the account `who`.
    ///
    /// # Errors
    /// As [`PrivateDir::open`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_as(path: &Path, links: Links, who: AccountId) -> PrivateResult<PrivateDir> {
        Self::open_for(path, links, who)
    }

    fn open_for(path: &Path, links: Links, who: sys::Account) -> PrivateResult<PrivateDir> {
        let dir =
            Dir::open_path(path, links == Links::Follow).map_err(at("open the folder", path))?;
        let opened = PrivateDir {
            dir,
            path: path.to_path_buf(),
            who,
        };
        opened.verify()?;
        Ok(opened)
    }

    fn verify(&self) -> PrivateResult<()> {
        self.dir
            .check(&self.who)
            .map_err(at("check the folder", &self.path))?;
        self.dir
            .check_parent(&self.who)
            .map_err(at("check the folder", &self.path))
    }

    /// The path this folder was opened by (for messages; every operation uses the handle).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The folder `name` inside this one, made owner-only if it is missing and checked if it exists.
    ///
    /// # Errors
    /// [`PrivateError::BadName`], [`PrivateError::Refused`] for an untrusted existing folder, [`PrivateError::Io`].
    pub fn child(&self, name: &str) -> PrivateResult<PrivateDir> {
        let os = plain(name)?;
        let path = self.path.join(name);
        let dir = open_or_make(&self.dir, os, &path)?;
        dir.check(&self.who)
            .map_err(at("check the folder", &path))?;
        // The account is a Copy user id on Unix and a SID string on Windows.
        #[allow(clippy::clone_on_copy)]
        let who = self.who.clone();
        Ok(PrivateDir { dir, path, who })
    }

    /// Writes `bytes` to `name` atomically: a uniquely named temporary file is created exclusively and owner-only in this
    /// folder, written, flushed, then renamed over `name`, all relative to the held handle; a link planted at the temporary name is
    /// never followed.
    ///
    /// # Errors
    /// [`PrivateError::BadName`] or [`PrivateError::Io`]; the temporary file is removed on failure.
    pub fn write_atomic(&self, name: &str, bytes: &[u8]) -> PrivateResult<()> {
        let os = plain(name)?;
        let path = self.path.join(name);
        let (tmp, mut file) = self.create_temp(name, &path)?;
        let tmp_os = OsStr::new(&tmp);
        let result = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(io_err("write", &path))
            .and_then(|()| {
                self.dir
                    .rename(&file, tmp_os, os)
                    .map_err(io_err("replace", &path))
            })
            .and_then(|()| {
                self.dir
                    .sync()
                    .map_err(io_err("flush the folder of", &path))
            });
        drop(file);
        if result.is_err() {
            // Best effort: the failure being reported is the first one.
            let _ = self.dir.unlink(tmp_os);
        }
        result
    }

    /// A temporary file name for `name`, created exclusively (a taken name is retried with another).
    fn create_temp(&self, name: &str, path: &Path) -> PrivateResult<(String, File)> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        self.create_temp_with(name, path, || COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// [`PrivateDir::create_temp`] with the source of the unique part passed in (tests make every name collide).
    fn create_temp_with(
        &self,
        name: &str,
        path: &Path,
        mut next: impl FnMut() -> u64,
    ) -> PrivateResult<(String, File)> {
        let mut last = None;
        for _ in 0..8 {
            let n = next();
            let tmp = format!(".{name}.{}-{n}.tmp", std::process::id());
            match self.dir.create_file_at(OsStr::new(&tmp)) {
                Ok(file) => return Ok((tmp, file)),
                Err(super::error::Fail::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    last = Some(e)
                }
                Err(e) => return Err(at("create the temporary file for", path)(e)),
            }
        }
        Err(io_err("create the temporary file for", path)(
            last.unwrap_or_else(|| std::io::ErrorKind::AlreadyExists.into()),
        ))
    }

    /// Opens the file `name` for reading, refusing a link, a non-file, another owner and any group or other access.
    ///
    /// # Errors
    /// [`PrivateError::BadName`], [`PrivateError::Refused`] for an untrusted file, [`PrivateError::Io`] (a missing file is
    /// [`std::io::ErrorKind::NotFound`]).
    pub fn open_file(&self, name: &str) -> PrivateResult<PrivateFile> {
        let os = plain(name)?;
        let path = self.path.join(name);
        let file = self
            .dir
            .open_file_at(os, &self.who)
            .map_err(at("open", &path))?;
        Ok(PrivateFile { file, path })
    }

    /// Reads the whole file `name`, at most `max_bytes`.
    ///
    /// # Errors
    /// As [`PrivateDir::open_file`], and [`PrivateError::TooLarge`].
    pub fn read(&self, name: &str, max_bytes: u64) -> PrivateResult<Vec<u8>> {
        self.open_file(name)?.read_to_vec(max_bytes)
    }

    /// Removes the file `name`.
    ///
    /// # Errors
    /// [`PrivateError::BadName`] or [`PrivateError::Io`].
    pub fn remove_file(&self, name: &str) -> PrivateResult<()> {
        let os = plain(name)?;
        self.dir
            .unlink(os)
            .map_err(io_err("remove", &self.path.join(name)))
    }
}

impl PrivateFile {
    /// The path the file was opened by (for messages).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The open file, which is the checked object.
    pub fn as_file(&self) -> &File {
        &self.file
    }

    /// Gives up the type and returns the open file.
    pub fn into_file(self) -> File {
        self.file
    }

    /// Reads to the end, at most `max_bytes`.
    ///
    /// # Errors
    /// [`PrivateError::TooLarge`] when the file is longer, [`PrivateError::Io`].
    pub fn read_to_vec(mut self, max_bytes: u64) -> PrivateResult<Vec<u8>> {
        let mut out = Vec::new();
        (&mut self.file)
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut out)
            .map_err(io_err("read", &self.path))?;
        if out.len() as u64 > max_bytes {
            return Err(PrivateError::TooLarge {
                path: self.path,
                limit: max_bytes,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod common_tests {
    use super::*;
    use crate::folder::Refusal;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn refusal(e: PrivateError) -> Refusal {
        let PrivateError::Refused { why, .. } = e else {
            panic!("{e}")
        };
        why
    }

    /// Two creators racing to make the same new folder must both end up holding it (review P1-1).
    #[test]
    fn concurrent_create_all_of_the_same_new_folder_all_succeed() {
        use std::sync::{Arc, Barrier};
        let t = temp();
        for round in 0..100 {
            let path = Arc::new(t.path().join(format!("r{round}")).join("a").join("b"));
            let barrier = Arc::new(Barrier::new(4));
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    let (path, barrier) = (path.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        PrivateDir::create_all(&path).map(|_| ())
                    })
                })
                .collect();
            for th in threads {
                th.join().unwrap().unwrap();
            }
        }
    }

    #[test]
    fn concurrent_child_of_the_same_new_folder_all_succeed() {
        use std::sync::{Arc, Barrier};
        let t = temp();
        let parent = Arc::new(PrivateDir::create_all(&t.path().join("p")).unwrap());
        for round in 0..100 {
            let barrier = Arc::new(Barrier::new(4));
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    let (parent, barrier) = (parent.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        parent.child(&format!("c{round}")).map(|_| ())
                    })
                })
                .collect();
            for th in threads {
                th.join().unwrap().unwrap();
            }
        }
    }

    #[test]
    fn names_must_be_plain() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        for bad in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(
                matches!(d.write_atomic(bad, b""), Err(PrivateError::BadName(_))),
                "{bad:?}"
            );
            assert!(matches!(d.open_file(bad), Err(PrivateError::BadName(_))));
            assert!(matches!(d.child(bad), Err(PrivateError::BadName(_))));
            assert!(matches!(d.remove_file(bad), Err(PrivateError::BadName(_))));
        }
        assert!(d.write_atomic("a b.json", b"").is_ok());
        assert!(
            PrivateError::BadName("x".into())
                .to_string()
                .contains("plain")
        );
    }

    #[test]
    fn remove_file_removes_and_reports_a_missing_one() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        d.write_atomic("f", b"x").unwrap();
        d.remove_file("f").unwrap();
        assert!(matches!(
            d.remove_file("f").unwrap_err(),
            PrivateError::Io { .. }
        ));
    }

    #[test]
    fn io_and_refusal_errors_display_with_their_path() {
        let e = PrivateError::Io {
            op: "write",
            path: PathBuf::from("/x"),
            source: std::io::ErrorKind::Other.into(),
        };
        assert!(e.to_string().starts_with("could not write /x"));
        assert!(std::error::Error::source(&e).is_some());
        assert!(std::error::Error::source(&PrivateError::BadName(String::new())).is_none());
        assert!(
            PrivateError::NotAbsolute(PathBuf::from("r"))
                .to_string()
                .contains("absolute")
        );
    }

    #[test]
    fn child_makes_missing_folders_owner_only_and_checks_existing_ones() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        let c = d.child("c").unwrap();
        c.write_atomic("f", b"x").unwrap();
        assert_eq!(d.child("c").unwrap().read("f", 5).unwrap(), b"x");
        std::fs::write(d.path().join("file"), b"x").unwrap();
        assert_eq!(refusal(d.child("file").unwrap_err()), Refusal::NotAFolder);
    }

    #[test]
    fn write_atomic_replaces_and_leaves_no_temporary_file() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        d.write_atomic("f", b"one").unwrap();
        d.write_atomic("f", b"two").unwrap();
        assert_eq!(d.read("f", 10).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![OsStr::new("f")], "no temporary file is left");
    }

    #[test]
    fn a_relative_path_is_refused() {
        assert!(matches!(
            PrivateDir::create_all(Path::new("rel/dir")),
            Err(PrivateError::NotAbsolute(_))
        ));
    }

    #[test]
    fn a_file_in_place_of_a_folder_is_refused() {
        let t = temp();
        let f = t.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(
            refusal(PrivateDir::open(&f, Links::Refuse).unwrap_err()),
            Refusal::NotAFolder
        );
        assert_eq!(
            refusal(PrivateDir::create_all(&f).unwrap_err()),
            Refusal::NotAFolder
        );
    }

    #[test]
    fn reading_is_bounded() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        d.write_atomic("f", b"12345").unwrap();
        assert_eq!(d.read("f", 5).unwrap(), b"12345");
        let e = d.read("f", 4).unwrap_err();
        assert!(matches!(e, PrivateError::TooLarge { limit: 4, .. }));
        assert!(e.to_string().contains("larger than 4"));
        let f = d.open_file("f").unwrap();
        assert!(f.path().ends_with("f"));
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f.into_file(), &mut s).unwrap();
        assert_eq!(s, "12345");
        assert!(
            matches!(d.open_file("none").unwrap_err(), PrivateError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::folder::Refusal;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn mode_of(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    fn set_mode(p: &Path, mode: u32) {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn refusal(e: PrivateError) -> Refusal {
        let PrivateError::Refused { why, .. } = e else {
            panic!("{e}")
        };
        why
    }

    /// Failing test 6 (ADR 0092): a folder made under umask 002 is owner-only from its first fstat, with no narrowing call after.
    #[test]
    fn a_folder_made_under_a_loose_umask_is_owner_only_from_creation() {
        let t = temp();
        // SAFETY: the umask is process-wide; the value is restored below and the test only reads modes it just set.
        let old = unsafe { libc::umask(0o002) };
        let made = PrivateDir::create_all(&t.path().join("a").join("b"))
            .and_then(|d| d.write_atomic("f", b"x").map(|()| d));
        // SAFETY: restores the value read above.
        unsafe { libc::umask(old) };
        let made = made.unwrap();
        assert_eq!(mode_of(made.path()), 0o700);
        assert_eq!(mode_of(&t.path().join("a")), 0o700);
        assert_eq!(mode_of(&t.path().join("a/b/f")), 0o600);
    }

    #[test]
    fn an_existing_trusted_folder_is_reused_and_its_contents_kept() {
        let t = temp();
        let p = t.path().join("d");
        PrivateDir::create_all(&p)
            .unwrap()
            .write_atomic("f", b"one")
            .unwrap();
        let again = PrivateDir::create_all(&p).unwrap();
        assert_eq!(again.read("f", 10).unwrap(), b"one");
        let opened = PrivateDir::open(&p, Links::Refuse).unwrap();
        assert_eq!(opened.path(), p);
        assert_eq!(opened.read("f", 10).unwrap(), b"one");
    }

    /// Failing test 3, the type's half: an own folder with mode 0777 is refused and left as it was.
    #[test]
    fn a_world_writable_folder_is_refused_and_its_mode_is_not_changed() {
        let t = temp();
        for mode in [0o777, 0o770, 0o707, 0o772] {
            let p = t.path().join(format!("d{mode:o}"));
            std::fs::create_dir(&p).unwrap();
            set_mode(&p, mode);
            for result in [
                PrivateDir::create_all(&p),
                PrivateDir::open(&p, Links::Refuse),
            ] {
                assert!(
                    matches!(refusal(result.unwrap_err()), Refusal::OpenToOthers { .. }),
                    "{mode:o}"
                );
            }
            assert_eq!(mode_of(&p), mode, "never tightened");
            assert_eq!(std::fs::read_dir(&p).unwrap().count(), 0, "nothing written");
        }
    }

    #[test]
    fn a_folder_others_can_only_read_is_kept() {
        let t = temp();
        let p = t.path().join("d");
        std::fs::create_dir(&p).unwrap();
        set_mode(&p, 0o755);
        PrivateDir::open(&p, Links::Refuse).unwrap();
    }

    #[test]
    fn the_refusal_message_says_what_to_fix() {
        let t = temp();
        let p = t.path().join("d");
        std::fs::create_dir(&p).unwrap();
        set_mode(&p, 0o777);
        let e = PrivateDir::open(&p, Links::Refuse).unwrap_err();
        let text = e.to_string();
        assert!(
            text.contains("chmod go-w") && text.contains("data-folder variable"),
            "{text}"
        );
        assert!(text.contains(&p.display().to_string()), "{text}");
        assert!(std::error::Error::source(&e).is_some());
    }

    #[test]
    fn a_folder_of_another_owner_is_refused() {
        let t = temp();
        let p = t.path().join("d");
        std::fs::create_dir(&p).unwrap();
        let other = sys::current_account().unwrap() + 1;
        let r = refusal(PrivateDir::open_as(&p, Links::Refuse, other).unwrap_err());
        assert!(matches!(r, Refusal::OtherOwner { .. }), "{r:?}");
        let r = refusal(PrivateDir::create_all_as(&p, other).unwrap_err());
        assert!(matches!(r, Refusal::OtherOwner { .. }), "{r:?}");
    }

    #[test]
    fn a_link_in_place_of_the_folder_is_refused_and_its_target_untouched() {
        let t = temp();
        let target = t.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        let link = t.path().join("d");
        symlink(&target, &link).unwrap();
        for result in [
            PrivateDir::open(&link, Links::Refuse),
            PrivateDir::create_all(&link),
        ] {
            assert_eq!(refusal(result.unwrap_err()), Refusal::Symlink);
        }
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn a_link_is_followed_only_when_asked_and_its_target_is_checked() {
        let t = temp();
        let good = t.path().join("good");
        let bad = t.path().join("bad");
        std::fs::create_dir(&good).unwrap();
        std::fs::create_dir(&bad).unwrap();
        set_mode(&bad, 0o777);
        let (to_good, to_bad) = (t.path().join("l1"), t.path().join("l2"));
        symlink(&good, &to_good).unwrap();
        symlink(&bad, &to_bad).unwrap();
        PrivateDir::open(&to_good, Links::Follow).unwrap();
        assert!(matches!(
            refusal(PrivateDir::open(&to_bad, Links::Follow).unwrap_err()),
            Refusal::OpenToOthers { .. }
        ));
    }

    #[test]
    fn a_file_in_place_of_a_folder_is_refused() {
        let t = temp();
        let f = t.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(
            refusal(PrivateDir::open(&f, Links::Refuse).unwrap_err()),
            Refusal::NotAFolder
        );
        assert_eq!(
            refusal(PrivateDir::create_all(&f).unwrap_err()),
            Refusal::NotAFolder
        );
        // A link to a file, followed, is also not a folder.
        let l = t.path().join("l");
        symlink(&f, &l).unwrap();
        assert_eq!(
            refusal(PrivateDir::open(&l, Links::Follow).unwrap_err()),
            Refusal::NotAFolder
        );
        // A file in the middle of the path.
        let e = PrivateDir::create_all(&f.join("sub").join("deeper")).unwrap_err();
        assert_eq!(refusal(e), Refusal::NotAFolder);
    }

    /// Failing test 7, Unix half: an ancestor replaced by a link after the handle is held changes the name, not the folder.
    #[test]
    fn an_ancestor_replaced_after_the_handle_is_held_cannot_redirect_a_write() {
        let t = temp();
        let outer = t.path().join("outer");
        let held = PrivateDir::create_all(&outer.join("inner")).unwrap();
        let attacker = t.path().join("attacker");
        std::fs::create_dir(&attacker).unwrap();
        std::fs::rename(&outer, t.path().join("moved")).unwrap();
        symlink(&attacker, &outer).unwrap();
        held.write_atomic("secret", b"s").unwrap();
        assert_eq!(
            std::fs::read_dir(&attacker).unwrap().count(),
            0,
            "nothing landed at the link's target"
        );
        assert_eq!(
            std::fs::read(t.path().join("moved/inner/secret")).unwrap(),
            b"s"
        );
    }

    #[test]
    fn a_parent_others_can_change_is_refused() {
        let t = temp();
        let parent = t.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let p = parent.join("d");
        PrivateDir::create_all(&p).unwrap();
        for mode in [0o777, 0o770, 0o707] {
            set_mode(&parent, mode);
            let e = PrivateDir::open(&p, Links::Refuse).unwrap_err();
            let r = refusal(e);
            assert!(
                matches!(&r, Refusal::Parent(inner) if matches!(**inner, Refusal::OpenToOthers { .. })),
                "{mode:o}: {r:?}"
            );
            assert!(r.to_string().contains("parent folder"));
            assert!(PrivateDir::create_all(&p).is_err(), "{mode:o}");
        }
        // Sticky: others cannot rename entries they do not own, like /tmp.
        set_mode(&parent, 0o1777);
        PrivateDir::open(&p, Links::Refuse).unwrap();
    }

    #[test]
    fn the_parent_judgement_is_pure() {
        use crate::folder::FolderFacts;
        let f = |uid, mode| FolderFacts { uid, mode };
        assert_eq!(sys::parent_decision(f(5, 0o755), 5), Ok(()));
        assert_eq!(
            sys::parent_decision(f(0, 0o755), 5),
            Ok(()),
            "root's folder"
        );
        assert_eq!(sys::parent_decision(f(0, 0o1777), 5), Ok(()), "/tmp");
        assert!(matches!(
            sys::parent_decision(f(0, 0o775), 5),
            Err(Refusal::OpenToOthers { .. })
        ));
        assert!(matches!(
            sys::parent_decision(f(9, 0o755), 5),
            Err(Refusal::OtherOwner { uid: 9 })
        ));
        assert_eq!(sys::file_decision(f(5, 0o600), 5), Ok(()));
        assert!(matches!(
            sys::file_decision(f(5, 0o640), 5),
            Err(Refusal::OpenToOthers { .. })
        ));
        assert!(matches!(
            sys::file_decision(f(5, 0o604), 5),
            Err(Refusal::OpenToOthers { .. })
        ));
        assert!(matches!(
            sys::file_decision(f(6, 0o600), 5),
            Err(Refusal::OtherOwner { uid: 6 })
        ));
    }

    #[test]
    fn a_relative_path_and_the_root_are_refused() {
        assert!(matches!(
            PrivateDir::create_all(Path::new("rel/dir")),
            Err(PrivateError::NotAbsolute(_))
        ));
        let e = PrivateDir::create_all(Path::new("/")).unwrap_err();
        assert!(matches!(e, PrivateError::Io { .. }), "{e}");
        assert!(
            e.to_string()
                .contains("could not find an existing folder above")
        );
    }

    #[test]
    fn write_atomic_replaces_leaves_no_temp_file_and_is_owner_only() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        d.write_atomic("f", b"one").unwrap();
        d.write_atomic("f", b"two").unwrap();
        assert_eq!(d.read("f", 10).unwrap(), b"two");
        assert_eq!(mode_of(&d.path().join("f")), 0o600);
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![OsStr::new("f")], "no temporary file is left");
    }

    /// Failing test 4's shape, in the type: a link planted at the final name is replaced, never written through.
    #[test]
    fn a_link_planted_at_the_target_name_is_replaced_not_followed() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        let victim = t.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        symlink(&victim, d.path().join("f")).unwrap();
        d.write_atomic("f", b"new").unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        assert_eq!(d.read("f", 10).unwrap(), b"new");
    }

    #[test]
    fn a_link_planted_at_the_temporary_name_is_never_followed() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        let victim = t.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        let pid = std::process::id();
        symlink(&victim, d.path().join(format!(".f.{pid}-0.tmp"))).unwrap();
        // Every attempt meets the planted name: the exclusive create fails, it does not write through the link.
        let e = d
            .create_temp_with("f", d.path(), || 0)
            .map(|_| ())
            .unwrap_err();
        assert!(
            matches!(&e, PrivateError::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists),
            "{e}"
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        // A taken name is retried with the next one.
        let mut n = 0;
        let (name, _file) = d
            .create_temp_with("f", d.path(), || {
                n += 1;
                n - 1
            })
            .unwrap();
        assert!(name.ends_with("-1.tmp"), "{name}");
    }

    #[test]
    fn a_failed_replace_reports_and_removes_the_temporary_file() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        std::fs::create_dir(d.path().join("f")).unwrap();
        let e = d.write_atomic("f", b"x").unwrap_err();
        assert!(matches!(&e, PrivateError::Io { op: "replace", .. }), "{e}");
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            vec![OsStr::new("f")],
            "the temporary file is removed"
        );
    }

    /// Works as root too (CI runs as root, ER-12): the folder is removed under the held handle, so every create in it fails.
    #[test]
    fn a_folder_that_cannot_be_written_is_an_error() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        std::fs::remove_dir(d.path()).unwrap();
        let e = d.write_atomic("f", b"x").unwrap_err();
        let op = "create the temporary file for";
        assert!(
            matches!(&e, PrivateError::Io { op: o, .. } if *o == op),
            "{e}"
        );
        let e = d.child("c").unwrap_err();
        assert!(
            matches!(
                &e,
                PrivateError::Io {
                    op: "create folder",
                    ..
                }
            ),
            "{e}"
        );
    }

    /// Works as root too: a name longer than the OS allows fails the open with an I/O error, not a refusal.
    #[test]
    fn an_ancestor_that_cannot_be_opened_is_an_error_not_a_refusal() {
        let t = temp();
        let long = "x".repeat(300);
        let e = PrivateDir::create_all(&t.path().join(&long).join("a").join("b")).unwrap_err();
        assert!(
            matches!(
                &e,
                PrivateError::Io {
                    op: "open the folder",
                    ..
                }
            ),
            "{e}"
        );
        let e = PrivateDir::open(&t.path().join(&long), Links::Refuse).unwrap_err();
        assert!(matches!(&e, PrivateError::Io { .. }), "{e}");
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        let e = d.child(&long).unwrap_err();
        assert!(matches!(&e, PrivateError::Io { .. }), "{e}");
    }

    /// Review P2-2: the check of an existing child (the mutation that removed it survived).
    #[test]
    fn an_existing_child_open_to_others_is_refused_and_left_alone() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        d.child("c").unwrap();
        for mode in [0o777, 0o770, 0o707] {
            set_mode(&d.path().join("c"), mode);
            let why = refusal(d.child("c").unwrap_err());
            assert!(matches!(why, Refusal::OpenToOthers { .. }), "{mode:o}");
            assert_eq!(mode_of(&d.path().join("c")), mode, "never tightened");
        }
    }

    #[test]
    fn a_file_others_can_read_or_that_is_not_ours_or_not_a_file_is_refused() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"x").unwrap();
        for mode in [0o644, 0o640, 0o604, 0o666] {
            set_mode(&p, mode);
            let why = refusal(d.open_file("f").unwrap_err());
            assert!(matches!(why, Refusal::OpenToOthers { .. }), "{mode:o}");
        }
        set_mode(&p, 0o600);
        assert!(d.open_file("f").is_ok());
        let other = PrivateDir {
            dir: Dir::open_path(d.path(), false).unwrap(),
            path: d.path().to_path_buf(),
            who: sys::current_account().unwrap() + 1,
        };
        assert!(matches!(
            refusal(other.open_file("f").unwrap_err()),
            Refusal::OtherOwner { .. }
        ));
        std::fs::create_dir(d.path().join("sub")).unwrap();
        assert_eq!(refusal(d.open_file("sub").unwrap_err()), Refusal::NotAFile);
        symlink(&p, d.path().join("l")).unwrap();
        assert_eq!(refusal(d.open_file("l").unwrap_err()), Refusal::Symlink);
        let fifo = std::ffi::CString::new(d.path().join("pipe").to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path in the test's own folder.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(
            refusal(d.open_file("pipe").unwrap_err()),
            Refusal::NotAFile,
            "a planted pipe does not block"
        );
        assert!(
            matches!(d.open_file("none").unwrap_err(), PrivateError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn read_is_bounded() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        d.write_atomic("f", b"12345").unwrap();
        assert_eq!(d.read("f", 5).unwrap(), b"12345");
        let e = d.read("f", 4).unwrap_err();
        assert!(matches!(e, PrivateError::TooLarge { limit: 4, .. }));
        assert!(e.to_string().contains("larger than 4"));
        let f = d.open_file("f").unwrap();
        assert!(f.path().ends_with("f"));
        assert_eq!(
            f.as_file().metadata().unwrap().uid(),
            sys::current_account().unwrap()
        );
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f.into_file(), &mut s).unwrap();
        assert_eq!(s, "12345");
    }

    /// Where the mode bits show nothing: a macOS extended ACL that lets another principal in is refused.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_macos_acl_entry_for_another_principal_is_refused() {
        let t = temp();
        let p = t.path().join("d");
        std::fs::create_dir(&p).unwrap();
        let status = std::process::Command::new("/bin/chmod")
            .args([
                "+a",
                "everyone allow list,add_file,delete_child",
                p.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(matches!(
            refusal(PrivateDir::open(&p, Links::Refuse).unwrap_err()),
            Refusal::OpenToOthersAcl { .. }
        ));
        let d = t.path().join("ok");
        let made = PrivateDir::create_all(&d).unwrap();
        made.write_atomic("f", b"x").unwrap();
        let f = d.join("f");
        let status = std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone allow read", f.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(matches!(
            refusal(made.open_file("f").unwrap_err()),
            Refusal::OpenToOthersAcl { .. }
        ));
        // An entry that only denies takes nothing away from owner-only.
        let g = d.join("g");
        made.write_atomic("g", b"x").unwrap();
        let status = std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone deny delete", g.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(made.open_file("g").is_ok());
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::super::acl::{Ace, AceKind, Who};
    use super::*;
    use crate::folder::Refusal;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::process::Command;

    const EVERYONE: &str = "S-1-1-0";
    const AUTHENTICATED_USERS: &str = "S-1-5-11";
    const NOT_CONTENT_INDEXED: u32 = 0x2000;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn refusal(e: PrivateError) -> Refusal {
        let PrivateError::Refused { why, .. } = e else {
            panic!("{e}")
        };
        why
    }

    fn me() -> AccountId {
        sys::current_account().unwrap()
    }

    /// Runs `icacls` on a folder or file of the test's own temporary tree.
    fn icacls(path: &Path, args: &[&str]) {
        let out = Command::new("icacls")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "icacls {args:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    fn allows_only_me_and_system(owner: &Who, aces: &[Ace]) {
        assert_eq!(*owner, Who::Me, "owner is the user's SID");
        let who: Vec<_> = aces.iter().map(|a| (a.kind, a.who.clone())).collect();
        assert_eq!(
            who,
            vec![(AceKind::Allow, Who::Me), (AceKind::Allow, Who::System)],
            "{aces:?}"
        );
    }

    /// Failing test 6 (ADR 0092), Windows half: under a parent granting Authenticated Users modify the folder has owner-only
    /// rights from the first read of its descriptor, with no narrowing call after: it never inherited the entry.
    #[test]
    fn a_folder_made_under_a_parent_that_grants_authenticated_users_modify_is_owner_only() {
        let t = temp();
        let parent = t.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        icacls(
            &parent,
            &["/grant", &format!("*{AUTHENTICATED_USERS}:(OI)(CI)M")],
        );
        let d = PrivateDir::create_all(&parent.join("a").join("b")).unwrap();
        let (owner, aces) = d.dir.security(&me()).unwrap();
        allows_only_me_and_system(&owner, &aces);
        assert_ne!(
            std::fs::metadata(d.path()).unwrap().file_attributes() & NOT_CONTENT_INDEXED,
            0,
            "not content-indexed"
        );
        d.write_atomic("f", b"x").unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(0x0002_0000)
            .open(d.path().join("f"))
            .unwrap();
        let (owner, aces) = sys::file_security(&file, &me()).unwrap();
        allows_only_me_and_system(&owner, &aces);
        assert_ne!(
            file.metadata().unwrap().file_attributes() & NOT_CONTENT_INDEXED,
            0
        );
        assert_eq!(d.read("f", 4).unwrap(), b"x");
    }

    /// Failing test 1, the type's half: an existing folder with `Everyone` write is refused and names who.
    #[test]
    fn a_folder_everyone_can_write_in_is_refused_and_left_alone() {
        let t = temp();
        let p = t.path().join("d");
        std::fs::create_dir(&p).unwrap();
        icacls(&p, &["/grant", &format!("*{EVERYONE}:(OI)(CI)F")]);
        for result in [
            PrivateDir::create_all(&p),
            PrivateDir::open(&p, Links::Refuse),
        ] {
            assert_eq!(
                refusal(result.unwrap_err()),
                Refusal::OpenToOthersAcl {
                    who: EVERYONE.to_owned()
                }
            );
        }
        assert_eq!(std::fs::read_dir(&p).unwrap().count(), 0, "nothing written");
        let text = PrivateDir::open(&p, Links::Refuse).unwrap_err().to_string();
        assert!(text.contains("Security tab"), "{text}");
    }

    #[test]
    fn a_folder_of_another_owner_is_refused() {
        let t = temp();
        let p = t.path().join("d");
        std::fs::create_dir(&p).unwrap();
        let other = "S-1-5-32-545".to_owned();
        assert!(matches!(
            refusal(PrivateDir::open_as(&p, Links::Refuse, other).unwrap_err()),
            Refusal::OtherOwnerAccount { .. }
        ));
    }

    fn junction(link: &Path, target: &Path) {
        let out = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn a_junction_in_place_of_the_folder_is_refused_and_its_target_untouched() {
        let t = temp();
        let target = t.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        let link = t.path().join("d");
        junction(&link, &target);
        for result in [
            PrivateDir::open(&link, Links::Refuse),
            PrivateDir::create_all(&link),
        ] {
            assert_eq!(refusal(result.unwrap_err()), Refusal::Symlink);
        }
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    }

    /// Failing test 7, Windows half: while the handle is held the ancestors cannot be renamed or replaced by a junction, so a
    /// create lands in the held folder or fails; it never lands at a junction's target.
    #[test]
    fn an_ancestor_cannot_be_replaced_while_the_handle_is_held() {
        let t = temp();
        let outer = t.path().join("outer");
        let held = PrivateDir::create_all(&outer.join("inner")).unwrap();
        let attacker = t.path().join("attacker");
        std::fs::create_dir(&attacker).unwrap();
        assert!(
            std::fs::rename(&outer, t.path().join("moved")).is_err(),
            "no FILE_SHARE_DELETE on the held chain"
        );
        assert!(std::fs::remove_dir_all(&outer).is_err());
        held.write_atomic("secret", b"s").unwrap();
        assert_eq!(std::fs::read_dir(&attacker).unwrap().count(), 0);
        assert_eq!(
            std::fs::read(outer.join("inner").join("secret")).unwrap(),
            b"s"
        );
    }

    #[test]
    fn a_parent_where_others_can_delete_entries_is_refused() {
        let t = temp();
        let parent = t.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let p = parent.join("d");
        PrivateDir::create_all(&p).unwrap();
        icacls(&parent, &["/grant", &format!("*{EVERYONE}:(DC)")]);
        let r = refusal(PrivateDir::open(&p, Links::Refuse).unwrap_err());
        assert!(
            matches!(&r, Refusal::Parent(inner) if matches!(**inner, Refusal::OpenToOthersAcl { .. })),
            "{r:?}"
        );
        assert!(PrivateDir::create_all(&p).is_err());
    }

    #[test]
    fn a_file_others_can_read_or_that_is_not_ours_or_not_a_file_is_refused_on_windows() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        d.write_atomic("f", b"x").unwrap();
        assert!(d.open_file("f").is_ok());
        icacls(&d.path().join("f"), &["/grant", &format!("*{EVERYONE}:R")]);
        assert!(matches!(
            refusal(d.open_file("f").unwrap_err()),
            Refusal::OpenToOthersAcl { .. }
        ));
        std::fs::create_dir(d.path().join("sub")).unwrap();
        assert_eq!(refusal(d.open_file("sub").unwrap_err()), Refusal::NotAFile);
        let other = PrivateDir {
            dir: Dir::open_path(d.path(), false).unwrap(),
            path: d.path().to_path_buf(),
            who: EVERYONE.to_owned(),
        };
        d.write_atomic("g", b"x").unwrap();
        assert!(matches!(
            refusal(other.open_file("g").unwrap_err()),
            Refusal::OtherOwnerAccount { .. }
        ));
    }

    #[test]
    fn a_failed_replace_reports_and_removes_the_temporary_file() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        std::fs::create_dir(d.path().join("f")).unwrap();
        let e = d.write_atomic("f", b"x").unwrap_err();
        assert!(matches!(&e, PrivateError::Io { op: "replace", .. }), "{e}");
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            vec![OsStr::new("f")],
            "the temporary file is removed"
        );
    }

    #[test]
    fn a_taken_temporary_name_is_retried() {
        let t = temp();
        let d = PrivateDir::create_all(&t.path().join("d")).unwrap();
        let pid = std::process::id();
        std::fs::write(d.path().join(format!(".f.{pid}-0.tmp")), b"planted").unwrap();
        let e = d
            .create_temp_with("f", d.path(), || 0)
            .map(|_| ())
            .unwrap_err();
        assert!(
            matches!(&e, PrivateError::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists),
            "{e}"
        );
        let mut n = 0;
        let (name, _file) = d
            .create_temp_with("f", d.path(), || {
                n += 1;
                n - 1
            })
            .unwrap();
        assert!(name.ends_with("-1.tmp"), "{name}");
    }

    #[test]
    fn the_root_has_no_folder_above_it() {
        let e = PrivateDir::create_all(Path::new("C:\\")).unwrap_err();
        assert!(
            matches!(e, PrivateError::Io { .. } | PrivateError::Refused { .. }),
            "{e}"
        );
    }
}
