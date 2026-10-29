#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! The function port (plan section 2.1, package FN0).
//!
//! Pacenotes' functions (Understand, Review, the Debrief bridge and later Diagrams, Requirements, Tests & coverage) run in
//! the engine first and are packaged as out-of-process plugins later. This crate is the whole boundary, so that move is
//! mechanical:
//!
//! - what a function **contributes** ([`Contributions`]: tools, views, settings);
//! - what it may **ask the host** ([`HostCall`], answered by [`HostReply`]), each call carrying a [`Budget`] and, in
//!   process, a [`CancelToken`](paddock_jobs::CancelToken);
//! - the **request and reply** the host sends a function ([`FunctionRequest`], [`FunctionReply`]);
//! - the **typed errors** of both sides ([`FunctionError`], [`HostError`]);
//! - a **codec** ([`codec`]) that turns any message into bounded postcard bytes and back.
//!
//! Every message is plain data: no references, closures, trait objects or handles. Wire types use externally tagged enums
//! only (postcard cannot read internally or adjacently tagged ones; a test per message type round-trips it).
//!
//! The two in-process traits, [`Function`] and [`Host`], are the port's Rust face; an out-of-process plugin implements
//! `Function` in a wrapper that decodes frames and calls the same methods.

pub mod budget;
pub mod codec;
pub mod contribution;
pub mod error;
pub mod message;

pub use budget::{Budget, BudgetError, BudgetKind};
pub use codec::{CodecError, MAX_MESSAGE_BYTES, decode, encode};
pub use contribution::{Contributions, SettingSpec, SettingValue, ToolSpec, ViewKind, ViewSpec};
pub use error::{FunctionError, HostError};
pub use message::{FunctionReply, FunctionRequest, HostCall, HostReply, HostRequest, ToolOutput};

use paddock_jobs::CancelToken;

/// What a function may ask of the engine. The engine implements it over the index, history, parser and model ports; a
/// test implements it with a fake.
pub trait Host: Send + Sync {
    /// Answers one call within its budget, or fails with a typed error. A cancelled `cancel` ends the call with
    /// [`HostError::Cancelled`]. The reply is data: no connection, snapshot or handle crosses back into the function.
    fn call(&self, request: &HostRequest, cancel: &CancelToken) -> Result<HostReply, HostError>;
}

/// One function (Understand, Review, ...).
pub trait Function: Send + Sync {
    /// What the function contributes. Constant for the function's life.
    fn contributions(&self) -> &Contributions;

    /// Runs one tool. Must check `cancel` between units of work and ask `host` for anything outside itself. Returns its
    /// results as data; the host turns them into writes.
    fn invoke(&self, request: &FunctionRequest, host: &dyn Host, cancel: &CancelToken) -> FunctionReply;
}
