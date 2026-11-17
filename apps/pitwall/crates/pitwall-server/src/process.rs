//! The one place Pitwall starts child processes (clippy forbids `Command::new` elsewhere): no console window on
//! Windows, an empty environment plus an allow-list (and the restricting variables), below-normal priority, killed when
//! Pitwall drops the handle. The tokio command is made from `paddock_proc::command`, not from its own `Command::new`.

use std::ffi::OsStr;
use std::path::Path;
use tokio::process::Command;

/// Environment variables an engine inherits (nothing else: no tokens, proxies or user variables).
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
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "XDG_RUNTIME_DIR",
];

/// A command for `program` with the sanitised environment: the canonical no-console constructor
/// ([`paddock_proc::command`]), the one clearing function ([`paddock_proc::clean_env`]: [`KEPT_ENV`] plus the restricting
/// variables, ER-47), below-normal priority on Windows, killed when the handle drops.
pub fn command(program: impl AsRef<OsStr>, cwd: Option<&Path>) -> Command {
    let mut std_cmd = paddock_proc::command(program);
    paddock_proc::clean_env(&mut std_cmd, KEPT_ENV);
    if let Some(d) = cwd {
        std_cmd.current_dir(d);
    }
    #[cfg(windows)]
    paddock_proc::lower_priority(&mut std_cmd);
    let mut c = Command::from(std_cmd);
    c.kill_on_drop(true).stdin(std::process::Stdio::null());
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The child's environment as `env` / `set` print it, made while the test holds the environment lock (the command takes
    /// its copy of the environment when it is made, so the lock is let go before the child runs).
    async fn child_env(set: &[(&str, &str)]) -> String {
        let mut cmd = {
            let mut env = paddock_testkit::env_lock();
            for (k, v) in set {
                env.set(k, v);
            }
            if cfg!(windows) {
                let mut c = command("cmd", None);
                c.args(["/C", "set"]);
                c
            } else {
                command("/usr/bin/env", None)
            }
        };
        let out = cmd.output().await.unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[tokio::test]
    async fn environment_is_scrubbed() {
        let text = child_env(&[("PITWALL_SECRET_TEST", "x")]).await;
        assert!(!text.contains("PITWALL_SECRET_TEST"));
    }

    /// ER-47: the engine keeps the variables that restrict it though they are not in the allow list, with the caller's value (an
    /// engine inherits as every production child does, a loosening value too).
    #[tokio::test]
    async fn restricting_variables_reach_the_engine_as_the_caller_has_them() {
        for value in ["restricting", "loosen"] {
            let set: Vec<(&str, &str)> = paddock_proc::RESTRICTING
                .iter()
                .map(|r| (r.name, if value == "restricting" { r.pinned() } else { value }))
                .collect();
            let text = child_env(&set).await;
            for (k, v) in &set {
                assert!(text.contains(&format!("{k}={v}")), "{k}={v} was stripped: {text}");
            }
        }
    }
}
