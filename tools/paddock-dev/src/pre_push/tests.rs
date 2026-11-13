//! The push gate with a plan, a list of files and a fake machine: no process starts (ER-14).

use super::exec::{Done, Exec, RunOpts, Verdict, decide, run_all, run_all_with};
use super::gates::{Change, Facts, Gate, Need, Planned, plan_gates};
use super::{accept_not_measured, parse_refs, render};
use paddock_cli::Exit;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn plan() -> serde_json::Value {
    json!({
        "conformance": true,
        "apps": ["dyno", "pitwall"],
        "packages": {
            "crates": ["paddock-proc"],
            "apps/dyno": ["dyno-app"],
            "apps/pitwall": ["pitwall-app"],
            "apps/debrief": ["debrief-core"],
            "apps/debrief/fuzz": ["debrief-fuzz"],
        }
    })
}

fn facts() -> Facts {
    Facts {
        conformance: Some(Ok(["conformance/bin/paddock-conformance.mjs", "--app", "paddock", "--only", "C06"]
            .map(String::from)
            .to_vec())),
        conformance_env: None,
        windows_clippy: BTreeSet::from(["apps/dyno".to_string(), "apps/pitwall".to_string()]),
        image: Some(Ok("registry/paddock-ci:v7".into())),
        linux: true,
        kept_targets: None,
        pitwall_tests: vec!["scripts/ci/a.test.mjs".into()],
        dev_exe: "/x/dev".into(),
        target_dir: Some("/t/".into()),
    }
}

fn change(files: &[&str]) -> Change {
    Change {
        files: files.iter().map(|f| f.to_string()).collect(),
        ranges: vec!["abc..HEAD".into()],
    }
}

fn run_gates(planned: &[Planned]) -> Vec<&Gate> {
    planned
        .iter()
        .filter_map(|p| if let Planned::Run(g) = p { Some(g) } else { None })
        .collect()
}

fn ids(planned: &[Planned]) -> Vec<String> {
    run_gates(planned).iter().map(|g| g.id.clone()).collect()
}

#[test]
fn a_crates_change_reaches_every_gate_of_ci_er_9() {
    let planned = plan_gates(
        &plan(),
        &change(&["crates/paddock-proc/src/lib.rs", "apps/dyno/src/a.rs", "apps/pitwall/src/a.rs"]),
        &facts(),
    );
    let got = ids(&planned);
    for want in [
        "fmt crates",
        "lockfile crates",
        "clippy crates",
        "clippy-windows crates",
        "clippy-linux crates",
        "conformance",
        "commit-lint",
        "ipc-endpoints",
        "fmt apps/dyno",
        "clippy-windows apps/dyno",
        "pitwall-scripts",
        "pitwall-deny",
    ] {
        assert!(got.iter().any(|g| g == want), "{want} is missing from {got:?}");
    }
    assert!(!got.iter().any(|g| g.contains("fuzz")), "a fuzz workspace has its own job: {got:?}");
    let by = |id: &str| run_gates(&planned).into_iter().find(|g| g.id == id).cloned().unwrap();
    // CI's own command lines (ci.yml: crates lint with every feature; apps without).
    assert_eq!(
        by("clippy crates").args.join(" "),
        "clippy --workspace --all-targets --all-features --locked -- -D warnings"
    );
    assert_eq!(
        by("clippy apps/dyno").args.join(" "),
        "clippy --workspace --all-targets --locked -- -D warnings"
    );
    assert_eq!(by("fmt crates").args.join(" "), "fmt --all --check");
    assert_eq!(by("lockfile crates").args.join(" "), "metadata --locked --format-version 1");
    assert_eq!(
        by("clippy-windows crates").args.join(" "),
        "xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets --all-features --locked -- -D warnings"
    );
    assert_eq!(
        by("clippy-windows crates").env,
        [("CARGO_TARGET_DIR".to_string(), "/t/xwin".to_string())]
    );
    assert_eq!(by("clippy-windows crates").need, Need::Tool("cargo-xwin".into()));
    assert!(
        by("clippy-windows crates")
            .next
            .contains("rustup target add x86_64-pc-windows-msvc --toolchain")
    );
    assert_eq!(by("clippy-linux crates").need, Need::Docker);
    assert!(
        by("clippy-linux crates")
            .args
            .join(" ")
            .contains("--image registry/paddock-ci:v7 --platform linux/amd64")
    );
    assert_eq!(by("conformance").args.last().unwrap(), "C06");
    assert_eq!(
        by("commit-lint").args.join(" "),
        "conformance/bin/paddock-conformance.mjs commit-lint --range abc..HEAD"
    );
    assert_eq!(by("ipc-endpoints").program, "/x/dev");
}

#[test]
fn a_dyno_core_change_regenerates_the_wire_fixtures_and_others_skip_it() {
    // Was a lefthook entry beside the pre-push command (fix/test-temp-group-and-wire-fixtures); the hook is one command now.
    let planned = plan_gates(&plan(), &change(&["apps/dyno/crates/dyno-core/src/protocol.rs"]), &facts());
    let g = run_gates(&planned).into_iter().find(|g| g.id == "dyno-wire-fixtures").cloned();
    let g = g.expect("dyno-wire-fixtures is planned for a dyno-core change");
    assert_eq!(
        (g.program.as_str(), g.args.join(" ")),
        ("sh", "apps/dyno/scripts/regen-wire-fixtures.sh".to_string())
    );
    assert!(g.heavy, "it runs cargo test");
    let other = plan_gates(&plan(), &change(&["apps/dyno/src/a.rs"]), &facts());
    assert!(!run_gates(&other).iter().any(|g| g.id == "dyno-wire-fixtures"));
    assert!(
        other
            .iter()
            .any(|p| matches!(p, Planned::Skip { id, .. } if id == "dyno-wire-fixtures")),
        "skipped with a reason, not silently"
    );
}

