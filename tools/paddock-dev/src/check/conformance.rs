//! `./dev conformance`: the command line of `paddock-conformance` (`conformance/src/cli.mjs`) on the Rust host, option for option,
//! with the same output and exit codes, so a CI step can switch hosts without changing a line of its annotations: 0 clean, 1
//! findings, 2 usage, 6 a check did not look, 7 a check is broken (ADR 0089). It runs the checks registered in [`super::registry`];
//! the rest still run in the JavaScript host (`node conformance/bin/paddock-conformance.mjs`), which keeps `window-weight`, `sync`,
//! `commit-lint` and `run` as well. The parity tests (`tests/it/check_engine.rs`) hold both hosts to the same findings.

use super::config::Config;
use super::outcome::{self, exit_status};
use super::util::full_checkout;
use super::{Check, RunOptions, registry, report, run_checks, sources, today_utc};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: paddock-conformance --app <name> [--root <dir>] [--only C04,C06] [--json] [--out file] [--summary file]
                           [--strict C30,C36] [--no-graph] [--scope apps/<app>] [--bundle <dir>] [--artifacts <dir>] [--paddock <dir>] [--version vX.Y.Z] [--rules-md <file>]
                           [--preflight vX.Y.Z]
       paddock-conformance list [--json]       # --json: id, scope, mode, needs, file of every check (ADR 0060)
       paddock-conformance commands --json      # the subcommands, for docs that name them (ADR 0060)";

/// The subcommands the JavaScript host has, in one place (`COMMANDS` in `cli.mjs`).
pub const COMMANDS: [&str; 6] = ["list", "commands", "window-weight", "sync", "commit-lint", "run"];

/// The subcommands only the JavaScript host has so far.
const JS_ONLY: [&str; 4] = ["window-weight", "sync", "commit-lint", "run"];

const VALUE: [&str; 18] = [
    "stats",
    "max",
    "app",
    "root",
    "only",
    "out",
    "summary",
    "bundle",
    "artifacts",
    "paddock",
    "version",
    "preflight",
    "api",
    "repo",
    "allow-home",
    "strict",
    "scope",
    "rules-md",
];
const FLAGS: [&str; 6] = ["json", "keep", "write", "blocks-only", "record", "no-graph"];

/// The parsed `--key value` / `--flag` options.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    values: Vec<(String, String)>,
    flags: BTreeSet<String>,
}

impl Options {
    fn value(&self, k: &str) -> Option<&str> {
        self.values.iter().rev().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }

    fn flag(&self, k: &str) -> bool {
        self.flags.contains(k)
    }
}

/// Parses the options; an error is the message of the JavaScript host's `parseArgs`.
pub fn parse_args(argv: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        let Some(k) = a.strip_prefix("--") else {
            return Err(format!("unexpected argument: {a}"));
        };
        if FLAGS.contains(&k) {
            o.flags.insert(k.to_string());
        } else if VALUE.contains(&k) {
            let Some(v) = argv.get(i + 1) else {
                return Err(format!("--{k} needs a value"));
            };
            o.values.push((k.to_string(), v.clone()));
            i += 1;
        } else {
            return Err(format!("unknown option: {a}"));
        }
        i += 1;
    }
    Ok(o)
}

/// Every check the host has as data (`listChecks`): id, title, rule, scope, mode, needs and its source file.
pub fn list_checks(registry: &[Box<dyn Check>]) -> Value {
    let files = sources();
    Value::Array(
        registry
            .iter()
            .map(|c| {
                let file = files.iter().find(|(id, _)| *id == c.id()).map_or("", |(_, f)| *f);
                json!({
                    "id": c.id(), "title": c.title(), "rule": c.rule(), "scope": c.scope().name(),
                    "mode": c.mode().name(), "needs": c.needs().name(), "file": file,
                })
            })
            .collect(),
    )
}

