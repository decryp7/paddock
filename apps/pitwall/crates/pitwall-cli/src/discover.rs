//! Finding the server and the admin key. The decisions are pure (given the environment and a way to read files);
//! the real sources are the process environment, the file system and the OS credential store.
//!
//! The admin key is looked up in this order: `--key-file`, `PITWALL_ADMIN_KEY` (headless boxes), the OS credential
//! store (the entry the Pitwall window keeps), then the server's own `admin.key` in the data folder. It is never
//! printed and never part of an error message.

use pitwall_core::config::DEFAULT_PORT;
use pitwall_core::discovery::{self, Discovery};
use std::path::{Path, PathBuf};

/// The credential-store service name (the same entry the Pitwall window uses).
pub const KEYRING_SERVICE: &str = "Pitwall";
/// The credential-store account name.
pub const KEYRING_ACCOUNT: &str = "admin key";
/// Longest key accepted.
pub const MAX_KEY_LEN: usize = 256;

/// The operating systems whose data folder differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// `%APPDATA%\Pitwall`.
    Windows,
    /// `~/Library/Application Support/Pitwall`.
    Mac,
    /// `$XDG_DATA_HOME/pitwall`.
    Other,
}

impl Os {
    /// This build's OS.
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Other
        }
    }
}

/// The environment variables the CLI reads.
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// `PITWALL_HOME`: overrides the data folder.
    pub pitwall_home: Option<PathBuf>,
    /// `HOME`.
    pub home: Option<PathBuf>,
    /// `APPDATA` (Windows).
    pub appdata: Option<PathBuf>,
    /// `XDG_DATA_HOME`.
    pub xdg_data_home: Option<PathBuf>,
    /// `PITWALL_ADMIN_KEY`.
    pub admin_key: Option<String>,
    /// `PITWALL_API_KEY`.
    pub api_key: Option<String>,
}

impl Env {
    /// Reads the process environment.
    pub fn from_process() -> Env {
        let path = |n: &str| std::env::var_os(n).filter(|v| !v.is_empty()).map(PathBuf::from);
        let text = |n: &str| std::env::var(n).ok().filter(|v| !v.trim().is_empty());
        Env {
            pitwall_home: path("PITWALL_HOME"),
            home: path("HOME"),
            appdata: path("APPDATA"),
            xdg_data_home: path("XDG_DATA_HOME"),
            admin_key: text("PITWALL_ADMIN_KEY"),
            api_key: text("PITWALL_API_KEY"),
        }
    }
}

/// Pitwall's data folder (the same rule as the server's).
pub fn data_dir(env: &Env, os: Os) -> PathBuf {
    if let Some(h) = &env.pitwall_home {
        return h.clone();
    }
    let home = env.home.clone().unwrap_or_default();
    match os {
        Os::Windows => env.appdata.clone().unwrap_or(home).join("Pitwall"),
        Os::Mac => home.join("Library/Application Support/Pitwall"),
        Os::Other => env
            .xdg_data_home
            .clone()
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"))
            .join("pitwall"),
    }
}

/// The port: `--port`, else the discovery file's, else the well-known one. A damaged or foreign discovery file is
/// ignored (the well-known port is tried instead).
pub fn port(flag: Option<u16>, discovery_text: Option<&str>) -> u16 {
    flag.or_else(|| discovery_text.and_then(|t| Discovery::parse(t).ok()).map(|d| d.port))
        .unwrap_or(DEFAULT_PORT)
}

/// The path of the discovery file.
pub fn discovery_path(dir: &Path) -> PathBuf {
    dir.join(discovery::FILE_NAME)
}

/// Whether `raw` looks like a key at all: 1 to 256 printable ASCII characters after trimming.
pub fn parse_key(raw: &str) -> Option<String> {
    let k = raw.trim();
    (!k.is_empty() && k.len() <= MAX_KEY_LEN && k.bytes().all(|b| b.is_ascii_graphic())).then(|| k.to_string())
}

/// Where keys can be read from: the real edge or a test double.
pub trait Sources {
    /// A small text file (`None`: missing or unreadable).
    fn read_file(&self, path: &Path) -> Option<String>;
    /// The OS credential store's admin key.
    fn os_store(&self) -> Option<String>;
}

/// Why no admin key was found.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// `--key-file` does not hold a key.
    #[error("{0} does not hold a usable key. It should contain the admin key on one line.")]
    BadKeyFile(String),
    /// Nothing found anywhere.
    #[error(
        "no admin key found. On the machine where Pitwall runs it is stored by the Pitwall app; on a headless machine copy \
         the server's admin.key from the Pitwall data folder and either set PITWALL_ADMIN_KEY or pass --key-file <path>."
    )]
    Missing,
}

/// The admin key, in the documented order.
pub fn admin_key(key_file: Option<&Path>, env: &Env, dir: &Path, src: &dyn Sources) -> Result<String, KeyError> {
    if let Some(f) = key_file {
        return src
            .read_file(f)
            .and_then(|t| parse_key(&t))
            .ok_or_else(|| KeyError::BadKeyFile(f.display().to_string()));
    }
    env.admin_key
        .as_deref()
        .and_then(parse_key)
        .or_else(|| src.os_store().and_then(|k| parse_key(&k)))
        .or_else(|| src.read_file(&dir.join("admin.key")).and_then(|t| parse_key(&t)))
        .ok_or(KeyError::Missing)
}

