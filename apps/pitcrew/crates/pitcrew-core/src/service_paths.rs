//! Where the service keeps its files and its socket, per OS (ADR 0001, ADR 0009). Pure: the environment is a
//! function, so the service, the app and the tests agree on the same rules.
//!
//! - Windows: `%ProgramData%\Pitcrew` (machine-wide; the service is LocalSystem). The transport is a named pipe.
//! - macOS: `~/Library/Application Support/Pitcrew`; the socket is in the same folder.
//! - Linux: `$XDG_DATA_HOME/Pitcrew` (default `~/.local/share/Pitcrew`); the socket is in
//!   `$XDG_RUNTIME_DIR/pitcrew` when that is set (a per-user tmpfs, mode 0700, cleaned at log-out), else in the data folder.

use crate::platform::Os;
use std::path::{Path, PathBuf};

/// The Unix socket's file name.
pub const SOCKET_FILE: &str = "pitcrew.sock";

/// An absolute path from an environment variable; XDG says to ignore relative values.
fn absolute(env: &dyn Fn(&str) -> Option<PathBuf>, key: &str) -> Option<PathBuf> {
    env(key).filter(|p| p.is_absolute())
}

/// The service's data folder on `os`, or `None` when the environment names no home.
pub fn data_dir(os: Os, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match os {
        Os::Windows => env("ProgramData").map(|p| p.join("Pitcrew")),
        Os::Macos => env("HOME").map(|h| h.join("Library").join("Application Support").join("Pitcrew")),
        Os::Linux => absolute(env, "XDG_DATA_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".local").join("share")))
            .map(|d| d.join("Pitcrew")),
    }
}

/// The folder holding the service's Unix socket (not used on Windows).
pub fn socket_dir(os: Os, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match os {
        Os::Linux => absolute(env, "XDG_RUNTIME_DIR")
            .map(|r| r.join("pitcrew"))
            .or_else(|| data_dir(os, env)),
        _ => data_dir(os, env),
    }
}

/// The socket's full path.
pub fn socket_path(os: Os, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    socket_dir(os, env).map(|d| d.join(SOCKET_FILE))
}

/// Whether `dir` is one the service may use as a data folder: absolute.
pub fn is_usable(dir: &Path) -> bool {
    dir.is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| PathBuf::from(v))
    }

    #[test]
    fn each_os_has_its_data_folder() {
        let e = env(&[("ProgramData", "C:\\ProgramData"), ("HOME", "/home/ci")]);
        assert_eq!(data_dir(Os::Windows, &e), Some(PathBuf::from("C:\\ProgramData").join("Pitcrew")));
        assert_eq!(
            data_dir(Os::Macos, &e),
            Some(PathBuf::from("/home/ci/Library/Application Support/Pitcrew"))
        );
        assert_eq!(data_dir(Os::Linux, &e), Some(PathBuf::from("/home/ci/.local/share/Pitcrew")));
        assert_eq!(data_dir(Os::Linux, &env(&[])), None);
        assert_eq!(data_dir(Os::Windows, &env(&[("HOME", "/h")])), None);
        assert_eq!(data_dir(Os::Macos, &env(&[])), None);
    }

    #[test]
    fn linux_follows_xdg_but_ignores_relative_values() {
        let e = env(&[("HOME", "/home/ci"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(data_dir(Os::Linux, &e), Some(PathBuf::from("/data/Pitcrew")));
        let rel = env(&[("HOME", "/home/ci"), ("XDG_DATA_HOME", "data"), ("XDG_RUNTIME_DIR", "run")]);
        assert_eq!(data_dir(Os::Linux, &rel), Some(PathBuf::from("/home/ci/.local/share/Pitcrew")));
        assert_eq!(socket_dir(Os::Linux, &rel), Some(PathBuf::from("/home/ci/.local/share/Pitcrew")));
    }

    #[test]
    fn the_linux_socket_prefers_the_runtime_dir() {
        let e = env(&[("HOME", "/home/ci"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
        assert_eq!(socket_dir(Os::Linux, &e), Some(PathBuf::from("/run/user/1000/pitcrew")));
        assert_eq!(
            socket_path(Os::Linux, &e),
            Some(PathBuf::from("/run/user/1000/pitcrew/pitcrew.sock"))
        );
        let no_runtime = env(&[("HOME", "/home/ci")]);
        assert_eq!(
            socket_path(Os::Linux, &no_runtime),
            Some(PathBuf::from("/home/ci/.local/share/Pitcrew/pitcrew.sock"))
        );
        assert_eq!(
            socket_path(Os::Macos, &no_runtime),
            Some(PathBuf::from("/home/ci/Library/Application Support/Pitcrew/pitcrew.sock"))
        );
        assert_eq!(socket_path(Os::Linux, &env(&[])), None);
        assert!(is_usable(Path::new(if cfg!(windows) { "C:\\x" } else { "/x" })));
        assert!(!is_usable(Path::new("x")));
    }
}
