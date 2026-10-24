//! What can go wrong when an app talks to Pitwall, in terms the app acts on. Every error has a plain message that says what
//! to change and where ([`PitwallError::user_message`]) and the next step the UI offers ([`PitwallError::next_step`]).
//! Pitwall's typed errors (ADR 0003, the `code` of an `/x/v1` error or an OpenAI-shaped one) map as follows.
//!
//! | status / code | error |
//! |---|---|
//! | 401 | [`PitwallError::KeyRevoked`] when a key was sent, else [`PitwallError::NeedsPairing`] |
//! | 403 | [`PitwallError::Forbidden`] |
//! | 404, `preset_target_unavailable` | [`PitwallError::NoModelFits`] |
//! | 400, 413 | [`PitwallError::Invalid`] (`capability_unsupported` is [`PitwallError::Unsupported`]) |
//! | 429 | [`PitwallError::RateLimited`] with `Retry-After` |
//! | 503 `insufficient_memory` | [`PitwallError::Memory`] |
//! | 503 `overloaded`, other 503, 504 | [`PitwallError::Overloaded`] |
//! | anything else | [`PitwallError::Server`] |

use crate::loopback::HttpError;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// What the user can do about a problem; the UI turns each into a button (or a line of advice).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NextStep {
    /// Pitwall is installed but not running: open it.
    StartPitwall,
    /// Pitwall is not installed: set it up.
    InstallPitwall,
    /// Connect the app to Pitwall (the pairing code flow).
    Pair,
    /// Install a model that fits (Pitwall's model library).
    InstallModel,
    /// Try again in a moment.
    Wait,
    /// The app is older than Pitwall's connection version: update the app.
    UpdateApp,
    /// Pitwall is older than the app needs: update Pitwall.
    UpdatePitwall,
    /// Change the app's permissions in Pitwall.
    ChangePermissions,
    /// Nothing the user can do (retry may help).
    None,
}

/// Why a call to Pitwall failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PitwallError {
    /// Nothing answers: Pitwall is not running.
    NotRunning,
    /// The connection dropped in the middle of an answer: Pitwall stopped.
    Stopped,
    /// There is no key yet (or Pitwall does not know the app): pair first.
    NeedsPairing,
    /// The key was revoked or Pitwall's apps were reset; the stored key is useless.
    KeyRevoked,
    /// The user denied the pairing request in Pitwall.
    PairingDenied,
    /// The pairing request expired or is unknown to Pitwall.
    PairingExpired,
    /// Pitwall speaks another connection version than this crate.
    Incompatible {
        /// Pitwall's version text.
        version: String,
        /// Its `apiVersion`.
        found: u32,
    },
    /// Pitwall does not allow this (403): a permission of the app's key in Pitwall.
    Forbidden(String),
    /// No installed model fits the request (404, `preset_target_unavailable`).
    NoModelFits(String),
    /// The request needs something the chosen model or engine cannot do.
    Unsupported {
        /// The capability (`vision`, `tools`, ...).
        cap: String,
        /// Pitwall's reason, with what to change.
        hint: String,
    },
    /// Too many requests or a budget is used up (429).
    RateLimited {
        /// Pitwall's message.
        message: String,
        /// `Retry-After`.
        retry_after: Option<Duration>,
    },
    /// The model does not fit in memory now (503 `insufficient_memory`).
    Memory(String),
    /// Paused, stopped, queue full or the engine timed out (503, 504).
    Overloaded {
        /// Pitwall's message.
        message: String,
        /// `Retry-After`, when Pitwall sent one.
        retry_after: Option<Duration>,
    },
    /// The app sent something Pitwall refuses (400, 413) or the call was malformed before it left.
    Invalid(String),
    /// A limit ran out; the text names it.
    Timeout(&'static str),
    /// The caller cancelled.
    Cancelled,
    /// The answer is not what the contract says.
    Protocol(String),
    /// The link is not ready for the app yet; the text says why (Pitwall paused, no model, not looked at yet).
    NotReady(String),
    /// The engine failed (500) or another status.
    Server {
        /// The HTTP status.
        status: u16,
        /// Pitwall's code, empty when none.
        code: String,
        /// Pitwall's message.
        message: String,
    },
}

impl PitwallError {
    /// Pitwall went away (not running, or the connection dropped): the session's engine is lost.
    pub fn is_gone(&self) -> bool {
        matches!(self, PitwallError::NotRunning | PitwallError::Stopped)
    }

