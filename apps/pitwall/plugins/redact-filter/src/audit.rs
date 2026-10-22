//! The optional audit log: one JSON line per filtered request in `audit.log` in the plugin's data folder, with the
//! time and the counts of what was redacted, never any content. Bounded: past [`MAX_LOG_BYTES`] the log is moved to
//! `audit.log.1` (replacing the previous one) and a new one is started.

use crate::redact::Counts;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The size at which the log is rotated.
pub const MAX_LOG_BYTES: u64 = 4 << 20;

/// The log's file name in the data folder.
pub const LOG_FILE: &str = "audit.log";

/// One line: `{"at":<unix seconds>,"emails":n,"ipv4":n,"secrets":n}`.
pub fn line(at_secs: u64, c: Counts) -> String {
    format!(
        "{{\"at\":{at_secs},\"emails\":{},\"ipv4\":{},\"secrets\":{}}}\n",
        c.emails, c.ipv4, c.secrets
    )
}

/// Appends a line for `counts` to the log in `dir`. Errors are returned for the caller to ignore or report; the
/// filter's answer never depends on the log.
pub fn append(dir: &Path, counts: Counts) -> std::io::Result<()> {
    let path: PathBuf = dir.join(LOG_FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= MAX_LOG_BYTES) {
        std::fs::rename(&path, dir.join(format!("{LOG_FILE}.1")))?;
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line(at, counts).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_hold_counts_only_and_the_log_rotates() {
        let c = Counts {
            emails: 1,
            ipv4: 2,
            secrets: 3,
        };
        assert_eq!(line(5, c), "{\"at\":5,\"emails\":1,\"ipv4\":2,\"secrets\":3}\n");
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), c).unwrap();
        append(dir.path(), Counts::default()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert_eq!(text.lines().count(), 2);
        for l in text.lines() {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
            assert_eq!(keys, ["at", "emails", "ipv4", "secrets"]);
        }
        std::fs::write(dir.path().join(LOG_FILE), vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        append(dir.path(), c).unwrap();
        assert!(std::fs::metadata(dir.path().join("audit.log.1")).unwrap().len() >= MAX_LOG_BYTES);
        assert_eq!(std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap().lines().count(), 1);
        assert!(append(&dir.path().join("missing"), c).is_err());
    }
}
