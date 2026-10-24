//! What a Paddock app keeps on disk, and clearing it safely (plan X32). Built from Debrief's Settings > Storage (`storage/`:
//! categories, plan, bounded walk), Dyno's bounded size walk and its re-check before every delete (`cleanup/walk.rs`, `guard.rs`),
//! Pitwall's "never deletes by itself" leftovers and Pitcrew's cache clean.
//!
//! ```no_run
//! use paddock_storage::{Category, ClearChoice, ClearRequest, Kind, Storage, Item};
//! use paddock_env::{AppDirs, AppId};
//!
//! let dirs = AppDirs::for_app(&AppId::new("spotter", "Spotter")?)?;
//! let categories = vec![
//!     Category::new("logs", "Logs", Kind::Data).folder(dirs.logs()).entries(|p| p.ends_with("today.log").then(|| "being written".into())),
//!     Category::new("cache", "Cache", Kind::Cache).folder(dirs.cache()).entries(|_| None),
//! ];
//! let storage = Storage::new(std::sync::Arc::new(categories)).within(&dirs); // nothing outside the app's folders is ever removed
//! let usage = storage.scan(&())?;                      // the preview: sizes, what clearing would remove, the biggest items
//! let request = ClearRequest { categories: vec![ClearChoice { id: "cache".into(), confirmed: false }] };
//! let result = storage.clear(&request, &())?;          // re-checks every item right before deleting it
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! - [`Category`] / [`Source`]: what the app declares, fresh at every call: a folder everything is confined to, the top-level
//!   items (with the reason an item stays: today's log, the active model, what this run opened), a [`Kind`] (`cache` the app
//!   can rebuild, `data` that is the user's), a note, and an optional removal through the app ([`Item::removed_by`]).
//! - [`Storage::scan`]: measures with a bounded walk ([`walk`]: entry budget, time on an injected clock, depth, no links, one
//!   volume; a stopped walk says so and the sizes are lower bounds) and keeps what it saw as the preview.
//! - [`Storage::clear`]: refuses without a preview, asks a confirmation per `data` category, re-checks every item ([`guard`]:
//!   still there, same kind, real parent inside the category's folder, listed by the preview, unchanged) and reports what it left
//!   in place and why. Never outside the app's own folders ([`Storage::within`]), never by itself.
//! - [`Run`]: progress and Cancel, so both run as jobs (rule 11); `tauri` (feature) is the adapter with the commands and the
//!   `paddock://storage` event.

// `CommandError` is the family's wire envelope (X28); it is returned only on the refusal path, never in a loop.
#![allow(clippy::result_large_err)]

mod category;
mod error;
pub mod guard;
mod model;
mod run;
mod storage;
pub mod walk;

pub use category::{
    Category, How, Item, MAX_ID_LEN, MAX_ITEMS, Source, entries, from_this_run, valid_id,
};
pub use error::StorageError;
pub use model::{
    CategoryClearResult, ClearChoice, ClearRequest, ItemView, Kind, Skipped, StorageCategory,
    StorageClearResult, StorageUsage,
};
pub use run::{Recorder, Run};
pub use storage::{MAX_CATEGORIES, MAX_PREVIEW_ITEMS, RECHECK_ENTRIES, RECHECK_TIME, Storage};
pub use walk::MAX_SKIPPED_LISTED;

/// The event the outcome of a scan or a clear is sent on, to every window (a `StorageEvent` of the adapter).
pub const EVENT_NAME: &str = "paddock://storage";

paddock_commands::command_table! {
    /// The commands of the storage adapter (`paddock_storage::tauri::invoke_handler` serves them; the shell plugin folds them into
    /// its table). The settings dialog and main windows may use them.
    pub static COMMANDS;
    /// The last measured usage, if the app measured since it started.
    Query [MAIN | DIALOG] => tauri::storage_last,
    /// Measures what the app keeps as a job (returns at once; the outcome is an event).
    Mutation [MAIN | DIALOG] => tauri::storage_scan,
    /// Clears the chosen categories as a job (returns at once; the outcome is an event).
    Mutation [MAIN | DIALOG] => tauri::storage_clear,
    /// Cancels the scan or clear in progress.
    Mutation [MAIN | DIALOG] => tauri::storage_cancel,
    /// Shows a category's folder in the file manager.
    Mutation [MAIN | DIALOG] => tauri::storage_reveal,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(all(test, feature = "tauri"))]
mod generated;
