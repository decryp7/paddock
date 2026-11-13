//! The user's login-shell `PATH` for a supervised service.
//!
//! launchd and the systemd user manager start a service with a minimal `PATH`, so tools in Homebrew, nvm or a Python venv
//! are not found. The service asks the user's login shell (`$SHELL -l -i -c`, bounded output, timeout) for the `PATH` a
//! terminal has and merges it in front of its own, once, at start. Parsing and merging are pure; running the shell is thin.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::{Limits, Outcome, command, run_bounded};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one attempt to ask the shell may take (an `.zshrc` that hangs must not hang the service).
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest `PATH` accepted from a shell.
pub const MAX_PATH_LEN: usize = 32 * 1024;
const BEGIN: &str = "__PADDOCK_PATH_BEGIN__";
const END: &str = "__PADDOCK_PATH_END__";

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
pub fn shell_candidates(
    shell_var: Option<&str>,
    macos: bool,
    exists: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
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

/// Asks `shell` for its `PATH`: first as a login interactive shell (so `~/.zshrc`, where nvm and friends usually live,
/// runs), then as a plain login shell. `None` if neither answers within `timeout` (each attempt).
pub fn ask_shell(shell: &Path, timeout: Duration) -> Option<String> {
    let script = probe_script();
    for flags in [&["-l", "-i", "-c"][..], &["-l", "-c"][..]] {
        let mut cmd = command(shell);
        cmd.args(flags).arg(&script).env("TERM", "dumb");
        let Ok(out) = run_bounded(cmd, Limits::new(timeout).max_output(64 * 1024)) else {
            continue;
        };
        if !matches!(out.outcome, Outcome::Exited(_)) {
            continue;
        }
        if let Some(p) = parse_probe_output(&out.stdout_text()) {
            return Some(p);
        }
    }
    None
}

/// The login `PATH` of this machine's user, `None` when no shell answers (and always on Windows, where services use
/// the user's environment block).
pub fn login_path() -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let shell_var = std::env::var("SHELL").ok();
    login_path_from(
        shell_var.as_deref(),
        cfg!(target_os = "macos"),
        |p| p.is_file(),
        SHELL_TIMEOUT,
    )
}

/// [`login_path`] with the machine's facts given: `$SHELL`, whether this is macOS, which shell files exist and how long
/// each shell may take.
pub fn login_path_from(
    shell_var: Option<&str>,
    macos: bool,
    exists: impl Fn(&Path) -> bool,
    timeout: Duration,
) -> Option<String> {
    shell_candidates(shell_var, macos, exists)
        .iter()
        .find_map(|s| ask_shell(s, timeout))
}

