//! The plugin protocol: the body of every [`dyno_ipc::Frame`] between the host and a plugin.
//!
//! Append-only: new requests and replies go at the end of their enums. Errors travel as `Fail` frames with a
//! [`dyno_ipc::RemoteError`] whose code is one of [`codes`] (or `panic` from the IPC).

use crate::manifest::Manifest;
use crate::model::{ActionInfo, Candidate, CategoryInfo, RuleEvent, ScanLimits, SensorReading};
use serde::{Deserialize, Serialize};

/// One message: the host sends requests, the plugin answers with replies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginMessage {
    /// From the host.
    Request(PluginRequest),
    /// From the plugin.
    Reply(PluginReply),
}

/// What the host asks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginRequest {
    /// The manifest (the first call after connecting).
    Describe,
    /// Current sensor readings.
    Sensors,
    /// The cleanup categories.
    CleanupCategories,
    /// The candidates of one category.
    Scan {
        /// The category id.
        category: String,
        /// Bounds of the scan.
        limits: ScanLimits,
    },
    /// The actions offered.
    Actions,
    /// Run an action.
    RunAction {
        /// The action id.
        action: String,
        /// Why.
        event: RuleEvent,
    },
}

/// What the plugin answers, one variant per request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginReply {
    /// To `Describe`.
    Manifest(Manifest),
    /// To `Sensors`.
    Sensors(Vec<SensorReading>),
    /// To `CleanupCategories`.
    Categories(Vec<CategoryInfo>),
    /// To `Scan`.
    Candidates(Vec<Candidate>),
    /// To `Actions`.
    Actions(Vec<ActionInfo>),
    /// To `RunAction`: what happened.
    ActionDone(String),
}

/// The failure codes of the plugin protocol.
pub mod codes {
    /// The plugin does not contribute what was asked.
    pub const UNSUPPORTED: &str = dyno_ipc::wire::codes::UNSUPPORTED;
    /// The message was not a request.
    pub const BAD_REQUEST: &str = "badRequest";
    /// A category or action the plugin does not know.
    pub const NOT_FOUND: &str = "notFound";
    /// The plugin's own [`PluginError`](crate::PluginError).
    pub const FAILED: &str = "failed";
}
