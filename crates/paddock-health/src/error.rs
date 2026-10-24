//! The error the health and notification commands answer with: the shape of an app's command error (`code`,
//! `message`, `retryable`), so the UI handles it like any other.

use crate::health::RepairRefused;
use serde::Serialize;
use std::fmt;

/// What kind of failure.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthErrorCode {
    /// The shell's health or notification centre is not set up (yet).
    Unavailable,
    /// The request names something that does not exist or is out of bounds.
    Invalid,
    /// The repair was refused (the service works, is fixed in Settings, or nothing repairs it).
    Refused,
}

/// A failed health or notification command.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthError {
    pub code: HealthErrorCode,
    /// For the user, as is.
    pub message: String,
    /// Whether the same call may work later (only `unavailable`: the shell is starting).
    pub retryable: bool,
}

impl HealthError {
    /// The shell part is not set up.
    pub fn unavailable(what: &str) -> HealthError {
        HealthError {
            code: HealthErrorCode::Unavailable,
            message: format!("{what} is not ready yet"),
            retryable: true,
        }
    }

    /// A bad request.
    pub fn invalid(message: impl Into<String>) -> HealthError {
        HealthError {
            code: HealthErrorCode::Invalid,
            message: message.into(),
            retryable: false,
        }
    }
}

impl From<RepairRefused> for HealthError {
    fn from(r: RepairRefused) -> HealthError {
        let code = match r {
            RepairRefused::Unknown(_) => HealthErrorCode::Invalid,
            _ => HealthErrorCode::Refused,
        };
        HealthError {
            code,
            retryable: matches!(r, RepairRefused::NotStarted(_)),
            message: r.to_string(),
        }
    }
}

impl fmt::Display for HealthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HealthError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_become_command_errors() {
        let e = HealthError::from(RepairRefused::Unknown("x".into()));
        assert_eq!((e.code, e.retryable), (HealthErrorCode::Invalid, false));
        let e = HealthError::from(RepairRefused::Working("the store".into()));
        assert_eq!(e.code, HealthErrorCode::Refused);
        assert_eq!(
            e.to_string(),
            "the store is working; there is nothing to repair"
        );
        assert!(HealthError::from(RepairRefused::NotStarted("x".into())).retryable);
        let u = HealthError::unavailable("Service health");
        assert_eq!(
            serde_json::to_value(&u).unwrap(),
            serde_json::json!({"code": "unavailable", "message": "Service health is not ready yet", "retryable": true})
        );
        assert_eq!(HealthError::invalid("bad").code, HealthErrorCode::Invalid);
    }
}
