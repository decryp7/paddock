//! Where the service keeps its files (ADR 0001).
//!
//! - Windows: `%ProgramData%\Pitcrew` (machine-wide; the service runs as LocalSystem, the folder is writable by
//!   administrators only).
//! - macOS: `~/Library/Application Support/Pitcrew` (the service is the user's launch agent, ADR 0003).
//! - Linux: `$XDG_DATA_HOME/Pitcrew`, default `~/.local/share/Pitcrew` (the service is the user's systemd service).

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
    pitcrew_core::service_paths::data_dir(this_os(), &env)
}

/// The folder holding the Unix socket (Linux: `$XDG_RUNTIME_DIR/pitcrew` when set, else the data folder).
pub fn socket_dir() -> Option<PathBuf> {
    pitcrew_core::service_paths::socket_dir(this_os(), &|k| std::env::var_os(k).map(PathBuf::from))
}

fn this_os() -> pitcrew_core::platform::Os {
    use pitcrew_core::platform::Os;
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
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
        assert!(dir.ends_with("Pitcrew"), "{dir:?}");
        assert_eq!(data_dir_from(|_| None), None);
    }
}