#[test]
fn what_the_change_does_not_reach_is_skipped_with_a_reason() {
    let docs = json!({"conformance": false, "apps": [], "packages": {}});
    let c = Change {
        files: vec!["docs/x.md".into()],
        ranges: vec![],
    };
    let planned = plan_gates(&docs, &c, &facts());
    assert!(ids(&planned).is_empty(), "{:?}", ids(&planned));
    let skipped: Vec<&String> = planned
        .iter()
        .filter_map(|p| if let Planned::Skip { reason, .. } = p { Some(reason) } else { None })
        .collect();
    assert!(skipped.len() >= 5 && skipped.iter().all(|r| !r.is_empty()), "{skipped:?}");
    // An app-only change: no Linux Clippy of the shared crates, and Windows only where CI lints it.
    let app = plan_gates(&plan(), &change(&["apps/debrief/src/a.rs"]), &facts());
    let got = ids(&app);
    assert!(!got.contains(&"clippy-linux crates".to_string()), "{got:?}");
    assert!(
        !got.contains(&"clippy-windows apps/debrief".to_string()),
        "debrief has no windowsClippy: {got:?}"
    );
}

#[test]
fn a_unreadable_conformance_line_makes_the_check_broken_not_passed() {
    let mut f = facts();
    f.conformance = Some(Err("ci.yml has no line".into()));
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Broken { id, .. } if id == "conformance"))
    );
    let rows = run_all(&planned, &Fake::default(), 2);
    assert_eq!(render("h", &rows).exit, Exit::CheckBroken);
}

#[derive(Default)]
struct Fake {
    no_docker: bool,
    no_tools: Vec<String>,
    fail: Vec<String>,
    unstartable: Vec<String>,
    panics: Vec<String>,
    cut_short: Vec<String>,
    ran: Mutex<Vec<String>>,
}

impl Exec for Fake {
    fn has_tool(&self, p: &str) -> bool {
        !self.no_tools.iter().any(|t| t == p)
    }
    fn docker_up(&self) -> bool {
        !self.no_docker
    }
    fn run(&self, g: &Gate, _left: Duration) -> Result<Done, String> {
        self.ran.lock().unwrap().push(g.id.clone());
        assert!(!self.panics.contains(&g.id), "a runner bug");
        if self.unstartable.contains(&g.id) {
            return Err(format!("{} is not installed on this runner", g.program));
        }
        if self.cut_short.contains(&g.id) {
            return Ok(Done {
                success: false,
                how: "timed out after 900 s".into(),
                tail: String::new(),
                log: Some("/logs/x.log".into()),
            });
        }
        let fail = self.fail.iter().any(|f| f == &g.id);
        Ok(Done {
            success: !fail,
            how: if fail { "exit 1".into() } else { "exit 0".into() },
            tail: if fail { "error: boom".into() } else { String::new() },
            log: None,
        })
    }
}

fn verdicts(rows: &[(String, Verdict)]) -> std::collections::BTreeMap<String, Verdict> {
    rows.iter().cloned().collect()
}

#[test]
fn no_docker_means_not_measured_and_never_a_pass() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let fake = Fake {
        no_docker: true,
        ..Fake::default()
    };
    let rows = run_all(&planned, &fake, 3);
    let v = verdicts(&rows);
    assert!(
        matches!(&v["clippy-linux crates"], Verdict::NotMeasured { reason, next } if reason.contains("Docker") && next.contains("never starts"))
    );
    assert!(
        !fake.ran.lock().unwrap().iter().any(|r| r.starts_with("clippy-linux")),
        "the container was never started"
    );
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::NotMeasured, "only not-measured rows: exit 6");
    assert!(
        report.text.contains("NOT MEASURED  clippy-linux crates") && report.text.contains("do not push"),
        "{}",
        report.text
    );
}

#[test]
fn a_missing_xwin_is_not_measured_with_the_install_line() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let rows = run_all(
        &planned,
        &Fake {
            no_tools: vec!["cargo-xwin".into()],
            ..Fake::default()
        },
        2,
    );
    assert!(
        matches!(&verdicts(&rows)["clippy-windows crates"], Verdict::NotMeasured { next, .. } if next.contains("cargo install cargo-xwin"))
    );
}

#[test]
fn a_failure_is_one_line_with_the_cause_the_next_step_and_the_rerun_command() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let rows = run_all(
        &planned,
        &Fake {
            fail: vec!["fmt crates".into()],
            ..Fake::default()
        },
        2,
    );
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::Findings);
    let line = report
        .text
        .lines()
        .find(|l| l.contains("::error::fmt crates"))
        .unwrap_or_else(|| panic!("{}", report.text));
    assert!(
        line.contains("exit 1")
            && line.contains("Next step: format your own files")
            && line.contains("Re-run alone: cd crates && cargo fmt --all --check"),
        "{line}"
    );
    assert!(report.text.contains("error: boom"), "the tail of the output is shown");
    // A failure beats a not-measured row; a broken check beats both.
    let rows = run_all(
        &planned,
        &Fake {
            fail: vec!["fmt crates".into()],
            no_docker: true,
            ..Fake::default()
        },
        2,
    );
    assert_eq!(render("h", &rows).exit, Exit::Findings);
}

#[test]
fn everything_green_exits_zero_and_cheap_gates_start_first() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let fake = Fake::default();
    let rows = run_all(&planned, &fake, 1);
    assert_eq!(render("h", &rows).exit, Exit::Ok);
    let ran = fake.ran.lock().unwrap().clone();
    let first_clippy = ran.iter().position(|r| r.starts_with("clippy")).unwrap();
    let last_cheap = ran
        .iter()
        .rposition(|r| r.starts_with("fmt") || r == "conformance" || r == "commit-lint")
        .unwrap();
    assert!(
        last_cheap < first_clippy,
        "fmt, conformance and commit-lint before any Clippy: {ran:?}"
    );
}

#[test]
fn a_docker_failure_names_its_cause() {
    struct Oom;
    impl Exec for Oom {
        fn has_tool(&self, _: &str) -> bool {
            true
        }
        fn docker_up(&self) -> bool {
            true
        }
        fn run(&self, _: &Gate, _: Duration) -> Result<Done, String> {
            Ok(Done {
                success: false,
                how: "exit 137".into(),
                tail: String::new(),
                log: None,
            })
        }
    }
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let g = run_gates(&planned).into_iter().find(|g| g.id == "clippy-linux crates").unwrap();
    assert!(matches!(decide(g, &Oom, Duration::from_secs(60)), Verdict::Failed { next, .. } if next.contains("PADDOCK_LINUX_MEMORY")));
}

