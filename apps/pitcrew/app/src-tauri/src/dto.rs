//! The UI wire contract (camelCase JSON), mirrored by `app/src/app/core/api.types.ts`. Change both in one commit.

use pitcrew_core::distribution::RunnerKind;
use pitcrew_core::health::RunnerStatus;
use pitcrew_core::protocol::{ErrorCode, RunnerSnapshot, ServiceError, UpdateRecord};
use pitcrew_core::update::machine::RollbackReason;
use pitcrew_core::update::{UpdateOutcome, UpdatePhase};
use serde::Serialize;

/// `RunnerSnapshot` in `api.types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerSnapshotDto {
    /// The runner's name.
    pub name: String,
    /// Which runner it is.
    pub kind: RunnerKind,
    /// Its status.
    pub status: RunnerStatus,
    /// The installed version.
    pub current_version: Option<String>,
    /// The latest version found.
    pub latest_version: Option<String>,
    /// The last update.
    pub last_update: Option<LastUpdateDto>,
    /// The update in progress.
    pub update_phase: &'static str,
}

/// `LastUpdate` in `api.types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastUpdateDto {
    /// `updated`, `rolledBack`, `postponed`, `cancelled` or `failed`.
    pub result: &'static str,
    /// The version it came from.
    pub from: Option<String>,
    /// The version it went to (or tried to).
    pub to: String,
    /// Why, for failures, roll-backs and postponements.
    pub reason: Option<String>,
    /// When it ended (ms since the Unix epoch).
    pub finished_at_ms: u64,
}

/// `AppUpdateStatus` in `api.types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatusDto {
    /// Whether this build can update itself.
    pub available: bool,
    /// Why not.
    pub reason: Option<String>,
    /// Pitcrew's version.
    pub current_version: String,
}

/// `CommandError` in `api.types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct CommandError {
    /// The kind of error.
    pub code: CommandErrorCode,
    /// For the user: what happened and what to do.
    pub message: String,
}

/// The error kinds the UI distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum CommandErrorCode {
    NotImplemented,
    ServiceUnavailable,
    ProtocolMismatch,
    UnknownRunner,
    BadRequest,
    PermissionDenied,
    Busy,
    Internal,
}

impl From<ServiceError> for CommandError {
    fn from(e: ServiceError) -> Self {
        let code = match e.code {
            ErrorCode::NotImplemented => CommandErrorCode::NotImplemented,
            ErrorCode::ProtocolMismatch => CommandErrorCode::ProtocolMismatch,
            ErrorCode::UnknownRunner => CommandErrorCode::UnknownRunner,
            ErrorCode::BadRequest => CommandErrorCode::BadRequest,
            ErrorCode::PermissionDenied => CommandErrorCode::PermissionDenied,
            ErrorCode::Busy => CommandErrorCode::Busy,
            ErrorCode::Internal => CommandErrorCode::Internal,
        };
        CommandError { code, message: e.message }
    }
}

impl From<RunnerSnapshot> for RunnerSnapshotDto {
    fn from(s: RunnerSnapshot) -> Self {
        RunnerSnapshotDto {
            name: s.name.to_string(),
            kind: s.kind,
            status: s.status,
            current_version: s.current_version.map(|v| v.to_string()),
            latest_version: s.latest_version.map(|v| v.to_string()),
            last_update: s.last_update.map(LastUpdateDto::from),
            update_phase: phase_key(&s.update_phase),
        }
    }
}

fn phase_key(p: &UpdatePhase) -> &'static str {
    match p {
        UpdatePhase::Idle => "idle",
        UpdatePhase::Downloading { .. } => "downloading",
        UpdatePhase::Verifying { .. } => "verifying",
        UpdatePhase::Draining { .. } => "draining",
        UpdatePhase::Stopping { .. } => "stopping",
        UpdatePhase::Swapping { .. } => "swapping",
        UpdatePhase::Confirming { .. } => "confirming",
        UpdatePhase::RollingBack { .. } => "rollingBack",
    }
}

