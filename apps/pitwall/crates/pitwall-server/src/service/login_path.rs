//! The user's login-shell `PATH` for a supervised server.
//!
//! launchd and the systemd user manager start the server with a minimal `PATH`, so engine packs for external engines
//! (`mlx_lm.server`, a Python venv's tools, Homebrew binaries) are not found. The server asks the user's login shell
//! (`$SHELL -l -i -c`, bounded output, timeout) for the `PATH` a terminal has and merges it in front of its own,
//! once, at start. Adapted from Pitcrew's `login_env`. The parsing is pure; running the shell is thin.

use super::exec::run_with_timeout;
use super::spec::{merge_path, SUPERVISED_ENV};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one attempt to ask the shell may take (an `.zshrc` that hangs must not hang the server).
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest `PATH` accepted from a shell.
pub const MAX_PATH_LEN: usize = 32 * 1024;
const BEGIN: &str = "__PITWALL_PATH_BEGIN__";
const END: &str = "__PITWALL_PATH_END__";

/// The shell script that prints `$PATH` between markers (banners and prompts printed by rc files are ignored).
pub fn probe_script() -> String {
    format!("printf '%s%s%s' '{BEGIN}' \"$PATH\" '{END}'")
}

/// Extracts the `PATH` from a shell's output: bounded, absolute entries only, no control characters.
pub fn parse_probe_output(out: &str) -> Option<String> {
    let start = out.find(BEGIN)? + BEGIN.len();
    let len = out[start..].find(END)?;
    let raw = &out[start..start + len];
    if raw.len() > MAX_PATH_LEN || raw.chars().any(char::is_control) {
        return None;
    }
    let kept: Vec<&str> = raw.split(':').filter(|p| p.starts_with('/')).collect();
    (!kept.is_empty()).then(|| kept.join(":"))
}

/// The shells to try: `$SHELL` if it is an absolute path to a file, then the platform's usual ones.
pub fn shell_candidates(shell_var: Option<&str>, macos: bool, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let usual: &[&str] = if macos {
        &["/bin/zsh", "/bin/bash"]
    } else {
        &["/bin/bash", "/bin/sh"]
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for s in shell_var.into_iter().chain(usual.iter().copied()) {
        let p = PathBuf::from(s);
        if p.is_absolute() && exists(&p) && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Asks `shell` for its `PATH`: first as a login interactive shell (so `~/.zshrc`, where nvm and friends usually
/// live, runs), then as a plain login shell. `None` if neither answers within `timeout`.
pub fn ask_shell(shell: &Path, timeout: Duration) -> Option<String> {
    let script = probe_script();
    for flags in [&["-l", "-i", "-c"][..], &["-l", "-c"][..]] {
        let args: Vec<&str> = flags.iter().copied().chain(std::iter::once(script.as_str())).collect();
        if let Ok(out) = run_with_timeout(shell.as_os_str(), &args, &[("TERM", "dumb")], timeout) {
            if let Some(p) = parse_probe_output(&out.stdout) {
                return Some(p);
            }
        }
    }
    None
}

/// The login `PATH` of this machine's user, `None` when no shell answers.
pub fn login_path() -> Option<String> {
    let shell_var = std::env::var("SHELL").ok();
    shell_candidates(shell_var.as_deref(), cfg!(target_os = "macos"), |p| p.is_file())
        .iter()
        .find_map(|s| ask_shell(s, SHELL_TIMEOUT))
}

/// For a supervised server on macOS and Linux: puts the login shell's `PATH` in front of the inherited one. Done once,
/// at start, before any thread of ours reads the environment. Anything else (a hand-started server already has the
/// terminal's `PATH`; Windows services use the user's environment block) is left alone. Returns whether `PATH` changed.
pub fn apply_to_process() -> bool {
    if cfg!(windows) || std::env::var_os(SUPERVISED_ENV).is_none_or(|v| v.is_empty()) {
        return false;
    }
    let Some(login) = login_path() else { return false };
    let inherited = std::env::var("PATH").unwrap_or_default();
    let merged = merge_path(&login, &inherited, ':');
    if merged == inherited {
        return false;
    }
    std::env::set_var("PATH", merged);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_taken_from_between_the_markers() {
        let out = format!("welcome banner\n{BEGIN}/home/u/.nvm/bin:/opt/homebrew/bin:rel/bin:{END}\nbye");
        assert_eq!(parse_probe_output(&out).unwrap(), "/home/u/.nvm/bin:/opt/homebrew/bin");
        assert_eq!(parse_probe_output("no markers"), None);
        assert_eq!(parse_probe_output(&format!("{BEGIN}/a")), None, "unterminated");
        assert_eq!(parse_probe_output(&format!("{BEGIN}{END}")), None, "empty");
        assert_eq!(parse_probe_output(&format!("{BEGIN}/a\u{7}b{END}")), None, "control characters");
        assert_eq!(
            parse_probe_output(&format!("{BEGIN}{}{END}", "/a:".repeat(MAX_PATH_LEN))),
            None,
            "too long"
        );
    }

    // Unix paths ("/opt/fish") are not absolute under Windows' rules; login shells exist only on Unix.
    #[cfg(unix)]
    #[test]
    fn shells_are_absolute_existing_and_unique() {
        let exists = |p: &Path| p == Path::new("/bin/zsh") || p == Path::new("/opt/fish");
        assert_eq!(
            shell_candidates(Some("/opt/fish"), true, exists),
            vec![PathBuf::from("/opt/fish"), PathBuf::from("/bin/zsh")]
        );
        assert_eq!(shell_candidates(Some("zsh"), true, exists), vec![PathBuf::from("/bin/zsh")]);
        assert_eq!(shell_candidates(Some("/bin/zsh"), true, exists), vec![PathBuf::from("/bin/zsh")]);
        assert!(shell_candidates(None, false, |_| false).is_empty());
    }

    #[cfg(unix)]
    mod shells {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        #[test]
        fn a_login_shells_path_is_taken_and_its_noise_ignored() {
            let d = tempfile::tempdir().unwrap();
            let sh = script(
                d.path(),
                "fakezsh",
                &format!("echo banner; printf '{BEGIN}/home/u/.nvm/bin:/opt/homebrew/bin{END}'"),
            );
            assert_eq!(
                ask_shell(&sh, Duration::from_secs(10)).unwrap(),
                "/home/u/.nvm/bin:/opt/homebrew/bin"
            );
        }

        #[test]
        fn an_interactive_failure_falls_back_to_a_plain_login_shell() {
            let d = tempfile::tempdir().unwrap();
            let sh = script(
                d.path(),
                "fakesh",
                &format!("for a in \"$@\"; do [ \"$a\" = -i ] && exit 1; done; printf '{BEGIN}/plain/bin{END}'"),
            );
            assert_eq!(ask_shell(&sh, Duration::from_secs(10)).unwrap(), "/plain/bin");
        }

        #[test]
        fn a_hanging_silent_or_missing_shell_gives_none_in_time() {
            let d = tempfile::tempdir().unwrap();
            let hang = script(d.path(), "hang", "sleep 30");
            let started = Instant::now();
            assert_eq!(ask_shell(&hang, Duration::from_millis(300)), None);
            assert!(started.elapsed() < Duration::from_secs(10), "the timeout bounds both attempts");
            let silent = script(d.path(), "silent", "echo nothing useful");
            assert_eq!(ask_shell(&silent, Duration::from_secs(10)), None);
            assert_eq!(ask_shell(&d.path().join("missing"), Duration::from_secs(1)), None);
        }
    }
}
