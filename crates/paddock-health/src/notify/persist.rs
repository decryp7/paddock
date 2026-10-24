//! Saved messages: a window's project messages (saved by the app with its project, through
//! [`NotificationCentre::record`](super::NotificationCentre::record)) and the app's (a file in the app's data folder).
//! Versioned; a missing, damaged or newer record is an empty list (with a warning), a damaged message is skipped.

use super::centre::Notice;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use std::path::Path;

/// The record version written now.
pub const VERSION: u32 = 1;
/// The app file is never read past this size (it holds at most about a hundred short messages).
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Saved messages with their version.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticesRecord {
    pub version: u32,
    pub notices: Vec<Notice>,
}

impl NoticesRecord {
    /// The current version's record of `notices`.
    pub fn new(notices: Vec<Notice>) -> NoticesRecord {
        NoticesRecord {
            version: VERSION,
            notices,
        }
    }

    /// The messages of a saved record and why not all of them loaded. Nothing saved (`None` or `null`) is an empty list
    /// without a warning.
    pub fn from_value(value: Option<&Value>) -> (Vec<Notice>, Option<String>) {
        let Some(v) = value.filter(|v| !v.is_null()) else {
            return (Vec::new(), None);
        };
        let damaged = || {
            (
                Vec::new(),
                Some("the saved notifications are damaged and were not loaded".to_string()),
            )
        };
        match v.get("version").and_then(Value::as_u64) {
            Some(n) if n == u64::from(VERSION) => {}
            Some(n) if n > u64::from(VERSION) => {
                return (
                    Vec::new(),
                    Some(format!(
                        "the notifications were saved by a newer version of the app (version {n})"
                    )),
                );
            }
            _ => return damaged(),
        }
        let Some(list) = v.get("notices").and_then(Value::as_array) else {
            return damaged();
        };
        let notices: Vec<Notice> = list
            .iter()
            .filter_map(|n| serde_json::from_value(n.clone()).ok())
            .collect();
        let warning = (notices.len() < list.len()).then(|| {
            format!(
                "{} damaged notifications were skipped",
                list.len() - notices.len()
            )
        });
        (notices, warning)
    }
}

/// Reads the app file; missing, oversized or damaged gives an empty list (logged).
pub fn load_file(path: &Path) -> Vec<Notice> {
    let read = || -> Result<Value, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.len() > MAX_FILE_BYTES {
            return Err("the file is too large".into());
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    };
    match read() {
        Ok(v) => {
            let (notices, warning) = NoticesRecord::from_value(Some(&v));
            if let Some(w) = warning {
                log::warn!(target: "paddock_health", "Notifications file: {w}");
            }
            notices
        }
        Err(e) if path.exists() => {
            log::warn!(target: "paddock_health", "The notifications file could not be read and starts empty: {e}");
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

/// Writes the app file atomically (a temporary file next to it, then a rename).
pub fn save_file(path: &Path, notices: &[Notice]) -> io::Result<()> {
    let text =
        serde_json::to_vec(&NoticesRecord::new(notices.to_vec())).map_err(io::Error::other)?;
    write_atomic(path, &text)
}

/// Writes `bytes` to `path` through a temporary file in the same folder (created if missing) and a rename, so a crash
/// leaves the old file or the new one, never half of one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut tmp_name = name.to_os_string();
    tmp_name.push(".tmp");
    let tmp = dir.join(tmp_name);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::centre::{Centre, NewNotice, NoticeId, NoticeScope, NoticeSeverity};
    use serde_json::json;

    fn notices() -> Vec<Notice> {
        let mut c = Centre::new(10);
        c.post(
            NewNotice::new(
                "update:1.2",
                "update",
                NoticeSeverity::Info,
                "Version 1.2 is available",
                NoticeScope::App,
            ),
            NoticeId(1),
            5,
        );
        c.post(
            NewNotice::new(
                "session:crash",
                "session",
                NoticeSeverity::Warning,
                "The app did not close properly",
                NoticeScope::App,
            ),
            NoticeId(2),
            6,
        );
        c.notices().to_vec()
    }

    #[test]
    fn records_round_trip_and_damage_falls_back_to_empty() {
        let v = serde_json::to_value(NoticesRecord::new(notices())).unwrap();
        assert_eq!(NoticesRecord::from_value(Some(&v)), (notices(), None));
        assert_eq!(NoticesRecord::from_value(None), (vec![], None));
        assert_eq!(
            NoticesRecord::from_value(Some(&Value::Null)),
            (vec![], None)
        );
        for bad in [
            json!(1),
            json!({"version": 1}),
            json!({"notices": []}),
            json!({"version": 0, "notices": []}),
        ] {
            assert!(
                NoticesRecord::from_value(Some(&bad))
                    .1
                    .unwrap()
                    .contains("damaged"),
                "{bad}"
            );
        }
        assert!(
            NoticesRecord::from_value(Some(&json!({"version": 3, "notices": []})))
                .1
                .unwrap()
                .contains("newer")
        );
        let mut partly = v.clone();
        partly["notices"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": 1}));
        let (list, w) = NoticesRecord::from_value(Some(&partly));
        assert_eq!(
            (list.len(), w.as_deref()),
            (2, Some("1 damaged notifications were skipped"))
        );
        let back: NoticesRecord = serde_json::from_value(v).unwrap();
        assert_eq!(back, NoticesRecord::new(notices()));
    }

    #[test]
    fn the_app_file_round_trips_and_a_corrupt_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data/notifications.json");
        assert!(load_file(&path).is_empty(), "missing");
        save_file(&path, &notices()).unwrap();
        assert_eq!(load_file(&path), notices());
        assert!(
            !tmp.path().join("data/notifications.json.tmp").exists(),
            "no temporary file left"
        );
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_file(&path).is_empty(), "corrupt");
        std::fs::write(&path, json!({"version": 99, "notices": []}).to_string()).unwrap();
        assert!(load_file(&path).is_empty(), "newer");
        std::fs::write(&path, vec![b' '; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert!(load_file(&path).is_empty(), "too large");
        assert!(load_file(tmp.path()).is_empty(), "a folder cannot be read");
        // Writing into a file where the folder should be fails cleanly.
        let blocked = tmp.path().join("data/notifications.json/x.json");
        assert!(save_file(&blocked, &notices()).is_err());
        // A rename onto a folder fails and leaves no temporary file.
        let folder = tmp.path().join("folder");
        std::fs::create_dir_all(folder.join("inside")).unwrap();
        assert!(write_atomic(&folder, b"x").is_err());
        assert!(!tmp.path().join("folder.tmp").exists());
        assert!(write_atomic(Path::new("/"), b"x").is_err(), "no file name");
    }
}
