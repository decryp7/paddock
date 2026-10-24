use crate::{Name, Secret, Secrets, SecretsError};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Owner-only files in one directory, one file per secret. This is the fallback for a machine without a usable
/// credential store, and **only an app that opts in constructs it** (Pitwall's admin key); it is never a default.
///
/// - Unix: files are created with mode 0600 (written to a temporary 0600 file and renamed, so a value is never readable
///   by others, not even briefly) and a file with any group or other permission bit is refused on read
///   ([`SecretsError::InsecureFile`]).
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

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
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
        fs::create_dir_all(&self.dir)
            .map_err(|e| io("the secret folder could not be created", e))?;
        let path = self.path(name);
        let tmp = path.with_extension("tmp");
        let _ = fs::remove_file(&tmp);
        let mut f = create_private(&tmp).map_err(|e| io("the secret could not be stored", e))?;
        let written = f
            .write_all(value.expose().as_bytes())
            .and_then(|()| f.sync_all());
        drop(f);
        if let Err(e) = written.and_then(|()| fs::rename(&tmp, &path)) {
            let _ = fs::remove_file(&tmp);
            return Err(io("the secret could not be stored", e));
        }
        Ok(())
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
        // A file where the folder should be: even reading fails, with a message.
        assert!(blocked.get(&n("a")).is_err());
    }
}
