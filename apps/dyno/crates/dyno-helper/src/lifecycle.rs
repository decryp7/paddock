//! The helper's lifecycle file: a few lines saying when it started, served and stopped, and why. `helper.log` is root's (0600), so a
//! standard user's problem report could not show why a helper is down; this file is world-readable and holds no user content (a
//! time, an event word, the process id, the version and a refusal's kind), which is why it can be.
//!
//! Best effort: a helper that cannot write it still runs (the failure is logged). The file is bounded: it keeps the newest
//! [`MAX_LINES`] lines.

use std::io::Write;
use std::path::Path;

/// Lines kept.
pub const MAX_LINES: usize = 60;

/// One line: `t=<unix seconds> <event> pid=<pid> version=<version>`. `event` is a word from a fixed set (no user content):
/// `starting`, `serving`, `stopped`, `refused:<failure kind>`.
pub fn line(at: i64, event: &str, pid: u32, version: &str) -> String {
    format!("t={at} {event} pid={pid} version={version}")
}

/// Appends `line` to `path`, keeping the newest [`MAX_LINES`], and makes the file readable by everyone (Unix 0644).
///
/// # Errors
/// The I/O error; callers log it and carry on.
pub fn append(path: &Path, line: &str) -> std::io::Result<()> {
    let existing = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            return Err(std::io::Error::other("the lifecycle file is a link; it was not written"));
        }
        Ok(_) => std::fs::read_to_string(path).unwrap_or_default(),
        Err(_) => String::new(),
    };
    let mut lines: Vec<&str> = existing.lines().collect();
    lines.push(line);
    let keep = lines.len().saturating_sub(MAX_LINES);
    let mut text = lines[keep..].join("\n");
    text.push('\n');
    let mut file = std::fs::File::create(path)?;
    file.write_all(text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

/// Records `event` now; a failure is logged, never raised.
pub fn record(path: &Path, event: &str) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    if let Err(e) = append(path, &line(at, event, std::process::id(), crate::VERSION)) {
        log::warn!(Error = e.to_string().as_str(); "Helper: the lifecycle file could not be written");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_append_keep_the_newest_and_are_world_readable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lifecycle.log");
        for i in 0..(MAX_LINES + 5) {
            append(&p, &line(1_762_333_200, &format!("e{i}"), 7, "1.1.0")).unwrap();
        }
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), MAX_LINES);
        assert!(text.lines().last().unwrap().contains(&format!("e{}", MAX_LINES + 4)));
        assert!(!text.contains(" e0 "));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o644);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, "keep\n").unwrap();
        let p = dir.path().join("lifecycle.log");
        std::os::unix::fs::symlink(&target, &p).unwrap();
        assert!(append(&p, "x").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
    }

    #[test]
    fn the_line_has_no_free_text() {
        assert_eq!(line(5, "serving", 3, "1.1.0"), "t=5 serving pid=3 version=1.1.0");
    }
}
