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
//! - [`command`]: the canonical no-console `Command` constructor, [`command::RESTRICTING`] (the variables no policy strips from a child,
//!   ER-47) and [`command::clear_env`], the one function that clears a child's environment.
//! - [`vars`]: this process's environment as text, skipping (and naming) a variable that is not valid Unicode instead of panicking
//!   like `std::env::vars`, which Clippy forbids everywhere.
//! - [`cargo_layout`]: the profile folder above a test program or `OUT_DIR`, in cargo's old and 1.100 build-folder layouts.
//! - [`winpath`]: Windows paths without the verbatim `\\?\` prefix, in one place (`plain`, `for_tools`, `real*`, `same`).
//! - [`home_guard`]: the one "is this folder inside the home folder" check (symlinks resolved), shared by `heavy` and `paddock-dev`.
//! - [`text_busy`]: the bounded retry of `ETXTBSY` for a program that was written or ended a moment ago (shared by `paddock-proc`
//!   and the test kit).
//!
//! Tests use temporary folders only and never read or write the user's real folders.

mod app;
mod atomic;
pub mod cargo_layout;
pub mod command;
pub mod dev_override;
mod dirs;
mod error;
mod folder;
pub mod home_guard;
pub mod private;
mod process_vars;
pub mod starts;
pub mod text_busy;
mod versioned;
pub mod winpath;

pub use app::AppId;
pub use atomic::{write_atomic, write_atomic_created};
pub use command::{
    RESTRICTING, Restriction, clear_env, clear_env_from, clear_env_pinned_from, restricting_names,
};
pub use dirs::{AppDirs, MapVars, Os, PORTABLE_FLAG, ProcessVars, Source, Vars};
pub use error::{EnvError, Result};
pub use folder::{
    FolderFacts, Links, Refusal, check_data_folder, check_folder, check_folder_with,
    check_own_folder, create_private_dir_all, current_uid, decide, read_folder,
};
pub use private::{
    PrivateDir, PrivateError, PrivateFile, check_temp_parent, private_fallback_dir,
    private_fallback_dir_as,
};
pub use process_vars::{vars, vars_skipping};
pub use versioned::{
    DEFAULT_KEEP_CORRUPT, DEFAULT_MAX_BYTES, FIRST_VERSION, Loaded, Migration, Problem,
    VersionedFile,
};