    /// The key is no good: the app must pair again (and should delete its stored key).
    pub fn is_key_problem(&self) -> bool {
        matches!(self, PitwallError::KeyRevoked | PitwallError::NeedsPairing)
    }

    /// Whether trying the same call again later can help (the retry table of `retry.rs` uses this).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            PitwallError::NotRunning
                | PitwallError::Overloaded { .. }
                | PitwallError::RateLimited { .. }
        )
    }

    /// What the user can do about it.
    pub fn next_step(&self) -> NextStep {
        match self {
            PitwallError::NotRunning | PitwallError::Stopped => NextStep::StartPitwall,
            PitwallError::NeedsPairing
            | PitwallError::KeyRevoked
            | PitwallError::PairingDenied
            | PitwallError::PairingExpired => NextStep::Pair,
            PitwallError::Incompatible { found, .. } => {
                if *found > crate::API_VERSION {
                    NextStep::UpdateApp
                } else {
                    NextStep::UpdatePitwall
                }
            }
            PitwallError::Forbidden(_) => NextStep::ChangePermissions,
            PitwallError::NoModelFits(_) | PitwallError::Unsupported { .. } => {
                NextStep::InstallModel
            }
            PitwallError::RateLimited { .. }
            | PitwallError::Memory(_)
            | PitwallError::Overloaded { .. }
            | PitwallError::Timeout(_)
            | PitwallError::NotReady(_) => NextStep::Wait,
            PitwallError::Cancelled
            | PitwallError::Invalid(_)
            | PitwallError::Protocol(_)
            | PitwallError::Server { .. } => NextStep::None,
        }
    }

    /// A message for the user, naming `app` (the app's display name, "Debrief") where Pitwall lists it.
    pub fn user_message(&self, app: &str) -> String {
        match self {
            PitwallError::NotRunning => "Pitwall is not running. Open Pitwall and try again".into(),
            PitwallError::Stopped => "Pitwall stopped answering. Open Pitwall again and try again".into(),
            PitwallError::NeedsPairing => format!("{app} is not connected to Pitwall yet. Connect it first"),
            PitwallError::KeyRevoked => {
                format!("Pitwall no longer accepts {app}'s key (it was revoked or Pitwall's apps were reset). Connect {app} to Pitwall again")
            }
            PitwallError::PairingDenied => format!("Pitwall denied {app}'s request. Choose Connect to try again"),
            PitwallError::PairingExpired => {
                "The pairing request expired before it was approved in Pitwall. Choose Connect to try again".into()
            }
            PitwallError::Incompatible { version, found } => {
                if *found > crate::API_VERSION {
                    format!("Pitwall {version} speaks a newer connection version ({found}) than this {app} ({}). Update {app} to use it", crate::API_VERSION)
                } else {
                    format!("Pitwall {version} speaks an older connection version ({found}) than this {app} needs ({}). Update Pitwall to use it", crate::API_VERSION)
                }
            }
            PitwallError::Forbidden(m) => format!("Pitwall does not allow this for {app}: {m}. Change {app}'s permissions in Pitwall > Apps & keys"),
            PitwallError::NoModelFits(m) => format!("Pitwall has no model that suits {app} ({m}). Download a model in Pitwall"),
            PitwallError::Unsupported { cap, hint } => format!("This needs a model with {cap}: {hint}"),
            PitwallError::RateLimited { message, .. } => format!("Pitwall asks {app} to slow down: {message}"),
            PitwallError::Memory(m) => format!("Pitwall cannot load the model now: {m}"),
            PitwallError::Overloaded { message, .. } => format!("Pitwall cannot answer now: {message}"),
            PitwallError::Invalid(m) => format!("Pitwall refused the request: {m}"),
            PitwallError::Timeout(what) => format!("Pitwall did not answer in time ({what}). Try again; if Pitwall is busy, wait for it"),
            PitwallError::Cancelled => "cancelled".into(),
            PitwallError::NotReady(m) => format!("Pitwall is not ready for {app}: {m}"),
            PitwallError::Protocol(m) => format!("Pitwall answered something {app} does not understand ({m}). Update {app} or Pitwall"),
            PitwallError::Server { status, message, .. } => format!("Pitwall's engine failed (HTTP {status}): {message}"),
        }
    }
}

impl std::fmt::Display for PitwallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message("the app"))
    }
}

impl std::error::Error for PitwallError {}

impl From<HttpError> for PitwallError {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::Refused(_) => PitwallError::NotRunning,
            HttpError::Timeout(w) => PitwallError::Timeout(w),
            HttpError::Cancelled => PitwallError::Cancelled,
            HttpError::Closed => PitwallError::Stopped,
            HttpError::Protocol(m) => PitwallError::Protocol(m),
        }
    }
}

