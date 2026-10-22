//! Listing a folder: local, a mounted network drive or a UNC share. Listing a share that went away can block for a
//! long time inside the OS, so it runs on its own thread with a deadline, and failures become statuses the user can
//! act on ("not reachable", "no access") rather than raw I/O errors.

use spotter_core::limits::MAX_CANDIDATES;
use spotter_core::source::{FolderEntry, SourceError};
use spotter_core::time::Timestamp;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long listing a folder may take.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether a path is on a network: a UNC path (`\\server\share`, `//server/share`) or a macOS mounted volume.
pub fn is_network_path(path: &str) -> bool {
    path.starts_with(r"\\") || path.starts_with("//") || path.starts_with("/Volumes/")
}

/// Lists `path` (at most [`MAX_CANDIDATES`] entries) within `timeout`.
pub async fn list(path: &str, timeout: Duration) -> Result<Vec<FolderEntry>, SourceError> {
    let owned = PathBuf::from(path);
    let (tx, rx) = tokio::sync::oneshot::channel();
    // A plain thread rather than the blocking pool: a hung share must not hold a pool thread the app needs.
    std::thread::spawn(move || {
        let _ = tx.send(list_now(&owned));
    });
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => result.map_err(|e| explain(path, e)),
        Ok(Err(_)) => Err(SourceError::Failed(format!("Listing {path} stopped unexpectedly."))),
        Err(_) => Err(SourceError::Unreachable(format!(
            "{path} did not answer within {} seconds. The share may be offline or slow; Spotter will try again.",
            timeout.as_secs()
        ))),
    }
}

fn list_now(path: &Path) -> std::io::Result<Vec<FolderEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let Ok(entry) = entry else { continue };
        // Names that are not valid Unicode cannot hold a version Spotter can show; skip them.
        let Ok(name) = entry.file_name().into_string() else { continue };
        let meta = entry.metadata().ok();
        let modified = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| Timestamp(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)));
        out.push(FolderEntry {
            path: entry.path().to_string_lossy().into_owned(),
            name,
            is_dir: meta.as_ref().is_some_and(|m| m.is_dir()),
            size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()),
            modified,
        });
        if out.len() >= MAX_CANDIDATES {
            break;
        }
    }
    Ok(out)
}

fn explain(path: &str, e: std::io::Error) -> SourceError {
    match e.kind() {
        ErrorKind::PermissionDenied => SourceError::Unauthorized(format!(
            "You have no access to {path}. Spotter uses your own sign-in: open the folder once in Explorer or Finder to connect, or ask for access."
        )),
        ErrorKind::NotFound if is_network_path(path) => SourceError::Unreachable(format!(
            "{path} is not reachable. The share may be offline, disconnected or not mounted; Spotter will try again."
        )),
        ErrorKind::NotFound => SourceError::NotFound(format!("The folder {path} does not exist.")),
        ErrorKind::NotADirectory => SourceError::InvalidConfig(format!("{path} is a file, not a folder.")),
        _ if is_network_path(path) => SourceError::Unreachable(format!("{path} is not reachable ({e}). Spotter will try again.")),
        _ => SourceError::Failed(format!("{path} could not be read ({e}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn folders_list_with_metadata() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("R1.01.00T10.txt"), b"abc").unwrap();
        std::fs::create_dir(d.path().join("R1.02.00T01")).unwrap();
        let mut entries = list(&d.path().to_string_lossy(), LIST_TIMEOUT).await.unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].size, entries[0].is_dir), (Some(3), false));
        assert!(entries[0].modified.is_some());
        assert_eq!((entries[1].size, entries[1].is_dir), (None, true));
    }

    #[tokio::test]
    async fn missing_folders_files_and_shares_explain_themselves() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("nope").to_string_lossy().into_owned();
        assert!(matches!(list(&missing, LIST_TIMEOUT).await, Err(SourceError::NotFound(_))));
        let file = d.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        let e = list(&file.to_string_lossy(), LIST_TIMEOUT).await.unwrap_err();
        assert!(matches!(e, SourceError::InvalidConfig(_) | SourceError::Failed(_)), "{e:?}");
    }

    #[test]
    fn errors_map_to_statuses() {
        let io = |k: ErrorKind| std::io::Error::from(k);
        assert!(matches!(
            explain("/x", io(ErrorKind::PermissionDenied)),
            SourceError::Unauthorized(_)
        ));
        assert!(matches!(
            explain(r"\\srv\share", io(ErrorKind::NotFound)),
            SourceError::Unreachable(_)
        ));
        assert!(matches!(
            explain("/Volumes/Builds", io(ErrorKind::NotFound)),
            SourceError::Unreachable(_)
        ));
        assert!(matches!(explain("/x", io(ErrorKind::NotFound)), SourceError::NotFound(_)));
        assert!(matches!(explain("/x", io(ErrorKind::NotADirectory)), SourceError::InvalidConfig(_)));
        assert!(matches!(
            explain("//srv/share", io(ErrorKind::TimedOut)),
            SourceError::Unreachable(_)
        ));
        assert!(matches!(explain("/x", io(ErrorKind::TimedOut)), SourceError::Failed(_)));
        assert!(is_network_path(r"\\s\x") && !is_network_path("C:\\x"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn an_unreachable_share_is_a_status_not_a_hang() {
        // A reserved, never-resolving host name: the OS reports it quickly or the deadline does.
        let e = list(r"\\spotter-test-host.invalid\share", Duration::from_secs(20))
            .await
            .unwrap_err();
        assert!(matches!(e, SourceError::Unreachable(_)), "{e:?}");
    }
}
