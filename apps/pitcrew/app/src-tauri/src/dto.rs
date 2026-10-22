//! The UI wire contract's app-side types (camelCase JSON), mirrored by `app/src/app/core/api.types.ts`. Status,
//! history, disk and settings types are `pitcrew_core` types passed through unchanged. Change both sides in one
//! commit.

use pitcrew_core::config::{Config, SessionMode};
use pitcrew_core::gitea_api::JobDay;
use pitcrew_core::history::{HistoryEvent, HistorySummary};
use pitcrew_core::housekeeping::{CleanupPlan, CleanupPolicy, CleanupReport, DiskScan};
use pitcrew_core::metrics::{MachineSample, UsagePoint};
use pitcrew_core::notify::Notification;
use pitcrew_core::protocol::{ErrorCode, RunnerSnapshot, ServiceError, ServiceInfo};
use pitcrew_core::session::LoggedOffMode;
use pitcrew_core::time::Timestamp;
use serde::{Deserialize, Serialize};

/// `StatusReply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReply {
    /// The runners.
    pub runners: Vec<RunnerSnapshot>,
    /// The service.
    pub service: ServiceInfo,
}

/// `ActivityReply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityReply {
    /// The summary.
    pub summary: HistorySummary,
    /// Jobs per day by outcome.
    pub job_days: Vec<JobDay>,
    /// The newest events, newest first.
    pub events: Vec<HistoryEvent>,
    /// Whether a Gitea read token is set for the runner's server.
    pub has_api_token: bool,
}

/// `MetricsReply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsReply {
    /// The machine.
    pub machine: Vec<MachineSample>,
    /// The folder size.
    pub folder: Vec<(Timestamp, u64)>,
    /// The runner's own CPU and memory, with its processes.
    pub usage: Vec<UsagePoint>,
}

/// `DiskReply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskReply {
    /// The scan.
    pub scan: DiskScan,
    /// The dry-run preview.
    pub preview: CleanupPlan,
    /// The last cleanup.
    pub last_cleanup: Option<CleanupReport>,
    /// The policy.
    pub policy: CleanupPolicy,
}

/// `SettingsReply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsReply {
    /// The configuration.
    pub config: Config,
    /// Its problems.
    pub problems: Vec<String>,
    /// Whether it can be saved.
    pub read_only: bool,
}

/// `RegisterRequest` (from the UI). The token is handed to the service and dropped.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    /// Pitcrew's name.
    pub runner: String,
    /// The server.
    pub server_url: String,
    /// The registration token.
    pub token: String,
    /// The name the server shows.
    pub display_name: Option<String>,
    /// Labels.
    pub labels: Vec<String>,
    /// The folder (`None`: the default).
    pub dir: Option<String>,
    /// Where it runs.
    pub session: SessionMode,
    /// What it does while nobody is logged on.
    pub logged_off: LoggedOffMode,
    /// Keep the token in the OS credential store.
    pub remember_token: bool,
}

impl std::fmt::Debug for RegisterRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterRequest")
            .field("runner", &self.runner)
            .field("token", &"..")
            .finish_non_exhaustive()
    }
}

/// `AppUpdateStatus`.
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

/// `AppUpdateInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    /// The version.
    pub version: String,
    /// Release notes.
    pub notes: Option<String>,
    /// Publication date.
    pub date: Option<String>,
}

/// `ConnectionState`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ConnectionState {
    /// Connecting.
    Connecting,
    /// Connected.
    Connected {
        /// The service's version.
        service_version: String,
    },
    /// Not running or not installed.
    Unavailable {
        /// For the user.
        message: String,
    },
    /// Different protocol versions.
    Mismatch {
        /// For the user.
        message: String,
    },
}

/// `ServiceInstall`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstall {
    /// `windows`, `macos` or `linux`.
    pub platform: &'static str,
    /// Registered with the OS.
    pub installed: bool,
    /// Running.
    pub running: bool,
    /// Installing is possible from the app.
    pub can_install: bool,
    /// More, for the user (e.g. "approve Pitcrew in System Settings ▸ Login Items").
    pub message: Option<String>,
}

/// `BackendEvent`, emitted as the Tauri event `pitcrew://event`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackendEvent {
    /// A status.
    Status {
        /// It.
        status: StatusReply,
    },
    /// A notification.
    Notification {
        /// It.
        notification: Notification,
    },
    /// The connection changed.
    Connection {
        /// It.
        connection: ConnectionState,
    },
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

impl CommandError {
    /// An error with a code and a message.
    pub fn new(code: CommandErrorCode, message: impl Into<String>) -> Self {
        CommandError {
            code,
            message: message.into(),
        }
    }

    /// A refused argument.
    pub fn bad(message: impl Into<String>) -> Self {
        Self::new(CommandErrorCode::BadRequest, message)
    }

    /// Something that went wrong in the app.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(CommandErrorCode::Internal, message)
    }
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
    Cancelled,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_keep_their_code_and_message() {
        let e = CommandError::from(ServiceError::new(ErrorCode::Busy, "later"));
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"code": "busy", "message": "later"})
        );
        for code in [
            ErrorCode::NotImplemented,
            ErrorCode::ProtocolMismatch,
            ErrorCode::UnknownRunner,
            ErrorCode::BadRequest,
            ErrorCode::PermissionDenied,
            ErrorCode::Internal,
        ] {
            let e = CommandError::from(ServiceError::new(code, ""));
            assert_ne!(e.code, CommandErrorCode::Busy);
        }
        assert_eq!(CommandError::bad("x").code, CommandErrorCode::BadRequest);
        assert_eq!(CommandError::internal("x").code, CommandErrorCode::Internal);
    }

    #[test]
    fn events_and_connection_states_serialise_as_the_ui_expects() {
        let c = BackendEvent::Connection {
            connection: ConnectionState::Connected {
                service_version: "0.1.0".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::json!({"kind": "connection", "connection": {"state": "connected", "serviceVersion": "0.1.0"}})
        );
        let u = serde_json::to_value(ConnectionState::Unavailable { message: "m".into() }).unwrap();
        assert_eq!(u, serde_json::json!({"state": "unavailable", "message": "m"}));
    }

    #[test]
    fn register_requests_never_print_the_token() {
        let r: RegisterRequest = serde_json::from_value(serde_json::json!({
            "runner": "a", "serverUrl": "https://g", "token": "SECRET", "displayName": null, "labels": [],
            "dir": null, "session": "interactive", "loggedOff": "wait", "rememberToken": false
        }))
        .unwrap();
        assert!(!format!("{r:?}").contains("SECRET"));
    }
}