/// Longest message kept from the server.
const MAX_MESSAGE: usize = 500;

/// The typed error of a non-2xx answer: OpenAI's `{"error":{"message","code"}}`, the extended `{"error":{"code","message"}}`
/// and Ollama's `{"error":"..."}`. `had_key` says whether the request carried a key (a 401 then means it was revoked).
pub fn status_error(
    status: u16,
    retry_after: Option<&str>,
    had_key: bool,
    body: &[u8],
) -> PitwallError {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let message: String = v
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| v.get("error").and_then(Value::as_str))
        .unwrap_or("no details")
        .chars()
        .take(MAX_MESSAGE)
        .collect();
    let code = v
        .pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let retry_after = retry_after
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|s| Duration::from_secs(s.min(300)));
    match (status, code) {
        (401, _) if had_key => PitwallError::KeyRevoked,
        (401, _) => PitwallError::NeedsPairing,
        (403, _) => PitwallError::Forbidden(message),
        (_, "capability_unsupported") => PitwallError::Unsupported {
            cap: capability_in(&message),
            hint: message,
        },
        (404, _) | (_, "preset_target_unavailable") => PitwallError::NoModelFits(message),
        (429, _) => PitwallError::RateLimited {
            message,
            retry_after,
        },
        (503, "insufficient_memory") => PitwallError::Memory(message),
        (503, _) | (504, _) => PitwallError::Overloaded {
            message,
            retry_after,
        },
        (400, _) | (413, _) => PitwallError::Invalid(message),
        _ => PitwallError::Server {
            status,
            code: code.chars().take(64).collect(),
            message,
        },
    }
}

