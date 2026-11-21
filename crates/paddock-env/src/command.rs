//! The canonical constructor of a program's [`Command`] (no console window on Windows) and the one way a child's environment is
//! cleared ([`clear_env`], which keeps [`RESTRICTING`]).
//!
//! `paddock_proc::command` re-exports [`command`] (the shared crates and tools start programs through it, and clippy forbids
//! `Command::new` there), and the test kit's `TestCommand` builds on it, so a test's child has the same start flags as the
//! product's. It lives in this leaf crate because `paddock-proc`'s own tests use the test kit: the kit cannot depend on
//! `paddock-proc` (that made a publish cycle, ci.yml 7989; ADR 0100). Clippy forbids `Command::new` in the shared crates and every
//! app, so every program is started from here ([`command`], or [`attached_command`] without creation flags), and each start is
//! counted for budget tests ([`crate::starts`]). The registered copies of this constructor (`docs/known-copies.toml`, rule 55,
//! entry `no-console-command`) were folded into it; the entry keeps this file as the canonical one.

use std::ffi::{OsStr, OsString};
use std::process::Command;

/// Windows process creation flag: no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command for `program` that opens no console window on Windows (a GUI app starting a console tool would flash one,
/// or, under Remote Desktop, show it). The canonical constructor; clippy forbids `Command::new` elsewhere.
#[allow(clippy::disallowed_methods)] // the canonical constructor
pub fn command(program: impl AsRef<OsStr>) -> Command {
    // The counting seam of budget tests (`crate::starts`): one start per command made here.
    crate::starts::note();
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

/// A command with no creation flags: the child shares this process's console (a command-line front end that runs its helper
/// in the same terminal, so Ctrl+C and the output reach it) or shows windows of its own the usual way (the app starting itself
/// again after an update). Counted like [`command`]; use [`command`] for a tool whose console window must not appear.
#[allow(clippy::disallowed_methods)] // the canonical constructor without creation flags
pub fn attached_command(program: impl AsRef<OsStr>) -> Command {
    crate::starts::note();
    Command::new(program)
}

/// The prompt policy of a tool a check starts (ER-47: a named policy with a reason; children otherwise inherit the environment).
/// Applied last, after the environment is set:
///
/// - standard input is `/dev/null` (`NUL` on Windows), so a program that asks "install X? [Y/n]" reads end-of-file at once instead
///   of waiting for an answer nobody gives (on 2026-11-20 `cargo llvm-cov` waited 4 h on "install llvm-tools?");
/// - `RUSTUP_AUTO_INSTALL=0`, so rustup does not install a missing toolchain on its own (ER-45; Paddock run 8060).
///
/// A caller that needs one of them says so after this call (`cmd.stdin(Stdio::inherit())`, `cmd.env("RUSTUP_AUTO_INSTALL", "1")`);
/// nothing else of the environment is touched. A caller that pipes standard input (`stdin(Stdio::piped())`) does so after it too.
pub fn no_prompts(cmd: &mut Command) -> &mut Command {
    cmd.stdin(std::process::Stdio::null())
        .env("RUSTUP_AUTO_INSTALL", "0")
}

/// A variable that **restricts** what a child may do, with the values that restrict it (ER-47). A value that loosens it
/// (`PADDOCK_DEV_DOWNLOAD=1`, `RUSTUP_AUTO_INSTALL=1`, `CARGO_NET_OFFLINE=false`) is not a restriction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restriction {
    /// The variable.
    pub name: &'static str,
    /// The values that restrict (compared without case); the first is the one a policy pins when it must set one.
    pub values: &'static [&'static str],
}

impl Restriction {
    /// Whether `value` is one of the restricting values.
    pub fn restricts(&self, value: &OsStr) -> bool {
        self.values.iter().any(|v| value.eq_ignore_ascii_case(v))
    }

    /// The value a policy pins: the first restricting one.
    pub fn pinned(&self) -> &'static str {
        self.values[0]
    }
}

