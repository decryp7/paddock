//! Building a [`Command`]: no console window, a cleared environment with an allow-list, lower priority.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use std::process::Command;

/// Windows process creation flag: no console window ([`paddock_env::command::CREATE_NO_WINDOW`]).
pub use paddock_env::command::CREATE_NO_WINDOW;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Environment variables a sanitised child keeps ([`clean_env`]): what the OS and runtimes need, nothing ambient (no
/// tokens, proxies or user variables).
pub const KEPT_ENV: &[&str] = &[
    "PATH",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "USERPROFILE",
    "LANG",
    "LC_ALL",
    "RUST_BACKTRACE",
    "LLVM_PROFILE_FILE",
    "XDG_RUNTIME_DIR",
];

/// The variables [`clean_env`] never strips ([`paddock_env::RESTRICTING`]).
pub use paddock_env::RESTRICTING;

/// A command for `program` that opens no console window on Windows ([`paddock_env::command::command`], the one place programs
/// are started from; clippy forbids `Command::new` elsewhere).
pub use paddock_env::command::command;

/// Clears the child's environment and copies back only the variables in `keep` (use [`KEPT_ENV`]) that this process has, and
/// every restricting variable it has ([`paddock_env::RESTRICTING`]: a download ban, an offline switch; a child that lost one would
/// do what this process forbade, ER-47). Variables added afterwards with `env` still reach the child. The one clearing function:
/// clippy forbids `Command::env_clear` elsewhere.
pub fn clean_env(cmd: &mut Command, keep: &[&str]) {
    paddock_env::clear_env(cmd, keep);
}

/// Marks the command to start at lower priority so the app stays responsive: below-normal on Windows (this sets the whole
/// creation flags, still without a console window). On Unix nothing is set on the command: the priority is lowered right
/// after the spawn through the held child ([`Supervised::lower_priority`], [`Limits::lowered`]), because a `pre_exec` hook
/// forces fork+exec, and a fork carries copies of the process's descriptors (locks, pipes) until it execs. Not `nice(1)`:
/// running through it drops `DYLD_*` variables on macOS.
pub fn lower_priority(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(not(windows))]
    let _: &mut Command = cmd;
}

/// Starts `cmd`, trying again for a bounded time when the OS answers `ETXTBSY` ("text file busy"): the program was written or
/// ended a moment ago and something (a child another thread forked in the middle of the write, a dying process's address
/// space) still holds it. Every start of this crate goes through here, so a plugin or helper installed a moment ago is not
/// reported as a crash. Any other error returns at once. Not covered, on purpose: the guardian and its keeper (they start this
/// executable, which is not written a moment before) and Windows' `taskkill` (a system program; ETXTBSY is a Unix refusal). See [`paddock_env::text_busy`] for the cause, the bound and the
/// sources.
pub(crate) fn spawn_retrying(
    cmd: &mut std::process::Command,
) -> std::io::Result<std::process::Child> {
    let what = format!("start {}", cmd.get_program().to_string_lossy());
    paddock_env::text_busy::retry(&what, || cmd.spawn())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_is_cleared_to_the_allow_list() {
        // The test process always has PATH; a name it surely lacks must not appear.
        let mut c = command("x");
        c.env("PADDOCK_PROC_AMBIENT", "secret");
        clean_env(&mut c, &["PATH", "PADDOCK_PROC_NOT_SET_ANYWHERE"]);
        c.env("ADDED_AFTER", "1");
        let envs: Vec<_> = c
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.is_some()))
            .collect();
        assert!(envs.contains(&("PATH".to_string(), true)));
        assert!(envs.contains(&("ADDED_AFTER".to_string(), true)));
        assert!(
            !envs
                .iter()
                .any(|(k, _)| k == "PADDOCK_PROC_NOT_SET_ANYWHERE"),
            "unset variables are not invented"
        );
        assert!(
            !envs.iter().any(|(k, _)| k == "PADDOCK_PROC_AMBIENT"),
            "env_clear drops what was set before"
        );
    }

    /// ER-47: a variable that restricts the child survives the clearing, though nobody listed it in `keep`; a positive control
    /// (a variable of the same shape that is not restricting) is dropped.
    #[test]
    fn restricting_variables_survive_the_clearing_without_being_kept() {
        let mut env = paddock_testkit::env_lock();
        for k in paddock_env::RESTRICTING {
            env.set(k, "0");
        }
        env.set("PADDOCK_PROC_NOT_RESTRICTING", "0");
        let mut c = command("x");
        clean_env(&mut c, &["PATH"]);
        let seen: Vec<String> = c
            .get_envs()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for k in paddock_env::RESTRICTING {
            assert!(seen.iter().any(|s| s == k), "{k} was stripped: {seen:?}");
        }
        assert!(
            !seen.iter().any(|s| s == "PADDOCK_PROC_NOT_RESTRICTING"),
            "{seen:?}"
        );
    }

    #[test]
    fn a_child_sees_only_the_kept_variables() {
        use crate::testchild::{Mode, child};
        let mut c = child(Mode::PrintEnv);
        c.env("PADDOCK_PROC_CLEANED_AWAY", "1");
        clean_env(
            &mut c,
            &[
                "PATH",
                "SystemRoot",
                "SYSTEMROOT",
                "TMPDIR",
                "TEMP",
                "TMP",
                "HOME",
            ],
        );
        // Variables added after `clean_env` (the mode marker, this one) still reach the child.
        c.env("PADDOCK_PROC_AMBIENT", "ambient-value")
            .env("PADDOCK_PROC_MODE", "print_env");
        let out =
            crate::run_bounded(c, crate::Limits::new(std::time::Duration::from_secs(60))).unwrap();
        let text = out.stdout_text();
        let seen = crate::testchild::lines(&text);
        assert!(
            seen.contains(&"PADDOCK_PROC_AMBIENT=ambient-value"),
            "{seen:?}"
        );
        assert!(
            !seen
                .iter()
                .any(|l| l.starts_with("PADDOCK_PROC_CLEANED_AWAY")),
            "{seen:?}"
        );
        assert!(
            seen.iter().any(|l| l.starts_with("PATH=")),
            "an allow-listed variable is kept"
        );
    }
}
