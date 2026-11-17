//! The canonical constructor of a program's [`Command`] (no console window on Windows) and the one way a child's environment is
//! cleared ([`clear_env`], which keeps [`RESTRICTING`]).
//!
//! `paddock_proc::command` re-exports [`command`] (the shared crates and tools start programs through it, and clippy forbids
//! `Command::new` there), and the test kit's `TestCommand` builds on it, so a test's child has the same start flags as the
//! product's. It lives in this leaf crate because `paddock-proc`'s own tests use the test kit: the kit cannot depend on
//! `paddock-proc` (that made a publish cycle, ci.yml 7989; ADR 0100). Apps that cannot depend on it keep a registered copy
//! (`docs/known-copies.toml`, rule 55, entry `no-console-command`); this file is the canonical one.

use std::ffi::{OsStr, OsString};
use std::process::Command;

/// Windows process creation flag: no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command for `program` that opens no console window on Windows (a GUI app starting a console tool would flash one,
/// or, under Remote Desktop, show it). The canonical constructor; clippy forbids `Command::new` elsewhere.
#[allow(clippy::disallowed_methods)] // the canonical constructor
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    };
    cmd
}

/// Variables that **restrict** what a child may do, so a policy that clears a child's environment never strips them (ER-47):
/// a child that lost one would do what the caller forbade.
///
/// - `RUSTUP_AUTO_INSTALL`: `0` stops rustup (1.28.1 and later) installing a missing pinned toolchain into `~/.rustup` when a
///   probe runs (ER-45).
/// - `CARGO_NET_OFFLINE`: stops cargo reaching the network.
/// - `PADDOCK_DEV_DOWNLOAD`: `0` is the download ban of release and tag jobs; without it `./dev` downloads a prebuilt tool (ADR 0022).
/// - `PADDOCK_TOOLCHAIN_FAILED`: `1` marks that an earlier step of the job failed to install the toolchain, so `./dev` does not
///   try again.
///
/// Why: on 2026-11-15/16 a stripped `PADDOCK_DEV_DOWNLOAD=0` re-enabled release downloads (ER-47). Add a variable here when it
/// only ever makes a child do less; every clearing site reads this one list ([`clear_env`], `TestCommand`).
pub const RESTRICTING: &[&str] = &[
    "RUSTUP_AUTO_INSTALL",
    "CARGO_NET_OFFLINE",
    "PADDOCK_DEV_DOWNLOAD",
    "PADDOCK_TOOLCHAIN_FAILED",
];

/// Clears `cmd`'s environment, then sets every variable of `keep` and of [`RESTRICTING`] that `lookup` has (the caller's
/// environment: [`clear_env`] reads the process's, the test kit reads what it captured). The one function that clears a child's
/// environment; clippy forbids `Command::env_clear` elsewhere. Variables set on `cmd` afterwards still reach the child.
#[allow(clippy::disallowed_methods)] // the one clearing site
pub fn clear_env_from(cmd: &mut Command, keep: &[&str], lookup: impl Fn(&str) -> Option<OsString>) {
    cmd.env_clear();
    for key in keep.iter().chain(RESTRICTING) {
        if let Some(value) = lookup(key) {
            cmd.env(key, value);
        }
    }
}

/// [`clear_env_from`] over this process's environment.
pub fn clear_env(cmd: &mut Command, keep: &[&str]) {
    clear_env_from(cmd, keep, |k| std::env::var_os(k));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_for_the_program_and_nothing_else_is_added() {
        let c = command("some-tool");
        assert_eq!(c.get_program(), "some-tool");
        assert_eq!(c.get_args().count(), 0);
        assert_eq!(c.get_envs().count(), 0);
    }

    fn envs(c: &Command) -> Vec<(String, String)> {
        c.get_envs()
            .filter_map(|(k, v)| {
                v.map(|v| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect()
    }

    #[test]
    fn clearing_keeps_the_list_and_every_restricting_variable_the_caller_has() {
        let have = |k: &str| match k {
            "RUSTUP_AUTO_INSTALL"
            | "CARGO_NET_OFFLINE"
            | "PADDOCK_DEV_DOWNLOAD"
            | "PADDOCK_TOOLCHAIN_FAILED"
            | "KEPT"
            | "AMBIENT" => Some(OsString::from(format!("{k}-value"))),
            _ => None,
        };
        let mut c = command("x");
        c.env("BEFORE", "1");
        clear_env_from(&mut c, &["KEPT", "ABSENT"], have);
        let seen = envs(&c);
        for k in RESTRICTING {
            assert!(
                seen.contains(&((*k).to_string(), format!("{k}-value"))),
                "{k} was stripped: {seen:?}"
            );
        }
        assert!(seen.iter().any(|(k, _)| k == "KEPT"));
        assert!(
            !seen
                .iter()
                .any(|(k, _)| k == "AMBIENT" || k == "BEFORE" || k == "ABSENT"),
            "{seen:?}"
        );
    }

    #[test]
    fn clearing_over_nothing_sets_nothing_and_the_restricting_list_is_the_four_known_variables() {
        let mut c = command("x");
        clear_env_from(&mut c, &[], |_| None);
        assert!(envs(&c).is_empty());
        for k in [
            "RUSTUP_AUTO_INSTALL",
            "CARGO_NET_OFFLINE",
            "PADDOCK_DEV_DOWNLOAD",
            "PADDOCK_TOOLCHAIN_FAILED",
        ] {
            assert!(RESTRICTING.contains(&k), "{k} is not in RESTRICTING");
        }
    }
}
