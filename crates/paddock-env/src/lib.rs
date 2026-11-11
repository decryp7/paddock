//! Folders and state files for the Paddock apps (plan X7, decision D3).
//!
//! - [`AppDirs`]: the app's data, config, cache, log and runtime folders per OS, from an injected environment
//!   ([`Vars`]) so every layout is testable everywhere; one override variable `<ID>_HOME` (debug builds and tests only,
//!   ignored in release builds) and portable mode (a `portable.flag` beside the executable). Windows is `%LOCALAPPDATA%`
//!   for everything; [`AppDirs::legacy_roaming_data`] names the old Roaming folder for the one-time move.
//! - [`VersionedFile`]: a JSON state file with a `version`: atomic writes with fsync and a `.bak`, missing or damaged gives
//!   defaults (the damaged file is kept as `.corrupt-<time>`, a valid `.bak` is used), a newer version is refused and left
//!   untouched, older versions go through a migration chain once.
//! - [`check_folder`] and [`private_fallback_dir`]: the one folder-trust check (owner and mode from a handle) and the last-resort
//!   folder, which is per user and verified, never a shared fixed path.
//! - [`write_atomic`]: the atomic replace under it, for files that are not JSON state; [`write_atomic_created`] where
//!   the caller creates the temporary file (an owner-only, unshared file for a secret on Windows).
//! - [`winpath`]: Windows paths without the verbatim `\\?\` prefix, in one place (`plain`, `for_tools`, `real*`, `same`).
//!
//! Tests use temporary folders only and never read or write the user's real folders.

mod app;
mod atomic;
pub mod dev_override;
mod dirs;
mod error;
mod folder;
pub mod private;
mod versioned;
pub mod winpath;

pub use app::AppId;
pub use atomic::{write_atomic, write_atomic_created};
pub use dirs::{AppDirs, MapVars, Os, PORTABLE_FLAG, ProcessVars, Source, Vars};
pub use error::{EnvError, Result};
pub use folder::{
    FolderFacts, Links, Refusal, Tightened, check_data_folder, check_folder, check_folder_with,
    check_own_folder, create_private_dir_all, current_uid, decide, is_private_group,
    may_remove_group_write, others_among, read_folder,
};
#[cfg(unix)]
pub use folder::{group_has_other_members, tighten_group_write, tighten_group_write_with};
pub use private::{
    PrivateDir, PrivateError, PrivateFile, check_temp_parent, private_fallback_dir,
    private_fallback_dir_as,
};
pub use versioned::{
    DEFAULT_KEEP_CORRUPT, DEFAULT_MAX_BYTES, FIRST_VERSION, Loaded, Migration, Problem,
    VersionedFile,
};
