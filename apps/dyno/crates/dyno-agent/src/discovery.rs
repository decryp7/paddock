//! The discovery file (`agent.json`): how the app finds a running agent. It holds the private endpoint's name and the
//! per-launch token, so it is written readable by the owner only and removed when the agent exits. A file whose
//! process is gone is stale and ignored.

use dyno_ipc::Token;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What the app needs to connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// The IPC endpoint name.
    pub endpoint: String,
    /// The token, hex.
    pub token: String,
    /// The agent's process id.
    pub pid: u32,
    /// When it started (ms since the epoch).
    pub started: i64,
    /// The agent build (`CARGO_PKG_VERSION`), so a newer app can ask an older agent to restart.
    pub version: String,
    /// The app ↔ agent protocol version.
    pub protocol: u32,
}

impl Discovery {
    /// The token as bytes.
    pub fn token(&self) -> Option<Token> {
        Token::from_hex(&self.token)
    }

    /// Writes the file atomically with owner-only permissions.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        write_private(&tmp, &body)?;
        std::fs::rename(&tmp, path)
    }

    /// Reads the file; `None` when missing or damaged.
    pub fn read(path: &Path) -> Option<Discovery> {
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() > 16 * 1024 {
            return None;
        }
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// Removes the file if it still names this process.
    pub fn remove_if_mine(path: &Path) {
        if Discovery::read(path).is_some_and(|d| d.pid == std::process::id()) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(unix)]
fn write_private(path: &Path, body: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(body)
}

#[cfg(not(unix))]
fn write_private(path: &Path, body: &[u8]) -> std::io::Result<()> {
    // The data folder is under the user's profile, which Windows already restricts to the user.
    std::fs::write(path, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Discovery {
        Discovery {
            endpoint: "/tmp/x/dyno.sock".into(),
            token: Token::from_bytes([7; 32]).to_hex(),
            pid: std::process::id(),
            started: 5,
            version: "0.1.0".into(),
            protocol: 1,
        }
    }

    #[test]
    fn round_trips_privately_and_is_removed_by_its_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("agent.json");
        let d = sample();
        d.write(&path).unwrap();
        assert_eq!(Discovery::read(&path), Some(d.clone()));
        assert!(d.token().unwrap().matches(&[7; 32]));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let mut other = d.clone();
        other.pid = std::process::id() + 1;
        other.write(&path).unwrap();
        Discovery::remove_if_mine(&path);
        assert!(path.exists(), "another agent's file stays");
        d.write(&path).unwrap();
        Discovery::remove_if_mine(&path);
        assert!(!path.exists());
    }

    #[test]
    fn damaged_or_huge_files_are_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.json");
        assert_eq!(Discovery::read(&path), None);
        std::fs::write(&path, b"{nope").unwrap();
        assert_eq!(Discovery::read(&path), None);
        std::fs::write(&path, vec![b' '; 20_000]).unwrap();
        assert_eq!(Discovery::read(&path), None);
        assert!(Discovery {
            token: "zz".into(),
            ..sample()
        }
        .token()
        .is_none());
    }
}
