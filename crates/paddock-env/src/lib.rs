//! Folders and state files for the Paddock apps (plan X7, decision D3).
//!
//! - [`AppDirs`]: the app's data, config, cache, log and runtime folders per OS, from an injected environment
//!   ([`Vars`]) so every layout is testable everywhere; one override variable `<ID>_HOME` (debug builds and tests only,
//!   ignored in release builds) and portable mode (a `portable.flag` beside the executable). Windows is `%LOCALAPPDATA%`
//!   for everything; [`AppDirs::legacy_roaming_data`] names the old Roaming folder for the one-time move.
//! - [`VersionedFile`]: a JSON state file with a `version`: atomic writes with fsync and a `.bak`, missing or damaged gives
//!   defaults (the damaged file is kept as `.corrupt-<time>`, a valid `.bak` is used), a newer version is refused and left
//!   untouched, older versions go through a migration chain once.
//! - [`check_private_dir`] and [`private_fallback_dir`]: a folder only the current user controls; the last-resort folder is
//!   per user and verified, never a shared fixed path.
//! - [`write_atomic`]: the atomic replace under it, for files that are not JSON state.
//!
//! Tests use temporary folders only and never read or write the user's real folders.

mod app;
mod atomic;
mod dirs;
mod error;
mod private;
mod versioned;

pub use app::AppId;
pub use atomic::write_atomic;
pub use dirs::{AppDirs, MapVars, Os, PORTABLE_FLAG, ProcessVars, Source, Vars};
pub use error::{EnvError, Result};
pub use private::{Untrusted, check_private_dir, private_fallback_dir};
pub use versioned::{
    DEFAULT_KEEP_CORRUPT, DEFAULT_MAX_BYTES, FIRST_VERSION, Loaded, Migration, Problem,
    VersionedFile,
};
