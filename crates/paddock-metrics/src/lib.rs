//! Metrics for the Paddock apps (plan X30): what the dashboards of Pitcrew, Pitwall, Dyno and Debrief's agent each kept
//! their own copy of.
//!
//! - [`TieredStore`]: a generic time-series store. Every sample goes into every tier of a [`Retention`] (for example
//!   5 s for an hour, 1 min for a day, 10 min for a week, 1 h for the retention period); a [`Series`] says how samples fold
//!   into a bucket. Memory is bounded by the retention (`Retention::capacity`) whatever the uptime; late samples are
//!   dropped, never reordered. [`StoreFile`] persists it through `paddock-env` (versioned, atomic, damaged gives an empty
//!   store).
//! - [`MachineSample`]: the series every app shows (CPU, memory, free disk, the app's own memory).
//! - [`Sampler`]: reads the machine on the shared `Clock` through a [`MachineProbe`]; [`SystemProbe`] is the real one
//!   (`sysinfo`, and `paddock-proc` for the app's own memory: PSS on Linux, threads skipped, rule 26).
//! - [`volume_free`], [`free_bytes`]: free space of the volume holding a path (the mount with the longest matching
//!   mount point; verbatim Windows paths match plain ones). [`folder_size`]: a bounded walk (entries, depth, time,
//!   cancel; links are not followed).
//!
//! Pure except [`SystemProbe`] and [`free_bytes`]; times come from an injected `Clock`, so no test sleeps.

mod disk;
mod file;
mod machine;
mod native;
mod retention;
mod sampler;
mod series;
mod size;
mod store;

pub use disk::{Mount, Space, existing_ancestor, free_bytes, volume_free, volume_of};
pub use file::StoreFile;
pub use machine::{MachineAcc, MachineSample};
pub use native::SystemProbe;
pub use retention::{MAX_KEEP, MAX_POINTS, MAX_TIERS, Retention, RetentionError, TierSpec};
pub use sampler::{DEFAULT_MOUNTS_EVERY, MachineProbe, Sampler};
pub use series::{Mean, Millis, Series, bucket_start};
pub use size::{EntryKind, FolderSize, Limit, Listing, SizeLimits, folder_size, folder_size_with};
pub use store::{Pushed, TieredStore};
