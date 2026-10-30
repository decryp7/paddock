//! Typed update errors: what went wrong, said for the user, with the next step. Errors of the download crate and of the
//! network are classified once, here ([`UpdateError::from_download`], [`UpdateError::from_transport`]); everything after
//! works with [`UpdateError`].

use paddock_download::{DownloadError, TransportError, TransportErrorKind, human_bytes};
use serde::Serialize;

/// Update downloads larger than this are refused unless the app sets another limit (an installer is well under 200 MB).
pub const DEFAULT_MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// Why checking, downloading or installing an update failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// The build has no update public key (or no release address), so nothing could be verified: updates are off.
    NotAvailable,
    /// No network (the server's name could not be resolved, or no route).
    Offline,
    /// The app's Offline mode is on (local first: every network path is the user's explicit choice), so nothing was asked
    /// of the update server. The app's predicate decides ([`Updater::with_offline_mode`](crate::Updater::with_offline_mode)).
    OfflineMode,
    /// The update server did not answer, timed out or failed.
    Unreachable {
        detail: String,
    },
    /// The server answered without release information (no published release, or a 404).
    NoRelease,
    /// The release has no update for this platform.
    NoPlatform {
        detail: String,
    },
    /// The release information is malformed or does not pass validation; it is not used.
    InvalidManifest {
        detail: String,
    },
    /// The downloaded update does not carry a valid signature from the app's signing key, or the signature was made for
    /// another version than the one announced: refused and deleted.
    SignatureInvalid {
        detail: String,
    },
    /// The update is larger than the download limit.
    TooLarge {
        limit: u64,
    },
    /// There is not enough free space for the download (`detail` says how much is needed and free).
    DiskFull {
        detail: String,
    },
    /// The update could not be stored (the data folder is not writable).
    Storage {
        detail: String,
    },
    InstallFailed {
        detail: String,
    },
    /// The request does not fit what the updater is doing now (e.g. downloading twice).
    Busy {
        detail: String,
    },
    /// The request itself is not valid (an unknown release channel).
    Invalid {
        detail: String,
    },
}

/// The wire form: a stable `kind` for the UI's actions, plus texts it shows as they are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UpdateErrorDto {
    pub kind: UpdateErrorKind,
    pub message: String,
    pub next_step: String,
    pub detail: Option<String>,
}

/// The kind of an [`UpdateError`] on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum UpdateErrorKind {
    NotAvailable,
    Offline,
    OfflineMode,
    Unreachable,
    NoRelease,
    NoPlatform,
    InvalidManifest,
    SignatureInvalid,
    TooLarge,
    DiskFull,
    Storage,
    InstallFailed,
    Busy,
    Invalid,
}

impl UpdateError {
    pub fn kind(&self) -> UpdateErrorKind {
        use UpdateErrorKind as K;
        match self {
            UpdateError::NotAvailable => K::NotAvailable,
            UpdateError::Offline => K::Offline,
            UpdateError::OfflineMode => K::OfflineMode,
            UpdateError::Unreachable { .. } => K::Unreachable,
            UpdateError::NoRelease => K::NoRelease,
            UpdateError::NoPlatform { .. } => K::NoPlatform,
            UpdateError::InvalidManifest { .. } => K::InvalidManifest,
            UpdateError::SignatureInvalid { .. } => K::SignatureInvalid,
            UpdateError::TooLarge { .. } => K::TooLarge,
            UpdateError::DiskFull { .. } => K::DiskFull,
            UpdateError::Storage { .. } => K::Storage,
            UpdateError::InstallFailed { .. } => K::InstallFailed,
            UpdateError::Busy { .. } => K::Busy,
            UpdateError::Invalid { .. } => K::Invalid,
        }
    }

    pub fn message(&self) -> String {
        match self {
            UpdateError::NotAvailable => "Updates are not available in this build.".into(),
            UpdateError::Offline => "The network could not be reached.".into(),
            UpdateError::OfflineMode => {
                "Offline mode is on, so the update server was not asked.".into()
            }
            UpdateError::Unreachable { .. } => "The update server could not be reached.".into(),
            UpdateError::NoRelease => "The update server has no release information.".into(),
            UpdateError::NoPlatform { .. } => {
                "The latest release has no update for this system.".into()
            }
            UpdateError::InvalidManifest { .. } => {
                "The update information from the server is invalid, so it was ignored.".into()
            }
            UpdateError::SignatureInvalid { .. } => {
                "The update's signature is not valid, so the update was refused.".into()
            }
            UpdateError::TooLarge { limit } => {
                format!(
                    "The update is larger than {}, so it was not downloaded.",
                    human_bytes(*limit)
                )
            }
            UpdateError::DiskFull { .. } => "There is not enough disk space for the update.".into(),
            UpdateError::Storage { .. } => "The update could not be saved.".into(),
            UpdateError::InstallFailed { .. } => "The update could not be installed.".into(),
            UpdateError::Busy { detail } | UpdateError::Invalid { detail } => detail.clone(),
        }
    }

