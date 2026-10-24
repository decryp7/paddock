#![warn(missing_docs)]
//! Folder watching for the Paddock apps (plan X23): what the pack reloader, Debrief's watched folders and Spotter's sources
//! each wrote their own copy of.
//!
//! - [`Debouncer`]: change events in, one "due" out. Waits until the folder was quiet for `quiet`, but never longer than
//!   `max_wait` after the first event, on a [`paddock_jobs::time::Clock`] (tests move a `FakeClock`, nothing sleeps).
//! - [`ChangeSource`]: the port that says a folder changed. [`FakeSource`] for tests; `NotifySource` (feature `notify`)
//!   is the real adapter over the OS file-change notifications.
//! - [`spawn`]: a background thread that feeds a source into a [`Debouncer`] and calls your function when it is due.
//! - [`Settle`]: Debrief's "file has settled" rule as a pure state machine over folder snapshots ([`scan_folder`]): a file
//!   is ready when its size and modification time stopped changing, temporary names and empty files are skipped, a
//!   vanished or locked file is retried with back-off, then given up. [`Batcher`] groups ready files into one job.
//!
//! The core has no OS dependency; tests use temporary folders only.

mod debounce;
#[cfg(feature = "notify")]
mod native;
mod settle;
mod source;

pub use debounce::{Debouncer, Due};
#[cfg(feature = "notify")]
pub use native::NotifySource;
pub use settle::{
    Batcher, DEFAULT_BATCH_MAX_FILES, DEFAULT_BATCH_MAX_WAIT_MS, DEFAULT_BATCH_QUIET_MS,
    DEFAULT_DEBOUNCE_MS, DEFAULT_MAX_FILE_BYTES, FileStamp, MAX_ATTEMPTS, MAX_PENDING,
    MAX_SCAN_DEPTH, MAX_SCAN_ENTRIES, Ready, Scan, Settle, SettleConfig, Skipped,
    is_temporary_name, scan_folder, scan_folder_limited,
};
pub use source::{ChangeSource, FakeSource, WatchError, WatchHandle, spawn};
