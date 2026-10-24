//! The live feed of the Paddock apps (plan X29): Rust owns the state, windows only fold what it pushes.
//!
//! Five apps each built their own: Pitwall's reconnecting task and event folding, Dyno's agent link, Pitcrew's subscription
//! thread, Spotter's coalesced pushes, Debrief's channels. They share one shape, which this crate makes the only one:
//!
//! - The app's state implements [`LiveState`]: `apply(delta)` is the one fold, in Rust (for resynchronisation) and
//!   mirrored by the UI's fold (`@decryp7/paddock-ui/live`).
//! - A [`Hub`] holds the state and a sequence number. [`Hub::publish`] folds a delta, numbers it and queues it for every
//!   [`Subscription`]; [`Hub::reset`] replaces the state (a reconnect that seeds again) and sends a snapshot.
//! - A [`Frame`] is a `snapshot` (state, sequence, link), a `delta` (`from..=seq`) or a `link` change. A window applies a delta
//!   only when `from` is the next number; a gap asks for a snapshot ([`Hub::snapshot`], `live_snapshot` over IPC), and a
//!   window that reloads starts the same way.
//! - Every queue is bounded ([`Hub::with_queue_cap`]). Deltas that say "latest wins" merge into the one queued ([`LiveState::merge`]);
//!   a consumer that falls behind has its queue replaced by one snapshot, so memory never grows and nothing is silently
//!   lost.
//! - [`run`] keeps a [`Source`] connected: connect, seed, publish, and on any end reconnect with [`Backoff`](paddock_jobs::time::Backoff)
//!   on the shared `Clock`, announcing each step as the [`Link`] (connecting, connected, reconnecting in N ms, down).
//!
//! The core has no Tauri dependency; `paddock_live::tauri` (feature `tauri`) emits `paddock://live/<channel>` and answers
//! `live_snapshot`.

mod channel;
mod frame;
mod hub;
mod runner;

#[cfg(feature = "tauri")]
pub mod tauri;

pub use channel::{Channel, ChannelError};
pub use frame::{Frame, Link};
pub use hub::{DEFAULT_QUEUE_CAP, Hub, LiveState, MAX_QUEUE_CAP, MIN_QUEUE_CAP, Subscription};
pub use runner::{Publisher, RunEnd, Source, default_policy, run};
