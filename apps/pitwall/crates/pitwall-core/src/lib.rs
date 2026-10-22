//! Pitwall's pure core (ADR 0002): everything Pitwall decides, with no I/O. The server (`pitwall-server`) and the
//! app (`app/src-tauri`) carry the decisions out; time, memory readings and file contents come in as values.
//!
//! - Wire formats: [`api`] (OpenAI, Anthropic and Ollama compatible requests and responses, ADR 0003), [`llama`]
//!   (llama-server's protocol), [`sse`] (event streams), [`chat`] (the neutral request every API maps to).
//! - Serving: [`scheduler`] (priority queue and concurrency caps, ADR 0005), [`memory`] (when to release models,
//!   whether a load fits, ADR 0006), [`pressure`] (the OS's own pressure and foreground signals), [`select`] (capability-based model choice), [`presets`] (aliases with defaults, ADR 0014), [`backoff`] and [`supervise`] (engine restarts and the crash-loop guard).
//! - Models and engines: [`gguf`], [`quant`], [`fit`], [`hf`] (Hugging Face API answers), [`runtimes`] (the pinned
//!   llama.cpp builds), [`hardware`].
//! - Security and privacy: [`privacy`] (the download allow-list, ADR 0001), [`keys`] and [`limits`] (ADR 0007).
//! - State: [`config`] (versioned settings), [`metrics`], [`events`], [`usage`], [`discovery`], [`mgmt`] (the UI's
//!   wire contract), [`tasks`] (versioned task prompts), [`tokens`], [`cache`].
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod backoff;
pub mod benchmarks;
pub mod cache;
pub mod chat;
pub mod config;
pub mod discovery;
pub mod events;
pub mod fit;
pub mod gguf;
pub mod hardware;
pub mod hf;
pub mod ids;
pub mod keys;
pub mod limits;
pub mod llama;
pub mod memory;
pub mod metrics;
pub mod mgmt;
pub mod packs;
pub mod presets;
pub mod pressure;
pub mod privacy;
pub mod quant;
pub mod redact;
pub mod runtimes;
pub mod scheduler;
pub mod select;
pub mod slots;
pub mod sse;
pub mod supervise;
pub mod tasks;
pub mod time;
pub mod tokens;
pub mod update_state;
pub mod updates;
pub mod usage;

#[cfg(test)]
mod testing;

/// The version of the client contract apps code against (`/x/v1`, the discovery file's `apiVersion`, ADR 0009).
/// Additive changes keep it; anything else is a new version served next to the old one.
pub const API_VERSION: u32 = 1;

/// Bytes in a GiB.
pub const GIB: u64 = 1024 * 1024 * 1024;
/// Bytes in a MiB.
pub const MIB: u64 = 1024 * 1024;
