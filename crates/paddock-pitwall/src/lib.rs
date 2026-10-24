//! The one Pitwall client for every Paddock app (ADR 0004, plan docs/plans/workbench.md section 15, Debrief ADR 0059).
//!
//! Pitwall is the family's local LLM server and it is always optional. This crate is the app side of its client contract
//! (`/x/v1` plus the OpenAI-compatible API), extracted from Debrief 1.1's client so Debrief, Pacenotes, Sighter and the rest
//! speak to it the same way:
//!
//! - [`discovery`]: find the running Pitwall (`discovery.json`, else port 11435), loopback only, from `paddock-env` folders.
//! - [`keys`]: the app's `pw_` key in the credential store through `paddock-secrets`; never a file, a log or the UI.
//! - [`Client`]: health, pairing, select, models, jobs, tokens, tasks, embeddings and chat (whole or streamed, cancellable
//!   within a read slice), typed errors that say what to do ([`PitwallError`], [`NextStep`]), provenance from `X-Pitwall-*`,
//!   retries with back-off and `Retry-After` ([`retry`]) that never repeat a stream after its first delta.
//! - [`Link`]: one app's connection: probe, the consented pairing flow on a background thread, chat with the model Pitwall
//!   picked, and the record of how requests went so a vanished or revoking Pitwall is noticed at once.
//! - [`AiState`] and [`NeedsAiView`]: what every app renders for a missing Pitwall (the shared `<pd-needs-ai>`).
//! - [`engine`]: Automatic / Pitwall / Built-in, the pure [`engine::select`], the session switch and
//!   [`engine::run_or_fallback`] (one engine per request, never a silent mix).
//! - [`health`]: the optional service as a degraded `paddock-health` entry whose Repair does not depend on Pitwall.
//! - [`loopback`]: the bounded, cancellable `127.0.0.1` HTTP/1.1 transport itself, reusable for other local servers
//!   (Debrief's llama-server client).
//! - `testing` (feature `testing`): a fake Pitwall on an ephemeral port that mirrors Pitwall's routes.
//!
//! Everything is blocking and bounded: apps call it from a worker thread (Paddock rule 10). Tests never use the real
//! Pitwall, its port, its folder or the credential store.

pub mod client;
pub mod discovery;
pub mod engine;
pub mod error;
pub mod health;
pub mod keys;
pub mod loopback;
pub mod model;
pub mod request;
pub mod retry;
pub mod service;
pub mod status;
pub mod stream;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod ts;
pub mod wire;

pub use client::{AppIdentity, ChatReply, Client, EMBED_BATCH, EmbedReply, TaskReply};
pub use discovery::{Locator, WELL_KNOWN_PORT};
pub use engine::{EngineChoice, EngineKind, Routed, SelectInput, Session};
pub use error::{NextStep, PitwallError};
pub use keys::KeyVault;
pub use loopback::{CancelToken, Timeouts};
pub use model::{CachePolicy, CallOptions, Caps, ModelSel, Priority};
pub use request::{ChatRequest, EmbedRequest, Keep, Message, Part, Role, Task};
pub use service::{
    API_VERSION, FoundKind, Link, LinkConfig, LinkEvent, LinkView, PairingView, Ready, Tuning,
};
pub use status::{AiSnapshot, AiState, NeedsAiView};
pub use stream::ChatStream;
pub use wire::{CacheStatus, Health, JobInfo, ModelInfo, PairPoll, Provenance, Selected, Usage};
