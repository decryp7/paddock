//! Where the agent keeps its files: one folder per user (`DYNO_DATA_DIR` overrides it, which is how tests stay out of
//! the real profile).

use std::path::{Path, PathBuf};

/// The agent's files.
#[derive(Debug, Clone)]
pub struct Dirs {
    root: PathBuf,
    /// Where the dated logs are when that is not `<root>/logs` (the app's log folder in an install).
    log_folder: Option<PathBuf>,
    /// The OS cache folder for rebuildable files, when it is not `<root>/cache` (only the current user's real folders).
    cache_folder: Option<PathBuf>,
}

impl Dirs {
    /// The folder for the current user: `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno` or
    /// `$XDG_DATA_HOME/dyno` (`~/.local/share/dyno`); `DYNO_DATA_DIR` wins; the temp folder when nothing is known.
    pub fn current() -> Dirs {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let override_root = env("DYNO_DATA_DIR");
        let cache_folder = if override_root.is_some() {
            None
        } else if cfg!(windows) {
            env("LOCALAPPDATA").map(|d| d.join("Dyno").join("Cache"))
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|h| h.join("Library").join("Caches").join("Dyno"))
        } else {
            env("XDG_CACHE_HOME")
                .map(|d| d.join("dyno"))
                .or_else(|| env("HOME").map(|h| h.join(".cache").join("dyno")))
        };
        let root = override_root.unwrap_or_else(|| {
            let found = if cfg!(windows) {
                env("LOCALAPPDATA").map(|d| d.join("Dyno"))
            } else if cfg!(target_os = "macos") {
                env("HOME").map(|h| h.join("Library").join("Application Support").join("Dyno"))
            } else {
                env("XDG_DATA_HOME")
                    .map(|d| d.join("dyno"))
                    .or_else(|| env("HOME").map(|h| h.join(".local").join("share").join("dyno")))
            };
            found.unwrap_or_else(|| std::env::temp_dir().join("Dyno"))
        });
        Dirs {
            root,
            log_folder: None,
            cache_folder,
        }
    }

    /// A folder chosen by the caller.
    pub fn at(root: impl Into<PathBuf>) -> Dirs {
        Dirs {
            root: root.into(),
            log_folder: None,
            cache_folder: None,
        }
    }

    /// The data folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the macOS agent keeps Wake-on-LAN passwords: owner-only files, never the Keychain (see `dyno_platform::wol::os_store`).
    pub fn secrets_folder(&self) -> PathBuf {
        self.root.join("secrets")
    }

    /// The history database.
    pub fn database(&self) -> PathBuf {
        self.root.join("dyno.db")
    }

    /// The folder for rebuildable files: the OS cache folder (`~/Library/Caches/Dyno`, `$XDG_CACHE_HOME/dyno` or
    /// `~/.cache/dyno`, `%LOCALAPPDATA%\Dyno\Cache`), kept out of backups; `<data folder>/cache` when `DYNO_DATA_DIR` or
    /// [`Dirs::at`] names the data folder (tests, ER-14).
    pub fn cache_folder(&self) -> PathBuf {
        self.cache_folder.clone().unwrap_or_else(|| self.root.join("cache"))
    }

    /// The saved-views cache database (ADR 0029): rebuildable, so never in the data folder.
    pub fn cache_db(&self) -> PathBuf {
        self.cache_folder().join("cache.db")
    }

    /// Where the fetched text of one list subscription (or one geo database) is kept: `<cache folder>/intel/<id>`. The id is
    /// one of Dyno's own slugs (letters, digits and `-`), never user text; anything else is replaced.
    pub fn intel_cache(&self, id: &str) -> PathBuf {
        let slug: String = id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .take(64)
            .collect();
        self.cache_folder().join("intel").join(slug)
    }

    /// The user's own lists, subscriptions and allow entries (user data, so in the data folder, not in the cache).
    pub fn intel_lists(&self) -> PathBuf {
        self.root.join("intel-lists.json")
    }

    /// When each subscription and geo source was last tried: kept outside the cache, so clearing the cache cannot cause an early
    /// fetch.
    pub fn intel_fetch_state(&self) -> PathBuf {
        self.root.join("intel-fetch.json")
    }

    /// Where Dyno keeps tools it extracted itself (`tools/smartmontools-<version>/`): its own folder, no administrator rights.
    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// Where a tool download is staged before the helper gets it (rebuildable, so never user data); the job empties it.
    pub fn tool_staging(&self) -> PathBuf {
        self.cache_folder().join("tools")
    }

    /// The user's settings.
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// The benchmark run history.
    pub fn benchmarks(&self) -> PathBuf {
        self.root.join("benchmarks.json")
    }

    /// The discovery file the app reads to find a running agent.
    pub fn discovery(&self) -> PathBuf {
        self.root.join("agent.json")
    }

    /// The lock only one agent per data folder can hold ([`crate::instance`]).
    pub fn instance_lock(&self) -> PathBuf {
        self.root.join("agent.lock")
    }

    /// The same folders with the dated logs read from `folder` (set once by `run` from the install's log places).
    pub fn with_log_folder(mut self, folder: PathBuf) -> Dirs {
        self.log_folder = Some(folder);
        self
    }

    /// The folder the agent's and the app's dated logs are in: the one `run` named, else this folder's own [`Dirs::logs`], so a
    /// harness on a temporary folder never reads the machine's real logs.
    pub fn log_folder(&self) -> PathBuf {
        self.log_folder.clone().unwrap_or_else(|| self.logs())
    }

    /// The agent's log folder.
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Where plugins live (`<id>/plugin.toml`).
    pub fn plugins(&self) -> PathBuf {
        self.root.join("plugins")
    }

    /// Which kind of folder this is, for the logs (never the user's path): every launch path of an installed agent
    /// (the app's child, the login item, the Windows `Run` entry) must log "user data folder".
    pub fn kind(&self) -> &'static str {
        let overridden = std::env::var_os("DYNO_DATA_DIR").is_some_and(|v| !v.is_empty());
        folder_kind(&self.root, overridden, Dirs::current().root(), &std::env::temp_dir())
    }
}

