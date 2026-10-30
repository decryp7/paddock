//! The postcard-safe form of a [`JobSnapshot`], for apps whose jobs run in another process (an agent) and are relayed to the
//! UI (ADR 0012 §4). [`JobSnapshot`] is the UI's JSON shape and uses `flatten` and an internal tag, which a non-self-describing
//! codec such as postcard cannot carry. [`WireSnapshot`] says the same with plain structs and externally tagged enums;
//! `JobSnapshot::from(wire)` and `WireSnapshot::from(&snapshot)` convert both ways without loss.

use crate::cancel::CancelReason;
use crate::model::{JobId, JobKind, JobSnapshot, JobState, Phase, PhaseProgress, Unit};
use serde::{Deserialize, Serialize};

/// One stage of a job with how far it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WirePhase {
    /// Stable slug.
    pub id: String,
    /// Shown to the user.
    pub label: String,
    /// What the counts mean.
    pub unit: Unit,
    /// In all, when known.
    pub total: Option<u64>,
    /// So far.
    pub done: u64,
}

/// Where a job stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireState {
    /// Waiting for a worker.
    Queued,
    /// Working; the phase is `None` until the first stage starts.
    Running(Option<WirePhase>),
    /// Finished.
    Done,
    /// Stopped by an error.
    Failed(String),
    /// Stopped on request or at a deadline.
    Cancelled(CancelReason),
}

/// A job as it travels between processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSnapshot {
    /// The job's id.
    pub id: u64,
    /// The app-defined slug.
    pub kind: String,
    /// Shown to the user.
    pub title: String,
    /// What it works on.
    pub subject: Option<String>,
    /// Where it stands.
    pub state: WireState,
    /// Rises with every change.
    pub seq: u64,
    /// Milliseconds since it was registered.
    pub elapsed_ms: u64,
    /// Time left, only when trustworthy.
    pub eta_ms: Option<u64>,
    /// False while a step that cannot be interrupted runs.
    pub cancellable: bool,
    /// Why not, when `cancellable` is false.
    pub not_cancellable_reason: Option<String>,
}

impl WireSnapshot {
    /// Whether the job has ended.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            WireState::Done | WireState::Failed(_) | WireState::Cancelled(_)
        )
    }
}

impl From<&JobSnapshot> for WireSnapshot {
    fn from(s: &JobSnapshot) -> Self {
        let state = match &s.state {
            JobState::Queued => WireState::Queued,
            JobState::Running { phase } => WireState::Running(phase.as_ref().map(|p| WirePhase {
                id: p.phase.id.clone(),
                label: p.phase.label.clone(),
                unit: p.phase.unit,
                total: p.phase.total,
                done: p.done,
            })),
            JobState::Done => WireState::Done,
            JobState::Failed { message } => WireState::Failed(message.clone()),
            JobState::Cancelled { reason } => WireState::Cancelled(*reason),
        };
        WireSnapshot {
            id: s.id.0,
            kind: s.kind.0.clone(),
            title: s.title.clone(),
            subject: s.subject.clone(),
            state,
            seq: s.seq,
            elapsed_ms: s.elapsed_ms,
            eta_ms: s.eta_ms,
            cancellable: s.cancellable,
            not_cancellable_reason: s.not_cancellable_reason.clone(),
        }
    }
}

impl From<WireSnapshot> for JobSnapshot {
    fn from(w: WireSnapshot) -> Self {
        let state = match w.state {
            WireState::Queued => JobState::Queued,
            WireState::Running(phase) => JobState::Running {
                phase: phase.map(|p| PhaseProgress {
                    phase: Phase {
                        id: p.id,
                        label: p.label,
                        unit: p.unit,
                        total: p.total,
                    },
                    done: p.done,
                }),
            },
            WireState::Done => JobState::Done,
            WireState::Failed(message) => JobState::Failed { message },
            WireState::Cancelled(reason) => JobState::Cancelled { reason },
        };
        JobSnapshot {
            id: JobId(w.id),
            kind: JobKind(w.kind),
            title: w.title,
            subject: w.subject,
            state,
            seq: w.seq,
            elapsed_ms: w.elapsed_ms,
            eta_ms: w.eta_ms,
            cancellable: w.cancellable,
            not_cancellable_reason: w.not_cancellable_reason,
            // The wire form (Dyno's agent) does not carry the flag; jobs that cross it announce their success.
            quiet_on_success: false,
        }
    }
}
