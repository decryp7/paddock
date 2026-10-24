//! Knowledge packs for the Paddock apps (rule 21, plan X15, decision D11): the lists an app knows (cleanup categories,
//! model catalogues, presets, patterns) are TOML packs, not code. Taken from Spotter's `pack.rs`, Pitwall's `packs.rs`
//! and Dyno's `cleanup/pack.rs`: Dyno's rule for errors, Pitwall's for unknown keys, Spotter's built-in plus user split,
//! and one answer to what the three did differently.
//!
//! - [`PackKind`]: what an app declares: its `serde` type for one pack file ([`PackKind::Raw`]), the format number it
//!   reads and the conversion into the value it uses ([`PackKind::Pack`]), where it applies its own rules (Dyno's
//!   "a user pack cannot widen what is deleted"). The shape is typed once, at the edge (parse, don't validate).
//! - [`parse_pack`]: one file, text in, value out. Bounded by [`Limits`] (file size, entries per list, string length,
//!   nesting, number of nodes) before the typed parse; it only reads data and never runs anything. A pack is refused
//!   whole with a message that names the problem (and the line for a syntax error); keys the kind does not know are
//!   warnings that name the key.
//! - [`PackLoader`] over a [`PackSource`] (a folder, [`DirSource`], or a fake in tests): built-in packs first, then the
//!   user's `*.toml` files in name order. One bad pack never touches the rest; an unchanged file is not parsed again; a
//!   pack that becomes broken while running keeps its last good version and reports why; a file deleted between listing
//!   and reading is simply gone. The result is a [`Snapshot`] (packs, problems, warnings) built whole, then swapped in.
//! - [`PackStore`]: the current [`Snapshot`] behind a lock, replaced only by a finished reload, and observers told once
//!   per real change. [`user_packs_dir`] names the folder from [`paddock_env::AppDirs`].
//! - [`Reloader`]: hot reload without a restart. Change events (from a [`ChangeSource`]) are debounced on a
//!   [`paddock_jobs::time::Clock`], so a storm of events is one reload and tests move the clock instead of sleeping.
//!   [`watch::spawn`] runs it on a background thread; `watch::NotifySource` (feature `watch`) is the real-folder source.
//! - [`user_regex`]: regexes written by users, compiled with a pattern length cap and a compiled-size cap, with typed
//!   errors (the merge of Debrief's and Spotter's copies).
//! - [`Snapshot::health`]: the one-line summary for a `packs` health or notification entry.
//!
//! The core has no Tauri dependency. Tests use temporary folders only.

mod error;
#[cfg(test)]
mod generated;
mod kind;
mod limits;
mod loader;
mod parse;
mod reload;
mod snapshot;
mod source;
mod store;
#[cfg(feature = "tauri")]
pub mod tauri;
pub mod user_regex;
pub mod view;
pub mod watch;

pub use error::PackError;
pub use kind::{Origin, PackKind};
pub use limits::Limits;
pub use loader::{BuiltinPack, PackLoader};
pub use parse::{Parsed, parse_pack};
pub use reload::{Polled, Reloader};
pub use snapshot::{Health, LoadedPack, Problem, Snapshot};
pub use source::{DirSource, PackSource, user_packs_dir};
pub use store::PackStore;
pub use watch::ChangeSource;

/// The event every change of a pack kind is sent on, to every window (a [`view::PacksView`]).
pub const EVENT_NAME: &str = "paddock://packs";

paddock_commands::command_table! {
    /// The commands of the packs adapter (`paddock_packs::tauri::invoke_handler` serves them; the shell plugin folds them
    /// into its table). Every window may look; main windows and the Settings dialog reload.
    pub static COMMANDS;
    /// Every pack kind now.
    Query [WINDOW] => tauri::packs_status,
    /// Reads the pack folders again (the Repair of the packs health entry, and a Reload button).
    Mutation [MAIN | DIALOG] => tauri::packs_reload,
}