/// [`Dirs::kind`] with the environment passed in.
fn folder_kind(root: &Path, overridden: bool, current: &Path, temp: &Path) -> &'static str {
    if root != current {
        "caller's folder"
    } else if overridden {
        "DYNO_DATA_DIR"
    } else if root.starts_with(temp) {
        "temp folder (no LOCALAPPDATA or HOME)"
    } else {
        "user data folder"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_live_under_the_root() {
        let d = Dirs::at("/x/dyno");
        assert_eq!(d.database(), Path::new("/x/dyno/dyno.db"));
        assert_eq!(d.discovery(), Path::new("/x/dyno/agent.json"));
        assert_eq!(d.root(), Path::new("/x/dyno"));
        for p in [d.settings(), d.logs(), d.plugins(), d.instance_lock()] {
            assert!(p.starts_with("/x/dyno"));
        }
    }

    #[test]
    fn the_folder_kind_names_no_path() {
        let (cur, tmp) = (Path::new("/u/dyno"), Path::new("/tmp"));
        assert_eq!(folder_kind(cur, false, cur, tmp), "user data folder");
        assert_eq!(folder_kind(cur, true, cur, tmp), "DYNO_DATA_DIR");
        assert_eq!(folder_kind(Path::new("/x"), false, cur, tmp), "caller's folder");
        assert_eq!(
            folder_kind(Path::new("/tmp/Dyno"), false, Path::new("/tmp/Dyno"), tmp),
            "temp folder (no LOCALAPPDATA or HOME)"
        );
        assert!(!Dirs::at("/x").kind().is_empty());
    }

    #[test]
    fn the_cache_database_is_beside_the_data_when_the_data_folder_is_named_and_in_the_cache_folder_otherwise() {
        assert_eq!(Dirs::at("/x/dyno").cache_db(), Path::new("/x/dyno/cache/cache.db"));
        let mut d = Dirs::at("/x/dyno");
        d.cache_folder = Some(PathBuf::from("/c/Dyno"));
        assert_eq!(d.cache_db(), Path::new("/c/Dyno/cache.db"));
        assert!(!d.cache_db().starts_with(d.root()), "never inside the data folder");
    }

    #[test]
    fn rebuildable_intel_files_are_beside_the_data_in_tests_and_in_the_cache_folder_otherwise() {
        let d = Dirs::at("/x/dyno");
        assert_eq!(d.intel_cache("list-1"), Path::new("/x/dyno/cache/intel/list-1"));
        assert_eq!(
            d.intel_cache("../etc"),
            Path::new("/x/dyno/cache/intel/___etc"),
            "an id never leaves the folder"
        );
        let mut d = Dirs::at("/x/dyno");
        d.cache_folder = Some(PathBuf::from("/c/Dyno"));
        assert_eq!(d.intel_cache("a"), Path::new("/c/Dyno/intel/a"));
        assert!(!d.intel_cache("a").starts_with(d.root()), "never inside the data folder");
        assert!(
            d.intel_fetch_state().starts_with(d.root()),
            "the last fetch time is not in the cache"
        );
        assert!(d.intel_lists().starts_with(d.root()));
    }

    #[test]
    fn the_current_user_gets_a_named_folder() {
        let d = Dirs::current();
        assert!(d.root().to_string_lossy().to_lowercase().contains("dyno"));
    }
}
