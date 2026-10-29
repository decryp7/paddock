#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! Pacenotes' index, pure half (ADR 0005).
//!
//! Two ports keep the crate thin: [`WorkspaceFs`] (the file system, with [`DiskFs`] refusing links and anything outside the
//! workspace) and [`SymbolSource`] (the facts of one file; the real one is the parser worker of A3, so the app never parses in
//! process). The secret [`Denylist`] stands in front of the index, of `ReadAt` ([`GuardedHistory`]) and of everything that
//! serves content ([`redact`], [`Indexer::chunk_text`]). Nothing here spawns a process.

pub mod denylist;
pub mod facts;
pub mod fs;
pub mod guard;
pub mod indexer;
pub mod plan;
pub mod progress;
pub mod snapshot;
pub mod store;

pub use denylist::{Denied, Denylist, redact};
pub use facts::{CallFact, ChunkFact, FileFacts, ImportFact, SourceError, SymbolFact, SymbolSource};
pub use fs::{DirEntry, DiskFs, EntryKind, FsError, WorkspaceFs};
pub use guard::GuardedHistory;
pub use indexer::{IndexError, IndexLimits, IndexReport, Indexer};
pub use plan::{Current, Known, Plan, plan_changes};
pub use progress::{EpochClock, NoProgress, Progress, SystemEpoch};
pub use snapshot::{Snapshot, SnapshotLimits, snapshot, snapshot_with};
pub use store::{Batch, ChunkRow, IndexStore, Opened, RebuildReason, SearchHit, Stamp, Upsert};
