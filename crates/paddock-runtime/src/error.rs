use paddock_download::DownloadError;
use paddock_jobs::{CancelReason, Cancelled, JobError};
use std::fmt;
use std::path::PathBuf;

/// What can go wrong with a pinned runtime. Messages say what happened and what to do; none contains file content.
#[derive(Debug)]
pub enum RuntimeError {
    /// The catalogue built into the app is not valid (a build defect; the app's own test catches it).
    Catalogue(String),
    /// The catalogue pins no build for this platform.
    NotPinned {
        /// The runtime ("chromium").
        name: String,
        /// The platform key ("linux-aarch64"), or "this platform" when it has no key.
        platform: String,
    },
    /// The download failed; [`DownloadError::keeps_partial`] says whether a retry resumes.
    Download(DownloadError),
    /// The archive cannot be unpacked safely (a path or link that leaves the folder, too large, too many files, damaged).
    Archive(String),
    /// The archive does not contain the runtime's executable.
    NoExecutable,
    /// The release did not pass its self-test; it was moved to the quarantine folder and is not used.
    SelfTestFailed {
        /// The version that failed.
        version: String,
        /// What the self-test said.
        reason: String,
    },
    /// Another install of this runtime runs (in this process or another one).
    Busy,
    /// There is nothing to roll back to, or the version asked for is not installed.
    NotInstalled(String),
    /// The release is in use and cannot be removed; roll back or install another one first.
    InUse(String),
    /// Stopped on request or at the job's deadline. The partial download is kept for a resume.
    Cancelled(CancelReason),
    /// A file or folder operation failed.
    Io {
        /// What was being done ("create", "rename", ...).
        op: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
}

impl RuntimeError {
    pub(crate) fn io(op: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        RuntimeError::Io {
            op,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalogue(why) => write!(f, "the runtime catalogue is not valid: {why}"),
            Self::NotPinned { name, platform } => write!(
                f,
                "there is no {name} build for {platform}; this version of the app cannot download one here"
            ),
            Self::Download(e) => write!(f, "the download failed: {e}"),
            Self::Archive(why) => write!(f, "the downloaded archive cannot be used: {why}"),
            Self::NoExecutable => f.write_str(
                "the downloaded archive does not contain the program; it was not installed",
            ),
            Self::SelfTestFailed { version, reason } => write!(
                f,
                "version {version} did not pass its check ({reason}); it was set aside and the previous version stays in use"
            ),
            Self::Busy => f.write_str(
                "another installation of this runtime is running; wait for it to finish",
            ),
            Self::NotInstalled(what) => write!(f, "{what} is not installed"),
            Self::InUse(version) => write!(
                f,
                "version {version} is in use; install or roll back to another version before removing it"
            ),
            Self::Cancelled(_) => f.write_str(
                "the installation was cancelled; starting it again continues the download",
            ),
            Self::Io { op, path, source } => {
                write!(f, "could not {op} {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Download(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<DownloadError> for RuntimeError {
    fn from(e: DownloadError) -> Self {
        match e {
            DownloadError::Cancelled(r) => RuntimeError::Cancelled(r),
            e => RuntimeError::Download(e),
        }
    }
}

impl From<Cancelled> for RuntimeError {
    fn from(c: Cancelled) -> Self {
        RuntimeError::Cancelled(c.0)
    }
}

impl From<RuntimeError> for JobError {
    fn from(e: RuntimeError) -> Self {
        match e {
            RuntimeError::Cancelled(r) => JobError::Cancelled(r),
            e => JobError::Failed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_says_what_to_do() {
        let io = || std::io::Error::other("denied");
        let all = [
            RuntimeError::Catalogue("x".into()),
            RuntimeError::NotPinned {
                name: "n".into(),
                platform: "p".into(),
            },
            RuntimeError::Archive("a".into()),
            RuntimeError::NoExecutable,
            RuntimeError::SelfTestFailed {
                version: "1".into(),
                reason: "r".into(),
            },
            RuntimeError::Busy,
            RuntimeError::NotInstalled("x".into()),
            RuntimeError::InUse("1".into()),
            RuntimeError::Cancelled(CancelReason::User),
            RuntimeError::io("rename", "/p", io()),
        ];
        for e in &all {
            assert!(!e.to_string().is_empty());
        }
        assert!(all[9].to_string().contains("could not rename /p: denied"));
        assert!(std::error::Error::source(&all[9]).is_some());
        assert!(std::error::Error::source(&all[3]).is_none());
        let failed: JobError = RuntimeError::NoExecutable.into();
        assert!(matches!(failed, JobError::Failed(m) if m.contains("does not contain")));
        assert!(matches!(
            RuntimeError::from(Cancelled(CancelReason::Deadline)),
            RuntimeError::Cancelled(CancelReason::Deadline)
        ));
        assert!(matches!(
            RuntimeError::from(DownloadError::Cancelled(CancelReason::User)),
            RuntimeError::Cancelled(_)
        ));
    }
}