#[test]
fn only_the_ci_line_of_this_repository_is_read() {
    // Parity on the real tree: the line the gate runs is the one CI runs.
    let ci = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.gitea/workflows/ci.yml")).unwrap();
    let args = super::ci_line::conformance_args(&ci).unwrap();
    assert!(
        args.contains(&"--strict".to_string()) && args.iter().any(|a| a.starts_with("C06,")),
        "{args:?}"
    );
    // The line is in ci.yml as it is, or (the env form) its script on the step and its arguments in PADDOCK_CONFORMANCE_ARGS.
    assert!(
        ci.contains(&args.join(" "))
            || (ci.contains(&format!("node {} $PADDOCK_CONFORMANCE_ARGS", args[0])) && ci.contains(&args[1..].join(" ")))
    );
}

fn crates_gates() -> Vec<Planned> {
    plan_gates(&plan(), &change(&["crates/a.rs"]), &facts())
}

#[test]
fn a_gate_that_cannot_start_is_not_measured_never_passed() {
    let rows = run_all(
        &crates_gates(),
        &Fake {
            unstartable: vec!["fmt crates".into()],
            ..Fake::default()
        },
        2,
    );
    assert!(matches!(&verdicts(&rows)["fmt crates"], Verdict::NotMeasured { reason, .. } if reason.contains("not installed")));
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
}

#[test]
fn a_crashed_runner_and_a_row_that_never_ran_are_not_measured() {
    let fake = Fake {
        panics: vec!["fmt crates".into()],
        ..Fake::default()
    };
    let rows = run_all(&crates_gates(), &fake, 2);
    let v = verdicts(&rows);
    assert!(matches!(&v["fmt crates"], Verdict::NotMeasured { reason, .. } if reason.contains("crashed")));
    assert!(
        matches!(&v["lockfile apps/dyno"], Verdict::Passed { .. }),
        "the other gates still ran"
    );
    // With a budget of nothing nothing starts: every gate that would run is not measured, none passes.
    let fake = Fake::default();
    let rows = run_all_with(
        &crates_gates(),
        &fake,
        &RunOpts {
            workers: 2,
            budget: Duration::ZERO,
            stop: &AtomicBool::new(false),
        },
    );
    assert!(fake.ran.lock().unwrap().is_empty());
    for (id, v) in &rows {
        assert!(!matches!(v, Verdict::Passed { .. } | Verdict::Failed { .. }), "{id}: {v:?}");
    }
    assert!(matches!(&verdicts(&rows)["fmt crates"], Verdict::NotMeasured { reason, .. } if reason.contains("budget")));
    // A signal stops the rest the same way.
    let fake = Fake::default();
    let rows = run_all_with(
        &crates_gates(),
        &fake,
        &RunOpts {
            workers: 1,
            budget: Duration::from_secs(60),
            stop: &AtomicBool::new(true),
        },
    );
    assert!(fake.ran.lock().unwrap().is_empty());
    assert!(matches!(&verdicts(&rows)["fmt crates"], Verdict::NotMeasured { reason, .. } if reason.contains("interrupted")));
}

#[test]
fn a_timeout_is_not_measured_and_does_not_tell_you_to_fix_lints() {
    let rows = run_all(
        &crates_gates(),
        &Fake {
            cut_short: vec!["clippy crates".into()],
            ..Fake::default()
        },
        2,
    );
    let Verdict::NotMeasured { reason, next } = &verdicts(&rows)["clippy crates"] else {
        panic!("{rows:?}")
    };
    assert!(
        reason.contains("timed out")
            && reason.contains("/logs/x.log")
            && next.contains("re-run it alone")
            && !next.contains("fix the lints"),
        "{reason} {next}"
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
}

#[test]
fn the_worst_finding_decides_the_exit_code_7_then_5_then_6() {
    let nm = Verdict::NotMeasured {
        reason: "r".into(),
        next: "n".into(),
    };
    let failed = Verdict::Failed {
        cause: "c".into(),
        tail: String::new(),
        next: "n".into(),
        secs: 1.0,
    };
    let broken = Verdict::Broken {
        cause: "c".into(),
        next: "n".into(),
    };
    let row = |id: &str, v: &Verdict| (id.to_string(), v.clone());
    let all = [row("a", &nm), row("b", &failed), row("c", &broken)];
    assert_eq!(render("h", &all).exit, Exit::CheckBroken);
    assert_eq!(render("h", &all[..2]).exit, Exit::Findings);
    assert_eq!(render("h", &all[..1]).exit, Exit::NotMeasured);
    assert_eq!(render("h", &[row("x", &Verdict::Passed { secs: 0.1 })]).exit, Exit::Ok);
}

#[test]
fn the_cargo_folder_alone_reaches_linux_clippy_but_documentation_does_not() {
    let reached = |files: &[&str]| ids(&plan_gates(&plan(), &change(files), &facts())).contains(&"clippy-linux crates".to_string());
    assert!(reached(&[".cargo/config.toml"]), ".cargo/ is a trigger on its own");
    assert!(reached(&["crates/paddock-proc/src/a.rs"]));
    assert!(
        !reached(&["crates/paddock-proc/README.md", "crates/CHANGELOG.md"]),
        "documentation next to the code cannot change Clippy"
    );
    assert!(!reached(&["apps/dyno/src/a.rs"]));
}

#[test]
fn pitwalls_deny_needs_cargo_deny_and_an_empty_script_list_is_a_broken_check() {
    let planned = crates_gates();
    let deny = run_gates(&planned).into_iter().find(|g| g.id == "pitwall-deny").unwrap();
    assert_eq!(deny.need, Need::Tool("cargo-deny".into()));
    let rows = run_all(
        &planned,
        &Fake {
            no_tools: vec!["cargo-deny".into()],
            ..Fake::default()
        },
        2,
    );
    assert!(matches!(&verdicts(&rows)["pitwall-deny"], Verdict::NotMeasured { reason, .. } if reason.contains("cargo-deny")));
    let mut f = facts();
    f.pitwall_tests.clear();
    let planned = plan_gates(&plan(), &change(&["apps/pitwall/a.rs"]), &f);
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Broken { id, .. } if id == "pitwall-scripts"))
    );
    assert!(!ids(&planned).contains(&"pitwall-scripts".to_string()));
}

