//! One result per item for bulk commands (PRINCIPLES, "Bulk APIs"; the bulk-API redesign's G4): a command that works on
//! items takes all of them at once and answers with an [`ItemResult`] per item, in request order, so a caller with N items
//! makes one call and one bad item does not fail the others. On the wire an item is `{ "ok": <value> }` or
//! `{ "err": <CommandError> }`, mirrored as `ItemResult<T>` in `@decryp7/paddock-ui/ipc` (generated from this file by
//! `ts-rs`, checked by [`crate::testing::sync_generated`]).
//!
//! The two reply shapes say which errors belong to the whole batch:
//!
//! - [`BulkReply`] (most bulk commands): `Result<Vec<ItemResult<T>>, CommandError>`. The outer error is only for the
//!   whole call: a batch over its bound ([`check_batch`]), a guard refusal, a service that is not running. Everything
//!   about one item is that item's `err`.
//! - [`AtomicReply`] (an operation that is all or nothing by design, for example one transaction): `Result<Vec<T>,
//!   CommandError>`. There are no per-item errors; a failure fails the batch.
//!
//! The same shapes as Microsoft Graph JSON batching (a status per request) and GraphQL DataLoader (an error at a key's
//! index for that key alone); Google AIP-231 batch gets are atomic, which is [`AtomicReply`] here.

// `CommandError` is the family's wire envelope (X28), returned once per item or per call (as in paddock-files).
#![allow(clippy::result_large_err)]

use crate::error::CommandError;
use serde::{Deserialize, Serialize};

/// One item's outcome in a bulk command's reply: its value, or the error of that item alone.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemResult<T> {
    /// The item's value (`{ "ok": value }`; a unit value is `{ "ok": null }`).
    Ok(T),
    /// What went wrong with this item (`{ "err": CommandError }`).
    Err(CommandError),
}

impl<T> ItemResult<T> {
    /// Whether the item succeeded.
    pub fn is_ok(&self) -> bool {
        matches!(self, ItemResult::Ok(_))
    }

    /// The item as a `Result`.
    pub fn into_result(self) -> Result<T, CommandError> {
        match self {
            ItemResult::Ok(v) => Ok(v),
            ItemResult::Err(e) => Err(e),
        }
    }

    /// The item as a `Result` of references.
    pub fn as_ref(&self) -> Result<&T, &CommandError> {
        match self {
            ItemResult::Ok(v) => Ok(v),
            ItemResult::Err(e) => Err(e),
        }
    }

    /// Maps the value, keeping an error.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ItemResult<U> {
        match self {
            ItemResult::Ok(v) => ItemResult::Ok(f(v)),
            ItemResult::Err(e) => ItemResult::Err(e),
        }
    }
}

impl<T, E: Into<CommandError>> From<Result<T, E>> for ItemResult<T> {
    /// An item's `Result`, its error converted once at the command edge.
    fn from(r: Result<T, E>) -> Self {
        match r {
            Ok(v) => ItemResult::Ok(v),
            Err(e) => ItemResult::Err(e.into()),
        }
    }
}

/// The reply of a bulk command that is not atomic: one [`ItemResult`] per item in request order; the outer error is
/// only for the whole call (see the module docs).
pub type BulkReply<T> = Result<Vec<ItemResult<T>>, CommandError>;

/// The reply of a bulk command that is all or nothing by design: every value, or one error for the batch.
pub type AtomicReply<T> = Result<Vec<T>, CommandError>;

/// Runs `f` for every item in order and keeps each outcome: the plain loop behind a [`BulkReply`] when the work per item
/// has no cheaper shared form. A command that can share work (one scan, one transaction, one process) does that instead
/// and builds the results itself.
pub fn per_item<I, T, E, F>(items: I, mut f: F) -> Vec<ItemResult<T>>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> Result<T, E>,
    E: Into<CommandError>,
{
    items.into_iter().map(|item| f(item).into()).collect()
}

/// Refuses a batch of `len` items when it is over `max` (PRINCIPLES: bulk calls stay bounded): a `limit` error naming
/// `field`, not retryable. The caller pages or chunks larger inputs.
pub fn check_batch(len: usize, max: usize, field: &str) -> Result<(), CommandError> {
    if len <= max {
        return Ok(());
    }
    Err(CommandError::limit(format!(
        "{len} items were sent in one call; at most {max} are taken at once."
    ))
    .field(field)
    .hint("Send them in smaller batches.")
    .retry(false))
}

#[cfg(test)]
#[path = "item_tests.rs"]
mod tests;
