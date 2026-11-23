//! `dev audit deny`: the one `cargo deny` step of every workflow (ER-41, user 2026-11-23). Licences, bans and sources always block.
//! Advisories are a time-based condition: they block when the run's change touches the workspace's dependencies, when its base is
//! unknown, and on a tag, a release or a scheduled run; otherwise they only warn, so an advisory published today does not turn an
//! unrelated change red. The decision is [`crate::pre_push::deny::scope_of`], the same one the push gate reads; this file only runs the
//! commands it names.

use super::{Ran, Runner};
use crate::pre_push::deny::{Scope, scope_of};
use crate::pre_push::gates::Change;
use paddock_cli::{CliError, Exit, Report};
use serde_json::json;
use std::path::{Path, PathBuf};

/// One `cargo deny` command and whether its failure blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The program and its arguments.
    pub argv: Vec<String>,
    /// A failure is a `::warning::` only.
    pub warn_only: bool,
}

/// The commands of `scope`: `cargo deny <extra> check <categories>`, then the advisories alone as a warning row when the scope leaves
/// them out of the blocking one. `extra` is what goes before `check` (`--all-features --workspace`).
pub fn rows(scope: &Scope, extra: &[String]) -> Vec<Row> {
    let line = |cats: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = vec!["cargo".into(), "deny".into()];
        v.extend(extra.iter().cloned());
        v.push("check".into());
        v.extend(cats.iter().map(|c| c.to_string()));
        v
    };
    let mut out = vec![Row {
        argv: line(scope.blocking_categories()),
        warn_only: false,
    }];
    if scope.warning_args().is_some() {
        out.push(Row {
            argv: line(&["advisories"]),
            warn_only: true,
        });
    }
    out
}

/// Runs the rows in `dir`: the report text carries one annotation per failure, and the exit is findings when a blocking row failed or
/// a row could not run (not measured never passes, ER-33).
pub fn run_rows(runner: &dyn Runner, dir: &Path, scope: &Scope, rows: &[Row]) -> Report {
    let mut text = format!(
        "cargo deny in {}: {}\n",
        dir.display(),
        match scope {
            Scope::All { reason } => format!("advisories block ({reason})"),
            Scope::WithoutAdvisories { reason } => format!("advisories only warn ({reason})"),
        }
    );
    let mut blocked = false;
    let mut data = Vec::new();
    for r in rows {
        let ran: Ran = runner.run(dir, &r.argv);
        let cmd = r.argv.join(" ");
        let tail = |ran: &Ran| {
            let all = format!("{}\n{}", ran.stderr.trim_end(), ran.stdout.trim_end());
            all.lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        };
        match ran.code {
            Some(0) => text.push_str(&format!("ok: {cmd}\n")),
            Some(_) if r.warn_only => text.push_str(&format!(
                "::warning::{cmd} found an advisory this change does not touch (it blocks a change to the dependencies, a tag, a release and the nightly audit):\n{}\n",
                tail(&ran)
            )),
            Some(c) => {
                blocked = true;
                text.push_str(&format!("::error::{cmd} failed (exit {c}). Next: fix it or record it in deny.toml:\n{}\n", tail(&ran)));
            }
            None => {
                blocked = true;
                text.push_str(&format!("::error::{cmd} could not run ({}). Next: install cargo-deny; not measured is never a pass.\n", ran.stderr.trim()));
            }
        }
        data.push(json!({"command": cmd, "code": ran.code, "warn_only": r.warn_only}));
    }
    let report = Report::new(json!({"rows": data, "blocked": blocked}), text);
    if blocked { report.with_exit(Exit::Findings) } else { report }
}

/// The files the change adds on top of `base`, or `None` when the base cannot be resolved (then the advisories block).
fn changed_files(root: &Path, base: &str) -> Option<Vec<String>> {
    let range = format!("{base}...HEAD");
    crate::plan_repo::git_text(root, &["diff", "--name-only", &range]).map(|t| t.lines().map(str::to_string).collect())
}