#[test]
fn the_table_names_what_ci_runs_that_this_gate_does_not() {
    let report = render("h", &run_all(&crates_gates(), &Fake::default(), 2));
    for what in [
        "hakari",
        "machete",
        "cargo deny outside Pitwall",
        "wiki lint",
        "TypeScript",
        "Linux Clippy of the apps",
        "Linux Clippy of the shared crates",
        "reached only through dependencies",
    ] {
        assert!(report.text.contains(what), "{what} missing:\n{}", report.text);
    }
}

#[test]
fn a_named_not_measured_gate_may_pass_and_the_use_is_logged() {
    let state = tempfile::tempdir().unwrap();
    let mut rows = run_all(
        &crates_gates(),
        &Fake {
            no_docker: true,
            ..Fake::default()
        },
        2,
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    // A name that is not the gate's accepts nothing.
    accept_not_measured(&mut rows, &["clippy-linux".into()], state.path());
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    accept_not_measured(&mut rows, &["clippy-linux crates".into()], state.path());
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::Ok);
    assert!(report.text.contains("ACCEPTED      clippy-linux crates") && report.text.contains("logged"));
    assert!(
        report.text.contains("Pushed with 1 accepted not-measured row(s)") && !report.text.contains("Safe to push"),
        "{}",
        report.text
    );
    let log = std::fs::read_to_string(state.path().join("overrides.log")).unwrap();
    assert!(log.contains("gate=clippy-linux crates") && log.contains("Docker"), "{log}");
}

#[test]
fn the_pushed_commits_are_read_from_gits_pre_push_lines() {
    let zero = "0".repeat(40);
    let input = format!(
        "refs/heads/a aaa111 refs/heads/a {zero}\nrefs/heads/gone {zero} refs/heads/gone bbb222\nrefs/heads/c ccc333 refs/heads/c ddd444\n"
    );
    assert_eq!(parse_refs(&input), ["aaa111", "ccc333"], "a deletion pushes no commit");
    assert!(parse_refs("").is_empty());
}

#[test]
fn several_pushed_ranges_are_linted_each() {
    let c = Change {
        files: vec!["crates/a.rs".into()],
        ranges: vec!["m1..a".into(), "m2..b".into()],
    };
    let got = ids(&plan_gates(&plan(), &c, &facts()));
    assert!(
        got.contains(&"commit-lint m1..a".to_string()) && got.contains(&"commit-lint m2..b".to_string()),
        "{got:?}"
    );
}

