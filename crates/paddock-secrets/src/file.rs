use crate::{Name, Secret, Secrets, SecretsError};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(windows)]
#[path = "file_windows.rs"]
mod windows;

/// Owner-only files in one directory, one file per secret (each a [`SecretFile`]). This is the store for an app that
/// keeps its secrets in files (ADR 0057: every secret on macOS, Pitwall's on every OS).
///
/// - Unix: files are created with mode 0600 (written to a unique temporary 0600 file in the same folder, flushed and renamed, so a value
///   is never readable by others, not even briefly, and concurrent writers never share a temporary file); the folder is
///   created 0700 and made 0700 when it is loose and ours; a file with any group or other permission bit is refused on read
///   ([`SecretsError::InsecureFile`]), and so is a file or folder owned by another user, a symbolic link and anything that is
///   not a regular file.
/// - Windows: each file is created with a protected DACL that grants only the user running the app, and opened unshared
///   until it is renamed into place; a folder where others may create or change files is refused. On read, the owner and
///   DACL of the open handle are checked: an owner other than the user, SYSTEM or Administrators, or an entry for anyone
///   else, is refused, and so is a reparse point (a symbolic link or junction).
#[derive(Debug, Clone)]
pub struct FileSecrets {
    dir: PathBuf,
}

fn file_name(name: &Name) -> String {
    let mut out = String::new();
    for b in name.account().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out.push_str(".secret");
    out
}

fn io(what: &str, e: std::io::Error) -> SecretsError {
    SecretsError::Failed(format!("{what}: {e}"))
}

impl FileSecrets {
    /// Secrets in `dir` (created on the first write).
    pub fn new(dir: impl Into<PathBuf>) -> FileSecrets {
        FileSecrets { dir: dir.into() }
    }

    /// The directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, name: &Name) -> PathBuf {
        self.dir.join(file_name(name))
    }

    fn file(&self, name: &Name) -> SecretFile {
        SecretFile::new(self.path(name))
    }
}

/// One owner-only secret file at a path the app chooses (Pitwall's `<data>/admin.key`, which its CLI and scripts read
/// too): the file holds the value and nothing else. The same rules as [`FileSecrets`], which is a folder of these.
///
/// The value is plain text at rest (ADR 0057 §5): other users cannot read it, but any program running as the same user
/// can, like `~/.netrc`.
#[derive(Debug, Clone)]
pub struct SecretFile {
    path: PathBuf,
}

/// Why a Unix secret file or its folder is not trusted by its owner: either belongs to another user (who could read or swap
/// the secret), judged against the user running the app. Pure, so the decision is tested without a second account.
#[cfg(unix)]
fn foreign_owner(file_owner: u32, folder_owner: u32, running_user: u32) -> Option<&'static str> {
    if file_owner != running_user {
        Some("is owned by another user")
    } else if folder_owner != running_user {
        Some("is in a folder owned by another user")
    } else {
        None
    }
}

#[cfg(unix)]
fn insecure(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o077 != 0
}

/// The user running the app (Unix).
#[cfg(unix)]
fn current_user() -> u32 {
    paddock_proc::effective_uid()
}

/// The owner problem of the file at `path` (owned by `file_owner`, in a folder owned by `folder_owner`) for the user `me`,
/// as a refusal with the next step. A folder whose owner cannot be read is refused too: nothing is assumed.
#[cfg(unix)]
fn owner_refusal(
    path: &Path,
    file_owner: u32,
    folder_owner: std::io::Result<u32>,
    me: u32,
) -> Option<SecretsError> {
    let folder_owner = match folder_owner {
        Ok(uid) => uid,
        Err(e) => {
            return Some(SecretsError::Failed(format!(
                "the owner of the folder of {} could not be checked ({e}), so the secret is not trusted",
                path.display()
            )));
        }
    };
    foreign_owner(file_owner, folder_owner, me).map(|why| {
        SecretsError::Failed(format!(
            "{} {why}, so it is not trusted; remove it and store the secret again, or fix the owner (chown) and try again",
            path.display()
        ))
    })
}

