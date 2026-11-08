//! Typed errors. Each message says what happened and what the user can do; the app edge shows it as is.

use paddock_jobs::{CancelReason, JobError};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Why a [`Transport`](crate::Transport) request failed before a status line arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportErrorKind {
    /// The host could not be reached (no network, name not found, refused, reset).
    Connect,
    /// No answer in time.
    Timeout,
    /// The secure connection could not be verified (a TLS-inspecting proxy without its root certificate, a wrong clock).
    Tls,
    /// The configured proxy failed.
    Proxy,
    /// The address or the request cannot be sent at all.
    Invalid,
}

/// A failed request; `message` is the technical cause without query strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    pub kind: TransportErrorKind,
    pub message: String,
}

impl TransportError {
    pub fn new(kind: TransportErrorKind, message: impl Into<String>) -> Self {
        TransportError {
            kind,
            message: message.into(),
        }
    }

    /// Worth another attempt: the host could not be reached or did not answer in time.
    pub fn is_transient(&self) -> bool {
        matches!(
            self.kind,
            TransportErrorKind::Connect | TransportErrorKind::Timeout
        )
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            TransportErrorKind::Connect => "cannot connect",
            TransportErrorKind::Timeout => "no answer in time",
            TransportErrorKind::Tls => "the secure connection could not be verified",
            TransportErrorKind::Proxy => "the proxy failed",
            TransportErrorKind::Invalid => "the request is invalid",
        };
        write!(f, "{what} ({})", self.message)
    }
}

impl std::error::Error for TransportError {}

/// Why a download ended without a file. The partial file is kept for a resume after [`Cancelled`](Self::Cancelled),
/// [`Network`](Self::Network), [`Throttled`](Self::Throttled), [`Stalled`](Self::Stalled) and [`Io`](Self::Io) (when the host
/// serves byte ranges), and deleted after the others.
#[derive(Debug)]
pub enum DownloadError {
    /// The request itself is unusable (scheme, host, file name, checksum text).
    InvalidRequest(String),
    /// Stopped on request or at the job's deadline.
    Cancelled(CancelReason),
    /// The host answered with an error status that another attempt will not change.
    Http { host: String, status: u16 },
    /// The connection failed and kept failing.
    Network { host: String, detail: String },
    /// The host kept answering 429 or 503 even on one connection.
    Throttled { host: String },
    /// No byte arrived for the configured stall time.
    Stalled { host: String, secs: u64 },
    /// The file is (or grew) larger than the limit.
    TooLarge { size: u64, limit: u64 },
    /// The answer of a small GET ([`Fetch`](crate::Fetch)) is (or grew) larger than its limit.
    ReplyTooLarge { size: u64, limit: u64 },
    /// The host reports another size than the pinned one, or sent more or fewer bytes than it announced.
    SizeMismatch { expected: u64, actual: u64 },
    /// The bytes do not have the pinned SHA-256.
    ChecksumMismatch { expected: String, actual: String },
    /// The file changed on the host during the download, or the host broke its byte-range answers.
    Changed { host: String, detail: String },
    /// The host's answer cannot be used (a compressed body, another range than asked for).
    Protocol { host: String, detail: String },
    /// The disk is full.
    DiskFull,
    /// The destination's disk has not enough free space for the file (checked before writing): `needed` bytes are still to
    /// be written plus the reserve, `free` are available on the disk that holds `path` (the destination folder, absolute).
    NotEnoughSpace {
        needed: u64,
        free: u64,
        path: PathBuf,
    },
    /// Reading or writing the local file failed.
    Io(io::Error),
}

impl DownloadError {
    /// True when the partial file is kept, so running the same request again resumes.
    pub fn keeps_partial(&self) -> bool {
        matches!(
            self,
            DownloadError::Cancelled(_)
                | DownloadError::Network { .. }
                | DownloadError::Throttled { .. }
                | DownloadError::Stalled { .. }
                | DownloadError::DiskFull
                | DownloadError::NotEnoughSpace { .. }
                | DownloadError::Io(_)
        )
    }
}

