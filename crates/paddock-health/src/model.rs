//! The service-health model: a closed state per service, a plain cause and a repair action, and the snapshot and
//! change shapes windows read. No app types, no Tauri, no I/O.
//!
//! Wire contract: `ui/projects/paddock-ui/shell/generated/health.ts` (camelCase JSON, generated from these types).

use serde::{Deserialize, Serialize};
use std::fmt;

/// The event every window listens to; its payload is a [`Snapshot`].
pub const SERVICE_HEALTH_EVENT: &str = "paddock://service-health";

/// The longest service id (it becomes part of the cause key `health:<id>`).
pub const MAX_SERVICE_ID: usize = 64;

/// How a service is doing. A closed set: a UI never has to guess from text.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    /// Working.
    Ok,
    /// Starting; not an outage.
    Starting,
    /// Working with a limit (slow, or one part failed).
    Degraded,
    /// Not working.
    Down,
    /// Stopped by the supervisor after repeated failures, until it is reset.
    Quarantined,
    /// A repair is running.
    Repairing,
}

impl ServiceState {
    /// Whether things that need the service can be asked of it.
    pub fn is_ok(self) -> bool {
        matches!(self, ServiceState::Ok | ServiceState::Starting)
    }

    /// Whether the user has something to know (anything but `ok` and `starting`).
    pub fn is_problem(self) -> bool {
        !self.is_ok()
    }
}

/// What the shell can do to bring a service back. It is always served by the shell, never by the broken service
/// (Paddock rule 19).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepairAction {
    /// Start the process again.
    Restart,
    /// Forget the failure history and start again.
    Reset,
    /// Let a quarantined part run again.
    LiftQuarantine,
    /// Install what is missing.
    Install,
    /// Run the failed work again.
    Retry,
    /// Open the Settings section named by the cause ([`Cause::settings`]): what fixes it is the user's choice there, so
    /// the UI opens it instead of asking the shell.
    OpenSettings,
    /// Use the fallback instead of the service that went away.
    UseFallback,
}

/// Advice to raise a memory cap that stopped a process: the UI offers "Raise the cap…" (it opens the setting with this
/// value; the shell cannot change a user's setting by itself).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaiseCap {
    /// Which process, in the app's own words (`storeWorker`, `plugin:<id>`).
    pub target: String,
    pub suggested_mb: u32,
}

/// Why a service is not ok, in words for the user.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cause {
    /// One plain sentence.
    pub message: String,
    /// The next step for this cause.
    pub hint: Option<String>,
    /// The technical chain, for Details and bug reports.
    pub detail: Option<String>,
    /// A stable id of the wording (`store.crashed`), so the text can be translated later without a wire change.
    #[serde(default)]
    pub code: Option<String>,
    /// Set when a memory cap stopped the process: the UI offers "Raise the cap…" next to the repair.
    // Boxed (as `settings` is): `Cause` is the error of every repair and is kept small (clippy `result_large_err`).
    #[serde(default)]
    pub raise_cap: Option<Box<RaiseCap>>,
    /// The Settings section that fixes it (with the `openSettings` repair).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "string | null"))]
    pub settings: Option<Box<str>>,
}

impl Cause {
    /// A cause with only its message.
    pub fn new(message: impl Into<String>) -> Cause {
        Cause {
            message: message.into(),
            hint: None,
            detail: None,
            code: None,
            raise_cap: None,
            settings: None,
        }
    }

    /// The next step.
    pub fn hint(mut self, hint: impl Into<String>) -> Cause {
        self.hint = Some(hint.into());
        self
    }

    /// The technical chain; a blank one is none.
    pub fn detail(mut self, detail: impl Into<String>) -> Cause {
        let d = detail.into();
        self.detail = (!d.trim().is_empty()).then_some(d);
        self
    }

    /// The stable id of the wording.
    pub fn code(mut self, code: impl Into<String>) -> Cause {
        self.code = Some(code.into());
        self
    }

    /// The memory cap advice.
    pub fn raise_cap(mut self, target: impl Into<String>, suggested_mb: u32) -> Cause {
        self.raise_cap = Some(Box::new(RaiseCap {
            target: target.into(),
            suggested_mb,
        }));
        self
    }

    /// The Settings section that fixes it.
    pub fn settings(mut self, section: impl Into<String>) -> Cause {
        self.settings = Some(section.into().into_boxed_str());
        self
    }
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// One service as windows see it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHealth {
    /// Stable id registered by the app (`store`, `llm`, ...); the notification cause is `health:<id>`.
    pub service: String,
    /// What the user calls it ("the log store").
    pub label: String,
    pub state: ServiceState,
    /// Set while the state is not `ok`.
    pub cause: Option<Cause>,
    /// What "Repair" does now; none when the user cannot do anything.
    pub repair: Option<RepairAction>,
}

/// Every service, in the order the app registered them, and a counter that grows with each change (a window keeps
/// the newest).
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ServiceHealthSnapshot"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[cfg_attr(test, ts(type = "number"))]
    pub revision: u64,
    pub services: Vec<ServiceHealth>,
}

impl Snapshot {
    /// The entry of `service`.
    pub fn service(&self, service: &str) -> Option<&ServiceHealth> {
        self.services.iter().find(|s| s.service == service)
    }
}

/// One service changed state (the registry tells its sink).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub service: String,
    pub from: ServiceState,
    pub to: ServiceState,
}

/// The notification cause of a service (one notification per service outage).
pub fn cause_key(service: &str) -> String {
    format!("health:{service}")
}

/// Whether `id` is a valid service id: 1 to [`MAX_SERVICE_ID`] ASCII letters, digits, `-`, `_`, `.`, starting with a
/// letter.
pub fn is_service_id(id: &str) -> bool {
    id.len() <= MAX_SERVICE_ID
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