/// Creates the folder 0700 whatever the umask, and makes it 0700 (logged) when this user owns it and group or others have
/// any access to it without a sticky bit. A folder someone else owns is refused. On Windows a folder whose owner is not
/// the user, SYSTEM or Administrators, or where anyone else may create, delete or change entries, is refused.
///
/// Only the immediate folder is checked, not its parents: a parent that another user can write could still swap the whole
/// folder. The apps' data folders sit in the user's own profile, whose parents are the system's.
fn secure_folder(dir: &Path) -> Result<(), SecretsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| io("the secret folder could not be created", e))?;
        let meta = fs::metadata(dir).map_err(|e| io("the secret folder could not be read", e))?;
        if meta.uid() != current_user() {
            return Err(SecretsError::Failed(format!(
                "{} is owned by another user, so secrets are not stored in it; use a folder of your own",
                dir.display()
            )));
        }
        let mode = meta.permissions().mode();
        if mode & 0o077 != 0 && mode & 0o1000 == 0 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
                .map_err(|e| io("the secret folder could not be made private", e))?;
            log::info!(
                target: "paddock_secrets",
                "made {} private (mode 0700, it was {:o}): it holds an owner-only secret",
                dir.display(),
                mode & 0o7777
            );
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        fs::create_dir_all(dir).map_err(|e| io("the secret folder could not be created", e))?;
        let (owner, aces) = windows::folder_security(dir)
            .map_err(|e| io("the secret folder's access list could not be read", e))?;
        match crate::acl::folder_refusal(owner, &aces) {
            Some(r) => Err(SecretsError::Failed(format!(
                "{} {}",
                dir.display(),
                r.why()
            ))),
            None => Ok(()),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        fs::create_dir_all(dir).map_err(|e| io("the secret folder could not be created", e))
    }
}

/// Makes `dir` (created if missing) a folder only this user may change, so secrets can be stored in it whatever its parent
/// allows: mode 0700 on Unix; on Windows a protected access list that lets in only this user, SYSTEM and Administrators and
/// is inherited by what is created in it (nothing is inherited from the parent any more). For a folder under a parent where
/// others may create files, such as a test's temporary folder on a CI runner (`C:\gitea-runner\work\...`, run 7821), before
/// [`FileSecrets`] or [`SecretFile`] is used in it. A symbolic link or junction is refused, never followed.
pub fn make_folder_private(dir: &Path) -> Result<(), SecretsError> {
    let failed = |what: &str, e: std::io::Error| {
        SecretsError::Failed(format!("{} {what}: {e}", dir.display()))
    };
    fs::create_dir_all(dir).map_err(|e| failed("could not be created as a folder", e))?;
    let meta = fs::symlink_metadata(dir).map_err(|e| failed("could not be read", e))?;
    if !meta.is_dir() {
        return Err(SecretsError::Failed(format!(
            "{} is a link or not a folder, so it is not made private; use a real folder",
            dir.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| failed("could not be made private", e))
    }
    #[cfg(windows)]
    {
        windows::make_private(dir).map_err(|e| failed("could not be made private", e))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(())
    }
}

/// A temporary folder made private ([`make_folder_private`]): the tests' folder for secrets, whatever the temp folder of
/// the machine lets others do (CI run 7821: a Windows runner's job temp folder inherits a parent's entry for other users).
#[cfg(test)]
pub(crate) fn private_tempdir() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    make_folder_private(d.path()).unwrap();
    d
}

/// Creates the temporary file of a write. Unix: new, 0600, no link followed. Windows: new, with an owner-only security
/// descriptor from the start and no sharing while it is open, so no other principal can open it at any point.
fn create_temp(tmp: &Path) -> std::io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(tmp)
    }
    #[cfg(windows)]
    {
        windows::create_owner_only(tmp)
    }
    #[cfg(not(any(unix, windows)))]
    {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)
    }
}