/// The variables that restrict a child, each with its restricting values. A policy that clears a child's environment never
/// strips one the caller has set to a restricting value (ER-47): a child that lost it would do what the caller forbade.
///
/// - `RUSTUP_AUTO_INSTALL=0`: rustup (1.28.1 and later) does not install a missing pinned toolchain into `~/.rustup` when a probe
///   runs (ER-45).
/// - `CARGO_NET_OFFLINE=true` (or `1`): cargo does not reach the network.
/// - `PADDOCK_DEV_DOWNLOAD=0`: the download ban of release and tag jobs; without it `./dev` may download a prebuilt tool (ADR 0022).
/// - `PADDOCK_TOOLCHAIN_FAILED=1`: an earlier step of the job failed to install the toolchain, so `./dev` does not try again.
///
/// What a clearing site does with the value differs by who it serves. A production child ([`clear_env`], `paddock_proc::clean_env`)
/// inherits the caller's value as it is, the way every inherited variable is (ER-47). A test's child (the test kit's `TestCommand`,
/// [`clear_env_pinned_from`]) gets the variable only when the caller's value restricts, so a developer's `PADDOCK_DEV_DOWNLOAD=1`
/// never makes a test download. Why: on 2026-11-15/16 a stripped `PADDOCK_DEV_DOWNLOAD=0` re-enabled release downloads (ER-47).
/// Add a variable here only when one of its values only ever makes a child do less.
pub const RESTRICTING: &[Restriction] = &[
    Restriction {
        name: "RUSTUP_AUTO_INSTALL",
        values: &["0"],
    },
    Restriction {
        name: "CARGO_NET_OFFLINE",
        values: &["true", "1"],
    },
    Restriction {
        name: "PADDOCK_DEV_DOWNLOAD",
        values: &["0"],
    },
    Restriction {
        name: "PADDOCK_TOOLCHAIN_FAILED",
        values: &["1"],
    },
];

/// The names of [`RESTRICTING`].
pub fn restricting_names() -> impl Iterator<Item = &'static str> {
    RESTRICTING.iter().map(|r| r.name)
}

#[allow(clippy::disallowed_methods)] // the one clearing site
fn clear(cmd: &mut Command, keep: &[&str], lookup: &dyn Fn(&str) -> Option<OsString>, pin: bool) {
    cmd.env_clear();
    for key in keep {
        if let Some(value) = lookup(key) {
            cmd.env(key, value);
        }
    }
    for r in RESTRICTING {
        if let Some(value) = lookup(r.name)
            && (!pin || r.restricts(&value))
        {
            cmd.env(r.name, value);
        }
    }
}

/// Clears `cmd`'s environment, then sets every variable of `keep` and of [`RESTRICTING`] that `lookup` has, **as the caller has
/// it** (the caller's environment: [`clear_env`] reads the process's). The one function that clears a production child's
/// environment; clippy forbids `Command::env_clear` elsewhere. Variables set on `cmd` afterwards still reach the child.
pub fn clear_env_from(cmd: &mut Command, keep: &[&str], lookup: impl Fn(&str) -> Option<OsString>) {
    clear(cmd, keep, &lookup, false);
}

/// [`clear_env_from`] over this process's environment.
pub fn clear_env(cmd: &mut Command, keep: &[&str]) {
    clear_env_from(cmd, keep, |k| std::env::var_os(k));
}

