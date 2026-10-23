//! The app's own small persistent state (`<data dir>/app-state.json`): whether the user has been offered the service
//! yet. Versioned; a missing, damaged or newer file falls back to the defaults (AGENTS.md: new persistent state
//! tolerates missing/corrupt files).
//!
//! Also where the data folder is. The server owns that decision (`pitwall_server::paths::Dirs::for_user`); the shell
//! cannot depend on the server crate, so the rules are repeated here and pinned by tests that use the *target* OS's
//! rules, not the host's.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file's name.
pub const FILE_NAME: &str = "app-state.json";
/// The format this build writes and reads.
pub const VERSION: u32 = 1;
/// Largest state file read.
const MAX_BYTES: u64 = 64 * 1024;

/// The environment the data folder depends on.
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// `PITWALL_HOME`.
    pub pitwall_home: Option<String>,
    /// `HOME`.
    pub home: Option<String>,
    /// `APPDATA` (Windows).
    pub appdata: Option<String>,
    /// `XDG_DATA_HOME`.
    pub xdg_data_home: Option<String>,
}

/// The operating systems with their own path rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux.
    Linux,
}

impl Os {
    /// This build's OS.
    pub const fn host() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

fn join(os: Os, base: &str, parts: &[&str]) -> String {
    let sep = if os == Os::Windows { '\\' } else { '/' };
    let mut out = base.trim_end_matches(['/', '\\']).to_string();
    for p in parts {
        out.push(sep);
        out.push_str(p);
    }
    out
}

/// The data folder on `os` (a string with that OS's separators).
pub fn data_dir_for(os: Os, env: &Env) -> Option<String> {
    if let Some(h) = env.pitwall_home.as_ref().filter(|h| !h.is_empty()) {
        return Some(h.clone());
    }
    match os {
        Os::Windows => env.appdata.as_deref().map(|a| join(os, a, &["Pitwall"])),
        Os::MacOs => env
            .home
            .as_deref()
            .map(|h| join(os, h, &["Library", "Application Support", "Pitwall"])),
        Os::Linux => env
            .xdg_data_home
            .as_deref()
            .filter(|x| x.starts_with('/'))
            .map(|x| join(os, x, &["pitwall"]))
            .or_else(|| env.home.as_deref().map(|h| join(os, h, &[".local", "share", "pitwall"]))),
    }
}

/// The data folder of this user on this OS.
pub fn data_dir() -> Option<PathBuf> {
    let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let env = Env {
        pitwall_home: get("PITWALL_HOME"),
        home: get("HOME").or_else(|| get("USERPROFILE")),
        appdata: get("APPDATA"),
        xdg_data_home: get("XDG_DATA_HOME"),
    };
    data_dir_for(Os::host(), &env).map(PathBuf::from)
}

/// What the app remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppState {
    /// The format version.
    pub version: u32,
    /// The user has answered the "run Pitwall's service?" question (either way); it is not asked again.
    pub service_offered: bool,
    /// The app version that last registered the macOS background service (every ad-hoc build has a new code identity,
    /// so the registration is replaced once per version).
    pub service_registered_version: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            version: VERSION,
            service_offered: false,
            service_registered_version: None,
        }
    }
}

impl AppState {
    /// Reads the state; anything unusable gives the defaults.
    pub fn load(dir: &Path) -> AppState {
        let path = dir.join(FILE_NAME);
        let text = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= MAX_BYTES)
            .and_then(|_| std::fs::read_to_string(&path).ok());
        match text.and_then(|t| serde_json::from_str::<AppState>(&t).ok()) {
            Some(s) if s.version <= VERSION => s,
            _ => AppState::default(),
        }
    }

    /// Writes the state atomically.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(FILE_NAME);
        let tmp = dir.join(format!("{FILE_NAME}.new"));
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// What to do about the service at start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRun {
    /// Ask the user whether to run Pitwall's service (the UI shows the question).
    Ask,
    /// Nothing to ask.
    Nothing,
}

/// The first-run decision: ask once, only when nothing already runs the server and nothing is installed.
pub fn first_run(state: &AppState, service_installed: bool, server_running: bool) -> FirstRun {
    if state.service_offered || service_installed || server_running {
        FirstRun::Nothing
    } else {
        FirstRun::Ask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_folders_follow_the_target_os_rules() {
        let win = Env {
            appdata: Some(r"C:\Users\Ana\AppData\Roaming".into()),
            ..Env::default()
        };
        assert_eq!(data_dir_for(Os::Windows, &win).unwrap(), r"C:\Users\Ana\AppData\Roaming\Pitwall");
        let mac = Env {
            home: Some("/Users/ana".into()),
            ..Env::default()
        };
        assert_eq!(
            data_dir_for(Os::MacOs, &mac).unwrap(),
            "/Users/ana/Library/Application Support/Pitwall"
        );
        let lin = Env {
            home: Some("/home/ana".into()),
            ..Env::default()
        };
        assert_eq!(data_dir_for(Os::Linux, &lin).unwrap(), "/home/ana/.local/share/pitwall");
        let xdg = Env {
            xdg_data_home: Some("/data".into()),
            ..lin.clone()
        };
        assert_eq!(data_dir_for(Os::Linux, &xdg).unwrap(), "/data/pitwall");
        let rel = Env {
            xdg_data_home: Some("data".into()),
            ..lin
        };
        assert_eq!(data_dir_for(Os::Linux, &rel).unwrap(), "/home/ana/.local/share/pitwall");
        let home = Env {
            pitwall_home: Some("/tmp/pw".into()),
            ..Env::default()
        };
        assert_eq!(data_dir_for(Os::Windows, &home).unwrap(), "/tmp/pw");
        assert_eq!(data_dir_for(Os::MacOs, &Env::default()), None);
        let _ = data_dir();
        assert_eq!(Os::host(), Os::host());
    }

    #[test]
    fn state_round_trips_and_damage_gives_defaults() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(AppState::load(d.path()), AppState::default(), "missing file");
        AppState {
            service_offered: true,
            ..AppState::default()
        }
        .save(d.path())
        .unwrap();
        assert!(AppState::load(d.path()).service_offered);
        std::fs::write(d.path().join(FILE_NAME), "{ not json").unwrap();
        assert_eq!(AppState::load(d.path()), AppState::default(), "damaged file");
        std::fs::write(d.path().join(FILE_NAME), r#"{"version": 99, "serviceOffered": true}"#).unwrap();
        assert_eq!(AppState::load(d.path()), AppState::default(), "newer file");
        std::fs::write(d.path().join(FILE_NAME), "x".repeat(MAX_BYTES as usize + 1)).unwrap();
        assert_eq!(AppState::load(d.path()), AppState::default(), "oversized file");
        std::fs::write(d.path().join(FILE_NAME), "{}").unwrap();
        assert_eq!(AppState::load(d.path()), AppState::default(), "missing fields");
    }

    #[test]
    fn the_service_is_offered_once_and_only_when_nothing_runs_the_server() {
        let fresh = AppState::default();
        assert_eq!(first_run(&fresh, false, false), FirstRun::Ask);
        assert_eq!(first_run(&fresh, true, false), FirstRun::Nothing, "installed");
        assert_eq!(first_run(&fresh, false, true), FirstRun::Nothing, "already running");
        let answered = AppState {
            service_offered: true,
            ..fresh
        };
        assert_eq!(first_run(&answered, false, false), FirstRun::Nothing);
    }
}
