//! What to download, parsed once at the edge: the URL (with its host for the per-host caps), where the file goes, the pinned
//! checksum and size, and the size limit.

use crate::error::DownloadError;
use std::fmt;
use std::path::{Path, PathBuf};

/// Longest URL a request accepts.
pub const MAX_URL_LEN: usize = 8 * 1024;

/// A SHA-256 digest. Parsed from 64 hex digits (either case); shown as lower-case hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Digest(pub [u8; 32]);

impl Sha256Digest {
    /// Parses 64 hex digits.
    pub fn from_hex(text: &str) -> Result<Self, DownloadError> {
        let t = text.trim().as_bytes();
        let bad = || {
            DownloadError::InvalidRequest(format!(
                "'{}' is not a SHA-256 checksum (64 hex digits)",
                text.trim()
            ))
        };
        if t.len() != 64 {
            return Err(bad());
        }
        let mut out = [0u8; 32];
        for (i, [h, l]) in t.as_chunks::<2>().0.iter().enumerate() {
            let hi = (*h as char).to_digit(16).ok_or_else(bad)?;
            let lo = (*l as char).to_digit(16).ok_or_else(bad)?;
            out[i] = (hi * 16 + lo) as u8;
        }
        Ok(Sha256Digest(out))
    }

    /// Lower-case hex.
    pub fn to_hex(&self) -> String {
        paddock_jobs::hex(&self.0)
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Digest({})", self.to_hex())
    }
}

/// How the downloaded bytes are checked before the file appears under its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    /// The file must have this SHA-256; anything else is deleted and reported as [`DownloadError::ChecksumMismatch`].
    Sha256(Sha256Digest),
    /// No checksum is known in advance (the caller checks a signature, say): the digest is returned for the caller to check.
    Unpinned,
}

/// One file to download. Built with [`Request::new`], which refuses a URL that is not `http`/`https`, too long or without a host,
/// and a destination without a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    url: String,
    host: String,
    dest: PathBuf,
    integrity: Integrity,
    size: Option<u64>,
    max_bytes: Option<u64>,
}

impl Request {
    pub fn new(
        url: &str,
        dest: impl Into<PathBuf>,
        integrity: Integrity,
    ) -> Result<Self, DownloadError> {
        let dest = dest.into();
        let host = host_of(url)?;
        if dest.file_name().is_none() {
            return Err(DownloadError::InvalidRequest(format!(
                "'{}' does not name a file",
                dest.display()
            )));
        }
        Ok(Request {
            url: url.to_string(),
            host,
            dest,
            integrity,
            size: None,
            max_bytes: None,
        })
    }

    /// Pins the size: a server that reports another size is refused before anything is written.
    pub fn size(mut self, bytes: u64) -> Self {
        self.size = Some(bytes);
        self
    }

    /// Lowers (or raises) the size limit of [`Config::max_bytes`](crate::Config::max_bytes) for this file.
    pub fn max_bytes(mut self, bytes: u64) -> Self {
        self.max_bytes = Some(bytes);
        self
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// The host key of the per-host caps: lower-case host and port, without user info.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn dest(&self) -> &Path {
        &self.dest
    }

    pub fn integrity(&self) -> Integrity {
        self.integrity
    }

    pub fn pinned_size(&self) -> Option<u64> {
        self.size
    }

    pub(crate) fn limit(&self, default: u64) -> u64 {
        self.max_bytes.unwrap_or(default)
    }

    /// The partial file next to the destination (`<name>.part`); it becomes the destination by one rename.
    pub fn part_path(&self) -> PathBuf {
        with_suffix(&self.dest, ".part")
    }

    /// What a resumable download remembers (`<name>.part.json`).
    pub fn resume_path(&self) -> PathBuf {
        with_suffix(&self.dest, ".part.json")
    }
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

/// The host key of `url`: `host[:port]` in lower case, without user info. Refuses other schemes, control characters and spaces,
/// URLs over [`MAX_URL_LEN`] and a missing host.
pub fn host_of(url: &str) -> Result<String, DownloadError> {
    let bad = |why: &str| {
        DownloadError::InvalidRequest(format!("cannot download '{}': {why}", shorten(url)))
    };
    if url.len() > MAX_URL_LEN {
        return Err(bad("the address is too long"));
    }
    if url.chars().any(|c| c.is_control() || c == ' ') {
        return Err(bad("the address contains spaces or control characters"));
    }
    let lower = url.to_ascii_lowercase();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|s| lower.strip_prefix(s))
        .ok_or_else(|| bad("only http and https addresses can be downloaded"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    if host.is_empty() || host.starts_with(':') {
        return Err(bad("the address has no host"));
    }
    Ok(host.to_string())
}

fn shorten(url: &str) -> String {
    let base = url.split(['?', '#']).next().unwrap_or_default();
    match base.char_indices().nth(120) {
        Some((i, _)) => format!("{}…", &base[..i]),
        None => base.to_string(),
    }
}
