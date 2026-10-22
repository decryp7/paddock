//! The app's own log file: what it did on its own (repairs, errors) where the user and support can find it, since
//! stderr is lost when the app starts from Finder or at log-in. One line per event, bounded in size, never secrets or
//! runner output (those are in the service's and runners' logs).

use std::io::Write;
use std::path::PathBuf;

/// The log file grows to at most this size before it starts again (the previous file is kept as `app.log.1`).
pub const MAX_BYTES: u64 = 512 * 1024;

/// Where the log lives: `~/Library/Logs/Pitcrew/app.log` on macOS, `%LOCALAPPDATA%\\Pitcrew\\logs\\app.log` on
/// Windows, `$XDG_STATE_HOME/Pitcrew/app.log` (or `~/.local/state/Pitcrew/app.log`) on Linux.
pub fn path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Logs").join("Pitcrew").join("app.log"))
    } else if cfg!(windows) {
        env("LOCALAPPDATA").map(|d| d.join("Pitcrew").join("logs").join("app.log"))
    } else {
        env("XDG_STATE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".local").join("state")))
            .map(|d| d.join("Pitcrew").join("app.log"))
    }
}

/// Appends `message` with a UTC timestamp; failures are ignored (logging must never break the app).
pub fn write(message: &str) {
    if let Some(p) = path() {
        append(&p, message);
    }
    eprintln!("Pitcrew: {message}");
}

fn append(path: &std::path::Path, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    let line: String = message.chars().filter(|c| !c.is_control()).take(1000).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let _ = writeln!(
            f,
            "{} {line}",
            pitcrew_core::time::format_rfc3339(pitcrew_core::time::Timestamp(ms))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_appended_bounded_and_rotated() {
        let dir = std::env::temp_dir().join(format!("pitcrew-applog-{}", std::process::id()));
        let p = dir.join("app.log");
        append(&p, "first\nwith a newline");
        append(&p, "second");
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.contains("firstwith a newline") && text.contains("second"));
        std::fs::write(&p, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        append(&p, "after");
        assert!(p.with_extension("log.1").exists());
        assert!(std::fs::read_to_string(&p).unwrap().ends_with("after\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