/// Merges the login shell's `PATH` in front of the inherited one, dropping duplicates and empty entries. `sep` is the
/// target OS's list separator. The caller sets the result as `PATH` once, at start, before its threads read the
/// environment (this crate does not mutate the environment).
pub fn merge_path(login: &str, inherited: &str, sep: char) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for part in login.split(sep).chain(inherited.split(sep)) {
        if !part.is_empty() && !seen.contains(&part) {
            seen.push(part);
        }
    }
    seen.join(&sep.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_taken_from_between_the_markers() {
        let out =
            format!("welcome banner\n{BEGIN}/home/u/.nvm/bin:/opt/homebrew/bin:rel/bin:{END}\nbye");
        assert_eq!(
            parse_probe_output(&out).unwrap(),
            "/home/u/.nvm/bin:/opt/homebrew/bin"
        );
        assert_eq!(parse_probe_output("no markers"), None);
        assert_eq!(
            parse_probe_output(&format!("{BEGIN}/a")),
            None,
            "unterminated"
        );
        assert_eq!(parse_probe_output(&format!("{BEGIN}{END}")), None, "empty");
        assert_eq!(
            parse_probe_output(&format!("{BEGIN}rel:also/rel{END}")),
            None,
            "no absolute entry"
        );
        assert_eq!(
            parse_probe_output(&format!("{BEGIN}/a\u{7}b{END}")),
            None,
            "control characters"
        );
        assert_eq!(
            parse_probe_output(&format!("{BEGIN}{}{END}", "/a:".repeat(MAX_PATH_LEN))),
            None,
            "too long"
        );
    }

    #[test]
    fn the_script_prints_between_the_markers() {
        let s = probe_script();
        assert!(s.contains(BEGIN) && s.contains(END) && s.contains("$PATH"));
    }

    #[test]
    fn paths_merge_without_duplicates_or_empty_entries() {
        assert_eq!(merge_path("/a:/b::/c", "/c:/d:/a", ':'), "/a:/b:/c:/d");
        assert_eq!(merge_path("", "/x", ':'), "/x");
        assert_eq!(merge_path("C:\\a", "C:\\b;C:\\a", ';'), "C:\\a;C:\\b");
    }

    #[cfg(unix)]
    #[test]
    fn shells_are_absolute_existing_and_unique() {
        let exists = |p: &Path| p == Path::new("/bin/zsh") || p == Path::new("/opt/fish");
        assert_eq!(
            shell_candidates(Some("/opt/fish"), true, exists),
            vec![PathBuf::from("/opt/fish"), PathBuf::from("/bin/zsh")]
        );
        assert_eq!(
            shell_candidates(Some("zsh"), true, exists),
            vec![PathBuf::from("/bin/zsh")]
        );
        assert_eq!(
            shell_candidates(Some("/bin/zsh"), true, exists),
            vec![PathBuf::from("/bin/zsh")]
        );
        assert!(shell_candidates(None, false, |_| false).is_empty());
    }

    #[cfg(unix)]
    mod shells {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

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
                ask_shell(&sh, Duration::from_secs(60)).unwrap(),
                "/home/u/.nvm/bin:/opt/homebrew/bin"
            );
        }

        #[test]
        fn an_interactive_failure_falls_back_to_a_plain_login_shell() {
            let d = tempfile::tempdir().unwrap();
            let sh = script(
                d.path(),
                "fakesh",
                &format!(
                    "for a in \"$@\"; do [ \"$a\" = -i ] && exit 1; done; printf '{BEGIN}/plain/bin{END}'"
                ),
            );
            assert_eq!(
                ask_shell(&sh, Duration::from_secs(60)).unwrap(),
                "/plain/bin"
            );
        }

        #[test]
        fn a_shell_that_prints_junk_or_is_missing_gives_none() {
            let d = tempfile::tempdir().unwrap();
            let junk = script(d.path(), "junk", "echo nothing useful");
            assert_eq!(ask_shell(&junk, Duration::from_secs(60)), None);
            assert_eq!(
                ask_shell(&d.path().join("missing"), Duration::from_secs(60)),
                None
            );
        }

        #[test]
        fn the_first_candidate_shell_that_answers_gives_the_login_path() {
            let d = tempfile::tempdir().unwrap();
            let junk = script(d.path(), "junk", "echo nothing useful");
            let good = script(
                d.path(),
                "good",
                &format!("printf '{BEGIN}/login/bin{END}'"),
            );
            let junk_path = junk.to_string_lossy().into_owned();
            let good_path = good.to_string_lossy().into_owned();
            // `$SHELL` is tried first and the usual shells are only used where they exist.
            let only_ours = |p: &Path| p == junk || p == good;
            assert_eq!(
                login_path_from(Some(&junk_path), false, only_ours, Duration::from_secs(60)),
                None
            );
            let both = |p: &Path| p == junk || p == good;
            let shells = shell_candidates(Some(&good_path), false, both);
            assert_eq!(shells, std::slice::from_ref(&good));
            assert_eq!(
                login_path_from(Some(&good_path), false, both, Duration::from_secs(60)).as_deref(),
                Some("/login/bin")
            );
        }

        #[test]
        fn a_hanging_shell_gives_none_in_time_and_leaves_nothing_behind() {
            let d = tempfile::tempdir().unwrap();
            let marker = d.path().file_name().unwrap().to_string_lossy().into_owned();
            // `exec sleep` keeps the marker on a command line the leak check can find; the timeout ends the tree.
            let hang = script(
                d.path(),
                "hang",
                &format!("sleep 300 {marker} & sleep 300 {marker}"),
            );
            // Both attempts end at their timeout (a hanging shell gives no PATH); what counts is the outcome and that the
            // trees are gone, not how long it took.
            assert_eq!(ask_shell(&hang, Duration::from_millis(400)), None);
            let left: Vec<crate::ProcessId> = crate::processes_mentioning(&marker)
                .into_iter()
                .filter(|p| !p.has_ended())
                .collect();
            assert!(left.is_empty(), "left behind: {left:?}");
        }
    }
}
