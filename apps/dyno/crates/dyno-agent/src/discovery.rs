//! The discovery file (`agent.json`): how the app finds a running agent. It holds the private endpoint's name and the
//! per-launch token, so it is written readable by the owner only and removed when the agent exits. A file whose
//! process is gone is stale and ignored.

use paddock_ipc::Token;
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
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        paddock_proc::hint::write_private_atomic(path, &body)
    }

    /// Reads the file; `None` when missing or damaged.
    pub fn read(path: &Path) -> Option<Discovery> {
        serde_json::from_slice(&paddock_proc::hint::read_bounded(path)?).ok()
    }

    /// Removes the file only if it is still the one that was read as `seen` (same process, same start): a newer agent
    /// may have written its own meanwhile, and that one is never removed.
    pub fn remove_if_same(path: &Path, seen: &Discovery) {
        paddock_proc::hint::remove_if_same(path, seen, Discovery::read);
    }

    /// Removes the file if it still names this process.
    pub fn remove_if_mine(path: &Path) {
        paddock_proc::hint::remove_if_mine(path, Discovery::read);
    }
}

/// A discovery file is only a hint (rule 53): the shared helpers decide when it names a live agent.
impl paddock_proc::hint::Hint for Discovery {
    fn pid(&self) -> u32 {
        self.pid
    }
    fn started_ms(&self) -> i64 {
        self.started
    }
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
    fn a_file_is_removed_as_seen_only_while_it_is_still_that_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.json");
        let seen = sample();
        seen.write(&path).unwrap();
        // A newer agent wrote its own file after it was read: it stays.
        let newer = Discovery {
            started: seen.started + 1,
            ..seen.clone()
        };
        newer.write(&path).unwrap();
        Discovery::remove_if_same(&path, &seen);
        assert_eq!(Discovery::read(&path), Some(newer));
        let other_process = Discovery {
            pid: seen.pid + 1,
            ..seen.clone()
        };
        other_process.write(&path).unwrap();
        Discovery::remove_if_same(&path, &seen);
        assert!(path.exists(), "another process's file stays");
        seen.write(&path).unwrap();
        Discovery::remove_if_same(&path, &seen);
        assert!(!path.exists(), "the very file that was read is removed");
        Discovery::remove_if_same(&path, &seen);
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
