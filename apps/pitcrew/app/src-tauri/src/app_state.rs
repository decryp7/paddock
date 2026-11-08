//! The app's own small persistent state (`<app data dir>/app-state.json`): which app version last registered the macOS
//! background service. Versioned; a missing, damaged, oversized or newer file falls back to the defaults, and keys added
//! later are optional so older files still load.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The file's name.
pub const FILE_NAME: &str = "app-state.json";
/// The format this build writes and reads.
pub const VERSION: u32 = 1;
/// Largest state file read.
const MAX_BYTES: u64 = 64 * 1024;

/// What the app remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppState {
    /// The format version.
    pub version: u32,
    /// The app version that last registered the macOS background service (every ad-hoc build has a new code identity,
    /// so the registration is replaced once per version).
    pub service_registered_version: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            version: VERSION,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn temp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-appstate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn the_state_round_trips_and_a_missing_file_gives_defaults() {
        let d = temp("roundtrip");
        assert_eq!(AppState::load(d.as_path()), AppState::default());
        let s = AppState {
            service_registered_version: Some("9.9.9".into()),
            ..AppState::default()
        };
        s.save(&d.as_path().join("sub")).unwrap();
        assert_eq!(AppState::load(&d.as_path().join("sub")), s);
    }

    #[test]
    fn an_old_file_without_the_new_key_loads_and_a_damaged_or_newer_one_falls_back() {
        let d = temp("damaged");
        let f = d.as_path().join(FILE_NAME);
        std::fs::write(&f, r#"{"version":1}"#).unwrap();
        assert_eq!(AppState::load(d.as_path()).service_registered_version, None);
        std::fs::write(&f, "{ not json").unwrap();
        assert_eq!(AppState::load(d.as_path()), AppState::default());
        std::fs::write(&f, r#"{"version":99,"serviceRegisteredVersion":"1"}"#).unwrap();
        assert_eq!(AppState::load(d.as_path()), AppState::default());
        std::fs::write(&f, vec![b' '; 70 * 1024]).unwrap();
        assert_eq!(AppState::load(d.as_path()), AppState::default());
    }
}
