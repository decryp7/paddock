//! `dev ci rust-pin`: the compiler a release job is about to build with is the one `rust-toolchain.toml` pins. Reads `channel` from
//! the root `rust-toolchain.toml` with the TOML parser, runs `rustc --version` as the job's environment gives it, and fails with one
//! `::error::` line naming both versions when they differ or when the channel is a floating one (`stable`, `beta`, a date). A release
//! is therefore never built with a floating stable, whatever the job's environment says (`RUSTUP_TOOLCHAIN` outranks the file).

use crate::ci_tool::{NETWORK_ENV, ToolCall, error_line};
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Arguments of `dev ci rust-pin`.
#[derive(Args, Debug)]
pub struct RustPinArgs {
    /// The folder with `rust-toolchain.toml` (default: this checkout's root).
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// The `channel` of a `rust-toolchain.toml`'s text.
pub fn channel_of(text: &str) -> Option<String> {
    let t: toml::Table = text.parse().ok()?;
    t.get("toolchain")?.get("channel")?.as_str().map(str::to_string)
}

/// Whether a channel names one release (`1.100.0`), not a moving one.
pub fn is_exact(channel: &str) -> bool {
    let mut parts = channel.split('.');
    let all = (parts.next(), parts.next(), parts.next(), parts.next());
    matches!(all, (Some(a), Some(b), Some(c), None) if [a, b, c].iter().all(|p| !p.is_empty() && p.bytes().all(|x| x.is_ascii_digit())))
}

/// The version of `rustc --version` output (`rustc 1.100.0 (abc 2026-01-01)` gives `1.100.0`).
pub fn rustc_version(out: &str) -> Option<String> {
    let v = out.split_whitespace().nth(1)?;
    Some(v.split('-').next().unwrap_or(v).to_string())
}

/// `Ok(version)` when the compiler is the pinned one, else the whole `::error::` line.
pub fn check(channel: Option<&str>, rustc_out: &str) -> Result<String, String> {
    let Some(channel) = channel else {
        return Err(error_line(
            "rust-toolchain.toml has no [toolchain] channel.",
            "pin the family's one Rust version there; a release is built with it.",
        ));
    };
    if !is_exact(channel) {
        return Err(error_line(
            &format!("rust-toolchain.toml pins the floating channel {channel:?}."),
            "pin an exact version (for example 1.100.0): a release is never built with a moving compiler.",
        ));
    }
    let have = rustc_version(rustc_out).unwrap_or_else(|| format!("unreadable ({:?})", rustc_out.trim()));
    if have == channel {
        return Ok(have);
    }
    Err(error_line(
        &format!("rustc is {have} but rust-toolchain.toml pins {channel}."),
        "the job's environment (RUSTUP_TOOLCHAIN) or its image names another compiler; make setup-rust settle the pinned one before the build.",
    ))
}

/// `dev ci rust-pin`, against the real machine.
pub fn command(a: &RustPinArgs) -> Result<Report, CliError> {
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let root = match &a.root {
        Some(r) => r.clone(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?;
            crate::worktree::git::git_at(&cwd)
                .text(&["rev-parse", "--show-toplevel"])
                .map(|t| PathBuf::from(t.trim()))
                .map_err(|e| CliError::failure(format!("rust-pin: {e}")))?
        }
    };
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|e| CliError::failure(format!("rust-pin: cannot read {}/rust-toolchain.toml: {e}", root.display())))?;
    let ran = ToolCall::new("rustc")
        .args(["--version"])
        .pass(NETWORK_ENV.iter().copied())
        .pass(["RUSTUP_*", "CARGO_HOME", "RUSTC*"])
        .timeout(Duration::from_secs(60))
        .run(&env, &mut std::io::sink())
        .map_err(|e| CliError::failure(format!("rust-pin: cannot run rustc: {e}")))?;
    let version = check(channel_of(&text).as_deref(), &ran.stdout).map_err(|line| {
        eprintln!("{line}");
        CliError::failure("rust-pin failed")
    })?;
    Ok(Report::new(
        json!({ "ok": true, "version": version }),
        format!("rustc {version} is the version rust-toolchain.toml pins\n"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_compiler_passes_and_every_other_is_refused_naming_both() {
        let toml = "[toolchain]\nchannel = \"1.100.0\"\nprofile = \"minimal\"\n";
        let ch = channel_of(toml);
        assert_eq!(ch.as_deref(), Some("1.100.0"));
        assert_eq!(check(ch.as_deref(), "rustc 1.100.0 (abcdef 2026-11-01)\n").unwrap(), "1.100.0");
        let err = check(ch.as_deref(), "rustc 1.101.1 (abcdef 2026-11-10)\n").unwrap_err();
        assert!(
            err.starts_with("::error::") && err.contains("1.101.1") && err.contains("1.100.0") && err.contains("Next step"),
            "{err}"
        );
        assert!(
            check(ch.as_deref(), "garbage").is_err(),
            "an unreadable rustc is a failure, never a pass"
        );
        // A floating channel is refused whatever rustc says (the control: the same output passes with an exact pin).
        for floating in ["stable", "beta", "nightly-2026-11-01", "1.100"] {
            let err = check(Some(floating), "rustc 1.100.0 (x 2026-11-01)").unwrap_err();
            assert!(err.contains("floating channel"), "{floating}: {err}");
        }
        assert!(check(None, "rustc 1.100.0 (x)").is_err());
        assert_eq!(channel_of("[toolchain]\ncomponents = []\n"), None);
        assert_eq!(rustc_version("rustc 1.100.0-nightly (x)"), Some("1.100.0".into()));
    }
}
