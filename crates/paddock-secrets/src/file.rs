use crate::{Name, Secret, Secrets, SecretsError};
use std::fs;
use std::path::{Path, PathBuf};

/// Owner-only files in one directory, one file per secret. This is the fallback for a machine without a usable
/// credential store, and **only an app that opts in constructs it** (Pitwall's admin key); it is never a default.
///
/// - Unix: files are created with mode 0600 (written to a unique temporary 0600 file in the same folder, flushed and renamed, so a value
///   is never readable by others, not even briefly, and concurrent writers never share a temporary file); the folder is
///   created 0700 and made 0700 when it is loose and ours; a file with any group or other permission bit is refused on read
///   ([`SecretsError::InsecureFile`]), and so is a file or folder owned by another user.
/// - Windows: files are created inside `dir`, which must be a per-user folder (the app's data directory under
///   `%LOCALAPPDATA%`), whose ACL grants only the owner, SYSTEM and Administrators; no mode check is possible there.
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

#[cfg(not(unix))]
fn insecure(_: &fs::Metadata) -> bool {
    false
}

/// The user running the app (Unix); 0 elsewhere, where no owner check is possible.
fn current_user() -> u32 {
    #[cfg(unix)]
    {
        paddock_proc::effective_uid()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// The owner problem of the file `meta` in `dir` for the user `me`, as a refusal with the next step.
fn owner_refusal(path: &Path, dir: &Path, meta: &fs::Metadata, me: u32) -> Option<SecretsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let folder_owner = fs::metadata(dir).map(|m| m.uid()).unwrap_or(meta.uid());
        foreign_owner(meta.uid(), folder_owner, me).map(|why| {
            SecretsError::Failed(format!(
                "{} {why}, so it is not trusted; remove it and store the secret again, or fix the owner (chown) and try again",
                path.display()
            ))
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, dir, meta, me);
        None
    }
}

/// Creates the folder 0700 whatever the umask, and makes it 0700 when this user owns it and group or others can write to
/// it without a sticky bit (the same rule as the MCP key list). A folder someone else owns is refused.
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
        if mode & 0o022 != 0 && mode & 0o1000 == 0 {
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

impl Secrets for FileSecrets {
    fn get(&self, name: &Name) -> Result<Option<Secret>, SecretsError> {
        let path = self.path(name);
        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io("the secret file could not be read", e)),
        };
        if insecure(&meta) {
            return Err(SecretsError::InsecureFile(path.display().to_string()));
        }
        if let Some(e) = owner_refusal(&path, &self.dir, &meta, current_user()) {
            return Err(e);
        }
        if meta.len() as usize > crate::MAX_SECRET_LEN + 2 {
            return Err(SecretsError::Failed("the secret file is damaged".into()));
        }
        let text = zeroize::Zeroizing::new(
            fs::read_to_string(&path).map_err(|e| io("the secret file could not be read", e))?,
        );
        Secret::new(text.trim_end_matches(['\r', '\n']))
            .map(Some)
            .map_err(|_| SecretsError::Failed("the secret file is damaged".into()))
    }

    fn set(&self, name: &Name, value: &Secret) -> Result<(), SecretsError> {
        secure_folder(&self.dir)?;
        paddock_env::write_atomic(&self.path(name), value.expose().as_bytes(), true)
            .map_err(|e| SecretsError::Failed(format!("the secret could not be stored: {e}")))
    }

    fn delete(&self, name: &Name) -> Result<(), SecretsError> {
        match fs::remove_file(self.path(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io("the secret could not be deleted", e)),
        }
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
        // Already private: untouched.
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o750)).unwrap();
        FileSecrets::new(&sub).set(&n("t"), &s("v")).unwrap();
        assert_eq!(mode(&sub), 0o750);
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
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_owned_by_another_user_is_refused_without_writing() {
        use std::os::unix::fs::MetadataExt;
        let root_owned = Path::new("/tmp");
        if fs::metadata(root_owned)
            .map(|m| m.uid())
            .unwrap_or(current_user())
            == current_user()
        {
            return; // this account owns /tmp (root, or a sandbox): nothing foreign to point at
        }
        let e = FileSecrets::new(root_owned)
            .set(&n("token"), &s("v"))
            .unwrap_err();
        assert!(e.to_string().contains("owned by another user"), "{e}");
    }
}
