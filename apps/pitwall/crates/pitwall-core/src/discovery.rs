//! The discovery file (ADR 0009): how apps find a running Pitwall. The server writes `discovery.json` into the
//! user's Pitwall data folder (`%APPDATA%\Pitwall`, `~/Library/Application Support/Pitwall`,
//! `$XDG_DATA_HOME/pitwall`) when it starts and removes it when it stops. It holds no secret.
//!
//! An app reads it, checks that `pid` is alive and `apiVersion` is one it speaks, then calls `GET /x/v1/health`.
//! Without the file it may try the well-known port [`crate::config::DEFAULT_PORT`] on 127.0.0.1.

use serde::{Deserialize, Serialize};

/// The file's name in the data folder.
pub const FILE_NAME: &str = "discovery.json";
/// Largest discovery file an app should read.
pub const MAX_BYTES: usize = 16 * 1024;

/// The file's contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// The file format (1).
    pub format: u32,
    /// Pitwall's version.
    pub version: String,
    /// The client contract version (`/x/v1`).
    pub api_version: u32,
    /// The server's process id.
    pub pid: u32,
    /// The loopback port.
    pub port: u16,
    /// `http://127.0.0.1:<port>`.
    pub base_url: String,
    /// When the server started (Unix ms).
    pub started_at: u64,
}

impl Discovery {
    /// The record for a server on `port`.
    pub fn new(version: &str, pid: u32, port: u16, started_at: u64) -> Discovery {
        Discovery {
            format: 1,
            version: version.into(),
            api_version: crate::API_VERSION,
            pid,
            port,
            base_url: format!("http://127.0.0.1:{port}"),
            started_at,
        }
    }

    /// Parses the file as an app would: bounded, and only a loopback base URL is trusted.
    pub fn parse(text: &str) -> Result<Discovery, String> {
        if text.len() > MAX_BYTES {
            return Err("the discovery file is too large".into());
        }
        let d: Discovery = serde_json::from_str(text).map_err(|e| format!("the discovery file is damaged: {e}"))?;
        if d.base_url != format!("http://127.0.0.1:{}", d.port) || d.port < 1024 {
            return Err("the discovery file does not point at this machine".into());
        }
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_checks() {
        let d = Discovery::new("0.1.0", 42, 11435, 7);
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"apiVersion\":1") && json.contains("\"baseUrl\":\"http://127.0.0.1:11435\""));
        assert_eq!(Discovery::parse(&json).unwrap(), d);
        let evil = json.replace("http://127.0.0.1:11435", "http://evil.com:11435");
        assert!(Discovery::parse(&evil).unwrap_err().contains("this machine"));
        assert!(Discovery::parse("{").unwrap_err().contains("damaged"));
        assert!(Discovery::parse(&" ".repeat(MAX_BYTES + 1)).unwrap_err().contains("too large"));
    }
}
