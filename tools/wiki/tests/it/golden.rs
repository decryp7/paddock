//! The parity golden, run through the command line. Every folder of `tests/golden/cases/` is a wiki (`wiki/`, or the folder
//! its `args` file names) with the findings expected from it (`expected.txt`: the exit code, the exit code with `--strict`,
//! then the sorted `error`/`warning` lines). The cases without a `rust-only` marker were recorded from the Node lint
//! (scripts/wiki-lint.mjs at c3c6b0a08) run as a real command over the same arguments, and the Rust binary must print the
//! same; `rust-only` cases are checks the Node lint did not have, with hand-checked expectations. The Paddock wiki case is a
//! snapshot of docs/wiki at c3c6b0a08, so its links out of the wiki folder are reported as broken, as they were recorded.
//! To compare another implementation, run it over the same arguments (see `shared_args`) and diff against `expected.txt`.
use std::path::{Path, PathBuf};
use std::time::Duration;

fn golden() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn case_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(golden().join("cases"))
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The arguments every implementation gets (the Rust one also gets the two below it).
fn shared_args(name: &str) -> Vec<String> {
    let args_file = golden().join(format!("cases/{name}/args"));
    let mut args: Vec<String> = match std::fs::read_to_string(&args_file) {
        Ok(text) => text
            .lines()
            .flat_map(|l| l.split(' ').map(str::to_string).collect::<Vec<_>>())
            .collect(),
        Err(_) => vec!["--wiki".into(), format!("cases/{name}/wiki")],
    };
    args.insert(0, "lint".into());
    for a in [
        "--rules",
        "cases/rules.txt",
        "--rules",
        "cases/rules2.txt",
        "--er",
        "cases/er-none.md",
        "--today",
        "2026-10-31",
    ] {
        args.push(a.into());
    }
    args
}

/// The binary's exit code and its `error`/`warning` lines, sorted.
fn run_binary(name: &str, strict: bool) -> (i32, Vec<String>) {
    let mut cmd = paddock_proc::command(env!("CARGO_BIN_EXE_wiki"));
    cmd.current_dir(golden())
        .args(shared_args(name))
        .args(["--config", "cases/config.json", "--cite-by-line", "merge-steward"]);
    if strict {
        cmd.arg("--strict");
    }
    // The golden is the same everywhere: no CI annotations, no local settings, and no log file in the user's folders.
    cmd.env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITEA_ACTIONS")
        .env("WIKI_LOG_DIR", "off");
    let out = paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(Duration::from_secs(60))).unwrap();
    let code = match out.outcome {
        // paddock-cli's "findings" (5) is the Node lint's exit 1.
        paddock_proc::Outcome::Exited(s) => match s.code().unwrap_or(-1) {
            5 => 1,
            other => other,
        },
        other => panic!("{name}: {other:?}"),
    };
    // The one deliberate difference: the Node lint knew only the steward log and said so in the message; the tool takes the
    // log's name from `--cite-by-line`. Which lines and files are flagged is what is compared.
    let generic = "cites merge-steward by clock time; cite `merge-steward:<line>` instead";
    let node = "cites the steward log by clock time; cite `merge-steward.md:<line>` instead";
    let text = format!("{}{}", out.stdout_text(), out.stderr_text());
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.starts_with("error ") || l.starts_with("warning "))
        .map(|l| l.replace(generic, node))
        .flat_map(|l| per_source(&l))
        .collect();
    lines.sort();
    (code, lines)
}

/// A copy finding that names several rules sources (`shared with 2 rules sources: a, b`) as the Node lint printed it: one line
/// per source. Other lines are returned as they are.
fn per_source(line: &str) -> Vec<String> {
    let Some(at) = line.find(" rules sources: ") else {
        return vec![line.to_string()];
    };
    let head_end = line[..at].rfind("shared with ").map_or(0, |i| i + "shared with ".len());
    let tail_start = at + " rules sources: ".len();
    let Some(close) = line[tail_start..].find("): \"") else {
        return vec![line.to_string()];
    };
    let names = &line[tail_start..tail_start + close];
    names
        .split(", ")
        .map(|n| format!("{}{}{}", &line[..head_end], n, &line[tail_start + close..]))
        .collect()
}

fn rust_result(name: &str) -> String {
    let (code, lines) = run_binary(name, false);
    let (strict, _) = run_binary(name, true);
    let mut text = format!("exit {code}\nexit-strict {strict}\n");
    for l in lines {
        text.push_str(&l);
        text.push('\n');
    }
    text
}

#[test]
fn every_case_gives_the_recorded_findings_through_the_command_line() {
    let names = case_names();
    assert!(names.len() >= 17, "the cases are missing: {names:?}");
    let mut wrong = Vec::new();
    for name in &names {
        let expected = std::fs::read_to_string(golden().join(format!("cases/{name}/expected.txt"))).unwrap();
        let got = rust_result(name);
        if got != expected {
            wrong.push(format!("--- {name}\nexpected:\n{expected}got:\n{got}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_cases_cover_every_check() {
    let all: String = case_names()
        .iter()
        .map(|n| std::fs::read_to_string(golden().join(format!("cases/{n}/expected.txt"))).unwrap())
        .collect();
    for check in [
        "broken link",
        "has no heading",
        "not listed in index.md",
        "orphan",
        "verified: YYYY-MM-DD",
        "non-empty `sources:`",
        "in the future",
        "stale:",
        "bullet names no source",
        "log line must be",
        "clock time",
        "copies rule text",
        "missing index.md",
        "missing log.md",
    ] {
        assert!(all.contains(check), "no case expects: {check}");
    }
}