/// The real sources.
pub struct RealSources;

impl Sources for RealSources {
    fn read_file(&self, path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() as usize > MAX_KEY_LEN * 4 {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }

    fn os_store(&self) -> Option<String> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok()?.get_password().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fake {
        files: HashMap<PathBuf, String>,
        store: Option<String>,
    }
    impl Sources for Fake {
        fn read_file(&self, p: &Path) -> Option<String> {
            self.files.get(p).cloned()
        }
        fn os_store(&self) -> Option<String> {
            self.store.clone()
        }
    }
    fn fake(files: &[(&str, &str)], store: Option<&str>) -> Fake {
        Fake {
            files: files.iter().map(|(p, t)| (PathBuf::from(p), (*t).to_string())).collect(),
            store: store.map(String::from),
        }
    }

    #[test]
    fn data_dir_per_os() {
        // "Absolute" depends on the host: a Windows path needs a drive.
        let xdg = if cfg!(windows) { "C:/xdg" } else { "/xdg" };
        let env = Env {
            home: Some("/h".into()),
            appdata: Some("/ad".into()),
            xdg_data_home: Some(xdg.into()),
            ..Env::default()
        };
        assert_eq!(data_dir(&env, Os::Windows), PathBuf::from("/ad/Pitwall"));
        assert_eq!(data_dir(&env, Os::Mac), PathBuf::from("/h/Library/Application Support/Pitwall"));
        assert_eq!(data_dir(&env, Os::Other), PathBuf::from(xdg).join("pitwall"));
        let rel = Env {
            home: Some("/h".into()),
            xdg_data_home: Some("rel".into()),
            ..Env::default()
        };
        assert_eq!(data_dir(&rel, Os::Other), PathBuf::from("/h/.local/share/pitwall"));
        let over = Env {
            pitwall_home: Some("/p".into()),
            ..env
        };
        assert_eq!(data_dir(&over, Os::Mac), PathBuf::from("/p"));
        assert_eq!(data_dir(&Env::default(), Os::Windows), PathBuf::from("Pitwall"));
        let _ = Os::current();
    }

    #[test]
    fn port_order() {
        let d = Discovery::new("0.1.0", 1, 12345, 0);
        let text = serde_json::to_string(&d).unwrap();
        assert_eq!(port(Some(2000), Some(&text)), 2000);
        assert_eq!(port(None, Some(&text)), 12345);
        assert_eq!(port(None, Some("{")), DEFAULT_PORT);
        assert_eq!(port(None, Some(&text.replace("127.0.0.1", "evil.example"))), DEFAULT_PORT);
        assert_eq!(port(None, None), DEFAULT_PORT);
        assert!(discovery_path(Path::new("/d")).ends_with("discovery.json"));
    }

    #[test]
    fn key_parsing() {
        assert_eq!(parse_key(" abc123\n").as_deref(), Some("abc123"));
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("a b"), None);
        assert_eq!(parse_key("a\nb"), None);
        assert_eq!(parse_key(&"a".repeat(257)), None);
    }

    #[test]
    fn key_order() {
        let dir = Path::new("/d");
        let env = Env {
            admin_key: Some("fromenv".into()),
            ..Env::default()
        };
        let src = fake(&[("/k", "fromfile\n"), ("/d/admin.key", "datafile")], Some("fromstore"));
        assert_eq!(admin_key(Some(Path::new("/k")), &env, dir, &src).unwrap(), "fromfile");
        assert_eq!(admin_key(None, &env, dir, &src).unwrap(), "fromenv");
        assert_eq!(admin_key(None, &Env::default(), dir, &src).unwrap(), "fromstore");
        assert_eq!(
            admin_key(None, &Env::default(), dir, &fake(&[("/d/admin.key", "datafile")], None)).unwrap(),
            "datafile"
        );
        // a bad env value falls through to the next source
        let bad = Env {
            admin_key: Some("has space".into()),
            ..Env::default()
        };
        assert_eq!(admin_key(None, &bad, dir, &src).unwrap(), "fromstore");
    }

    #[test]
    fn key_errors_never_contain_the_key() {
        let src = fake(&[("/k", "has a space")], None);
        let e = admin_key(Some(Path::new("/k")), &Env::default(), Path::new("/d"), &src).unwrap_err();
        assert!(matches!(e, KeyError::BadKeyFile(_)) && !e.to_string().contains("has a space"));
        let e = admin_key(Some(Path::new("/missing")), &Env::default(), Path::new("/d"), &src).unwrap_err();
        assert!(e.to_string().contains("/missing"));
        let e = admin_key(None, &Env::default(), Path::new("/d"), &fake(&[], None)).unwrap_err();
        assert_eq!(e, KeyError::Missing);
        assert!(e.to_string().contains("PITWALL_ADMIN_KEY"));
    }

    #[test]
    fn real_sources_read_small_files_only() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("k");
        std::fs::write(&f, "abc\n").unwrap();
        assert_eq!(RealSources.read_file(&f).as_deref(), Some("abc\n"));
        assert_eq!(RealSources.read_file(d.path()), None);
        std::fs::write(&f, "a".repeat(5000)).unwrap();
        assert_eq!(RealSources.read_file(&f), None);
        assert_eq!(RealSources.read_file(&d.path().join("none")), None);
    }

    #[test]
    fn env_from_process_does_not_panic() {
        let _ = Env::from_process();
    }
}
