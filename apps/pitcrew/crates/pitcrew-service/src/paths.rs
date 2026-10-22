//! Where the service keeps its files (ADR 0001).
//!
//! - Windows: `%ProgramData%\Pitcrew` (machine-wide; the service runs as LocalSystem, the folder is writable by
//!   administrators only).
//! - macOS: `~/Library/Application Support/Pitcrew` (the service is the user's launch agent, ADR 0003).

use std::path::PathBuf;

/// The configuration file's name.
pub const CONFIG_FILE: &str = "pitcrew.toml";
/// The last good configuration, written before every save.
pub const CONFIG_BACKUP: &str = "pitcrew.toml.bak";

/// The service's data folder, from the environment the OS gives the service.
pub fn data_dir() -> Option<PathBuf> {
    data_dir_from(|k| std::env::var_os(k).map(PathBuf::from))
}

/// [`data_dir`] with the environment as a function (testable).
pub fn data_dir_from(env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    if cfg!(windows) {
        env("ProgramData").map(|p| p.join("Pitcrew"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support").join("Pitcrew"))
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("pitcrew"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_follows_the_platform_convention() {
        let env = |k: &str| match k {
            "ProgramData" => Some(PathBuf::from("C:\\ProgramData")),
            "HOME" => Some(PathBuf::from("/home/ci")),
            _ => None,
        };
        let dir = data_dir_from(env).unwrap();
        assert!(dir.ends_with("Pitcrew") || dir.ends_with("pitcrew"), "{dir:?}");
        assert_eq!(data_dir_from(|_| None), None);
    }
}
