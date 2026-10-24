//! What a stopped download remembers (`<name>.part.json`, versioned): the file it was, and how many leading bytes of the partial
//! file were written and hashed. Only that prefix is resumed; ranges written beyond it are fetched again (at most
//! [`Config::max_buffer`](crate::Config::max_buffer) bytes), because their hash was never taken.

use serde::{Deserialize, Serialize};
use std::path::Path;

const VERSION: u32 = 1;
const MAX_FILE: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResumeState {
    pub version: u32,
    pub url: String,
    pub size: u64,
    /// The pinned SHA-256 (hex), when there was one.
    pub sha256: Option<String>,
    /// The host's strong ETag or Last-Modified.
    pub validator: Option<String>,
    /// Leading bytes written and hashed.
    pub hashed: u64,
}

impl ResumeState {
    pub fn new(url: &str, size: u64, sha256: Option<String>, validator: Option<String>) -> Self {
        ResumeState {
            version: VERSION,
            url: url.to_string(),
            size,
            sha256,
            validator,
            hashed: 0,
        }
    }

    /// The saved state, or `None` when it is missing, too big, corrupt or of another version.
    pub fn load(path: &Path) -> Option<ResumeState> {
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() > MAX_FILE {
            return None;
        }
        let s: ResumeState = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        (s.version == VERSION && s.hashed <= s.size).then_some(s)
    }

    /// True when `self` (saved) describes the file `now` describes: the same size and pinned checksum, and either the same
    /// validator or, without one, the same address under a pinned checksum (which the end of the download checks anyway).
    pub fn matches(&self, now: &ResumeState) -> bool {
        if self.size != now.size || self.sha256 != now.sha256 {
            return false;
        }
        match (&self.validator, &now.validator) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.url == now.url && now.sha256.is_some(),
            _ => false,
        }
    }

    /// Written to a temporary file and renamed into place, so a crash leaves the old or the new state.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let body = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, path)
    }
}
