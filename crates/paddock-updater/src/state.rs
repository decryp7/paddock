//! The update flow as a pure state machine: check → available → downloading (a job: progress and cancel) → ready (verified)
//! → installing, with typed failures. [`UpdatePhase::next`] is the only way phases change; a step that does not fit the
//! current phase is refused with [`UpdateError::Busy`] (or [`UpdateError::NotAvailable`] in a build without a key) and
//! changes nothing.

use crate::error::UpdateError;
use serde::Serialize;

/// An update found by a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Plain text (never markup), bounded.
    pub notes: String,
    /// RFC 3339.
    pub pub_date: Option<String>,
    /// Download size in bytes, when the release says.
    #[cfg_attr(test, ts(type = "number | null"))]
    pub size: Option<u64>,
}

/// Where the updater is. On the wire: `{ kind: 'available', info: {...} }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdatePhase {
    /// This build cannot verify updates (no public key or no release address): checking is off for good.
    Unavailable,
    /// Nothing checked yet in this run.
    Idle,
    /// A check runs as job `job` (cancel it to stop).
    Checking {
        #[cfg_attr(test, ts(type = "number | null"))]
        job: Option<u64>,
    },
    UpToDate,
    Available {
        info: UpdateInfo,
    },
    /// The download (and then the signature check) runs as job `job`; its phases carry the progress.
    Downloading {
        info: UpdateInfo,
        #[cfg_attr(test, ts(type = "number | null"))]
        job: Option<u64>,
    },
    /// Downloaded and its signature verified.
    #[serde(rename_all = "camelCase")]
    Ready {
        info: UpdateInfo,
        /// Install when the app quits instead of now.
        install_on_quit: bool,
    },
    Installing {
        info: UpdateInfo,
    },
    /// `info` is kept when there is something to retry.
    Failed {
        #[cfg_attr(test, ts(as = "crate::error::UpdateErrorDto"))]
        error: UpdateError,
        info: Option<UpdateInfo>,
    },
}

/// What happened (a request, or a result of work).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Check,
    /// The check's job id, once registered.
    Job(u64),
    Checked(Result<Option<UpdateInfo>, UpdateError>),
    CheckCancelled,
    Download,
    Downloaded(Result<(), UpdateError>),
    Cancel,
    InstallOnQuit(bool),
    Install,
    InstallFailed(UpdateError),
    /// Forget the update found (the channel changed).
    Reset,
}

fn busy(detail: &str) -> UpdateError {
    UpdateError::Busy {
        detail: detail.into(),
    }
}

impl UpdatePhase {
    /// The phase after `event`, or why it does not apply now.
    pub fn next(&self, event: Event) -> Result<UpdatePhase, UpdateError> {
        use UpdatePhase as P;
        Ok(match (self, event) {
            (P::Unavailable, _) => return Err(UpdateError::NotAvailable),
            (P::Idle | P::UpToDate | P::Available { .. } | P::Failed { .. }, Event::Check) => {
                P::Checking { job: None }
            }
            (P::Checking { .. }, Event::Check) => {
                return Err(busy("The app is already checking for updates."));
            }
            (P::Downloading { .. } | P::Ready { .. } | P::Installing { .. }, Event::Check) => {
                return Err(busy("An update is already being downloaded or installed."));
            }

            (P::Checking { .. }, Event::Job(id)) => P::Checking { job: Some(id) },
            (P::Downloading { info, .. }, Event::Job(id)) => P::Downloading {
                info: info.clone(),
                job: Some(id),
            },
            (P::Checking { .. }, Event::Checked(Ok(Some(info)))) => P::Available { info },
            (P::Checking { .. }, Event::Checked(Ok(None))) => P::UpToDate,
            (P::Checking { .. }, Event::Checked(Err(error))) => P::Failed { error, info: None },
            (P::Checking { .. }, Event::CheckCancelled) => P::Idle,

            (P::Available { info }, Event::Download) => start_download(info),
            (
                P::Failed {
                    error,
                    info: Some(info),
                },
                Event::Download,
            ) if error.retry_download() => start_download(info),

            (P::Downloading { info, .. }, Event::Downloaded(Ok(()))) => P::Ready {
                info: info.clone(),
                install_on_quit: false,
            },
            (P::Downloading { info, .. }, Event::Downloaded(Err(error))) => P::Failed {
                error,
                info: Some(info.clone()),
            },
            (P::Downloading { info, .. } | P::Ready { info, .. }, Event::Cancel) => {
                P::Available { info: info.clone() }
            }

            (P::Ready { info, .. }, Event::InstallOnQuit(on)) => P::Ready {
                info: info.clone(),
                install_on_quit: on,
            },
            (P::Ready { info, .. }, Event::Install) => P::Installing { info: info.clone() },
            (P::Installing { info }, Event::InstallFailed(error)) => P::Failed {
                error,
                info: Some(info.clone()),
            },

            (P::Idle | P::UpToDate | P::Available { .. } | P::Failed { .. }, Event::Reset) => {
                P::Idle
            }
            (_, Event::Reset) => {
                return Err(busy(
                    "Wait until the update in progress is finished or cancelled.",
                ));
            }
            (_, Event::Download) => return Err(busy("There is no update to download.")),
            (_, Event::Cancel) => return Err(busy("There is no download to cancel.")),
            (_, Event::InstallOnQuit(_) | Event::Install) => {
                return Err(busy("No verified update is ready to install."));
            }
            // Results arriving after the user moved on (e.g. a check result after a cancel) are stale.
            (
                _,
                Event::Job(_)
                | Event::Checked(_)
                | Event::CheckCancelled
                | Event::Downloaded(_)
                | Event::InstallFailed(_),
            ) => {
                return Err(busy("This step no longer applies."));
            }
        })
    }

    /// Checks can start from here (also what the automatic check waits for).
    pub fn can_check(&self) -> bool {
        matches!(
            self,
            UpdatePhase::Idle
                | UpdatePhase::UpToDate
                | UpdatePhase::Available { .. }
                | UpdatePhase::Failed { .. }
        )
    }

    /// The update this phase is about, if any.
    pub fn info(&self) -> Option<&UpdateInfo> {
        match self {
            UpdatePhase::Available { info }
            | UpdatePhase::Downloading { info, .. }
            | UpdatePhase::Ready { info, .. }
            | UpdatePhase::Installing { info } => Some(info),
            UpdatePhase::Failed { info, .. } => info.as_ref(),
            _ => None,
        }
    }
}

fn start_download(info: &UpdateInfo) -> UpdatePhase {
    UpdatePhase::Downloading {
        info: info.clone(),
        job: None,
    }
}

/// How long after start-up the automatic check waits (not during start-up), and how often it runs.
pub const AUTO_CHECK_DELAY_SECS: u64 = 60;
pub const AUTO_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Whether the automatic check should run now: it is on, nothing else is going on, an update found earlier is not waiting,
/// the app has been running for [`AUTO_CHECK_DELAY_SECS`] and the last check is [`AUTO_CHECK_INTERVAL_SECS`] old.
pub fn auto_check_due(
    auto: bool,
    phase: &UpdatePhase,
    uptime_secs: u64,
    since_last_check_secs: Option<u64>,
) -> bool {
    auto && phase.can_check()
        && !matches!(phase, UpdatePhase::Available { .. })
        && uptime_secs >= AUTO_CHECK_DELAY_SECS
        && since_last_check_secs.is_none_or(|s| s >= AUTO_CHECK_INTERVAL_SECS)
}
