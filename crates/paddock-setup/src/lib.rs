//! `paddock-setup`: the installer's work for per-machine Windows installs (family ADR 0053).
//!
//! A small program, `paddock-setup.exe`, is carried inside each app's NSIS installer (built per app, with the app's update
//! key compiled in) and does what NSIS cannot do safely: refuse a per-user copy, check the install folder, stop every
//! session's programs, clean `Run` values, and, for updates, take the elevated hand-off (`apply-update`, ADR section 4.2).
//!
//! The crate is a **pure core** (this library: argument parsing, decisions over ports, version rules, hive-scan filtering,
//! the setup log, the `apply-update` sequence) and **thin native glue** (`native`, Windows only, no logic). Every decision is
//! unit-tested over fakes on any host; the glue is tested on a Windows machine (`tests/it/windows.rs`, ER-14: only objects
//! the test made).
//!
//! # Command line contract
//!
//! `paddock-setup <command> [options]`; every option is `--name value` or a bare `--flag`. Exit codes are [`exit::Exit`].
//! See [`args::Command`] for the commands and [`exit::Exit::code`] for the codes.
//!
//! # Logging (ER-27)
//!
//! `setup.log` lives in `$INSTDIR\updates\` and is opened only through a [`log::CheckedDir`], which only a passed
//! `check-dir` makes. It holds phases with start time and duration, processes acted on (image, pid, start time, session,
//! method, outcome), exit codes. No user names, profile paths or command lines.

pub mod apply;
pub mod args;
pub mod checkdir;
pub mod exit;
pub mod guard;
pub mod handoff;
pub mod log;
pub mod runvalues;
pub mod selftest;
pub mod samefolder;
pub mod staging;
pub mod stop;
pub mod versionres;

#[cfg(windows)]
pub mod native;

/// The names baked in by the per-app build (`PADDOCK_SETUP_*` environment variables at compile time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Baked {
    /// The bundle identifier (`com.decryp7.dyno`), the stem of the `Global\<identifier>.setup` mutex.
    pub identifier: Option<&'static str>,
    /// The product name (`Dyno`), the uninstall key name.
    pub product: Option<&'static str>,
    /// The main binary's file name (`dyno.exe`), whose version resource is the installed version.
    pub main: Option<&'static str>,
    /// The base64 minisign public key, the same text as `plugins.updater.pubkey`.
    pub update_key: Option<&'static str>,
}

/// What this build was compiled with.
pub const BAKED: Baked = Baked {
    identifier: option_env!("PADDOCK_SETUP_IDENTIFIER"),
    product: option_env!("PADDOCK_SETUP_PRODUCT"),
    main: option_env!("PADDOCK_SETUP_MAIN"),
    update_key: option_env!("PADDOCK_SETUP_UPDATE_KEY"),
};