#[test]
fn only_workspaces_with_a_changed_file_of_their_own_are_linted_and_the_linux_clippy_is_opt_in() {
    let planned = plan_gates(&plan(), &change(&["crates/paddock-proc/src/lib.rs"]), &facts());
    let got = ids(&planned);
    for want in [
        "fmt crates",
        "clippy crates",
        "clippy-windows crates",
        "lockfile apps/dyno",
        "lockfile apps/pitwall",
    ] {
        assert!(got.contains(&want.to_string()), "{want}: {got:?}");
    }
    for not in [
        "fmt apps/dyno",
        "clippy apps/dyno",
        "clippy-windows apps/dyno",
        "clippy apps/pitwall",
    ] {
        assert!(!got.contains(&not.to_string()), "{not} is a dependent's: CI covers it: {got:?}");
    }
    let skip = planned
        .iter()
        .find_map(|p| {
            if let Planned::Skip { id, reason } = p {
                (id == "clippy dependents").then_some(reason.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert!(skip.contains("apps/dyno") && skip.contains("CI covers"), "{skip}");
    // The longest workspace folder owns a file: a fuzz or sub-workspace file is not its parent's.
    let own = super::gates::own_workspaces(
        &plan(),
        &[
            "apps/debrief/src/a.rs".to_string(),
            "apps/dyno/notes.txt".to_string(),
            "docs/x.md".to_string(),
        ],
    );
    assert_eq!(own.into_iter().collect::<Vec<_>>(), ["apps/debrief", "apps/dyno"]);
    // Off by default: the Linux Clippy is a skipped row that says why, never a pass.
    let mut f = facts();
    f.linux = false;
    let planned = plan_gates(&plan(), &change(&["crates/paddock-proc/src/lib.rs"]), &f);
    assert!(!ids(&planned).contains(&"clippy-linux crates".to_string()));
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Skip { id, reason } if id == "clippy-linux crates" && reason.contains("CI covers it")))
    );
}

#[test]
fn the_override_names_full_row_ids_and_is_announced() {
    let state = tempfile::tempdir().unwrap();
    let mut rows = run_all(
        &crates_gates(),
        &Fake {
            no_docker: true,
            no_tools: vec!["cargo-xwin".into()],
            ..Fake::default()
        },
        2,
    );
    // A first word (`clippy-windows`) or another row's id accepts nothing: only the exact row.
    accept_not_measured(
        &mut rows,
        &["clippy-windows".into(), "clippy-windows apps/dyno".into()],
        state.path(),
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    let ids = super::accepted_ids(
        &["a b".into()],
        &std::collections::HashMap::from([("PADDOCK_ACCEPT_NOT_MEASURED".to_string(), " x , y,,".to_string())]),
    );
    assert_eq!(ids, ["a b", "x", "y"]);
}

/// A pushed commit is checked out on its own and removed after: a dirty main tree cannot change what is checked.
#[test]
fn a_pushed_commit_is_checked_out_in_a_throw_away_worktree_that_is_removed() {
    use crate::plan_repo::git_text;
    let d = tempfile::tempdir().unwrap();
    let main = d.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let git = |args: &[&str]| git_text(&main, args).unwrap_or_else(|| panic!("git {args:?}"));
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    // The test is about which commit is checked out, not line endings: a Windows machine's `core.autocrlf=true` would give CRLF.
    git(&["config", "core.autocrlf", "false"]);
    std::fs::write(main.join("a.txt"), "pushed\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "one"]);
    let sha = git(&["rev-parse", "HEAD"]).trim().to_string();
    std::fs::write(main.join("a.txt"), "dirty\n").unwrap();
    let path;
    {
        let c = super::Detached::new(&main, &d.path().join("wts"), &sha).unwrap();
        path = c.path.clone();
        assert_eq!(
            std::fs::read_to_string(path.join("a.txt")).unwrap(),
            "pushed\n",
            "the commit, not the dirty tree"
        );
    }
    assert!(!path.exists(), "removed after the run");
    assert_eq!(
        std::fs::read_to_string(main.join("a.txt")).unwrap(),
        "dirty\n",
        "the main tree was not touched"
    );
}

#[test]
fn a_kept_build_folder_per_workspace_is_used_when_the_caller_sets_none() {
    let mut f = facts();
    f.target_dir = None;
    f.kept_targets = Some("/work/pre-push/targets/".into());
    let planned = plan_gates(&plan(), &change(&["crates/a.rs", "apps/dyno/src/a.rs"]), &f);
    let env = |id: &str| run_gates(&planned).into_iter().find(|g| g.id == id).unwrap().env.clone();
    let var = |v: &str| vec![("CARGO_TARGET_DIR".to_string(), v.to_string())];
    assert_eq!(env("clippy crates"), var("/work/pre-push/targets/crates"));
    assert_eq!(env("clippy apps/dyno"), var("/work/pre-push/targets/apps-dyno"));
    assert_eq!(env("clippy-windows apps/dyno"), var("/work/pre-push/targets/apps-dyno/xwin"));
    // The caller's own CARGO_TARGET_DIR wins (the host Clippy inherits it; the Windows one gets its xwin folder).
    f.target_dir = Some("/mine".into());
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    let g = run_gates(&planned).into_iter().find(|g| g.id == "clippy crates").unwrap().clone();
    assert!(g.env.is_empty());
}

#[test]
fn worktree_folders_name_their_run_by_pid_and_start_time_and_only_ended_runs_are_swept() {
    use super::{parse_tree_name, stale_trees, tree_name};
    let name = tree_name(4242, 1_700_000_000_123, "0123456789abcdef");
    assert_eq!(name, "pp-4242-1700000000123-0123456789ab");
    assert_eq!(parse_tree_name(&name), Some((4242, 1_700_000_000_123)));
    assert_eq!(parse_tree_name("not-ours"), None);
    let names = vec![name.clone(), tree_name(7, 99, "aaa"), "other".to_string()];
    // A pid that is alive with another start time is a different process: the leftover is stale.
    let alive = |pid: u32, start: u64| pid == 4242 && start == 1_700_000_000_123;
    assert_eq!(stale_trees(&names, &alive), [tree_name(7, 99, "aaa")]);
}

#[test]
fn the_pre_push_folder_is_never_in_the_home_folder() {
    use super::refuse_home;
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let h = home.to_str().unwrap();
    assert!(refuse_home(&home.join("work/pre-push"), Some(h)).is_err());
    assert!(refuse_home(&home, Some(h)).is_err());
    assert!(refuse_home(&d.path().join("disk/pre-push"), Some(h)).is_ok());
    assert!(refuse_home(&d.path().join("anywhere"), None).is_ok());
}

/// A crashed run's locked worktree is found and removed by the next run, a live one is left alone.
#[test]
fn a_leftover_locked_worktree_of_an_ended_run_is_swept_and_a_live_ones_is_kept() {
    use crate::plan_repo::git_text;
    let d = tempfile::tempdir().unwrap();
    let main = d.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let git = |args: &[&str]| git_text(&main, args).unwrap_or_else(|| panic!("git {args:?}"));
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(main.join("a.txt"), "x\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "one"]);
    let sha = git(&["rev-parse", "HEAD"]).trim().to_string();
    let trees = d.path().join("trees");
    let crashed = super::Detached::new(&main, &trees, &sha).unwrap();
    let path = crashed.path.clone();
    std::mem::forget(crashed); // the process died: no Drop ran
    assert!(
        git(&["worktree", "list", "--porcelain"]).contains("locked"),
        "it is locked while its run lives"
    );
    assert_eq!(
        super::sweep_stale(&main, &trees, &|_, _| true),
        0,
        "a run that may be alive keeps its folder"
    );
    assert!(path.exists());
    assert_eq!(super::sweep_stale(&main, &trees, &|_, _| false), 1);
    assert!(!path.exists());
    assert!(!git(&["worktree", "list", "--porcelain"]).contains("pp-"));
}

#[test]
fn extra_pushed_refs_are_not_measured_rows_and_never_passes() {
    use super::exec::Verdict;
    let rows = super::extra_ref_rows(&["aaaaaaaaaaaaaaaa".to_string(), "bbb".to_string()]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "pushed commit aaaaaaaaaaaa");
    assert!(
        rows.iter()
            .all(|(_, v)| matches!(v, Verdict::NotMeasured { next, .. } if next.contains("one at a time")))
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
}

/// The sweep touches only its own `pp-*` entries under its own base: another agent's worktree whose folder is missing (an unmounted
/// drive) keeps its record, and a leftover of ours whose folder is gone is removed.
#[test]
fn the_sweep_never_drops_a_foreign_worktree_whose_folder_is_missing() {
    use crate::plan_repo::git_text;
    let d = tempfile::tempdir().unwrap();
    let main = d.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let git = |args: &[&str]| git_text(&main, args).unwrap_or_else(|| panic!("git {args:?}"));
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(main.join("a.txt"), "x\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "one"]);
    let sha = git(&["rev-parse", "HEAD"]).trim().to_string();
    // Somebody else's worktree, unlocked, on a drive that is not mounted now.
    let foreign = d.path().join("other-drive").join("agent-wt");
    git(&["worktree", "add", "--detach", foreign.to_str().unwrap(), &sha]);
    std::fs::remove_dir_all(&foreign).unwrap();
    // Our own leftover, whose folder is gone too.
    let trees = d.path().join("trees");
    let crashed = super::Detached::new(&main, &trees, &sha).unwrap();
    let ours = crashed.path.clone();
    std::mem::forget(crashed);
    std::fs::remove_dir_all(&ours).unwrap();
    assert_eq!(super::sweep_stale(&main, &trees, &|_, _| false), 1);
    let list = git(&["worktree", "list", "--porcelain"]);
    assert!(list.contains("agent-wt"), "the foreign worktree's record survives:\n{list}");
    assert!(!list.contains("pp-"), "our leftover is removed:\n{list}");
}

/// A symlink, a relative path and `~` cannot hide a folder inside the home folder.
#[cfg(unix)]
#[test]
fn a_symlink_into_home_a_relative_path_and_a_tilde_are_refused() {
    use super::refuse_home;
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    std::fs::create_dir_all(home.join("work")).unwrap();
    let link = d.path().join("elsewhere");
    std::os::unix::fs::symlink(home.join("work"), &link).unwrap();
    let h = home.to_str().unwrap();
    assert!(refuse_home(&link.join("pre-push"), Some(h)).is_err(), "through a symlink");
    assert!(refuse_home(std::path::Path::new("relative/pre-push"), Some(h)).is_err());
    assert!(refuse_home(std::path::Path::new("~/pre-push"), Some(h)).is_err());
    let outside = d.path().join("disk").join("pre-push");
    assert!(refuse_home(&outside, Some(h)).is_ok());
    assert!(outside.exists(), "an allowed folder is created");
    // A refused folder is not created, not even empty.
    let inside = home.join("work/never/pre-push");
    assert!(refuse_home(&inside, Some(h)).is_err());
    assert!(!home.join("work/never").exists(), "nothing was made inside home");
    let through_link = link.join("also-never");
    assert!(refuse_home(&through_link, Some(h)).is_err());
    assert!(!home.join("work/also-never").exists());
}

mod known_commit {
    use super::super::known::{self, Evidence, Lookups};
    use super::super::{PrePushArgs, command_in, node_env_problem};
    use crate::plan_repo::git_text;
    use paddock_cli::Exit;
    use std::path::{Path, PathBuf};

    struct Fake {
        ls: Option<String>,
        ci: Result<u64, String>,
        asked: std::cell::RefCell<Vec<String>>,
    }
    impl Lookups for Fake {
        fn ls_remote(&self, remote: &str) -> Option<String> {
            self.asked.borrow_mut().push(format!("ls-remote {remote}"));
            self.ls.clone()
        }
        fn ci_green(&self, sha: &str) -> Result<u64, String> {
            self.asked.borrow_mut().push(format!("ci {sha}"));
            self.ci.clone()
        }
    }
    fn fake(ls: Option<&str>, ci: Result<u64, &str>) -> Fake {
        Fake {
            ls: ls.map(String::from),
            ci: ci.map_err(String::from),
            asked: Default::default(),
        }
    }

    fn args() -> PrePushArgs {
        PrePushArgs {
            root: None,
            base: None,
            remote: Some("origin".into()),
            only: vec![],
            dry_run: false,
            files: vec![],
            refs_from_stdin: true,
            linux: false,
            accept_not_measured: vec![],
        }
    }

    /// A repository with two commits; main is at the first, origin/train at the second (the 2026-11-13 shape).
    fn repo() -> (tempfile::TempDir, PathBuf, String, String) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("r");
        std::fs::create_dir(&r).unwrap();
        let git = |a: &[&str]| git_text(&r, a).unwrap_or_else(|| panic!("git {a:?}"));
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(r.join("a.txt"), "1\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "one"]);
        let one = git(&["rev-parse", "HEAD"]).trim().to_string();
        git(&["update-ref", "refs/remotes/origin/main", &one]);
        std::fs::write(r.join("a.txt"), "2\n").unwrap();
        git(&["commit", "-q", "-am", "two"]);
        let two = git(&["rev-parse", "HEAD"]).trim().to_string();
        git(&["update-ref", "refs/remotes/origin/train", &two]);
        (d, r, one, two)
    }

    fn state(r: &Path) -> PathBuf {
        r.join(".git/paddock-pre-push")
    }

    fn push_line(sha: &str) -> String {
        format!("refs/heads/x {sha} refs/heads/main {}\n", "0".repeat(40))
    }

    #[test]
    fn a_commit_on_a_remote_branch_without_evidence_is_checked_not_passed() {
        let (_d, r, _one, two) = repo();
        let l = fake(None, Err("no run"));
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&two), &l);
        assert!(found.is_none(), "origin/train holds it, but that is no evidence");
    }

    #[test]
    fn a_pass_record_of_the_exact_sha_and_tree_is_the_evidence() {
        let (_d, r, one, two) = repo();
        let l = fake(None, Err("no run"));
        known::write_record(&r, &state(&r), &two, "origin/main", &["fmt crates".into()], 1_700_000_000).unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&two), &l);
        assert_eq!(
            found,
            Some(vec![Evidence::Record {
                id: format!("{}@1700000000", &two[..12])
            }])
        );
        assert!(trail[0].contains("pass record"), "{trail:?}");
        // The record of another sha is no evidence; a record whose tree changed is none either.
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&one), &l);
        assert!(found.is_none());
        std::fs::copy(
            state(&r).join("records").join(format!("{two}.json")),
            state(&r).join("records").join(format!("{one}.json")),
        )
        .unwrap();
        let (found, _) = known::find(&r, &state(&r), None, &[one], &l);
        assert!(found.is_none(), "a record is read only for its own sha");
        let rec = state(&r).join("records").join(format!("{two}.json"));
        let text = std::fs::read_to_string(&rec).unwrap();
        let tree = text.split("\"tree\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        std::fs::write(&rec, text.replace(&tree, &"0".repeat(40))).unwrap();
        let (found, _) = known::find(&r, &state(&r), None, &[two], &l);
        assert!(found.is_none(), "the tree id no longer matches");
    }

    #[test]
    fn a_complete_green_ci_run_counts_only_when_the_pushed_remote_has_the_sha_as_a_tip() {
        let (_d, r, _one, two) = repo();
        let tip = format!("{two}\trefs/heads/train/x\n");
        let green = fake(Some(&tip), Ok(8053));
        let (found, trail) = known::find(&r, &state(&r), Some("origin"), std::slice::from_ref(&two), &green);
        assert_eq!(found, Some(vec![Evidence::CiRun { run: 8053 }]));
        assert!(trail.iter().any(|t| t.contains("git ls-remote origin (live)")), "{trail:?}");
        for (ls, remote) in [
            (None, Some("origin")),
            (Some("1111\trefs/heads/other\n"), Some("origin")),
            (Some(tip.as_str()), None),
        ] {
            let l = fake(ls, Ok(8053));
            let (found, _) = known::find(&r, &state(&r), remote, std::slice::from_ref(&two), &l);
            assert!(found.is_none(), "{ls:?} {remote:?}");
            assert!(
                !l.asked.borrow().iter().any(|a| a.starts_with("ci ")),
                "no CI lookup without a confirmed tip"
            );
        }
        let red = fake(Some(&tip), Err("red or partial"));
        let (found, trail) = known::find(&r, &state(&r), Some("origin"), &[two], &red);
        assert!(
            found.is_none() && trail.last().unwrap().contains("no complete green CI run"),
            "{trail:?}"
        );
    }

    #[test]
    fn evidence_applies_only_to_the_hooks_push_with_nothing_overridden() {
        let pushed = vec!["a".repeat(40)];
        assert!(known::applicable(&args(), &pushed));
        assert!(!known::applicable(&args(), &[]), "a delete-only push has no commit");
        for tweak in [
            (|a: &mut PrePushArgs| a.base = Some("origin/main".into())) as fn(&mut PrePushArgs),
            |a| a.files = vec!["x".into()],
            |a| a.only = vec!["fmt".into()],
            |a| a.dry_run = true,
            |a| a.refs_from_stdin = false,
        ] {
            let mut a = args();
            tweak(&mut a);
            assert!(!known::applicable(&a, &pushed));
            // Only the no-stdin tweak is a full run (a person's working-tree run may leave a record); every other tweak is partial.
            assert_eq!(
                known::recordable(&a, 0, 0, Exit::Ok),
                !a.refs_from_stdin,
                "no record from a partial run"
            );
        }
        assert!(known::recordable(&args(), 0, 0, Exit::Ok));
        assert!(!known::recordable(&args(), 1, 0, Exit::Ok) && !known::recordable(&args(), 0, 1, Exit::Ok));
        assert!(!known::recordable(&args(), 0, 0, Exit::NotMeasured) && !known::recordable(&args(), 0, 0, Exit::Findings));
    }

    #[test]
    fn the_known_commit_report_is_its_own_verdict_and_says_not_re_checked() {
        let (_d, r, _one, two) = repo();
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5).unwrap();
        let rep = command_in(
            &PrePushArgs {
                root: Some(r.clone()),
                ..args()
            },
            &r,
            &push_line(&two),
            &fake(None, Err("x")),
        )
        .unwrap();
        assert_eq!(rep.exit, Exit::Ok);
        assert_eq!(rep.data["verdict"], "known_commit");
        assert_eq!(rep.data["not_rechecked"], true);
        assert_eq!(rep.data["commits"][0]["sha"], two.as_str());
        assert_eq!(rep.data["commits"][0]["evidence"]["kind"], "record");
        assert_eq!(rep.data["remote"], "origin");
        assert!(rep.data.get("passed").is_none(), "it is not a measured pass: {}", rep.data);
        assert!(rep.text.starts_with("pre-push: not re-checked: known commit"), "{}", rep.text);
        let log = std::fs::read_to_string(state(&r).join("known-commits.log")).unwrap();
        assert!(
            log.contains(&two) && log.contains("remote=origin") && log.contains("pass record"),
            "{log}"
        );
    }

    #[test]
    fn a_delete_only_push_and_an_explicit_base_never_use_evidence() {
        let (_d, r, _one, two) = repo();
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5).unwrap();
        let l = fake(None, Err("x"));
        let delete = format!("(delete) {} refs/heads/old {}\n", "0".repeat(40), two);
        let got = command_in(
            &PrePushArgs {
                root: Some(r.clone()),
                ..args()
            },
            &r,
            &delete,
            &l,
        );
        assert!(got.as_ref().map_or(true, |rep| rep.data["verdict"] != "known_commit"), "{got:?}");
        let a = PrePushArgs {
            root: Some(r.clone()),
            base: Some("origin/main".into()),
            ..args()
        };
        let got = command_in(&a, &r, &push_line(&two), &l);
        assert!(got.as_ref().map_or(true, |rep| rep.data["verdict"] != "known_commit"), "{got:?}");
    }

    fn reads<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |p| files.iter().find(|(n, _)| *n == p).map(|(_, t)| t.to_string())
    }
    const MANIFEST: &str = r#"{"dependencies":{"yaml":"^2.9.1"}}"#;
    const LOCK: &str = r#"{"packages":{"node_modules/yaml":{"version":"2.9.1"}}}"#;

    #[test]
    fn the_node_install_is_stale_when_a_version_differs_missing_or_unreadable_and_ready_when_equal() {
        let ok = [
            ("node_modules/.package-lock.json", LOCK),
            ("package-lock.json", LOCK),
            ("conformance/package.json", MANIFEST),
        ];
        assert_eq!(node_env_problem(false, &reads(&ok)), None);
        let old = r#"{"packages":{"node_modules/yaml":{"version":"2.8.0"}}}"#;
        let stale = [
            ("node_modules/.package-lock.json", old),
            ("package-lock.json", LOCK),
            ("conformance/package.json", MANIFEST),
        ];
        let why = node_env_problem(false, &reads(&stale)).unwrap();
        assert!(why.contains("2.8.0") && why.contains("2.9.1"), "{why}");
        let gone = [
            ("node_modules/.package-lock.json", r#"{"packages":{}}"#),
            ("package-lock.json", LOCK),
            ("conformance/package.json", MANIFEST),
        ];
        assert!(node_env_problem(false, &reads(&gone)).unwrap().contains("missing"));
        assert!(node_env_problem(false, &reads(&[])).unwrap().contains("nothing is installed"));
        assert!(
            node_env_problem(false, &reads(&ok[..2])).is_some(),
            "an unreadable manifest is a problem, not a pass"
        );
        assert!(
            node_env_problem(true, &reads(&ok))
                .unwrap()
                .contains("throw-away worktree has no node_modules")
        );
    }
}