/// The workspace folder relative to the repository root, `/` separated.
fn workspace_of(root: &Path, dir: &Path) -> String {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(dir)
        .strip_prefix(canon(root))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// Whether the environment says this run takes every advisory: a tag ref or a scheduled event.
pub fn strict_from_env(get: &dyn Fn(&str) -> Option<String>) -> bool {
    get("GITHUB_REF").is_some_and(|r| r.starts_with("refs/tags/")) || get("GITHUB_EVENT_NAME").as_deref() == Some("schedule")
}

/// `dev audit deny`.
pub fn command(root: Option<PathBuf>, strict: bool, base: Option<String>, extra: Vec<String>) -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(root.as_deref())?;
    let dir = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let workspace = workspace_of(&root, &dir);
    let strict = strict || strict_from_env(&|k| std::env::var(k).ok());
    // The run's real base, from the workflow (the plan's `base`). No base, or one that cannot be resolved, is an unknown base: every
    // advisory blocks (ER-33), never a guess such as origin/main.
    let base = base
        .or_else(|| std::env::var("PADDOCK_BASE_REF").ok())
        .filter(|b| !b.trim().is_empty());
    let files = base.as_deref().and_then(|b| changed_files(&root, b));
    let change = Change {
        base: files.as_ref().and(base),
        files: files.unwrap_or_default(),
        ranges: Vec::new(),
    };
    let scope = scope_of(&change, &workspace, strict);
    Ok(run_rows(&super::Real, &dir, &scope, &rows(&scope, &extra)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Answers by category: a command naming `advisories` (or none: all four) fails when `adv`, one naming `licenses` when `lic`.
    struct Fake {
        adv: bool,
        lic: bool,
        calls: Mutex<Vec<String>>,
    }
    impl Runner for Fake {
        fn run(&self, _dir: &Path, argv: &[String]) -> Ran {
            let line = argv.join(" ");
            self.calls.lock().unwrap().push(line.clone());
            let all = !["advisories", "licenses", "bans", "sources"].iter().any(|c| line.contains(c));
            let fails = (self.adv && (all || line.contains("advisories"))) || (self.lic && (all || line.contains("licenses")));
            Ran {
                code: Some(i32::from(fails)),
                stdout: String::new(),
                stderr: if fails {
                    "error[vulnerability]: RUSTSEC-2026-9999".into()
                } else {
                    String::new()
                },
            }
        }
    }

    fn change(files: &[&str]) -> Change {
        Change {
            files: files.iter().map(|f| f.to_string()).collect(),
            ranges: vec![],
            base: Some("origin/main".into()),
        }
    }

    fn go(adv: bool, lic: bool, files: &[&str], strict: bool) -> (Report, Vec<String>) {
        let scope = scope_of(&change(files), "crates", strict);
        let f = Fake {
            adv,
            lic,
            calls: Mutex::new(Vec::new()),
        };
        let extra: Vec<String> = vec!["--workspace".into()];
        let r = run_rows(&f, Path::new("crates"), &scope, &rows(&scope, &extra));
        (r, f.calls.into_inner().unwrap())
    }

    #[test]
    fn an_advisory_warns_on_a_change_that_leaves_the_dependencies_alone_and_blocks_one_that_touches_them_a_tag_and_an_unknown_base() {
        let (r, calls) = go(true, false, &["crates/a/src/lib.rs"], false);
        assert_eq!(r.exit, Exit::Ok, "{}", r.text);
        assert!(r.text.contains("::warning::") && !r.text.contains("::error::"), "{}", r.text);
        assert_eq!(
            calls,
            [
                "cargo deny --workspace check licenses bans sources",
                "cargo deny --workspace check advisories"
            ]
        );
        // The same advisory blocks when the change touches a lockfile or manifest of the workspace, and on a tag or the nightly.
        for (files, strict) in [(&["crates/a/Cargo.toml"][..], false), (&["crates/a/src/lib.rs"][..], true)] {
            let (r, calls) = go(true, false, files, strict);
            assert_eq!(r.exit, Exit::Findings, "{files:?} {strict}: {}", r.text);
            assert_eq!(calls, ["cargo deny --workspace check"]);
        }
        let mut c = change(&["crates/a/src/lib.rs"]);
        c.base = None;
        assert!(matches!(scope_of(&c, "crates", false), Scope::All { .. }));
        // Licences, bans and sources block on every change, with no advisory.
        let (r, _) = go(false, true, &["crates/a/src/lib.rs"], false);
        assert_eq!(r.exit, Exit::Findings, "{}", r.text);
        // A command that could not run is not a pass.
        struct Down;
        impl Runner for Down {
            fn run(&self, _: &Path, _: &[String]) -> Ran {
                Ran {
                    code: None,
                    stdout: String::new(),
                    stderr: "no cargo-deny".into(),
                }
            }
        }
        let scope = scope_of(&change(&["crates/a/src/lib.rs"]), "crates", false);
        assert_eq!(run_rows(&Down, Path::new("."), &scope, &rows(&scope, &[])).exit, Exit::Findings);
    }

    #[test]
    fn a_tag_or_a_scheduled_event_is_strict_and_a_branch_push_is_not() {
        let env =
            |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert!(strict_from_env(&env(&[("GITHUB_REF", "refs/tags/v1.0.0")])));
        assert!(strict_from_env(&env(&[("GITHUB_EVENT_NAME", "schedule")])));
        assert!(!strict_from_env(&env(&[
            ("GITHUB_REF", "refs/heads/main"),
            ("GITHUB_EVENT_NAME", "push")
        ])));
    }
}
