//! A local-only MCP server for the Paddock apps (plan X22, ADR 0007).
//!
//! The app registers [`ToolDef`]s and [`ResourceDef`]s on a [`Server`]; the crate owns the protocol, the limits, the paging, the
//! audit log and the transports. The rules of ADR 0007 are the defaults:
//!
//! 1. **Local only.** [`serve_stdio`] (the host starts the app's `--mcp` mode) and [`serve_loopback`] (127.0.0.1 only, a
//!    handshake line with a per-client [`KeyStore`] key, at most N clients). There is no other listener.
//! 2. **Off by default.** [`Policy::default`] is switched off; the app's settings page builds the [`Policy`] (which tools, which
//!    actions) and calls [`Server::set_policy`].
//! 3. **Every call is audited**: [`AuditEntry`] with redacted arguments, result size, duration and outcome, in an [`AuditLog`] the app
//!    shows and clears, and one `paddock_mcp` line in the app log (through the `log` facade, so `paddock-log` writes it).
//! 4. **Bounded and paged**: arguments are validated against the tool's schema, a result is cut at its byte limit, a call has a
//!    time limit (the handler's [`CallCtx::cancel`] is cancelled and the call abandoned), cursors page long results ([`page`]).
//! 5. **Read-only by default**: [`ToolDef::read_only`] tools are shown when MCP is on; a [`ToolDef::action`] stays hidden and
//!    refused until [`Policy::allow_action`] names it. Sighter's own safety rules (allow-list, confirmation, budgets, stop key) sit on
//!    top of that in Sighter and are not relaxed here.
//! 8. **Tool output is untrusted data** to the host; the [`client`] module is what a host such as Pitwall builds requests with.
//!
//! Wire shapes follow MCP 2025-06-18 ([`types`], [`jsonrpc`]); the contract test (`tests/contract.rs`) pins them. Batches are not
//! part of MCP and are refused. A tool panicking or hanging cannot take the server down.

pub mod audit;
pub mod client;
pub mod jsonrpc;
mod keys;
mod lines;
mod loopback;
pub mod page;
mod registry;
pub mod schema;
mod server;
pub mod types;

pub use audit::{AuditEntry, AuditLog, AuditSink, Outcome, redact_arguments};
pub use keys::{KEY_PREFIX, KeyEntry, KeyStore, KeyVerifier};
pub use lines::{serve_lines, serve_stdio};
pub use loopback::{Serving, serve_loopback};
pub use registry::{
    Access, CallCtx, Handler, Policy, RegisterError, ResourceDef, ToolDef, ToolError, ToolOutput,
};
pub use server::{Limits, Server, Session};
