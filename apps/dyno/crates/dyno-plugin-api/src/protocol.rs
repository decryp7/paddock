//! The plugin protocol: the body of every `paddock-ipc` frame between the host and a plugin.
//!
//! Append-only: new requests and replies go at the end of their enums. Errors travel as `Fail` frames with a
//! `paddock_ipc::RemoteError` whose code is one of [`codes`] (or `panic` from the IPC).

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

/// The major version the IPC handshake carries between the host and a plugin: bump it on any incompatible change to the enums
/// above (new variants at the end need no bump). A plugin built for another major is refused at the handshake, saying which side
/// to update.
pub const WIRE_MAJOR: u16 = 1;

/// The largest frame between the host and a plugin, in bytes.
pub const MAX_FRAME: usize = 4 * 1024 * 1024;

/// The protocol as the handshake carries it.
pub fn wire() -> paddock_ipc::ProtocolVersion {
    paddock_ipc::ProtocolVersion::new(WIRE_MAJOR, 0)
}

/// The failure codes of the plugin protocol.
pub mod codes {
    /// The plugin does not contribute what was asked.
    pub const UNSUPPORTED: &str = "unsupported";
    /// The message was not a request.
    pub const BAD_REQUEST: &str = "badRequest";
}
