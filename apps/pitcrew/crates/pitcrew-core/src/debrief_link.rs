//! "Open this job's log in Debrief" (ADR 0018): Debrief is the owner's log-analysis app. Pitcrew hands it a file; the
//! pure parts are here: the program's name (one constant), where an installed Debrief usually lives, and cutting one
//! job's lines out of a runner log by time.
//!
//! The hand-off is the command line: Debrief opens the paths it is given (and a running Debrief takes them over as a
//! second instance). A deep link would need a registered scheme on each OS; the command line works today everywhere.

use crate::logline;
use crate::platform::Os;
use crate::time::{parse_rfc3339, Timestamp};
use std::path::PathBuf;

/// The name of Debrief's command-line program.
pub const DEBRIEF_CLI: &str = "debrief";
/// What the program was called before Debrief was renamed. Found without [`DEBRIEF_CLI`], it means Debrief is too old
/// to take part in "Open in Debrief"; Pitcrew only says so, it never starts it.
pub const LEGACY_DEBRIEF_CLI: &str = "imt";
/// The name of the application in the places apps are installed.
pub const DEBRIEF_APP: &str = "Debrief";
/// Most bytes of a job's log handed over.
pub const MAX_EXTRACT_BYTES: usize = 32 * 1024 * 1024;
/// A line this close to the job (before it started, after it ended) is part of it: the runner logs around a job.
pub const SLACK_MS: u64 = 5_000;