    pub fn next_step(&self) -> &'static str {
        match self {
            UpdateError::NotAvailable => {
                "This build was made without an update signing key, so it cannot verify updates. Download new versions from the releases page."
            }
            UpdateError::Offline => "Check your internet connection or proxy, then try again.",
            UpdateError::OfflineMode => "Turn Offline mode off in Settings to check for updates.",
            UpdateError::Unreachable { .. } => {
                "Try again later. If it keeps failing, download the new version from the releases page."
            }
            UpdateError::NoRelease => {
                "There may be no published release yet. Try again later or look at the releases page."
            }
            UpdateError::NoPlatform { .. } => {
                "Download the installer for your system from the releases page."
            }
            UpdateError::InvalidManifest { .. } => {
                "Try again later; if it persists, report it to the maintainers."
            }
            UpdateError::SignatureInvalid { .. } => {
                "The download was deleted. Do not install it by hand; check for updates again, and report this to the maintainers if it repeats."
            }
            UpdateError::TooLarge { .. } => "Download the installer from the releases page.",
            UpdateError::DiskFull { .. } => "Free some disk space, then try again.",
            UpdateError::Storage { .. } => {
                "Check that the app's data folder is writable, then try again."
            }
            UpdateError::InstallFailed { .. } => {
                "Try again, or install the new version from the releases page."
            }
            UpdateError::Busy { .. } => "Wait for the current step to finish.",
            UpdateError::Invalid { .. } => "Choose one of the options offered.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            UpdateError::Unreachable { detail }
            | UpdateError::NoPlatform { detail }
            | UpdateError::InvalidManifest { detail }
            | UpdateError::SignatureInvalid { detail }
            | UpdateError::DiskFull { detail }
            | UpdateError::Storage { detail }
            | UpdateError::InstallFailed { detail } => Some(detail.clone()),
            _ => None,
        }
    }

    /// A download can simply be tried again (not after a refused signature or a size over the limit: check again first).
    pub fn retry_download(&self) -> bool {
        !matches!(
            self,
            UpdateError::SignatureInvalid { .. }
                | UpdateError::TooLarge { .. }
                | UpdateError::NotAvailable
        )
    }

    pub fn to_dto(&self) -> UpdateErrorDto {
        UpdateErrorDto {
            kind: self.kind(),
            message: self.message(),
            next_step: self.next_step().into(),
            detail: self.detail(),
        }
    }

    /// A failed request for the release information (no status line).
    pub fn from_transport(e: &TransportError) -> UpdateError {
        match e.kind {
            TransportErrorKind::Connect if looks_offline(&e.message) => UpdateError::Offline,
            _ => UpdateError::Unreachable {
                detail: e.to_string(),
            },
        }
    }

    /// A failed update download. A cancel is not an error of the update (the caller handles it first); it maps to `Busy`.
    pub fn from_download(e: &DownloadError) -> UpdateError {
        let detail = e.to_string();
        match e {
            DownloadError::Network { detail: d, .. } if looks_offline(d) => UpdateError::Offline,
            DownloadError::TooLarge { limit, .. } => UpdateError::TooLarge { limit: *limit },
            DownloadError::DiskFull | DownloadError::NotEnoughSpace { .. } => {
                UpdateError::DiskFull { detail }
            }
            DownloadError::Io(_) => UpdateError::Storage { detail },
            DownloadError::InvalidRequest(_) => UpdateError::InvalidManifest { detail },
            DownloadError::Cancelled(_) => UpdateError::Busy { detail },
            _ => UpdateError::Unreachable { detail },
        }
    }
}

impl From<UpdateError> for UpdateErrorDto {
    fn from(e: UpdateError) -> UpdateErrorDto {
        e.to_dto()
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.detail() {
            Some(d) => write!(f, "{} ({d})", self.message()),
            None => f.write_str(&self.message()),
        }
    }
}

impl std::error::Error for UpdateError {}

impl Serialize for UpdateError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_dto().serialize(s)
    }
}

/// A connection failure that means "no network" rather than "server down": name resolution failed, or no route.
pub fn looks_offline(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "dns error",
        "failed to lookup address",
        "no such host is known",
        "name or service not known",
        "nodename nor servname",
        "temporary failure in name resolution",
        "network is unreachable",
        "no route to host",
    ]
    .iter()
    .any(|p| t.contains(p))
}
