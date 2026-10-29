//! The plugin protocol: the body of every [`dyno_ipc::Frame`] between the host and a plugin.
//!
//! Append-only: new requests and replies go at the end of their enums. Errors travel as `Fail` frames with a
//! [`dyno_ipc::RemoteError`] whose code is one of [`codes`] (or `panic` from the IPC).

use crate::manifest::Manifest;
use crate::model::SensorReading;
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
}

/// What the plugin answers, one variant per request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginReply {
    /// To `Describe`.
    Manifest(Manifest),
    /// To `Sensors`.
    Sensors(Vec<SensorReading>),
}

/// The failure codes of the plugin protocol.
pub mod codes {
    /// The plugin does not contribute what was asked.
    pub const UNSUPPORTED: &str = dyno_ipc::wire::codes::UNSUPPORTED;
    /// The message was not a request.
    pub const BAD_REQUEST: &str = "badRequest";
}