/// A stale npm install made conformance fail with "yaml cannot be loaded", which counted as a finding (2026-11-13).
#[test]
fn a_missing_conformance_install_is_environment_not_ready_never_a_failure_or_pass() {
    let mut f = facts();
    f.conformance_env = Some("node_modules/yaml is missing".into());
    let planned = plan_gates(&plan(), &change(&["conformance/a.mjs"]), &f);
    let Some(Planned::Broken { cause, next, .. }) = planned
        .iter()
        .find(|p| matches!(p, Planned::Broken { id, .. } if id == "conformance"))
    else {
        panic!("conformance must be Broken, not run: {planned:?}");
    };
    assert!(cause.starts_with("environment not ready"), "{cause}");
    assert!(next.contains("npm ci --ignore-scripts -w conformance"), "{next}");
    assert!(!ids(&planned).contains(&"conformance".to_string()), "it does not run");
}

mod base_choice {
    use super::super::known::Lookups;
    use super::super::{PrePushArgs, choose_base, command_in};
    use crate::plan_repo::git_text;
    use std::path::{Path, PathBuf};

    /// `git ls-remote` as the remote would answer it: a fixed text or unreachable.
    struct Remote(Option<String>);
    impl Lookups for Remote {
        fn ls_remote(&self, _: &str) -> Option<String> {
            self.0.clone()
        }
        fn ci_green(&self, _: &str) -> Result<u64, String> {
            Err("no run".into())
        }
    }
    /// The remote's branches as `ls-remote` text, from refs/remotes/origin/* of the fixture (the sha each ref points at).
    fn live(r: &Path) -> Remote {
        let refs = git_text(r, &["for-each-ref", "--format=%(objectname)\t%(refname)", "refs/remotes/origin/"]).unwrap();
        Remote(Some(
            refs.lines()
                .map(|l| l.replacen("refs/remotes/origin/", "refs/heads/", 1) + "\n")
                .collect(),
        ))
    }