impl From<io::Error> for DownloadError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::StorageFull {
            DownloadError::DiskFull
        } else {
            DownloadError::Io(e)
        }
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadError::InvalidRequest(m) => f.write_str(m),
            DownloadError::Cancelled(r) => write!(
                f,
                "the download was cancelled ({r}); run it again to resume"
            ),
            DownloadError::Http { host, status } => write!(
                f,
                "{host} answered HTTP {status}; check the address or try again later"
            ),
            DownloadError::Network { host, detail } => {
                write!(
                    f,
                    "the connection to {host} kept failing ({detail}); check the network and run it again to resume"
                )
            }
            DownloadError::Throttled { host } => write!(
                f,
                "{host} is limiting downloads (HTTP 429); try again later to resume"
            ),
            DownloadError::Stalled { host, secs } => {
                write!(
                    f,
                    "no data arrived from {host} for {secs} s; run it again to resume"
                )
            }
            DownloadError::ReplyTooLarge { size, limit } => {
                write!(
                    f,
                    "the answer is {size} bytes, above the {limit}-byte limit of a small request"
                )
            }
            DownloadError::TooLarge { size, limit } => {
                write!(
                    f,
                    "the file is {} bytes, above the {limit}-byte download limit",
                    size
                )
            }
            DownloadError::SizeMismatch { expected, actual } => write!(
                f,
                "the file should be {expected} bytes but the host has {actual}; the partial file was deleted, please try again"
            ),
            DownloadError::ChecksumMismatch { expected, actual } => write!(
                f,
                "the checksum does not match (expected {}…, got {}…); the file was deleted, please try again",
                &expected[..expected.len().min(12)],
                &actual[..actual.len().min(12)]
            ),
            DownloadError::Changed { host, detail } => {
                write!(
                    f,
                    "the file changed on {host} during the download ({detail}); the partial file was deleted, please try again"
                )
            }
            DownloadError::Protocol { host, detail } => {
                write!(
                    f,
                    "{host} sent an answer that cannot be used ({detail}); the partial file was deleted, please try again"
                )
            }
            DownloadError::DiskFull => {
                f.write_str("the disk is full; free some space and run it again to resume")
            }
            DownloadError::NotEnoughSpace { needed, free, path } => write!(
                f,
                "the download needs {} of free space in {}{}, {} is free; free some space there or choose another folder, then run it again",
                human_bytes(*needed),
                path.display(),
                drive_of(path)
                    .map(|d| format!(" (drive {d})"))
                    .unwrap_or_default(),
                human_bytes(*free)
            ),
            DownloadError::Io(e) => write!(f, "the file could not be written ({e})"),
        }
    }
}

impl std::error::Error for DownloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DownloadError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// The drive letter of a Windows path (`C:`), `None` elsewhere.
fn drive_of(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};
    match path.components().next()? {
        Component::Prefix(p) => match p.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => Some(format!("{}:", d as char)),
            _ => None,
        },
        _ => None,
    }
}

/// A size for people, in decimal units as the operating systems show disk space: `1.5 GB`, `700 MB`, `12 bytes`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(1_000_000_000, "GB"), (1_000_000, "MB"), (1_000, "KB")];
    match UNITS.iter().find(|(size, _)| n >= *size) {
        Some((size, unit)) => {
            let tenths = (n as u128 * 10 + *size as u128 / 2) / *size as u128;
            if tenths >= 1000 || tenths.is_multiple_of(10) {
                format!("{} {unit}", (tenths + 5) / 10)
            } else {
                format!("{}.{} {unit}", tenths / 10, tenths % 10)
            }
        }
        None => format!("{n} bytes"),
    }
}

impl From<DownloadError> for JobError {
    fn from(e: DownloadError) -> Self {
        match e {
            DownloadError::Cancelled(r) => JobError::Cancelled(r),
            other => JobError::Failed(other.to_string()),
        }
    }
}