fn exe(name: &str, os: Os) -> String {
    if os == Os::Windows {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The places to look for Debrief's program, best first, after the directories of `PATH` (which the caller searches
/// first). `env` reads an environment variable.
pub fn install_candidates(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    install_candidates_of(DEBRIEF_CLI, os, env)
}

/// [`install_candidates`] for a program of another name (the legacy one).
pub fn install_candidates_of(cli: &str, os: Os, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let program = exe(cli, os);
    let mut out = Vec::new();
    match os {
        Os::Macos => {
            out.push(PathBuf::from(format!("/Applications/{DEBRIEF_APP}.app/Contents/MacOS/{program}")));
            if let Some(h) = env("HOME") {
                out.push(PathBuf::from(&h).join(format!("Applications/{DEBRIEF_APP}.app/Contents/MacOS/{program}")));
                out.push(PathBuf::from(h).join(".cargo/bin").join(&program));
            }
            out.push(PathBuf::from("/usr/local/bin").join(&program));
            out.push(PathBuf::from("/opt/homebrew/bin").join(&program));
        }
        Os::Windows => {
            for var in ["LOCALAPPDATA", "ProgramFiles"] {
                if let Some(base) = env(var) {
                    let base = PathBuf::from(base);
                    out.push(base.join("Programs").join(DEBRIEF_APP).join(&program));
                    out.push(base.join(DEBRIEF_APP).join(&program));
                }
            }
        }
        Os::Linux => {
            if let Some(h) = env("HOME") {
                out.push(PathBuf::from(&h).join(".local/bin").join(&program));
                out.push(PathBuf::from(h).join(".cargo/bin").join(&program));
            }
            out.push(PathBuf::from("/usr/local/bin").join(&program));
            out.push(PathBuf::from("/usr/bin").join(&program));
            out.push(PathBuf::from(format!("/opt/{DEBRIEF_APP}/{program}")));
        }
    }
    out
}

/// The time a runner log line carries (`time="2026-10-22T12:00:00+02:00"`), when it has one.
pub fn line_time(line: &str) -> Option<Timestamp> {
    logline::fields(line)
        .into_iter()
        .find(|(k, _)| k == "time")
        .and_then(|(_, v)| parse_rfc3339(&v))
        .or_else(|| line.split_whitespace().next().and_then(parse_rfc3339))
}

/// The lines of `text` that belong to the job that ran from `from` to `to` (with [`SLACK_MS`] around it), at most
/// [`MAX_EXTRACT_BYTES`]. A line without a time belongs to the line before it. Returns the text and whether any line
/// matched.
pub fn extract_window(text: &str, from: Timestamp, to: Timestamp) -> (String, bool) {
    let (lo, hi) = (from.0.saturating_sub(SLACK_MS), to.0.saturating_add(SLACK_MS));
    let mut out = String::new();
    let mut inside = false;
    let mut any = false;
    for line in text.lines() {
        if let Some(t) = line_time(line) {
            inside = (lo..=hi).contains(&t.0);
        }
        if inside {
            if out.len() + line.len() + 1 > MAX_EXTRACT_BYTES {
                break;
            }
            out.push_str(line);
            out.push('\n');
            any = true;
        }
    }
    (out, any)
}

/// A file name for the hand-off: `job-<run>-<job>.log` made of safe characters only.
pub fn handoff_file_name(runner: &str, label: &str) -> String {
    let clean = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .take(40)
            .collect::<String>()
    };
    format!("{}-{}.log", clean(runner), clean(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        parse_rfc3339(s).unwrap()
    }

    #[test]
    fn candidates_depend_on_the_os_and_use_the_constant() {
        let env = |k: &str| match k {
            "HOME" => Some("/Users/me".to_string()),
            "LOCALAPPDATA" => Some(r"C:\Users\me\AppData\Local".to_string()),
            _ => None,
        };
        let mac = install_candidates(Os::Macos, &env);
        assert_eq!(
            mac[0],
            PathBuf::from(format!("/Applications/Debrief.app/Contents/MacOS/{DEBRIEF_CLI}"))
        );
        assert!(mac.iter().any(|p| p.starts_with("/Users/me/.cargo/bin")));
        let win = install_candidates(Os::Windows, &env);
        assert!(win[0].to_string_lossy().ends_with(&format!("{DEBRIEF_CLI}.exe")));
        let lin = install_candidates(Os::Linux, &|_| None);
        assert!(lin.iter().all(|p| p.file_name().is_some_and(|n| n == DEBRIEF_CLI)));
        assert!(install_candidates(Os::Windows, &|_| None).is_empty());
    }

    #[test]
    fn the_cli_is_debrief_and_the_legacy_name_has_its_own_candidates() {
        assert_eq!(DEBRIEF_CLI, "debrief");
        let env = |k: &str| (k == "HOME").then(|| "/Users/me".to_string());
        let old = install_candidates_of(LEGACY_DEBRIEF_CLI, Os::Macos, &env);
        assert!(old.iter().all(|p| p.file_name().is_some_and(|n| n == "imt")));
        assert_eq!(old.len(), install_candidates(Os::Macos, &env).len());
    }

    #[test]
    fn a_jobs_lines_are_cut_out_by_time() {
        let log = "time=\"2026-10-22T12:00:00+00:00\" level=info msg=\"before\"\n\
                   time=\"2026-10-22T12:10:00+00:00\" level=info msg=\"task started\"\n\
                   a continuation line\n\
                   time=\"2026-10-22T12:12:00+00:00\" level=info msg=\"step\"\n\
                   time=\"2026-10-22T12:15:02+00:00\" level=info msg=\"done\"\n\
                   time=\"2026-10-22T12:30:00+00:00\" level=info msg=\"after\"\n";
        let (text, any) = extract_window(log, ts("2026-10-22T12:10:00Z"), ts("2026-10-22T12:15:00Z"));
        assert!(any);
        assert_eq!(text.lines().count(), 4, "{text}");
        assert!(text.contains("a continuation line") && text.contains("done") && !text.contains("before") && !text.contains("after"));
        let (none, any) = extract_window(log, ts("2025-01-01T00:00:00Z"), ts("2025-01-01T01:00:00Z"));
        assert!(none.is_empty() && !any);
        let (empty, any) = extract_window("", ts("2026-10-22T12:10:00Z"), ts("2026-10-22T12:15:00Z"));
        assert!(empty.is_empty() && !any);
        // Leading RFC 3339 time works too (the supervisor's own lines).
        assert!(line_time("2026-10-22T12:10:00Z supervisor: x").is_some());
        assert!(line_time("no time here").is_none());
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(handoff_file_name("mac mini", "build/run 12"), "mac_mini-build_run_12.log");
        assert!(!handoff_file_name("../..", "..").contains('/'));
    }
}
