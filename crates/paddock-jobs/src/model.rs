//! The wire model: what the UI receives (camelCase JSON). Illegal states are unrepresentable: only a running job has a phase,
//! only a failed job has a message, only a cancelled job has a reason.

use crate::cancel::CancelReason;
use paddock_limits::LimitExceeded;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique per registry; serialised as a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub u64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job-{}", self.0)
    }
}

/// An app-defined slug for what the job does ("import", "download", "scan"); the UI picks icon and grouping from it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobKind(pub String);

impl JobKind {
    pub fn new(slug: impl Into<String>) -> Self {
        JobKind(slug.into())
    }
}

/// What a phase counts; the UI formats it (a number, or a size).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    /// Files, events, pages, steps.
    Items,
    /// Bytes transferred or processed.
    Bytes,
}

/// A named stage of a job with its own scale. A phase never restarts under the same label: the next stage is a new phase.
/// `total: None` means the size is not known yet: the UI shows an indeterminate bar and the growing `done` count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    /// Stable slug for tests and translations ("download", "verify").
    pub id: String,
    /// Shown to the user, sentence case ("Finding files", "Downloading").
    pub label: String,
    pub unit: Unit,
    pub total: Option<u64>,
}

impl Phase {
    /// A phase counting items with a known total.
    pub fn items(id: &str, label: &str, total: u64) -> Self {
        Self::new(id, label, Unit::Items, Some(total))
    }

    /// A phase counting items whose total is not known yet (discovery): indeterminate bar, growing count.
    pub fn discovering(id: &str, label: &str) -> Self {
        Self::new(id, label, Unit::Items, None)
    }

    /// A phase counting bytes; `total` from Content-Length or the file size when known.
    pub fn bytes(id: &str, label: &str, total: Option<u64>) -> Self {
        Self::new(id, label, Unit::Bytes, total)
    }

    fn new(id: &str, label: &str, unit: Unit, total: Option<u64>) -> Self {
        Phase {
            id: id.to_string(),
            label: label.to_string(),
            unit,
            total,
        }
    }
}

/// A phase plus how far it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseProgress {
    #[serde(flatten)]
    pub phase: Phase,
    pub done: u64,
}

impl PhaseProgress {
    /// 0..=1 when the total is known and non-zero.
    pub fn fraction(&self) -> Option<f64> {
        let total = self.phase.total.filter(|t| *t > 0)?;
        Some((self.done.min(total) as f64) / total as f64)
    }
}

/// The job state machine: `queued -> running(phase)* -> done | failed | cancelled`; a queued job may also be cancelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum JobState {
    Queued,
    /// `phase` is `None` until the first phase starts (shown as "Starting").
    Running {
        phase: Option<PhaseProgress>,
    },
    Done,
    Failed {
        message: String,
        /// The limit that stopped the job, when one did (ADR 0019): the UI offers its remedy (Raise it, Retry).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<LimitExceeded>,
    },
    Cancelled {
        reason: CancelReason,
    },
}

/// A transition the state machine forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: &'static str,
    pub to: &'static str,
}

impl fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a job cannot go from {} to {}", self.from, self.to)
    }
}

impl std::error::Error for IllegalTransition {}

impl JobState {
    fn name(&self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running { .. } => "running",
            JobState::Done => "done",
            JobState::Failed { .. } => "failed",
            JobState::Cancelled { .. } => "cancelled",
        }
    }

    /// Done, failed or cancelled: nothing follows.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobState::Done | JobState::Failed { .. } | JobState::Cancelled { .. }
        )
    }

    /// The legal next state, or why not.
    pub fn transition(self, to: JobState) -> Result<JobState, IllegalTransition> {
        let ok = matches!(
            (&self, &to),
            (
                JobState::Queued,
                JobState::Running { .. } | JobState::Cancelled { .. }
            ) | (JobState::Running { .. }, _)
        );
        if ok {
            Ok(to)
        } else {
            Err(IllegalTransition {
                from: self.name(),
                to: to.name(),
            })
        }
    }
}

/// One event on the wire: the whole job as of `seq`. Snapshots are idempotent, so dropping or coalescing intermediate ones is safe;
/// the UI keeps the highest `seq` per id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: JobId,
    pub kind: JobKind,
    pub title: String,
    /// What the job works on ("ntdll.pdb", "3 folders"); a display string, never parsed.
    pub subject: Option<String>,
    #[serde(flatten)]
    pub state: JobState,
    pub seq: u64,
    pub elapsed_ms: u64,
    /// Time left, only when trustworthy (see `EtaEstimator`).
    pub eta_ms: Option<u64>,
    /// False while a step that cannot be interrupted runs; then `not_cancellable_reason` says why.
    pub cancellable: bool,
    pub not_cancellable_reason: Option<String>,
}
