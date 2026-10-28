//! The disk-space map (read-only): a bounded size tree of one folder. Nothing here changes a file.

pub mod usage;
pub mod walk;

use dyno_core::disk::SizeNode;
use walk::Budget;

/// The size tree of `path` (the system drive when `None`).
pub fn disk_usage(path: Option<&str>) -> Result<SizeNode, String> {
    let root = match path {
        Some(p) => usage::check(p)?,
        None => usage::default_root(),
    };
    Ok(usage::size_tree(&root, Budget::new(usage::MAX_ENTRIES, usage::MAX_SECS)))
}
