use crate::Origin;
use serde::Serialize;

/// A problem with one pack file, attributed to it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// The pack's file name (or the built-in pack's name).
    pub file: String,
    /// What to fix, in words for the user.
    pub message: String,
}

/// One pack that loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPack<P> {
    /// The file name (`cleanup.toml`) or the built-in pack's name.
    pub name: String,
    /// Built-in or the user's.
    pub origin: Origin,
    /// The app's value.
    pub pack: P,
}

/// Everything loaded from the built-in packs and the user's folder at one moment. Built whole, then swapped in; readers
/// holding an old one are not disturbed.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot<P> {
    /// Counts up with every real change of the set (0 before the first load).
    pub revision: u64,
    /// The packs that loaded: built-in first, then the user's in file-name order. The app merges them (later wins,
    /// duplicates are its rule).
    pub packs: Vec<LoadedPack<P>>,
    /// Packs refused (or whose newer version was refused and an older good one kept), unreadable files, files over
    /// the pack limit.
    pub problems: Vec<Problem>,
    /// Unknown keys and other notes that did not stop a pack.
    pub warnings: Vec<Problem>,
}

impl<P> Default for Snapshot<P> {
    fn default() -> Self {
        Self {
            revision: 0,
            packs: Vec::new(),
            problems: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// The one-line summary for a `packs` health or notification entry.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "level", rename_all = "camelCase")]
pub enum Health {
    /// Every pack loaded and nothing was ignored.
    Ok,
    /// Everything usable loaded, but some keys were ignored.
    Notes {
        /// What to tell the user.
        message: String,
    },
    /// At least one pack was refused; the rest are in use.
    Degraded {
        /// What to tell the user (names the first problem and how many more).
        message: String,
    },
}

impl<P> Snapshot<P> {
    /// The summary for the health entry; the first problem is named, the rest counted.
    pub fn health(&self) -> Health {
        if let Some(first) = self.problems.first() {
            let more = self.problems.len() - 1;
            let tail = if more == 0 {
                String::new()
            } else {
                format!(" (and {more} more)")
            };
            return Health::Degraded {
                message: format!("{}: {}{tail}", first.file, first.message),
            };
        }
        if let Some(first) = self.warnings.first() {
            let more = self.warnings.len() - 1;
            let tail = if more == 0 {
                String::new()
            } else {
                format!(" (and {more} more)")
            };
            return Health::Notes {
                message: format!("{}: {}{tail}", first.file, first.message),
            };
        }
        Health::Ok
    }
}
