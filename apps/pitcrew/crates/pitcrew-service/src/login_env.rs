//! The user's `PATH` for the runners (macOS and Linux).
//!
//! launchd and the systemd user manager start the service with a minimal `PATH`, so jobs fail with "Cannot find:
//! node in PATH". The service asks the user's login shell (`$SHELL -l -i -c`, bounded output, timeout) for the `PATH`
//! a terminal would have and passes it to the runners; when the shell does not answer, a sane fallback is used
//! (`pitcrew_core::runner_env::fallback_path`). The parsing and the fallback are pure and live in the core; this
//! module only runs the shell and looks at the disk.

use crate::process::{quiet_command, run_with_timeout};
use pitcrew_core::runner_env::{fallback_path, newest_node_version, parse_probe_output, probe_script, FallbackInputs};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long one attempt to ask the shell may take (an `.zshrc` that hangs must not hang the service).
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an answer is reused. Runners (re)start rarely; a change in the user's shell set-up is picked up within this.
pub const CACHE_FOR: Duration = Duration::from_secs(60);

/// Asks `shell` for its `PATH`: first as a login interactive shell (so `~/.zshrc`, where nvm usually lives, runs),
/// then as a plain login shell. `None` if neither answers within `timeout`.
pub fn ask_shell(shell: &Path, timeout: Duration) -> Option<String> {
    let script = probe_script();
    for flags in [&["-l", "-i", "-c"][..], &["-l", "-c"][..]] {
        let mut cmd = quiet_command(shell);
        cmd.args(flags).arg(&script).env("TERM", "dumb");
        if let Ok(out) = run_with_timeout(cmd, timeout) {
            if let Some(p) = parse_probe_output(&out.stdout) {
                return Some(p);
            }
        }
    }
    None
}

/// The shells to try: `$SHELL` if it is an absolute path to a file, then the platform's usual ones.
pub fn shell_candidates(shell_var: Option<&str>, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let usual: &[&str] = if cfg!(target_os = "macos") {
        &["/bin/zsh", "/bin/bash"]
    } else {
        &["/bin/bash", "/bin/sh"]
    };
    for s in shell_var.into_iter().chain(usual.iter().copied()) {
        let p = PathBuf::from(s);
        if p.is_absolute() && exists(&p) && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// The fallback `PATH` for this machine.
pub fn machine_fallback() -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let nvm = home.join(".nvm/versions/node");
    let names: Vec<String> = std::fs::read_dir(&nvm)
        .map(|d| d.filter_map(Result::ok).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    let nvm_node_bin = newest_node_version(names.iter().map(String::as_str)).map(|v| nvm.join(v).join("bin"));
    let inputs = FallbackInputs {
        home,
        nvm_node_bin,
        inherited: std::env::var("PATH").unwrap_or_default(),
    };
    fallback_path(&inputs, |p| p.is_dir())
}

/// The login `PATH`, asked fresh when the cached answer is older than [`CACHE_FOR`].
#[derive(Debug, Default)]
pub struct LoginPath {
    cached: Mutex<Option<(Instant, String)>>,
}

impl LoginPath {
    /// A resolver with nothing cached.
    pub fn new() -> LoginPath {
        LoginPath::default()
    }

    /// The `PATH` to give the runners: the login shell's, else the fallback. Never empty.
    pub fn get(&self) -> String {
        let mut cached = self.cached.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, path)) = cached.as_ref() {
            if at.elapsed() < CACHE_FOR {
                return path.clone();
            }
        }
        let shell_var = std::env::var("SHELL").ok();
        let path = shell_candidates(shell_var.as_deref(), |p| p.is_file())
            .iter()
            .find_map(|s| ask_shell(s, SHELL_TIMEOUT))
            .unwrap_or_else(machine_fallback);
        *cached = Some((Instant::now(), path.clone()));
        path
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pitcrew-loginenv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_shells_path_is_taken_and_its_noise_ignored() {
        let d = tmp("ok");
        // Like a login shell: prints a banner, then the script's markers with its own PATH.
        let sh = script(
            &d,
            "fakezsh",
            "echo banner; printf '__PITCREW_PATH_BEGIN__/home/u/.nvm/bin:/opt/homebrew/bin__PITCREW_PATH_END__'",
        );
        assert_eq!(
            ask_shell(&sh, Duration::from_secs(10)).unwrap(),
            "/home/u/.nvm/bin:/opt/homebrew/bin"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_interactive_failure_falls_back_to_a_plain_login_shell() {
        let d = tmp("flags");
        let sh = script(
            &d,
            "fakesh",
            "for a in \"$@\"; do [ \"$a\" = -i ] && exit 1; done; printf '__PITCREW_PATH_BEGIN__/plain/bin__PITCREW_PATH_END__'",
        );
        assert_eq!(ask_shell(&sh, Duration::from_secs(10)).unwrap(), "/plain/bin");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_hanging_or_silent_or_missing_shell_gives_none() {
        let d = tmp("bad");
        let hang = script(&d, "hang", "sleep 30");
        let started = Instant::now();
        assert_eq!(ask_shell(&hang, Duration::from_millis(300)), None);
        assert!(started.elapsed() < Duration::from_secs(15));
        let silent = script(&d, "silent", "echo hello");
        assert_eq!(ask_shell(&silent, Duration::from_secs(10)), None);
        assert_eq!(ask_shell(&d.join("missing"), Duration::from_secs(1)), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn shell_candidates_prefer_the_users_shell_and_skip_missing_or_relative_ones() {
        let all = |_: &Path| true;
        let c = shell_candidates(Some("/usr/bin/fish"), all);
        assert_eq!(c[0], PathBuf::from("/usr/bin/fish"));
        assert!(c.len() >= 2);
        assert_eq!(shell_candidates(Some("zsh"), all)[0].as_os_str() == "zsh", false);
        assert!(shell_candidates(Some("/nope"), |p| p != Path::new("/nope"))
            .iter()
            .all(|p| p != Path::new("/nope")));
        assert!(shell_candidates(None, |_| false).is_empty());
        let dup = shell_candidates(Some("/bin/bash"), all);
        assert_eq!(dup.iter().filter(|p| **p == Path::new("/bin/bash")).count(), 1);
    }

    #[test]
    fn the_fallback_always_has_the_system_folders() {
        let p = machine_fallback();
        assert!(p.contains("/usr/bin") && p.contains("/bin"), "{p}");
    }

    #[test]
    fn the_answer_is_cached() {
        let lp = LoginPath::new();
        let first = lp.get();
        assert!(!first.is_empty());
        assert_eq!(lp.get(), first);
    }
}