fn abs(root: Option<&str>) -> PathBuf {
    let p = Path::new(root.unwrap_or("."));
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Runs the command line `argv` (without the program name), writing to `out` and `err`; returns the exit status.
pub fn run(argv: &[String], out: &mut dyn Write, err: &mut dyn Write, env: &dyn Fn(&str) -> Option<String>) -> i32 {
    let usage_error = |err: &mut dyn Write, m: &str| {
        let _ = writeln!(err, "paddock-conformance: {m}");
        outcome::USAGE
    };
    let Some(cmd) = argv.first() else {
        let _ = writeln!(err, "{USAGE}");
        return outcome::USAGE;
    };
    if cmd == "--help" || cmd == "-h" {
        let _ = writeln!(err, "{USAGE}");
        return 0;
    }
    if JS_ONLY.contains(&cmd.as_str()) {
        return usage_error(
            err,
            &format!(
                "`{cmd}` is not in the Rust host yet. Fix: run `node conformance/bin/paddock-conformance.mjs {cmd} ...` (the JavaScript host keeps it)."
            ),
        );
    }
    if cmd == "list" || cmd == "commands" {
        // `dev gates check` reads both from the current folder: a sparse checkout is named here, not by a later ENOENT.
        if let Some(p) = full_checkout::problem(&abs(None), env) {
            let _ = writeln!(err, "{p}");
            return outcome::USAGE;
        }
        if cmd == "commands" {
            let _ = writeln!(out, "{}", json!(COMMANDS));
            return 0;
        }
        let reg = registry();
        if argv[1..].iter().any(|a| a == "--json") {
            let _ = writeln!(out, "{}", list_checks(&reg));
        } else {
            for c in &reg {
                let _ = writeln!(out, "{}  rule {}  {}", c.id(), c.rule(), c.title());
            }
        }
        return 0;
    }
    let o = match parse_args(argv) {
        Ok(o) => o,
        Err(m) => return usage_error(err, &m),
    };
    let root = abs(o.value("root"));
    let subject = crate::pre_push::outcome::subject_of(&root, None);
    if let Some(p) = full_checkout::problem(&root, env) {
        let _ = writeln!(err, "{p}");
        return outcome::USAGE;
    }
    let config = match Config::load(&root, o.value("app")) {
        Ok(c) => c,
        Err(m) => return usage_error(err, &m),
    };
    let only = o
        .value("only")
        .map(|s| s.split(',').map(|x| x.trim().to_uppercase()).collect::<Vec<_>>());
    let opts = RunOptions {
        root,
        config,
        scope: o.value("scope").map(str::to_string),
        only,
        files: None,
        today: today_utc(),
        base_ref: None,
    };
    let reg = registry();
    let result = match run_checks(&opts, &reg) {
        Ok(r) => r,
        Err(u) => {
            let known: Vec<&str> = reg.iter().map(|c| c.id()).collect();
            return usage_error(
                err,
                &format!(
                    "unknown check: {} (the Rust host has {}; the rest run in the JavaScript host)",
                    u.0.join(", "),
                    known.join(", ")
                ),
            );
        }
    };
    let md = report::format_markdown(&result);
    let json = report::result_json(&result);
    let pretty = serde_json::to_string_pretty(&json).unwrap_or_default();
    // A report file that cannot be written does not hide the result: the report is printed, the cause and the next step go to
    // standard error as an annotation, and the run ends with its own status (ER-27).
    let mut unwritten = false;
    let mut cannot_write = |err: &mut dyn Write, what: &str, path: &str, e: &std::io::Error| {
        unwritten = true;
        let _ = writeln!(
            err,
            "::error::paddock-conformance: cannot write the {what} {path}: {e}. Fix: create the folder and make it writable, or give another path to --{what}."
        );
    };
    for (key, text) in [("out", format!("{pretty}\n")), ("summary", md.clone())] {
        if let Some(path) = o.value(key)
            && let Err(e) = std::fs::write(path, text)
        {
            cannot_write(err, key, path, &e);
        }
    }
    if let Some(path) = env("GITHUB_STEP_SUMMARY").filter(|p| !p.is_empty()) {
        let appended = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .and_then(|mut f| writeln!(f, "{md}"));
        if let Err(e) = appended {
            cannot_write(err, "summary", &path, &e);
        }
    }
    if o.flag("json") {
        let _ = writeln!(out, "{pretty}");
    } else {
        let _ = writeln!(out, "{}", report::format_text(&result));
    }
    // `--strict C36`: the warnings of these checks fail the run too (a check that is a warning for apps may block in one repo).
    let strict: BTreeSet<String> = o
        .value("strict")
        .unwrap_or("")
        .split(',')
        .map(|x| x.trim().to_uppercase())
        .filter(|x| !x.is_empty())
        .collect();
    let status = exit_status(&result.findings, &strict, &result.checks, &subject);
    if unwritten && status != outcome::CHECK_BROKEN {
        return outcome::IO_ERROR;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_parse_as_the_javascript_host_does() {
        let o = parse_args(&args(&["--app", "x", "--json", "--only", "C39,C53"])).unwrap();
        assert_eq!(
            (o.value("app"), o.value("only"), o.flag("json")),
            (Some("x"), Some("C39,C53"), true)
        );
        assert_eq!(parse_args(&args(&["x"])).unwrap_err(), "unexpected argument: x");
        assert_eq!(parse_args(&args(&["--nope"])).unwrap_err(), "unknown option: --nope");
        assert_eq!(parse_args(&args(&["--app"])).unwrap_err(), "--app needs a value");
        assert_eq!(parse_args(&args(&["--scope", "apps/a"])).unwrap().value("scope"), Some("apps/a"));
    }
}
