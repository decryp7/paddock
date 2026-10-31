//! The family's one IPC (ADR 0001 step 1, plan X13), from Debrief's `debrief-ipc` and Dyno's `dyno-ipc`, with Pitcrew's
//! service pipe access list. Synchronous and thread-based; queues and frames are bounded; decoding never panics.
//!
//! - [`frame`]: the frame codec (4-byte length, postcard [`Frame`]); [`wire`]: message bodies and [`Json`].
//! - [`endpoint`]: a private address (a Unix socket in a 0700 folder, an owner-only named pipe; never TCP).
//! - [`token`]: the per-launch secret; [`handshake`]: `Hello`/`Welcome` with token, protocol version and peer pid.
//! - [`peer`]: a connection after the handshake: calls both ways, streams with credits, cancel, pushes, typed errors;
//!   one frame over the limit fails one call, not the connection.
//! - [`launch`] / [`child`]: start a child that connects back (through `paddock-proc`), and the child's loop.
//! - [`link`]: an app and its companion: the discovery file, the server, and the client with reconnect back-off
//!   (`paddock_jobs::time`) and "replace another build".
//!
//! Wire rules: bodies are postcard, so wire types never use `untagged`, `flatten`, internal tags or `skip_serializing_if`;
//! adding a variant at the end of an enum is a minor protocol change ([`ProtocolVersion`]).
//!
//! The reference for the wire (framing, encoding, handshake, versioning, streams, limits) is `docs/ipc-wire.md` in the
//! repository.

pub mod child;
pub mod endpoint;
pub mod error;
pub mod frame;
pub mod handshake;
pub mod launch;
pub mod link;
pub mod peer;
pub mod token;
pub mod wire;

pub use endpoint::Access;
pub use error::{IpcError, RemoteError, codes, panic_message};
pub use frame::{Frame, FrameLimit, ProtocolVersion, Skew};
pub use handshake::{Client, Connection, Host};
pub use launch::{Bootstrap, Child, ChildSpec, launch};
pub use peer::{
    CallCtx, CallStream, CancelProbe, Handler, NoCalls, Peer, PeerOptions, StreamEvent,
    decode_request, encode_reply, typed,
};
pub use token::Token;
pub use wire::Json;
