//! Building a [`Command`]: no console window, a cleared environment with an allow-list, lower priority.

use std::ffi::OsStr;
use std::process::Command;

/// Windows process creation flag: no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
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

/// A command for `program` that opens no console window on Windows (a GUI app starting a console tool would flash one,
/// or, under Remote Desktop, show it). The only place the apps start programs; clippy forbids `Command::new` elsewhere.
#[allow(clippy::disallowed_methods)] // the single allowed place
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

/// Clears the child's environment and copies back only the variables in `keep` (use [`KEPT_ENV`]) that this process has.
/// Variables added afterwards with `env` still reach the child.
pub fn clean_env(cmd: &mut Command, keep: &[&str]) {
    cmd.env_clear();
    for key in keep {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
}

/// Starts the child at lower priority so the app stays responsive: below-normal on Windows (this sets the whole creation
/// flags, still without a console window), `setpriority` before exec elsewhere. Not `nice(1)`: running through it drops
/// `DYLD_*` variables on macOS.
pub fn lower_priority(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(unix)]
    crate::sys::lower_priority_before_exec(cmd);
    #[cfg(not(any(windows, unix)))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_for_the_program_and_nothing_else_is_added() {
        let c = command("some-tool");
        assert_eq!(c.get_program(), "some-tool");
        assert_eq!(c.get_args().count(), 0);
    }

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

    #[cfg(unix)]
    #[test]
    fn lower_priority_raises_the_niceness_of_the_child() {
        use crate::testchild::{Mode, child};
        let mut c = child(Mode::PrintNice);
        lower_priority(&mut c);
        let out =
            crate::run_bounded(c, crate::Limits::new(std::time::Duration::from_secs(60))).unwrap();
        let nice: i32 = crate::testchild::lines(&out.stdout_text())[0]
            .trim()
            .parse()
            .unwrap();
        assert!(nice >= 10, "niceness {nice}");
    }
}