    /// main: m1; train (origin/train/t): m1-t1-t2.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("r");
        std::fs::create_dir(&r).unwrap();
        for a in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@t"],
            &["config", "user.name", "t"],
        ] {
            git_text(&r, a).unwrap();
        }
        commit(&r, "m1");
        git_text(&r, &["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        git_text(&r, &["checkout", "-q", "-b", "train"]).unwrap();
        commit(&r, "t1");
        commit(&r, "t2");
        git_text(&r, &["update-ref", "refs/remotes/origin/train/t", "HEAD"]).unwrap();
        (d, r)
    }
    fn commit(r: &Path, f: &str) {
        std::fs::write(r.join(f), f).unwrap();
        git_text(r, &["add", f]).unwrap();
        git_text(r, &["commit", "-q", "-m", f]).unwrap();
    }
    fn sha(r: &Path, rev: &str) -> String {
        git_text(r, &["rev-parse", rev]).unwrap().trim().to_string()
    }

    #[test]
    fn a_branch_made_from_a_train_branch_is_measured_from_the_trains_live_tip() {
        let (_d, r) = repo();
        let train = sha(&r, "origin/train/t");
        commit(&r, "w1");
        let (base, why) = choose_base(&r, Some("origin"), &sha(&r, "HEAD"), &live(&r));
        assert_eq!(base, train, "{why}");
        assert!(why.contains("train/t") && why.contains("(1)") && why.contains("live"), "{why}");
    }

    #[test]
    fn a_branch_made_from_main_is_measured_from_main() {
        let (_d, r) = repo();
        let main = sha(&r, "origin/main");
        git_text(&r, &["checkout", "-q", "-b", "w", "origin/main"]).unwrap();
        commit(&r, "w1");
        let (base, _) = choose_base(&r, Some("origin"), &sha(&r, "HEAD"), &live(&r));
        assert_eq!(base, main);
    }

    #[test]
    fn main_merged_into_a_train_based_branch_is_still_measured_from_the_train() {
        let (_d, r) = repo();
        let train = sha(&r, "origin/train/t");
        git_text(&r, &["checkout", "-q", "main"]).unwrap();
        commit(&r, "m2");
        git_text(&r, &["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        git_text(&r, &["checkout", "-q", "-b", "w", "origin/train/t"]).unwrap();
        commit(&r, "w1");
        git_text(&r, &["merge", "-q", "--no-edit", "origin/main"]).unwrap();
        let (base, why) = choose_base(&r, Some("origin"), &sha(&r, "HEAD"), &live(&r));
        assert_eq!(base, train, "{why}");
    }

    #[test]
    fn an_unreachable_remote_or_no_remote_falls_back_to_origin_main_and_says_so() {
        let (_d, r) = repo();
        commit(&r, "w1");
        let tip = sha(&r, "HEAD");
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(None));
        assert_eq!(base, "origin/main");
        assert!(
            why.contains("could not be read") && why.contains("measuring from origin/main"),
            "{why}"
        );
        let (base, why) = choose_base(&r, None, &tip, &live(&r));
        assert_eq!((base.as_str(), why.contains("no remote was named")), ("origin/main", true));
        // Tips this clone has no object for are skipped.
        let (base, why) = choose_base(
            &r,
            Some("origin"),
            &tip,
            &Remote(Some(format!("{}\trefs/heads/x\n", "1".repeat(40)))),
        );
        assert_eq!(base, "origin/main", "{why}");
    }

    #[test]
    fn every_pushed_commit_already_on_the_remote_is_never_an_empty_pass_it_measures_from_origin_main() {
        let (_d, r) = repo();
        let two = sha(&r, "origin/train/t");
        let (base, why) = choose_base(&r, Some("origin"), &two, &live(&r));
        assert_eq!(base, "origin/main", "{why}");
        assert!(
            why.contains("already on origin's refs/heads/") && why.contains("no evidence"),
            "{why}"
        );
    }

    #[test]
    fn a_push_already_on_the_remote_without_evidence_runs_the_checks_instead_of_nothing_to_check() {
        let (_d, r) = repo();
        let two = sha(&r, "origin/train/t");
        let args = PrePushArgs {
            root: Some(r.clone()),
            base: None,
            remote: Some("origin".into()),
            only: vec![],
            dry_run: true,
            files: vec![],
            refs_from_stdin: true,
            linux: false,
            accept_not_measured: vec![],
        };
        let line = format!("refs/heads/x {two} refs/heads/main {}\n", "0".repeat(40));
        let got = command_in(&args, &r, &line, &live(&r));
        assert!(got.as_ref().map_or(true, |rep| !rep.text.contains("nothing to check")), "{got:?}");
    }
}
