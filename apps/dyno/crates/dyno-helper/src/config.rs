//! What `install` records for the daemon: the one authorised user and the agent program that may call. Read at
//! start; damage means the daemon refuses to serve (fail closed) and says why.

use dyno_core::helper_wire::HelperUser;
use dyno_ipc::Token;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The schema of `config.json`.
pub const SCHEMA: u32 = 1;
/// The largest config or token file the daemon reads.
const MAX_FILE: u64 = 16 * 1024;

/// The installed configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperConfig {
    /// [`SCHEMA`].
    pub schema: u32,
    /// The only user the daemon serves.
    pub user: HelperUser,
    /// The only program allowed to call (full path of `dyno-agent`).
    pub agent_exe: PathBuf,
    /// The Dyno version that installed it.
    pub version: String,
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

fn read_bounded(path: &Path) -> Result<String, ConfigError> {
    let meta = std::fs::metadata(path).map_err(|e| ConfigError::Unreadable(format!("{} cannot be read: {e}", path.display())))?;
    if meta.len() > MAX_FILE {
        return Err(ConfigError::Invalid(format!("{} is too large to be Dyno's file.", path.display())));
    }
    std::fs::read_to_string(path).map_err(|e| ConfigError::Unreadable(format!("{} cannot be read: {e}", path.display())))
}

impl HelperConfig {
    /// Parses the configuration text.
    pub fn parse(text: &str) -> Result<HelperConfig, ConfigError> {
        let c: HelperConfig =
            serde_json::from_str(text).map_err(|e| ConfigError::Invalid(format!("The helper's configuration is damaged: {e}")))?;
        if c.schema != SCHEMA {
            return Err(ConfigError::Invalid(format!(
                "The helper's configuration has schema {}, expected {SCHEMA}.",
                c.schema
            )));
        }
        if !c.agent_exe.is_absolute() {
            return Err(ConfigError::Invalid(
                "The helper's configuration names a relative agent path.".into(),
            ));
        }
        Ok(c)
    }

    /// Reads the configuration file.
    pub fn load(path: &Path) -> Result<HelperConfig, ConfigError> {
        HelperConfig::parse(&read_bounded(path)?)
    }

    /// The file's text.
    pub fn to_text(&self) -> String {
        // A plain struct of strings and numbers always serialises.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Reads the token file (64 hex digits, optionally followed by a newline).
pub fn load_token(path: &Path) -> Result<Token, ConfigError> {
    Token::from_hex(read_bounded(path)?.trim())
        .ok_or_else(|| ConfigError::Invalid(format!("{} does not hold a Dyno token.", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> HelperConfig {
        HelperConfig {
            schema: SCHEMA,
            user: HelperUser::Uid(501),
            // Absolute on every platform: the parser checks it against the host, and "/Applications" has no drive on Windows.
            agent_exe: PathBuf::from(if cfg!(windows) {
                r"C:\Applications\Dyno.app\Contents\MacOS\dyno-agent"
            } else {
                "/Applications/Dyno.app/Contents/MacOS/dyno-agent"
            }),
            version: "1.0.0".into(),
        }
    }

    #[test]
    fn the_configuration_roundtrips_and_damage_is_refused() {
        let c = sample();
        assert_eq!(HelperConfig::parse(&c.to_text()).unwrap(), c);
        assert!(matches!(HelperConfig::parse("{"), Err(ConfigError::Invalid(_))));
        assert!(matches!(HelperConfig::parse(""), Err(ConfigError::Invalid(_))));
        let other = HelperConfig { schema: 9, ..sample() };
        assert!(HelperConfig::parse(&other.to_text()).unwrap_err().to_string().contains("schema 9"));
        let relative = HelperConfig {
            agent_exe: PathBuf::from("dyno-agent"),
            ..sample()
        };
        assert!(HelperConfig::parse(&relative.to_text()).is_err());
    }

    #[test]
    fn files_are_read_with_a_bound_and_tokens_must_be_hex() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        std::fs::write(&cfg, sample().to_text()).unwrap();
        assert_eq!(HelperConfig::load(&cfg).unwrap(), sample());
        assert!(matches!(
            HelperConfig::load(&dir.path().join("missing")),
            Err(ConfigError::Unreadable(_))
        ));
        let big = dir.path().join("big");
        std::fs::write(&big, vec![b'x'; 20_000]).unwrap();
        assert!(matches!(HelperConfig::load(&big), Err(ConfigError::Invalid(_))));
        let tok = dir.path().join("token");
        let t = Token::random().unwrap();
        std::fs::write(&tok, format!("{}\n", t.to_hex())).unwrap();
        assert!(load_token(&tok).unwrap().matches(t.as_bytes()));
        std::fs::write(&tok, "not hex").unwrap();
        assert!(matches!(load_token(&tok), Err(ConfigError::Invalid(_))));
    }
}