/// The capability a Pitwall message names (`vision`, `tools`, ...), else `a capability`.
fn capability_in(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    ["vision", "tools", "json", "embeddings", "rerank"]
        .into_iter()
        .find(|c| lower.contains(c))
        .unwrap_or("a capability")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(code: &str) -> Vec<u8> {
        format!(r#"{{"error":{{"message":"m","type":"t","code":"{code}"}}}}"#).into_bytes()
    }

    #[test]
    fn statuses_become_typed_errors_with_advice() {
        assert_eq!(
            status_error(401, None, true, &body("unauthorized")),
            PitwallError::KeyRevoked
        );
        assert_eq!(
            status_error(401, None, false, &body("unauthorized")),
            PitwallError::NeedsPairing
        );
        assert_eq!(
            status_error(403, None, true, &body("forbidden")),
            PitwallError::Forbidden("m".into())
        );
        assert!(matches!(
            status_error(404, None, true, &body("not_found")),
            PitwallError::NoModelFits(_)
        ));
        assert!(matches!(
            status_error(503, None, true, &body("preset_target_unavailable")),
            PitwallError::NoModelFits(_)
        ));
        assert_eq!(
            status_error(429, Some(" 5 "), true, &body("rate_limited")),
            PitwallError::RateLimited {
                message: "m".into(),
                retry_after: Some(Duration::from_secs(5))
            }
        );
        assert!(matches!(
            status_error(429, Some("soon"), true, &body("rate_limited")),
            PitwallError::RateLimited {
                retry_after: None,
                ..
            }
        ));
        assert!(matches!(
            status_error(429, Some("99999"), true, &body("rate_limited")),
            PitwallError::RateLimited { retry_after: Some(d), .. } if d == Duration::from_secs(300)
        ));
        assert!(matches!(
            status_error(503, None, true, &body("insufficient_memory")),
            PitwallError::Memory(_)
        ));
        assert!(
            matches!(status_error(503, Some("5"), true, &body("overloaded")), PitwallError::Overloaded { retry_after: Some(d), .. } if d == Duration::from_secs(5))
        );
        assert!(matches!(
            status_error(504, None, true, &body("timeout")),
            PitwallError::Overloaded { .. }
        ));
        assert!(matches!(
            status_error(413, None, true, &body("request_too_large")),
            PitwallError::Invalid(_)
        ));
        assert!(matches!(
            status_error(400, None, true, br#"{"error":{"code":"capability_unsupported","message":"the model has no vision; use an engine build with mtmd"}}"#),
            PitwallError::Unsupported { cap, .. } if cap == "vision"
        ));
        assert!(
            matches!(status_error(500, None, true, b"not json"), PitwallError::Server { status: 500, message, .. } if message == "no details")
        );
        // Ollama's shape.
        assert!(
            matches!(status_error(400, None, true, br#"{"error":"bad"}"#), PitwallError::Invalid(m) if m == "bad")
        );
        // A long message is cut.
        let long = format!(r#"{{"error":{{"message":"{}"}}}}"#, "x".repeat(2000));
        assert!(
            matches!(status_error(400, None, true, long.as_bytes()), PitwallError::Invalid(m) if m.len() == MAX_MESSAGE)
        );
        assert_eq!(capability_in("nothing"), "a capability");
    }

    #[test]
    fn every_error_says_what_to_do_and_names_the_app() {
        let all = [
            (
                PitwallError::NotRunning,
                NextStep::StartPitwall,
                "not running",
            ),
            (
                PitwallError::Stopped,
                NextStep::StartPitwall,
                "stopped answering",
            ),
            (
                PitwallError::NeedsPairing,
                NextStep::Pair,
                "Debrief is not connected",
            ),
            (
                PitwallError::KeyRevoked,
                NextStep::Pair,
                "no longer accepts Debrief's key",
            ),
            (
                PitwallError::PairingDenied,
                NextStep::Pair,
                "denied Debrief",
            ),
            (PitwallError::PairingExpired, NextStep::Pair, "expired"),
            (
                PitwallError::Incompatible {
                    version: "2.0.0".into(),
                    found: 2,
                },
                NextStep::UpdateApp,
                "Update Debrief",
            ),
            (
                PitwallError::Incompatible {
                    version: "0.1.0".into(),
                    found: 0,
                },
                NextStep::UpdatePitwall,
                "Update Pitwall",
            ),
            (
                PitwallError::Forbidden("x".into()),
                NextStep::ChangePermissions,
                "Apps & keys",
            ),
            (
                PitwallError::NoModelFits("x".into()),
                NextStep::InstallModel,
                "Download a model",
            ),
            (
                PitwallError::Unsupported {
                    cap: "vision".into(),
                    hint: "h".into(),
                },
                NextStep::InstallModel,
                "with vision",
            ),
            (
                PitwallError::RateLimited {
                    message: "x".into(),
                    retry_after: None,
                },
                NextStep::Wait,
                "slow down",
            ),
            (
                PitwallError::Memory("x".into()),
                NextStep::Wait,
                "cannot load",
            ),
            (
                PitwallError::Overloaded {
                    message: "x".into(),
                    retry_after: None,
                },
                NextStep::Wait,
                "cannot answer",
            ),
            (PitwallError::Timeout("x"), NextStep::Wait, "in time"),
            (
                PitwallError::NotReady("paused".into()),
                NextStep::Wait,
                "not ready for Debrief: paused",
            ),
            (PitwallError::Invalid("x".into()), NextStep::None, "refused"),
            (PitwallError::Cancelled, NextStep::None, "cancelled"),
            (
                PitwallError::Protocol("x".into()),
                NextStep::None,
                "Update Debrief or Pitwall",
            ),
            (
                PitwallError::Server {
                    status: 500,
                    code: String::new(),
                    message: "x".into(),
                },
                NextStep::None,
                "HTTP 500",
            ),
        ];
        for (e, step, text) in all {
            assert_eq!(e.next_step(), step, "{e:?}");
            assert!(
                e.user_message("Debrief").contains(text),
                "{e:?}: {}",
                e.user_message("Debrief")
            );
        }
        assert!(
            PitwallError::Stopped.is_gone()
                && PitwallError::NotRunning.is_gone()
                && !PitwallError::Cancelled.is_gone()
        );
        assert!(
            PitwallError::KeyRevoked.is_key_problem()
                && PitwallError::NeedsPairing.is_key_problem()
        );
        assert!(
            PitwallError::Overloaded {
                message: String::new(),
                retry_after: None
            }
            .is_transient()
                && !PitwallError::Invalid(String::new()).is_transient()
        );
        assert!(PitwallError::NotRunning.to_string().contains("not running"));
    }

    #[test]
    fn http_errors_map() {
        assert_eq!(PitwallError::from(HttpError::Closed), PitwallError::Stopped);
        assert_eq!(
            PitwallError::from(HttpError::Refused("r".into())),
            PitwallError::NotRunning
        );
        assert!(matches!(
            PitwallError::from(HttpError::Protocol("p".into())),
            PitwallError::Protocol(_)
        ));
        assert_eq!(
            PitwallError::from(HttpError::Cancelled),
            PitwallError::Cancelled
        );
        assert_eq!(
            PitwallError::from(HttpError::Timeout("t")),
            PitwallError::Timeout("t")
        );
    }
}
