//! The discovery file: how an app finds its running companion (agent, service, worker). It holds the private endpoint,
//! the token, the companion's pid and build, so it is written owner-only (atomically, through `paddock-env`) and removed
//! when the companion stops. A file whose process is gone is stale and ignored; a file anyone else could have written is
//! not trusted.

use crate::frame::ProtocolVersion;
use crate::token::Token;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Largest discovery file read.
pub const MAX_DISCOVERY: u64 = 16 * 1024;

/// What a client needs to connect, and what it compares to decide whether to replace the companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// The endpoint name (socket path or pipe name).
    pub endpoint: String,
    /// The token, 64 hex digits.
    pub token: String,
    /// The companion's process id.
    pub pid: u32,
    /// The companion's build (its version, or version plus commit): a client of another build replaces it.
    pub build: String,
    /// The protocol the companion serves.
    pub protocol: ProtocolVersion,
    /// When it started, ms since the epoch (for messages).
    pub started_ms: i64,
}

/// What [`read`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// No file, or a damaged or oversized one.
    Nothing,
    /// A file whose process is not running any more.
    Stale(Discovery),
    /// A file that others could have written or read (not owner-only, or not ours); never used.
    Untrusted(String),
    /// A file whose process runs.
    Live(Discovery),
}

impl Discovery {
    /// The token, if the file holds a valid one.
    pub fn token(&self) -> Option<Token> {
        Token::from_hex(&self.token)
    }

    /// Writes the file atomically, readable by the owner only.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        paddock_env::write_atomic(path, &body, true).map_err(std::io::Error::other)
    }

    /// Removes the file if it still names process `pid` (a companion leaves another one's file alone).
    pub fn remove_if_owned_by(path: &Path, pid: u32) {
        if let Found::Live(d) | Found::Stale(d) = read(path, |_| true)
            && d.pid == pid
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Reads the discovery file at `path`, with `is_alive` deciding whether its process runs (`paddock_proc::is_alive` in
/// apps; tests inject one).
pub fn read(path: &Path, is_alive: impl Fn(u32) -> bool) -> Found {
    let Ok(meta) = std::fs::metadata(path) else {
        return Found::Nothing;
    };
    if let Err(why) = private(&meta) {
        return Found::Untrusted(why);
    }
    if meta.len() > MAX_DISCOVERY {
        return Found::Nothing;
    }
    let Ok(d) = std::fs::read(path)
        .map_err(|_| ())
        .and_then(|b| serde_json::from_slice::<Discovery>(&b).map_err(|_| ()))
    else {
        return Found::Nothing;
    };
    if is_alive(d.pid) {
        Found::Live(d)
    } else {
        Found::Stale(d)
    }
}

#[cfg(unix)]
fn private(meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and cannot fail.
    #[allow(unsafe_code)]
    let me = unsafe { libc::geteuid() };
    if meta.uid() != me {
        return Err(format!(
            "the discovery file belongs to user {}, not to this user",
            meta.uid()
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(format!(
            "the discovery file is readable or writable by others (mode {:o})",
            meta.mode() & 0o777
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn private(_meta: &std::fs::Metadata) -> Result<(), String> {
    // Windows: the file is in the user's profile (`%LOCALAPPDATA%`), which only the user and administrators may write.
    Ok(())
}
