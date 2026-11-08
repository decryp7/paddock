//! What `install` records for the daemon: the owner (the account that turned the helper on, who may turn it off and read
//! its audit log). Read at start; damage means the daemon refuses to serve (fail closed) and says why. There is no token:
//! who a caller is comes from the OS (`paddock-ipc` identity), never from a secret in a file.

use dyno_core::helper_wire::HelperUser;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The schema of `config.json` (2: the owner; 1 had a token and a single served user).
pub const SCHEMA: u32 = 2;
/// The largest config file the daemon reads.
const MAX_FILE: u64 = 16 * 1024;

/// The installed configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperConfig {
    /// [`SCHEMA`].
    pub schema: u32,
    /// The account that turned the helper on.
    pub owner: HelperUser,
    /// The Dyno version that installed it.
    pub version: String,
    /// Where the app it serves is (the orphan watch removes the helper when it stays gone); none on Linux.
    #[serde(default)]
    pub app: Option<std::path::PathBuf>,
}

/// Why the daemon cannot start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A file is missing or unreadable.
    Unreadable(String),
    /// A file is damaged or of another schema.
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Unreadable(m) | ConfigError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ConfigError {}

impl HelperConfig {
    /// A configuration for `owner`, written by version `version`.
    pub fn new(owner: HelperUser, version: &str) -> HelperConfig {
        HelperConfig {
            schema: SCHEMA,
            owner,
            version: version.to_string(),
            app: None,
        }
    }

    /// With the app location the orphan watch looks for.
    #[must_use]
    pub fn with_app(mut self, app: Option<std::path::PathBuf>) -> Self {
        self.app = app;
        self
    }

    /// Parses the configuration text.
    ///
    /// # Errors
    /// [`ConfigError::Invalid`].
    pub fn parse(text: &str) -> Result<HelperConfig, ConfigError> {
        let c: HelperConfig =
            serde_json::from_str(text).map_err(|e| ConfigError::Invalid(format!("The helper's configuration is damaged: {e}")))?;
        if c.schema != SCHEMA {
            return Err(ConfigError::Invalid(format!(
                "The helper's configuration has schema {}, expected {SCHEMA}. Turn the helper off and on again in Settings > Dyno > Administrator readings.",
                c.schema
            )));
        }
        Ok(c)
    }

    /// Reads the configuration file.
    ///
    /// # Errors
    /// [`ConfigError`].
    pub fn load(path: &Path) -> Result<HelperConfig, ConfigError> {
        let meta = std::fs::metadata(path).map_err(|e| ConfigError::Unreadable(format!("{} cannot be read: {e}", path.display())))?;
        if meta.len() > MAX_FILE {
            return Err(ConfigError::Invalid(format!("{} is too large to be Dyno's file.", path.display())));
        }
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Unreadable(format!("{} cannot be read: {e}", path.display())))?;
        HelperConfig::parse(&text)
    }

    /// The file's text.
    pub fn to_text(&self) -> String {
        // A plain struct of strings and numbers always serialises.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> HelperConfig {
        HelperConfig::new(HelperUser::Uid(501), "1.1.0")
    }

    #[test]
    fn the_configuration_roundtrips_and_damage_is_refused() {
        let c = sample();
        assert_eq!(HelperConfig::parse(&c.to_text()).unwrap(), c);
        assert!(matches!(HelperConfig::parse("{"), Err(ConfigError::Invalid(_))));
        assert!(matches!(HelperConfig::parse(""), Err(ConfigError::Invalid(_))));
        let other = HelperConfig { schema: 1, ..sample() };
        let e = HelperConfig::parse(&other.to_text()).unwrap_err().to_string();
        assert!(e.contains("schema 1") && e.contains("Settings > Dyno > Administrator readings"));
    }

    #[test]
    fn files_are_read_with_a_bound() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        std::fs::write(&cfg, sample().to_text()).unwrap();
        assert_eq!(HelperConfig::load(&cfg).unwrap(), sample());
        let missing = HelperConfig::load(&dir.path().join("missing")).unwrap_err();
        assert!(matches!(missing, ConfigError::Unreadable(_)));
        let big = dir.path().join("big");
        std::fs::write(&big, vec![b'x'; 20_000]).unwrap();
        assert!(matches!(HelperConfig::load(&big), Err(ConfigError::Invalid(_))));
        std::fs::write(&cfg, [0xff, 0xfe]).unwrap();
        assert!(matches!(HelperConfig::load(&cfg), Err(ConfigError::Unreadable(_))));
    }
}