impl From<UpdateRecord> for LastUpdateDto {
    fn from(r: UpdateRecord) -> Self {
        let (result, plan, reason) = match r.outcome {
            UpdateOutcome::Updated { plan } => ("updated", plan, None),
            UpdateOutcome::RolledBack { plan, reason } => (
                "rolledBack",
                plan,
                Some(match reason {
                    RollbackReason::ExitedEarly => "The new runner exited before the server accepted it.".to_string(),
                    RollbackReason::NotAcceptedInTime => "The server did not accept the new runner in time.".to_string(),
                }),
            ),
            UpdateOutcome::Postponed { plan, reason } => ("postponed", plan, Some(format!("{reason:?}"))),
            UpdateOutcome::Cancelled { plan } => ("cancelled", plan, None),
            UpdateOutcome::Failed { plan, reason, .. } => ("failed", plan, Some(reason)),
        };
        LastUpdateDto {
            result,
            from: plan.from.map(|v| v.to_string()),
            to: plan.to.to_string(),
            reason,
            finished_at_ms: r.finished_at.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::names::RunnerName;
    use pitcrew_core::time::Timestamp;
    use pitcrew_core::update::UpdatePlan;
    use pitcrew_core::version::RunnerVersion;

    fn plan() -> UpdatePlan {
        UpdatePlan {
            from: Some(RunnerVersion::parse("5.1.0").unwrap()),
            to: RunnerVersion::parse("5.2.0").unwrap(),
        }
    }

    #[test]
    fn snapshots_serialise_as_the_ui_expects() {
        let s = RunnerSnapshot {
            name: RunnerName::parse("build-pc").unwrap(),
            kind: RunnerKind::GiteaRunner,
            status: RunnerStatus::NotRegistered,
            current_version: Some(RunnerVersion::parse("5.1.0").unwrap()),
            latest_version: None,
            last_update: Some(UpdateRecord {
                outcome: UpdateOutcome::RolledBack {
                    plan: plan(),
                    reason: RollbackReason::NotAcceptedInTime,
                },
                finished_at: Timestamp(42),
            }),
            update_phase: UpdatePhase::RollingBack {
                plan: plan(),
                reason: RollbackReason::ExitedEarly,
            },
        };
        let json = serde_json::to_value(RunnerSnapshotDto::from(s)).unwrap();
        assert_eq!(json["name"], "build-pc");
        assert_eq!(json["kind"], "gitea-runner");
        assert_eq!(json["status"], "notRegistered");
        assert_eq!(json["currentVersion"], "5.1.0");
        assert_eq!(json["latestVersion"], serde_json::Value::Null);
        assert_eq!(json["updatePhase"], "rollingBack");
        assert_eq!(json["lastUpdate"]["result"], "rolledBack");
        assert_eq!(json["lastUpdate"]["from"], "5.1.0");
        assert_eq!(json["lastUpdate"]["finishedAtMs"], 42);
    }

    #[test]
    fn every_outcome_maps_to_a_result() {
        let outcomes = [
            (UpdateOutcome::Updated { plan: plan() }, "updated"),
            (UpdateOutcome::Cancelled { plan: plan() }, "cancelled"),
            (
                UpdateOutcome::Postponed {
                    plan: plan(),
                    reason: pitcrew_core::update::machine::PostponeReason::Busy,
                },
                "postponed",
            ),
            (
                UpdateOutcome::Failed {
                    plan: plan(),
                    reason: "checksum".into(),
                    needs_attention: false,
                },
                "failed",
            ),
            (
                UpdateOutcome::RolledBack {
                    plan: plan(),
                    reason: RollbackReason::ExitedEarly,
                },
                "rolledBack",
            ),
        ];
        for (outcome, result) in outcomes {
            assert_eq!(
                LastUpdateDto::from(UpdateRecord {
                    outcome,
                    finished_at: Timestamp(0)
                })
                .result,
                result
            );
        }
        for p in [
            UpdatePhase::Idle,
            UpdatePhase::Downloading { plan: plan() },
            UpdatePhase::Verifying { plan: plan() },
        ] {
            assert!(!phase_key(&p).is_empty());
        }
    }

    #[test]
    fn errors_keep_their_code_and_message() {
        let e = CommandError::from(ServiceError::not_implemented("Runner status"));
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"code": "notImplemented", "message": "Runner status is not implemented yet"})
        );
        for code in [
            ErrorCode::ProtocolMismatch,
            ErrorCode::UnknownRunner,
            ErrorCode::BadRequest,
            ErrorCode::PermissionDenied,
            ErrorCode::Busy,
            ErrorCode::Internal,
        ] {
            let e = CommandError::from(ServiceError {
                code,
                message: String::new(),
            });
            assert_ne!(e.code, CommandErrorCode::NotImplemented);
        }
    }
}
