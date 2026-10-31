//! Where the agent keeps its files: one folder per user (`DYNO_DATA_DIR` overrides it, which is how tests stay out of
//! the real profile).

use std::path::{Path, PathBuf};

/// The agent's files.
#[derive(Debug, Clone)]
pub struct Dirs {
    root: PathBuf,
    /// Where the dated logs are when that is not `<root>/logs` (the app's log folder in an install).
    log_folder: Option<PathBuf>,
}

impl Dirs {
    /// The folder for the current user: `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno` or
    /// `$XDG_DATA_HOME/dyno` (`~/.local/share/dyno`); `DYNO_DATA_DIR` wins; the temp folder when nothing is known.
    pub fn current() -> Dirs {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let root = env("DYNO_DATA_DIR").unwrap_or_else(|| {
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
        Dirs { root, log_folder: None }
    }

    /// A folder chosen by the caller.
    pub fn at(root: impl Into<PathBuf>) -> Dirs {
        Dirs {
            root: root.into(),
            log_folder: None,
        }
    }

    /// The data folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The history database.
    pub fn database(&self) -> PathBuf {
        self.root.join("dyno.db")
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
    fn the_current_user_gets_a_named_folder() {
        let d = Dirs::current();
        assert!(d.root().to_string_lossy().to_lowercase().contains("dyno"));
    }
}
