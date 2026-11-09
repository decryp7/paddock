//! The server's admin key as the app reads it. The server owns the key and `<data dir>/admin.key` is its only store, on
//! every OS (ADR 0057 WP-4, rule 72): an owner-only file read through `paddock-secrets`, which refuses a file others can
//! read, a link, another user's file and anything that is not a regular file. The app keeps no copy of its own: 1.0.x kept
//! one in the OS credential store, which an ad-hoc signed update cannot read on macOS without a password dialog. The key is
//! never logged and never part of an error message.

use paddock_secrets::SecretFile;
use std::path::PathBuf;
use std::sync::Mutex;

/// Longest key accepted.
pub const MAX_KEY_LEN: usize = 256;

/// Whether `k` looks like a key at all: 1 to 256 printable ASCII characters. Everything else (a damaged file, a
/// stray newline in the middle) is refused before it can reach a header.
pub fn parse_key(raw: &str) -> Option<String> {
    let k = raw.trim();
    (!k.is_empty() && k.len() <= MAX_KEY_LEN && k.bytes().all(|b| b.is_ascii_graphic())).then(|| k.to_string())
}

/// The admin key as the client needs it.
pub trait AdminKeys: Send + Sync {
    /// The key to use now (cached).
    fn current(&self) -> Option<String>;
    /// Reads the key again from the server's file and updates the cache. Called once after an `Unauthorized` answer.
    fn refresh(&self) -> Option<String>;
}

/// The key from the server's `admin.key`, cached until a refresh.
pub struct StoredAdminKey {
    file: SecretFile,
    cached: Mutex<Option<String>>,
    /// The last refusal logged, so a file that stays untrusted is logged once, not on every request.
    last_problem: Mutex<Option<String>>,
}

impl StoredAdminKey {
    /// The key in `file` (the server's `<data dir>/admin.key`).
    pub fn new(file: PathBuf) -> StoredAdminKey {
        StoredAdminKey {
            file: SecretFile::new(file),
            cached: Mutex::new(None),
            last_problem: Mutex::new(None),
        }
    }

    fn read_file(&self) -> Option<String> {
        let read = self.file.read();
        let problem = match &read {
            Ok(Some(k)) if parse_key(k.expose()).is_none() => Some("it does not hold a key".to_string()),
            Err(e) => Some(e.to_string()),
            _ => None,
        };
        let mut last = self.last_problem.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(why) = problem.as_ref().filter(|p| last.as_ref() != Some(*p)) {
            log::warn!(
                target: "pitwall",
                "the admin key in {} is not used: {why}. Restart Pitwall's server (it writes a new key) or fix the file",
                self.file.path().display()
            );
        }
        *last = problem;
        read.ok().flatten().and_then(|k| parse_key(k.expose()))
    }

    fn remember(&self, key: Option<String>) -> Option<String> {
        *self.cached.lock().unwrap_or_else(|p| p.into_inner()) = key.clone();
        key
    }
}

impl AdminKeys for StoredAdminKey {
    fn current(&self) -> Option<String> {
        if let Some(k) = self.cached.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            return Some(k);
        }
        let key = self.read_file();
        self.remember(key)
    }

    fn refresh(&self) -> Option<String> {
        let key = self.read_file();
        self.remember(key)
    }
}

/// A fixed key, for tests.
#[cfg(any(test, feature = "test-support"))]
pub struct StaticKeys(String);

#[cfg(any(test, feature = "test-support"))]
impl StaticKeys {
    /// Always `key`.
    pub fn new(key: &str) -> StaticKeys {
        StaticKeys(key.to_string())
    }
}

#[cfg(any(test, feature = "test-support"))]
impl AdminKeys for StaticKeys {
    fn current(&self) -> Option<String> {
        Some(self.0.clone())
    }
    fn refresh(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The key file as the server writes it: owner-only (0600 on Unix, an owner-only DACL on Windows).
    fn server_writes(file: &Path, key: &str) {
        SecretFile::new(file).write(&paddock_secrets::Secret::new(key).unwrap()).unwrap();
    }

    #[test]
    fn keys_are_checked_before_use() {
        assert_eq!(parse_key("  pw_abc123\n").as_deref(), Some("pw_abc123"));
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("has space"), None);
        assert_eq!(parse_key("two\nlines"), None);
        assert_eq!(parse_key(&"a".repeat(MAX_KEY_LEN + 1)), None);
        assert_eq!(parse_key("pw_\u{e9}"), None);
    }

    #[test]
    fn the_file_is_the_only_source_and_a_refresh_reads_it_again() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("admin.key");
        let keys = StoredAdminKey::new(file.clone());
        assert_eq!(keys.current(), None, "no server has run yet");
        server_writes(&file, "pw_first");
        assert_eq!(keys.current().as_deref(), Some("pw_first"));
        // The server made a new key: the cache holds the old one until a refresh, which trusts the file.
        server_writes(&file, "pw_second");
        assert_eq!(keys.current().as_deref(), Some("pw_first"));
        assert_eq!(keys.refresh().as_deref(), Some("pw_second"));
        assert_eq!(keys.current().as_deref(), Some("pw_second"));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(keys.refresh(), None, "no file: no key, there is no other copy");
    }

    #[test]
    fn a_damaged_key_file_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("admin.key");
        server_writes(&file, "bad key with spaces");
        let keys = StoredAdminKey::new(file.clone());
        assert_eq!(keys.current(), None);
        assert_eq!(keys.refresh(), None, "the same problem is logged once");
        server_writes(&file, &"x".repeat(MAX_KEY_LEN * 3));
        assert_eq!(keys.refresh(), None);
        assert_eq!(StaticKeys::new("s").current().as_deref(), Some("s"));
        assert_eq!(StaticKeys::new("s").refresh().as_deref(), Some("s"));
    }

    /// A key others could read, or a link planted in its place, is not used (ADR 0057: the file is the only store).
    #[cfg(unix)]
    #[test]
    fn a_key_file_others_can_read_or_a_link_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("admin.key");
        server_writes(&file, "pw_key");
        let keys = StoredAdminKey::new(file.clone());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(keys.current(), None, "readable by others: refused");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(keys.refresh().as_deref(), Some("pw_key"));
        let outside = d.path().join("outside");
        server_writes(&outside, "pw_planted");
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(&outside, &file).unwrap();
        assert_eq!(keys.refresh(), None, "a link is never followed");
    }
}
