//! The app's own log file: what it did on its own (start-up problems, repairs, failed actions) where the user and
//! support can find it, since stderr is lost when the app starts from Finder or at log-in. One line per event,
//! bounded in size, never fetched content, secrets or file contents.

use std::io::Write;
use std::path::PathBuf;

/// The log grows to at most this size before it starts again (the previous file is kept as `app.log.1`).
pub const MAX_BYTES: u64 = 512 * 1024;

/// The folder the log lives in: `~/Library/Logs/Spotter` on macOS, `%LOCALAPPDATA%\Spotter\logs` on Windows,
/// `$XDG_STATE_HOME/Spotter` (or `~/.local/state/Spotter`) on Linux.
pub fn dir() -> Option<PathBuf> {
    dir_with(|k| std::env::var_os(k).map(PathBuf::from))
}

fn dir_with(env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Logs").join("Spotter"))
    } else if cfg!(windows) {
        env("LOCALAPPDATA").map(|d| d.join("Spotter").join("logs"))
    } else {
        env("XDG_STATE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".local").join("state")))
            .map(|d| d.join("Spotter"))
    }
}

/// Appends `message` with a UTC timestamp; failures are ignored (logging must never break the app).
pub fn write(message: &str) {
    // Unit tests never write to the user's real log folder.
    if let Some(d) = dir().filter(|_| !cfg!(test)) {
        append(&d.join("app.log"), message);
    }
    eprintln!("Spotter: {message}");
}

/// `message` as one bounded line without control characters.
pub fn one_line(message: &str) -> String {
    message.chars().filter(|c| !c.is_control()).take(1000).collect()
}

fn append(path: &std::path::Path, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), one_line(message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_follows_the_platform_and_never_the_real_environment_in_tests() {
        let env = |k: &str| match k {
            "HOME" => Some(PathBuf::from("/home/u")),
            "LOCALAPPDATA" => Some(PathBuf::from("/local")),
            "XDG_STATE_HOME" => Some(PathBuf::from("/state")),
            _ => None,
        };
        let d = dir_with(env).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(d, PathBuf::from("/home/u/Library/Logs/Spotter"));
        } else if cfg!(windows) {
            assert_eq!(d, PathBuf::from("/local/Spotter/logs"));
        } else {
            assert_eq!(d, PathBuf::from("/state/Spotter"));
            let relative = |k: &str| match k {
                "XDG_STATE_HOME" => Some(PathBuf::from("relative")),
                "HOME" => Some(PathBuf::from("/home/u")),
                _ => None,
            };
            assert_eq!(dir_with(relative).unwrap(), PathBuf::from("/home/u/.local/state/Spotter"));
        }
        assert_eq!(dir_with(|_| None), None);
        write("a test line goes to stderr only");
    }

    #[test]
    fn lines_are_appended_bounded_and_rotated() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("app.log");
        append(&p, "first\nwith a newline");
        append(&p, "second");
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.contains("firstwith a newline") && text.contains("second"));
        std::fs::write(&p, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&p, "after");
        assert!(p.with_extension("log.1").exists());
        assert!(std::fs::read_to_string(&p).unwrap().ends_with("after\n"));
        assert_eq!(one_line(&"y".repeat(5000)).len(), 1000);
    }
}
