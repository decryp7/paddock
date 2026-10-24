//! Safe archive unpacking (plan X24): zip, tar, tar.gz and tar.xz from the network or a user, into a folder, without trusting a
//! single byte of it.
//!
//! - [`extract`] unpacks all or nothing: into a staging folder beside the destination, then a rename swaps it in. A failure, a
//!   limit or a cancel removes the staging folder and leaves the old destination as it was.
//! - Every name is parsed once ([`Limits`], `..`, absolute paths, drive letters, backslashes, control characters, Windows device
//!   names and case-only duplicates are refused); files are created with `create_new`, so nothing is overwritten.
//! - [`Limits`] cap entries, file size, total size, the expansion ratio against the archive's size (a zip bomb), depth and path
//!   length; the caps are checked while writing, not after.
//! - [`Links`] says what happens to symbolic and hard links: refused, skipped, copied (target resolved inside the tree) or kept as
//!   relative links that cannot point out of the tree. Devices, fifos and sockets are never created.
//! - [`Layout`] keeps, strips or flattens the tree; a name filter picks the wanted entries (the runtimes unpack only their
//!   programs and libraries).
//! - Progress and cancel go through [`Control`]; [`extract_job`] runs it as a `paddock-jobs` job.
//!
//! Formats are recognised from the content ([`Format::sniff`]). Zstd is not supported (no app needs it yet). The readers
//! never panic on any input (a fuzz target lives in `fuzz/`).

mod control;
mod error;
mod job;
mod name;
mod options;
mod readers;
mod stage;
mod writer;

pub use control::{Control, NoControl};
pub use error::{ArchiveError, Result};
pub use job::extract_job;
pub use options::{Filter, Layout, Limits, Links, Modes, Options};
pub use readers::Format;
pub use stage::{extract, sweep_stale};
pub use writer::Report;
