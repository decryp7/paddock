//! The declarative view model plugins return (ADR 0002): a [`ViewDoc`] is pure data, validated here and rendered by the
//! host's generic `<pd-view>` components, never by code of the plugin.
//!
//! - [`model`] has the wire types ([`ViewDoc`], [`Node`], [`NodeKind`], [`ActionRef`], [`ViewAction`], [`ViewPatch`], ...):
//!   JSON (camelCase, for the UI) and postcard (the IPC), both round-trip tested; the TypeScript of the same types is
//!   generated into `@decryp7/paddock-ui` (`view/generated/view.ts`).
//! - [`validate()`] checks the limits of the ADR and returns a [`ViewError`] that names the node path; it never panics.
//! - [`apply_patch`] applies a [`ViewPatch`] (replace a subtree by node id, or a whole new document) to a document, all or
//!   nothing.
//! - [`to_json`], [`from_json`], [`to_postcard`] and [`from_postcard`] encode with the size limit; decoding is
//!   depth-guarded.
//!
//! Interaction is request/response: the host sends a [`ViewAction`], the plugin answers a [`ViewPatch`]. Anything long
//! runs as a job, not inside a view call.

mod codec;
mod error;
mod grant;
pub mod limits;
mod model;
mod patch;
mod validate;

#[cfg(test)]
mod tests;

pub use codec::{DECODE_DEPTH, from_json, from_postcard, to_json, to_postcard};
pub use error::{PatchError, ViewError};
pub use grant::{GrantRef, MAX_NAME, valid_ident};
pub use model::*;
pub use patch::apply_patch;
pub use validate::{validate, validate_action};

/// Nodes nest at most this deep (the root is level 1).
pub const MAX_DEPTH: usize = 16;
/// Nodes in one document: nodes, tree nodes, graph nodes, diagram elements, table rows, findings, matrix cells, fields.
pub const MAX_NODES: usize = 20_000;
/// The largest encoded document, in postcard bytes (and in JSON text).
pub const MAX_ENCODED: usize = 1024 * 1024;
/// The longest string, in bytes.
pub const MAX_STRING: usize = 8 * 1024;
/// The longest id, in bytes.
pub const MAX_ID: usize = 128;
/// The longest URL, in bytes.
pub const MAX_URL: usize = 2048;
/// The largest arguments of one action, as JSON text in bytes.
pub const MAX_ARGS: usize = 64 * 1024;
/// Nodes of a graph or elements of a diagram.
pub const MAX_GRAPH_NODES: usize = 5_000;
/// Edges of a graph or relations of a diagram.
pub const MAX_GRAPH_EDGES: usize = 20_000;
/// Rows of an inline table; more must be `Paged`.
pub const MAX_INLINE_ROWS: usize = 2_000;
/// Values of one chart, over all its series.
pub const MAX_CHART_POINTS: usize = 20_000;
/// The largest number a `u64` of the model may hold (2^53 - 1), so JavaScript keeps it exact.
pub const MAX_SAFE_INT: u64 = (1 << 53) - 1;