/// Opens the secret for reading without following a symbolic link and without waiting on a pipe planted in its place;
/// `None` when there is no file.
#[cfg(unix)]
fn open_secret(path: &Path) -> Result<Option<fs::File>, SecretsError> {
    use std::os::unix::fs::OpenOptionsExt;
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(f) => Ok(Some(f)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => Err(symlink_refusal(path)),
        Err(e) => Err(io("the secret file could not be read", e)),
    }
}

#[cfg(windows)]
fn open_secret(path: &Path) -> Result<Option<fs::File>, SecretsError> {
    match windows::open_no_reparse(path) {
        Ok(Some(f)) => Ok(Some(f)),
        Ok(None) => Err(symlink_refusal(path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io("the secret file could not be read", e)),
    }
}

fn symlink_refusal(path: &Path) -> SecretsError {
    SecretsError::Failed(format!(
        "{} is a symbolic link, so it is not trusted; remove it and store the secret again",
        path.display()
    ))
}

/// The checks on the open file: a regular file, owner-only, owned by `me` (the user running the app, Unix), of a sane size.
fn check_open(path: &Path, file: &fs::File, me: u32) -> Result<(), SecretsError> {
    let meta = file
        .metadata()
        .map_err(|e| io("the secret file could not be read", e))?;
    if !meta.is_file() {
        return Err(SecretsError::Failed(format!(
            "{} is not a regular file, so it is not trusted; remove it and store the secret again",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if insecure(&meta) {
            return Err(SecretsError::InsecureFile(path.display().to_string()));
        }
        if meta.nlink() != 1 {
            return Err(SecretsError::Failed(format!(
                "{} has more than one name (a hard link), so it is not trusted; remove the other names or the file and store the secret again",
                path.display()
            )));
        }
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if let Some(e) = owner_refusal(path, meta.uid(), fs::metadata(dir).map(|m| m.uid()), me) {
            return Err(e);
        }
    }
    #[cfg(not(unix))]
    let _ = me;
    #[cfg(windows)]
    {
        // Owner and DACL of the open handle, not of the path, which could be swapped in between.
        let (owner, aces) = windows::file_security(file)
            .map_err(|e| io("the secret file's access list could not be read", e))?;
        match crate::acl::file_refusal(owner, &aces) {
            Some(crate::acl::Refusal::Shared) => {
                return Err(SecretsError::InsecureFile(path.display().to_string()));
            }
            Some(r) => {
                return Err(SecretsError::Failed(format!(
                    "{} {}",
                    path.display(),
                    r.why()
                )));
            }
            None => {}
        }
    }
    if meta.len() as usize > crate::MAX_SECRET_LEN + 2 {
        return Err(SecretsError::Failed("the secret file is damaged".into()));
    }
    Ok(())
}

/// Writes `bytes` to `path` atomically and owner-only, the mechanism of [`SecretFile::write`] for any trust-bearing file
/// (an access list, not only a secret): the folder is made private or refused (a Windows folder where others may write
/// is refused, a Unix one owned by another user too), the new content goes to a unique temporary file in the same folder
/// that is created new, owner-only from its first byte (0600 and no link followed on Unix; a protected DACL and no sharing
/// on Windows), flushed and renamed over the target (or over a link, which is replaced, never followed). No backup copy.
pub fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<(), SecretsError> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    secure_folder(dir)?;
    paddock_env::write_atomic_created(path, bytes, &mut create_temp)
        .map_err(|e| SecretsError::Failed(format!("the secret could not be stored: {e}")))
}

impl SecretFile {
    /// The secret at `path` (its folder is created on the first write).
    pub fn new(path: impl Into<PathBuf>) -> SecretFile {
        SecretFile { path: path.into() }
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The secret, if the file exists. Refused with a message (never the value) when the file is not owner-only, not ours,
    /// a link or not a regular file, or damaged.
    pub fn read(&self) -> Result<Option<Secret>, SecretsError> {
        let Some(mut file) = open_secret(&self.path)? else {
            return Ok(None);
        };
        #[cfg(unix)]
        check_open(&self.path, &file, current_user())?;
        #[cfg(not(unix))]
        check_open(&self.path, &file, 0)?;
        let mut text = zeroize::Zeroizing::new(String::new());
        (&mut file)
            .take(crate::MAX_SECRET_LEN as u64 + 3)
            .read_to_string(&mut text)
            .map_err(|e| io("the secret file could not be read", e))?;
        Secret::new(text.trim_end_matches(['\r', '\n']))
            .map(Some)
            .map_err(|_| SecretsError::Failed("the secret file is damaged".into()))
    }

    /// Stores (or replaces) the secret atomically: a new owner-only file renamed over the old one (or over a link, which is
    /// replaced, never followed).
    pub fn write(&self, value: &Secret) -> Result<(), SecretsError> {
        write_owner_only(&self.path, value.expose().as_bytes())
    }

    /// Removes the secret; no error when there is none.
    pub fn delete(&self) -> Result<(), SecretsError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io("the secret could not be deleted", e)),
        }
    }
}

impl Secrets for FileSecrets {
    fn get(&self, name: &Name) -> Result<Option<Secret>, SecretsError> {
        self.file(name).read()
    }

    fn set(&self, name: &Name, value: &Secret) -> Result<(), SecretsError> {
        self.file(name).write(value)
    }

    fn delete(&self, name: &Name) -> Result<(), SecretsError> {
        self.file(name).delete()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(k: &str) -> Name {
        Name::new("channel:w1:2", k).unwrap()
    }
    fn s(v: &str) -> Secret {
        Secret::new(v).unwrap()
    }

    /// The shared writer for any trust-bearing file (ADR 0092 WP-0): owner-only from the first byte on both OS, replaces the
    /// old content, leaves no temporary file and no backup, and what it wrote passes the secret-file checks (on Windows
    /// that reads the DACL of the handle). Needs a Windows run for the DACL half.
    #[test]
    fn write_owner_only_writes_and_replaces_a_private_file() {
        let d = private_tempdir();
        let f = d.path().join("data/apps.json");
        write_owner_only(&f, b"one").unwrap();
        write_owner_only(&f, b"two").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"two");
        let names: Vec<_> = fs::read_dir(f.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            vec![std::ffi::OsString::from("apps.json")],
            "no temporary file, no backup"
        );
        assert_eq!(
            SecretFile::new(&f).read().unwrap().unwrap().expose(),
            "two",
            "it passes the owner-only checks"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&f).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    /// A link planted where the file goes is replaced, never written through.
    #[cfg(unix)]
    #[test]
    fn write_owner_only_never_writes_through_a_link() {
        let d = private_tempdir();
        let outside = d.path().join("outside");
        fs::write(&outside, "precious").unwrap();
        let f = d.path().join("apps.json");
        std::os::unix::fs::symlink(&outside, &f).unwrap();
        write_owner_only(&f, b"list").unwrap();
        assert_eq!(fs::read_to_string(&outside).unwrap(), "precious");
        assert_eq!(fs::read(&f).unwrap(), b"list");
        assert!(!fs::symlink_metadata(&f).unwrap().file_type().is_symlink());
    }

    #[test]
    fn a_secret_round_trips_and_deletes() {
        let d = private_tempdir();
        let f = FileSecrets::new(d.path().join("sub"));
        assert_eq!(f.dir(), d.path().join("sub"));
        assert_eq!(f.get(&n("token")).unwrap(), None);
        f.set(&n("token"), &s("one")).unwrap();
        f.set(&n("token"), &s("two")).unwrap();
        assert_eq!(f.get(&n("token")).unwrap(), Some(s("two")));
        assert_eq!(
            fs::read_dir(f.dir()).unwrap().count(),
            1,
            "no temporary file is left"
        );
        f.delete(&n("token")).unwrap();
        f.delete(&n("token")).unwrap();
        assert_eq!(f.get(&n("token")).unwrap(), None);
    }

    #[test]
    fn a_folder_is_made_private_and_a_file_or_link_is_refused() {
        let d = private_tempdir();
        let dir = d.path().join("a/b");
        make_folder_private(&dir).unwrap();
        FileSecrets::new(&dir).set(&n("t"), &s("v")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
            make_folder_private(&dir).unwrap();
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let link = d.path().join("link");
            std::os::unix::fs::symlink(&dir, &link).unwrap();
            assert!(
                make_folder_private(&link).is_err(),
                "a link is not followed"
            );
        }
        let plain = d.path().join("plain");
        fs::write(&plain, "x").unwrap();
        let e = make_folder_private(&plain).unwrap_err();
        assert!(e.to_string().contains("plain"), "{e}");
    }

    #[test]
    fn names_become_safe_file_names() {
        assert_eq!(
            file_name(&n("read token")),
            "channel%3Aw1%3A2%3Aread%20token.secret"
        );
        let weird = Name::new("../x", "k").unwrap();
        assert!(!file_name(&weird).contains('/'));
    }

    #[cfg(unix)]
    #[test]
    fn files_are_owner_only_and_loose_files_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = private_tempdir();
        let f = FileSecrets::new(d.path());
        f.set(&n("token"), &s("v")).unwrap();
        let p = f.path(&n("token"));
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        let e = f.get(&n("token")).unwrap_err();
        assert!(matches!(e, SecretsError::InsecureFile(_)));
        assert!(
            e.to_string().contains("0600") && !e.to_string().contains('v')
                || e.to_string().contains("0600")
        );
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(f.get(&n("token")).unwrap(), Some(s("v")));
    }

    #[test]
    fn damaged_files_and_io_errors_are_reported_without_values() {
        let d = private_tempdir();
        let f = FileSecrets::new(d.path());
        let p = f.path(&n("token"));
        f.set(&n("token"), &s("v")).unwrap();
        fs::write(&p, "two\nlines").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let e = f.get(&n("token")).unwrap_err();
        assert!(e.to_string().contains("damaged") && !e.to_string().contains("two"));
        fs::write(&p, "x".repeat(crate::MAX_SECRET_LEN + 10)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(
            f.get(&n("token"))
                .unwrap_err()
                .to_string()
                .contains("damaged")
        );
        // A directory where the file should be: read, write and delete all fail with a message.
        fs::remove_file(&p).unwrap();
        fs::create_dir(&p).unwrap();
        #[cfg(unix)]
        assert!(f.get(&n("token")).is_err());
        assert!(f.set(&n("token"), &s("v")).is_err());
        assert!(f.delete(&n("token")).is_err());
        assert_eq!(
            fs::read_dir(f.dir()).unwrap().count(),
            1,
            "the failed write left no temporary file"
        );
        // A file where the folder should be.
        let plain = d.path().join("plain");
        fs::write(&plain, "x").unwrap();
        let blocked = FileSecrets::new(plain.join("deeper"));
        assert!(blocked.set(&n("a"), &s("v")).is_err());
        // A file where the folder should be: even reading fails, with a message (Windows reports the path as missing instead).
        #[cfg(unix)]
        assert!(blocked.get(&n("a")).is_err());
    }

    #[test]
    fn two_threads_setting_one_name_never_leave_a_half_written_file() {
        let d = private_tempdir();
        let f = FileSecrets::new(d.path());
        let values = ["a".repeat(200), "b".repeat(200)];
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|sc| {
            for v in &values {
                let (f, barrier, values) = (&f, &barrier, &values);
                sc.spawn(move || {
                    barrier.wait();
                    for _ in 0..200 {
                        f.set(&n("token"), &s(v)).unwrap();
                        let got = f.get(&n("token")).unwrap().unwrap();
                        assert!(values.iter().any(|x| got == s(x)), "a whole value");
                    }
                });
            }
        });
        assert_eq!(
            fs::read_dir(f.dir()).unwrap().count(),
            1,
            "no temporary file is left"
        );
    }

    #[cfg(unix)]
    #[test]
    fn another_users_file_or_folder_is_refused_with_a_next_step() {
        assert_eq!(foreign_owner(501, 501, 501), None);
        assert!(
            foreign_owner(0, 501, 501)
                .unwrap()
                .contains("owned by another user")
        );
        assert!(
            foreign_owner(501, 0, 501)
                .unwrap()
                .contains("folder owned by another user")
        );
        assert!(foreign_owner(0, 0, 501).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_is_created_private_and_a_loose_own_folder_is_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        let d = private_tempdir();
        let sub = d.path().join("a/b");
        FileSecrets::new(&sub).set(&n("t"), &s("v")).unwrap();
        assert_eq!(mode(&sub), 0o700);
        for loose in [0o775, 0o777, 0o770, 0o757] {
            fs::set_permissions(&sub, fs::Permissions::from_mode(loose)).unwrap();
            FileSecrets::new(&sub).set(&n("t"), &s("v")).unwrap();
            assert_eq!(mode(&sub), 0o700, "from {loose:o}");
        }
        // Sticky and writable by others: left as it is.
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o1777)).unwrap();
        FileSecrets::new(&sub).set(&n("t"), &s("v")).unwrap();
        assert_eq!(mode(&sub), 0o1777);
        // Readable by the group or others (not writable): made private too, so nobody else can list the secrets.
        for readable in [0o750, 0o755, 0o705] {
            fs::set_permissions(&sub, fs::Permissions::from_mode(readable)).unwrap();
            FileSecrets::new(&sub).set(&n("t"), &s("v")).unwrap();
            assert_eq!(mode(&sub), 0o700, "from {readable:o}");
        }
        // Already private: untouched.
        FileSecrets::new(&sub).set(&n("t"), &s("v")).unwrap();
        assert_eq!(mode(&sub), 0o700);
    }

    #[test]
    fn one_secret_file_at_a_chosen_path_round_trips() {
        let d = private_tempdir();
        let f = SecretFile::new(d.path().join("data/admin.key"));
        assert_eq!(f.path(), d.path().join("data/admin.key"));
        assert_eq!(f.read().unwrap(), None);
        f.write(&s("pw_first")).unwrap();
        f.write(&s("pw_second")).unwrap();
        assert_eq!(f.read().unwrap(), Some(s("pw_second")));
        assert_eq!(
            fs::read_to_string(f.path()).unwrap(),
            "pw_second",
            "the file holds the value only (the CLI and scripts read it)"
        );
        let names: Vec<_> = fs::read_dir(d.path().join("data"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["admin.key"], "no temporary file is left");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode(f.path()), 0o600);
            assert_eq!(mode(&d.path().join("data")), 0o700);
        }
        f.delete().unwrap();
        f.delete().unwrap();
        assert_eq!(f.read().unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_in_place_of_the_secret_is_never_followed() {
        use std::os::unix::fs::PermissionsExt;
        let d = private_tempdir();
        let outside = d.path().join("outside");
        fs::write(&outside, "theirs").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        let dir = d.path().join("data");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("admin.key");
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        let f = SecretFile::new(&path);
        let e = f.read().unwrap_err().to_string();
        assert!(
            e.contains("symbolic link") && e.contains("admin.key"),
            "{e}"
        );
        assert!(!e.contains("theirs"), "no value in the message");
        // Writing replaces the link with a file of our own; the link's target is untouched.
        f.write(&s("mine")).unwrap();
        assert!(
            !fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "theirs");
        assert_eq!(f.read().unwrap(), Some(s("mine")));
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_or_folder_in_place_of_the_secret_is_refused_without_blocking() {
        let d = private_tempdir();
        let path = d.path().join("admin.key");
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        // A plain open would wait for a writer forever; the read must come back with a refusal.
        let e = SecretFile::new(&path).read().unwrap_err().to_string();
        assert!(e.contains("not a regular file"), "{e}");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let e = SecretFile::new(&path).read().unwrap_err().to_string();
        assert!(
            e.contains("not a regular file") || e.contains("could not be read"),
            "{e}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_is_refused_when_the_user_running_the_app_is_not_its_owner() {
        let d = private_tempdir();
        let f = FileSecrets::new(d.path());
        f.set(&n("token"), &s("v")).unwrap();
        let p = f.path(&n("token"));
        let me = current_user();
        assert!(owner_refusal(&p, me, Ok(me), me).is_none());
        let e = owner_refusal(&p, me, Ok(me), me + 1).unwrap().to_string();
        assert!(
            e.contains("owned by another user") && e.contains("chown"),
            "{e}"
        );
        assert!(
            !e.contains('v') || e.contains("token"),
            "no value in the message"
        );
        // The same refusal on the opened file, as a read by another user would meet it.
        let open = fs::File::open(&p).unwrap();
        let e = check_open(&p, &open, me + 1).unwrap_err().to_string();
        assert!(e.contains("owned by another user"), "{e}");
        assert!(check_open(&p, &open, me).is_ok());
    }

    /// Review P2-1: a file of ours in a folder of another user, or in a folder whose owner cannot be read, is refused.
    #[cfg(unix)]
    #[test]
    fn a_file_in_a_foreign_or_unreadable_folder_is_refused() {
        let p = Path::new("/x/admin.key");
        let me = 501;
        let e = owner_refusal(p, me, Ok(me + 1), me).unwrap().to_string();
        assert!(e.contains("folder owned by another user"), "{e}");
        let e = owner_refusal(p, me, Err(std::io::Error::other("gone")), me)
            .unwrap()
            .to_string();
        assert!(
            e.contains("could not be checked") && e.contains("gone"),
            "{e}"
        );
        assert!(owner_refusal(p, me, Ok(me), me).is_none());
    }

    /// Review P2-1: every group or other bit is refused, read, write or execute.
    #[cfg(unix)]
    #[test]
    fn every_group_or_other_bit_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = private_tempdir();
        let f = SecretFile::new(d.path().join("admin.key"));
        f.write(&s("pw_x")).unwrap();
        for loose in [0o640, 0o604, 0o660, 0o620, 0o602, 0o610, 0o601] {
            fs::set_permissions(f.path(), fs::Permissions::from_mode(loose)).unwrap();
            assert!(
                matches!(f.read(), Err(SecretsError::InsecureFile(_))),
                "{loose:o}"
            );
        }
        for private in [0o600, 0o400] {
            fs::set_permissions(f.path(), fs::Permissions::from_mode(private)).unwrap();
            assert_eq!(f.read().unwrap(), Some(s("pw_x")), "{private:o}");
        }
    }

    /// Review P2-2: a second name for the secret (a hard link someone kept) is refused.
    #[cfg(unix)]
    #[test]
    fn a_hard_linked_secret_is_refused() {
        let d = private_tempdir();
        let f = SecretFile::new(d.path().join("admin.key"));
        f.write(&s("pw_x")).unwrap();
        let other = d.path().join("kept");
        fs::hard_link(f.path(), &other).unwrap();
        let e = f.read().unwrap_err().to_string();
        assert!(e.contains("more than one name"), "{e}");
        fs::remove_file(&other).unwrap();
        assert_eq!(f.read().unwrap(), Some(s("pw_x")));
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_owned_by_another_user_is_refused_without_writing() {
        use std::os::unix::fs::MetadataExt;
        let root_owned = Path::new("/tmp");
        // When this account owns /tmp (root, or a sandbox) there is nothing foreign to point at.
        let foreign = fs::metadata(root_owned)
            .map(|m| m.uid())
            .unwrap_or(current_user())
            != current_user();
        if foreign {
            let e = FileSecrets::new(root_owned)
                .set(&n("token"), &s("v"))
                .unwrap_err();
            assert!(e.to_string().contains("owned by another user"), "{e}");
        }
    }
}
