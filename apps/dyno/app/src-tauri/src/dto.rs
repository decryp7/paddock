//! The wire types the UI sees: `AppInfo`, `AgentStatus` and `CommandError` of `app/src/app/core/api.types.ts`
//! (camelCase JSON). Everything else the UI receives is `dyno_core`'s own JSON.

use dyno_core::protocol::{AgentError, ErrorCode as AgentCode};
use serde::Serialize;

/// `AppInfo`: what the About page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// The app's version.
    pub version: String,
    /// `windows`, `macos` or `linux`.
    pub os: &'static str,
    /// Whether this build can check for updates.
    pub updates_available: bool,
    /// Why not, when it cannot.
    pub updates_reason: Option<String>,
}

/// `ElevationStatus`: the answer to "Run as administrator" and "Go back to normal rights".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevationStatus {
    /// Whether the agent now runs with administrator rights.
    pub elevated: bool,
    /// A calm sentence when the request did not change anything (the prompt was cancelled).
    pub message: Option<String>,
}

/// The OS name the UI uses.
pub const fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// `AgentState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    /// Looking for the agent or starting it.
    Starting,
    /// Connected.
    Running,
    /// The link dropped; reconnecting.
    Restarting,
    /// Could not reach or start the agent after several tries (it keeps trying).
    Failed,
}

/// `AgentStatus`, pushed on `dyno://agent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// The state.
    pub state: AgentState,
    /// A sentence for the user, when there is something to add.
    pub message: Option<String>,
}

impl AgentStatus {
    /// A status.
    pub fn new(state: AgentState, message: Option<String>) -> Self {
        AgentStatus { state, message }
    }
}

/// `ErrorCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The agent is not running.
    AgentUnavailable,
    /// Gone.
    NotFound,
    /// The OS refused.
    AccessDenied,
    /// Not possible here (or not built yet).
    Unsupported,
    /// Invalid request.
    BadRequest,
    /// Took too long.
    Timeout,
    /// Another operation of the kind is running.
    Busy,
    /// A bug.
    Internal,
}

/// `CommandError`: what every failed command rejects with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct CommandError {
    /// The kind.
    pub code: ErrorCode,
    /// For the user: what happened and what to do.
    pub message: String,
}

impl CommandError {
    /// An error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        CommandError {
            code,
            message: message.into(),
        }
    }

    /// A refused argument.
    pub fn bad(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }

    /// Something that went wrong in the app.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// There is no agent connection right now.
    pub fn unavailable() -> Self {
        Self::new(
            ErrorCode::AgentUnavailable,
            "The Dyno agent is not running. It is restarted automatically; try again in a moment.",
        )
    }
}

impl From<AgentError> for CommandError {
    fn from(e: AgentError) -> Self {
        let code = match e.code {
            AgentCode::AgentUnavailable => ErrorCode::AgentUnavailable,
            AgentCode::NotFound => ErrorCode::NotFound,
            AgentCode::AccessDenied => ErrorCode::AccessDenied,
            AgentCode::Unsupported => ErrorCode::Unsupported,
            AgentCode::BadRequest => ErrorCode::BadRequest,
            AgentCode::Timeout => ErrorCode::Timeout,
            AgentCode::Busy => ErrorCode::Busy,
            AgentCode::Internal => ErrorCode::Internal,
        };
        CommandError { code, message: e.message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shapes_match_api_types() {
        let info = AppInfo {
            version: "0.1.0".into(),
            os: os_name(),
            updates_available: false,
            updates_reason: Some("x".into()),
        };
        let v = serde_json::to_value(info).unwrap();
        assert_eq!(v["updatesAvailable"], json!(false));
        assert_eq!(v["updatesReason"], json!("x"));
        assert!(["windows", "macos", "linux"].contains(&v["os"].as_str().unwrap()));
        assert_eq!(
            serde_json::to_value(AgentStatus::new(AgentState::Restarting, None)).unwrap(),
            json!({"state": "restarting", "message": null})
        );
        assert_eq!(
            serde_json::to_value(CommandError::unavailable()).unwrap()["code"],
            json!("agentUnavailable")
        );
    }

    #[test]
    fn every_agent_code_maps_to_the_same_name() {
        for (code, name) in [
            (AgentCode::AgentUnavailable, "agentUnavailable"),
            (AgentCode::NotFound, "notFound"),
            (AgentCode::AccessDenied, "accessDenied"),
            (AgentCode::Unsupported, "unsupported"),
            (AgentCode::BadRequest, "badRequest"),
            (AgentCode::Timeout, "timeout"),
            (AgentCode::Busy, "busy"),
            (AgentCode::Internal, "internal"),
        ] {
            let ours = serde_json::to_value(CommandError::from(AgentError::new(code, "m"))).unwrap();
            let theirs = serde_json::to_value(AgentError::new(code, "m")).unwrap();
            assert_eq!(ours["code"], json!(name));
            assert_eq!(ours, theirs, "the agent and the UI agree on the JSON");
        }
    }
}