/// Like [`clear_env_from`], for a test's child: a [`RESTRICTING`] variable is set only when the caller's value restricts
/// (`PADDOCK_DEV_DOWNLOAD=1` is dropped, not passed). The test kit reads what it captured through `lookup`.
pub fn clear_env_pinned_from(
    cmd: &mut Command,
    keep: &[&str],
    lookup: impl Fn(&str) -> Option<OsString>,
) {
    clear(cmd, keep, &lookup, true);
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

    fn names_all(c: &Command) -> Vec<String> {
        envs(c).into_iter().map(|(k, _)| k).collect()
    }

    /// Every restricting variable with a restricting value and with a loosening one.
    fn have(restricting: bool) -> impl Fn(&str) -> Option<OsString> {
        move |k: &str| {
            RESTRICTING.iter().find(|r| r.name == k).map(|r| {
                if restricting {
                    OsString::from(r.pinned())
                } else {
                    OsString::from("loosen")
                }
            })
        }
    }

    #[test]
    fn no_prompts_closes_stdin_and_pins_the_install_ban_and_nothing_else() {
        let mut c = command("x");
        c.env("KEEP", "1");
        no_prompts(&mut c);
        let e = envs(&c);
        assert!(
            e.contains(&("RUSTUP_AUTO_INSTALL".into(), "0".into())),
            "{e:?}"
        );
        assert!(e.contains(&("KEEP".into(), "1".into())), "{e:?}");
        assert_eq!(e.len(), 2, "{e:?}");
        // The child reads end-of-file at once (`cat` ends with no output); `output()` would otherwise wait for this test's stdin.
        #[cfg(unix)]
        {
            let mut c = command("cat");
            no_prompts(&mut c);
            let out = c
                .stdout(std::process::Stdio::piped())
                .output()
                .expect("cat runs");
            assert!(out.status.success() && out.stdout.is_empty());
        }
    }

    #[test]
    fn a_caller_can_opt_in_after_the_policy() {
        let mut c = command("x");
        no_prompts(&mut c);
        c.env("RUSTUP_AUTO_INSTALL", "1");
        assert!(envs(&c).contains(&("RUSTUP_AUTO_INSTALL".into(), "1".into())));
    }

    #[test]
    fn production_clearing_inherits_every_value_restricting_or_not() {
        for restricting in [true, false] {
            let mut c = command("x");
            c.env("BEFORE", "1");
            clear_env_from(&mut c, &["KEPT"], have(restricting));
            let seen = names_all(&c);
            for n in restricting_names() {
                assert!(
                    seen.iter().any(|s| s == n),
                    "{n} was stripped (restricting value: {restricting}): {seen:?}"
                );
            }
            assert!(!seen.iter().any(|s| s == "BEFORE"), "{seen:?}");
        }
    }

    #[test]
    fn pinned_clearing_passes_a_restricting_value_and_drops_a_loosening_one() {
        let mut c = command("x");
        clear_env_pinned_from(&mut c, &[], have(true));
        assert_eq!(names_all(&c).len(), RESTRICTING.len(), "{:?}", envs(&c));
        let mut c = command("x");
        clear_env_pinned_from(&mut c, &[], have(false));
        assert!(
            envs(&c).is_empty(),
            "a loosening value reached the child: {:?}",
            envs(&c)
        );
        // The loosening values the review named, one by one.
        for (k, v) in [
            ("PADDOCK_DEV_DOWNLOAD", "1"),
            ("RUSTUP_AUTO_INSTALL", "1"),
            ("CARGO_NET_OFFLINE", "false"),
        ] {
            let mut c = command("x");
            clear_env_pinned_from(&mut c, &[], |n| (n == k).then(|| OsString::from(v)));
            assert!(envs(&c).is_empty(), "{k}={v} reached the child");
        }
    }

    #[test]
    fn the_restricting_list_names_the_four_known_variables_each_with_a_value() {
        for k in [
            "RUSTUP_AUTO_INSTALL",
            "CARGO_NET_OFFLINE",
            "PADDOCK_DEV_DOWNLOAD",
            "PADDOCK_TOOLCHAIN_FAILED",
        ] {
            assert!(
                restricting_names().any(|n| n == k),
                "{k} is not in RESTRICTING"
            );
        }
        assert!(RESTRICTING.iter().all(|r| !r.values.is_empty()));
        assert!(
            RESTRICTING
                .iter()
                .all(|r| r.restricts(std::ffi::OsStr::new(r.pinned())))
        );
    }
}
