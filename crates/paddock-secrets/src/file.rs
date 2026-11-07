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
/// - Windows: each file gets a protected DACL that grants only its owner (set on the empty temporary file, before the value
///   is written); a file whose DACL grants anyone but its owner, SYSTEM or Administrators is refused on read, and so is a
///   reparse point (a symbolic link or junction).
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

/// The owner problem of the file `meta` in `dir` for the user `me`, as a refusal with the next step.
#[cfg(unix)]
fn owner_refusal(path: &Path, dir: &Path, meta: &fs::Metadata, me: u32) -> Option<SecretsError> {
    use std::os::unix::fs::MetadataExt;
    let folder_owner = fs::metadata(dir).map(|m| m.uid()).unwrap_or(meta.uid());
    foreign_owner(meta.uid(), folder_owner, me).map(|why| {
        SecretsError::Failed(format!(
            "{} {why}, so it is not trusted; remove it and store the secret again, or fix the owner (chown) and try again",
            path.display()
        ))
    })
}

/// Creates the folder 0700 whatever the umask, and makes it 0700 when this user owns it and group or others have any
/// access to it without a sticky bit. A folder someone else owns is refused.
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
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir).map_err(|e| io("the secret folder could not be created", e))
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
        if insecure(&meta) {
            return Err(SecretsError::InsecureFile(path.display().to_string()));
        }
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if let Some(e) = owner_refusal(path, dir, &meta, me) {
            return Err(e);
        }
    }
    #[cfg(not(unix))]
    let _ = me;
    #[cfg(windows)]
    if windows::shared_with_others(path)
        .map_err(|e| io("the secret file's access list could not be read", e))?
    {
        return Err(SecretsError::InsecureFile(path.display().to_string()));
    }
    if meta.len() as usize > crate::MAX_SECRET_LEN + 2 {
        return Err(SecretsError::Failed("the secret file is damaged".into()));
    }
    Ok(())
}

/// The step on the empty temporary file: an owner-only DACL on Windows; nothing on Unix, where the file is created 0600.
fn owner_only(tmp: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        windows::restrict_to_owner(tmp)
    }
    #[cfg(not(windows))]
    {
        let _ = tmp;
        Ok(())
    }
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
        let dir = self
            .path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        secure_folder(dir)?;
        paddock_env::write_atomic_prepared(
            &self.path,
            value.expose().as_bytes(),
            true,
            &mut owner_only,
        )
        .map_err(|e| SecretsError::Failed(format!("the secret could not be stored: {e}")))
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

    #[test]
    fn a_secret_round_trips_and_deletes() {
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
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
        let d = tempfile::tempdir().unwrap();
        let f = FileSecrets::new(d.path());
        f.set(&n("token"), &s("v")).unwrap();
        let p = f.path(&n("token"));
        let meta = fs::metadata(&p).unwrap();
        let me = current_user();
        assert!(owner_refusal(&p, d.path(), &meta, me).is_none());
        let e = owner_refusal(&p, d.path(), &meta, me + 1)
            .unwrap()
            .to_string();
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
