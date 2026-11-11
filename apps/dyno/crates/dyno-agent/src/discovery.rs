//! The discovery file (`agent.json`): how the app finds a running agent. It holds the private endpoint's name and the
//! per-launch token, so it is written readable by the owner only and removed when the agent exits. A file whose
//! process is gone is stale and ignored.

use paddock_ipc::Token;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The folder holding the discovery file at `path` must be this account's and closed to others, for the writer and every reader
/// alike (one rule, `paddock_env::check_data_folder`).
fn trusted_folder(path: &Path) -> Result<(), paddock_env::Refusal> {
    match path.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => paddock_env::check_data_folder(dir, "agent.json"),
        None => Ok(()),
    }
}

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
    /// The build id of the agent's program file ([`crate::build_id`]); empty in a file an older agent wrote, which
    /// therefore never matches a build the app can name.
    #[serde(default)]
    pub build: String,
}

impl Discovery {
    /// The token as bytes.
    pub fn token(&self) -> Option<Token> {
        Token::from_hex(&self.token)
    }

    /// Writes the file atomically and owner-only through the shared private-folder type (ADR 0092): the folder is made or checked from
    /// its handle (a link to the user's own folder is followed), the temporary file is created exclusive and owner-only inside it
    /// and renamed over the name; a folder others can change is refused and nothing is written.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let (Some(dir), Some(name)) = (
            path.parent().filter(|d| !d.as_os_str().is_empty()),
            path.file_name().and_then(|n| n.to_str()),
        ) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the discovery file needs a folder and a name",
            ));
        };
        let dir = crate::paths::open_private_io(dir, "the discovery file's folder")?;
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        dir.write_atomic(name, &body).map_err(|e| std::io::Error::other(e.to_string()))
    }

    /// Reads the file; `None` when missing or damaged.
    /// Also `None` when the folder holding it belongs to another user or others can change it: a planted file is not even a hint
    /// (the refusal is logged once per change, with its reason). A link to the user's own folder is followed.
    pub fn read(path: &Path) -> Option<Discovery> {
        trusted_folder(path).ok()?;
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

/// Why the discovery file was written again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rewrite {
    /// The file was gone, or unreadable.
    Missing,
    /// The file describes something else (another process, another build, another endpoint).
    Other,
}

/// Makes the file at `path` describe `mine`, the live agent that holds the data folder's lock: when it is missing, damaged or names
/// anything else, it is written again atomically by the same writer as at start (rule 53: only the lock holder writes it, and
/// only about itself). Returns why it was written, or `None` when it already was right.
///
/// # Errors
/// The write failed (the caller logs it and tries again at the next look).
pub fn ensure_describes(path: &Path, mine: &Discovery) -> std::io::Result<Option<Rewrite>> {
    let why = match Discovery::read(path) {
        Some(d) if d == *mine => return Ok(None),
        Some(_) => Rewrite::Other,
        None => Rewrite::Missing,
    };
    mine.write(path)?;
    Ok(Some(why))
}

/// The thread body: every look interval, while this agent holds the data folder's lock, makes sure `agent.json` still describes it
/// (a harness, a cleanup tool or an installer may have deleted it, and nothing else would write it again: every new app would wait
/// for an agent it cannot find). One warning per rewrite, with the reason.
pub fn guard(shared: &crate::agent::Shared, path: std::path::PathBuf, mine: Discovery) {
    while crate::build_id::wait_or_stop(shared, crate::build_id::watch_every()) {
        match ensure_describes(&path, &mine) {
            Ok(None) => {}
            Ok(Some(why)) => {
                // A stop that began meanwhile removes the file as the agent's last act; do not leave one behind it.
                if shared.stopped() {
                    Discovery::remove_if_mine(&path);
                    return;
                }
                log::warn!(
                    Reason = match why {
                        Rewrite::Missing => "the discovery file was missing or unreadable",
                        Rewrite::Other => "the discovery file named another process or build",
                    },
                    AgentPid = mine.pid;
                    "Discovery file written again"
                );
            }
            Err(e) => log::warn!(Why = e.to_string().as_str(); "Discovery file could not be written again; trying at the next look"),
        }
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
            build: String::new(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_data_folder_the_user_owns_is_read_and_written() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        sample().write(&link.join("agent.json")).unwrap();
        assert_eq!(Discovery::read(&link.join("agent.json")), Some(sample()));
        assert_eq!(Discovery::read(&real.join("agent.json")), Some(sample()));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_in_a_folder_others_can_change_is_neither_read_nor_written() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("open");
        std::fs::create_dir(&dir).unwrap();
        sample().write(&dir.join("agent.json")).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(Discovery::read(&dir.join("agent.json")), None);
        let e = sample().write(&dir.join("agent.json")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_of_another_account_is_not_trusted() {
        // `/` belongs to root: following a link to it is refused, so a planted link cannot lead the agent to a foreign folder.
        if paddock_env::current_uid() == 0 {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink("/", &link).unwrap();
        assert_eq!(Discovery::read(&link.join("agent.json")), None);
        assert!(sample().write(&link.join("agent.json")).is_err());
    }

    #[test]
    fn the_file_is_written_again_when_it_is_missing_or_names_another_agent_and_left_alone_when_right() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agent.json");
        let mine = sample();
        assert_eq!(ensure_describes(&path, &mine).unwrap(), Some(Rewrite::Missing), "no file");
        assert_eq!(Discovery::read(&path), Some(mine.clone()));
        assert_eq!(ensure_describes(&path, &mine).unwrap(), None, "a right file is not touched");
        // (the file, why it is written again): another pid, another build, another endpoint, damage.
        let table = [
            (
                Discovery {
                    pid: mine.pid + 1,
                    ..mine.clone()
                },
                Rewrite::Other,
            ),
            (
                Discovery {
                    build: "other".into(),
                    ..mine.clone()
                },
                Rewrite::Other,
            ),
            (
                Discovery {
                    endpoint: "elsewhere".into(),
                    ..mine.clone()
                },
                Rewrite::Other,
            ),
        ];
        for (file, why) in table {
            file.write(&path).unwrap();
            assert_eq!(ensure_describes(&path, &mine).unwrap(), Some(why), "{file:?}");
            assert_eq!(Discovery::read(&path), Some(mine.clone()));
        }
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(
            ensure_describes(&path, &mine).unwrap(),
            Some(Rewrite::Missing),
            "damaged counts as missing"
        );
        assert_eq!(Discovery::read(&path), Some(mine));
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
    #[cfg(unix)]
    #[test]
    fn a_discovery_file_under_a_parent_others_can_change_is_refused_and_not_written() {
        let (_tmp, open) = crate::paths::tests::open_parent();
        let e = sample().write(&open.join("data").join("agent.json")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert!(!open.join("data").join("agent.json").exists(), "nothing was written");
    }

    #[cfg(unix)]
    #[test]
    fn the_discovery_file_is_owner_only_and_no_temporary_file_is_left() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data").join("agent.json");
        sample().write(&path).unwrap();
        sample().write(&path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("agent.json")]);
    }
    #[cfg(windows)]
    #[test]
    fn windows_a_discovery_file_in_a_folder_everyone_can_write_is_refused_and_a_private_one_round_trips() {
        let (tmp, open) = crate::paths::tests::everyone_folder();
        let e = sample().write(&open.join("agent.json")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        assert!(!open.join("agent.json").exists());
        let own = tmp.path().join("own").join("agent.json");
        sample().write(&own).unwrap();
        assert_eq!(Discovery::read(&own), Some(sample()));
    }
}
