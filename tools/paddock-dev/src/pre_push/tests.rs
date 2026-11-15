//! The push gate with a plan, a list of files and a fake machine: no process starts (ER-14).

use super::budget::Timing;
use super::exec::{Done, Exec, Kind, Limits, QUEUE_MAX, Row, RunOpts, decide, run_all_timed, run_all_with, test_subject};
use super::gates::{Change, Facts, Gate, Need, Planned, plan_gates};
use super::{accept_not_measured, parse_refs, render, render_timed};
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
    assert_eq!(
        (by("conformance").program.as_str(), by("conformance").args.join(" ")),
        ("/x/dev", "ci run conformance paddock".to_string())
    );
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
    /// What every gate that ran reports as its queue and run time (as `heavy`'s timing line would).
    timing: Option<Timing>,
    /// Gates that never got a slot: `how` is heavy's queue-limit text.
    no_slot: Vec<String>,
    ran: Mutex<Vec<String>>,
    /// The limits every gate was started with.
    limits: Mutex<Vec<(String, Limits)>>,
}

impl Exec for Fake {
    fn has_tool(&self, p: &str) -> bool {
        !self.no_tools.iter().any(|t| t == p)
    }
    fn docker_up(&self) -> bool {
        !self.no_docker
    }
    fn run(&self, g: &Gate, limits: Limits) -> Result<Done, String> {
        self.limits.lock().unwrap().push((g.id.clone(), limits));
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
                timing: None,
            });
        }
        if self.no_slot.contains(&g.id) {
            return Ok(Done {
                success: false,
                how: "queue limit: no slot within 30 s".into(),
                tail: String::new(),
                log: Some("/logs/q.log".into()),
                timing: Some(Timing { queued: 31.0, ran: 0.0 }),
            });
        }
        let fail = self.fail.iter().any(|f| f == &g.id);
        Ok(Done {
            success: !fail,
            how: if fail { "exit 1".into() } else { "exit 0".into() },
            tail: if fail { "error: boom".into() } else { String::new() },
            log: None,
            timing: self.timing,
        })
    }
}

fn run_all(planned: &[Planned], exec: &dyn Exec, workers: usize) -> Vec<(String, Row)> {
    super::exec::run_all(planned, exec, workers, &test_subject())
}

/// A row's reason and next step when it is a plain not-measured row.
fn nm(row: &Row) -> Option<(String, String)> {
    (row.kind() == Kind::NotMeasured).then(|| (row.reason(), row.next.clone()))
}

fn passed(row: &Row) -> bool {
    row.kind() == Kind::Passed
}

fn verdicts(rows: &[(String, Row)]) -> std::collections::BTreeMap<String, Row> {
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
    assert!(matches!(nm(&v["clippy-linux crates"]), Some((reason, next)) if reason.contains("Docker") && next.contains("never starts")));
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
    assert!(matches!(nm(&verdicts(&rows)["clippy-windows crates"]), Some((_, next)) if next.contains("cargo install cargo-xwin")));
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
        fn run(&self, _: &Gate, _: Limits) -> Result<Done, String> {
            Ok(Done {
                success: false,
                how: "exit 137".into(),
                tail: String::new(),
                log: None,
                timing: None,
            })
        }
    }
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let g = run_gates(&planned).into_iter().find(|g| g.id == "clippy-linux crates").unwrap();
    assert!({
        let r = decide(g, &Oom, Duration::from_secs(60), &test_subject());
        r.kind() == Kind::Failed && r.next.contains("PADDOCK_LINUX_MEMORY")
    });
}

#[test]
fn the_conformance_gate_runs_the_contracts_list_and_a_broken_contract_is_broken_never_a_pass() {
    // The real tree: the arguments the gate runs are the contract's (the one copy CI's `./dev ci run conformance paddock` runs).
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let args = crate::ci_run::contract::conformance_args_at(&root).unwrap();
    let f = Facts {
        conformance: Some(Ok(args.clone())),
        ..facts()
    };
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    let g = run_gates(&planned).into_iter().find(|g| g.id == "conformance").unwrap();
    assert_eq!(
        (g.program.as_str(), g.args.clone()),
        ("/x/dev", ["ci", "run", "conformance", "paddock"].map(String::from).to_vec()),
        "through the executor, like CI"
    );
    assert!(!args.is_empty());
    // An unreadable contract: the gate is BROKEN (exit 7), it does not silently run nothing.
    let f = Facts {
        conformance: Some(Err("conformance/paddock.ci.json cannot be read".into())),
        ..facts()
    };
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Broken { id, .. } if id == "conformance"))
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
    assert!(matches!(nm(&verdicts(&rows)["fmt crates"]), Some((reason, _)) if reason.contains("not installed")));
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
    assert!(matches!(nm(&v["fmt crates"]), Some((reason, _)) if reason.contains("crashed")));
    assert!(passed(&v["lockfile apps/dyno"]), "the other gates still ran");
    // With a budget of nothing nothing starts: every gate that would run is not measured, none passes.
    let fake = Fake::default();
    let rows = run_all_with(
        &crates_gates(),
        &fake,
        &RunOpts {
            workers: 2,
            budget: Duration::ZERO,
            queue: QUEUE_MAX,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
        },
    );
    assert!(fake.ran.lock().unwrap().is_empty());
    for (id, v) in &rows {
        assert!(!matches!(v.kind(), Kind::Passed | Kind::Failed), "{id}: {v:?}");
    }
    assert!(matches!(nm(&verdicts(&rows)["fmt crates"]), Some((reason, _)) if reason.contains("budget")));
    // A signal stops the rest the same way.
    let fake = Fake::default();
    let rows = run_all_with(
        &crates_gates(),
        &fake,
        &RunOpts {
            workers: 1,
            budget: Duration::from_secs(60),
            queue: QUEUE_MAX,
            stop: &AtomicBool::new(true),
            subject: &test_subject(),
        },
    );
    assert!(fake.ran.lock().unwrap().is_empty());
    assert!(matches!(nm(&verdicts(&rows)["fmt crates"]), Some((reason, _)) if reason.contains("interrupted")));
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
    let Some((reason, next)) = nm(&verdicts(&rows)["clippy crates"]) else {
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

/// A gate that ran and exited 0, as a row.
fn ran_ok(id: &str, secs: f64) -> Row {
    let done = Done {
        success: true,
        how: "exit 0".into(),
        tail: String::new(),
        log: None,
        timing: None,
    };
    Row::from_run(id, &test_subject(), &done, "", secs)
}

/// ADR 0089: the row reads how the run ended; a failed run is a finding, never a pass (delete `done.success` and this goes red).
#[test]
fn a_row_from_a_run_reads_its_exit() {
    let run = |success: bool| Done {
        success,
        how: if success { "exit 0" } else { "exit 1" }.into(),
        tail: String::new(),
        log: None,
        timing: None,
    };
    assert_eq!(
        Row::from_run("g", &test_subject(), &run(true), "g (exit 1)", 1.0).kind(),
        Kind::Passed
    );
    let failed = Row::from_run("g", &test_subject(), &run(false), "g (exit 1)", 1.0);
    assert_eq!(failed.kind(), Kind::Failed);
    assert!(failed.reason().contains("g (exit 1)"));
}

#[test]
fn the_worst_finding_decides_the_exit_code_7_then_5_then_6() {
    let nm = Row::not_measured("r", "n");
    let failed = Row::failed("g", &test_subject(), "c".into(), String::new(), "n".into(), 1.0);
    let broken = Row::broken("c", "n");
    let row = |id: &str, v: &Row| (id.to_string(), v.clone());
    let all = [row("a", &nm), row("b", &failed), row("c", &broken)];
    assert_eq!(render("h", &all).exit, Exit::CheckBroken);
    assert_eq!(render("h", &all[..2]).exit, Exit::Findings);
    assert_eq!(render("h", &all[..1]).exit, Exit::NotMeasured);
    assert_eq!(render("h", &[row("x", &ran_ok("x", 0.1))]).exit, Exit::Ok);
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
    assert!(matches!(nm(&verdicts(&rows)["pitwall-deny"]), Some((reason, _)) if reason.contains("cargo-deny")));
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
    let rows = super::extra_ref_rows(&["aaaaaaaaaaaaaaaa".to_string(), "bbb".to_string()]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "pushed commit aaaaaaaaaaaa");
    assert!(
        rows.iter()
            .all(|(_, v)| matches!(nm(v), Some((_, next)) if next.contains("one at a time")))
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

    /// A pass of the push gate for `sha`, made the only way a pass is made: through `measure`.
    fn pass_for(r: &Path, sha: &str) -> paddock_cli::Outcome {
        let subject = crate::pre_push::outcome::subject_of(r, Some(sha));
        paddock_cli::measure(
            paddock_cli::CheckId::new("pre-push"),
            subject,
            crate::pre_push::exec::tool(),
            paddock_cli::outcome::UnixSeconds(1),
            || Ok(Vec::new()),
        )
    }

    fn push_line(sha: &str) -> String {
        format!("refs/heads/x {sha} refs/heads/main {}\n", "0".repeat(40))
    }

    #[test]
    fn a_commit_on_a_remote_branch_without_evidence_is_checked_not_passed() {
        let (_d, r, _one, two) = repo();
        let l = fake(None, Err("no run"));
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&two), &["origin/main"], &l);
        assert!(found.is_none(), "origin/train holds it, but that is no evidence");
    }

    #[test]
    fn a_pass_record_of_the_exact_sha_and_tree_is_the_evidence() {
        let (_d, r, one, two) = repo();
        let l = fake(None, Err("no run"));
        known::write_record(
            &r,
            &state(&r),
            &two,
            "origin/main",
            &["fmt crates".into()],
            1_700_000_000,
            &pass_for(&r, &two),
        )
        .unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&two), &["origin/main"], &l);
        let found = found.unwrap();
        assert!(
            matches!(&found[..], [Evidence::Record { id, outcome: paddock_cli::Outcome::KnownCommit(_) }] if *id == format!("{}@1700000000", &two[..12])),
            "{found:?}"
        );
        assert!(trail[0].contains("pass record"), "{trail:?}");
        // The record of another sha is no evidence; a record whose tree changed is none either.
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&one), &["origin/main"], &l);
        assert!(found.is_none());
        std::fs::copy(
            state(&r).join("records").join(format!("{two}.json")),
            state(&r).join("records").join(format!("{one}.json")),
        )
        .unwrap();
        let (found, _) = known::find(&r, &state(&r), None, &[one], &["origin/main"], &l);
        assert!(found.is_none(), "a record is read only for its own sha");
        let rec = state(&r).join("records").join(format!("{two}.json"));
        let text = std::fs::read_to_string(&rec).unwrap();
        let tree = text.split("\"tree\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        std::fs::write(&rec, text.replace(&tree, &"0".repeat(40))).unwrap();
        let (found, _) = known::find(&r, &state(&r), None, &[two], &["origin/main"], &l);
        assert!(found.is_none(), "the tree id no longer matches");
    }

    #[test]
    fn a_complete_green_ci_run_counts_only_when_the_pushed_remote_has_the_sha_as_a_tip() {
        let (_d, r, _one, two) = repo();
        let tip = format!("{two}\trefs/heads/train/x\n");
        let green = fake(Some(&tip), Ok(8053));
        let (found, trail) = known::find(&r, &state(&r), Some("origin"), std::slice::from_ref(&two), &["origin/main"], &green);
        assert!(
            matches!(
                found.as_deref(),
                Some([Evidence::CiRun {
                    run: 8053,
                    outcome: paddock_cli::Outcome::KnownCommit(_)
                }])
            ),
            "{found:?}"
        );
        assert!(trail.iter().any(|t| t.contains("git ls-remote origin (live)")), "{trail:?}");
        for (ls, remote) in [
            (None, Some("origin")),
            (Some("1111\trefs/heads/other\n"), Some("origin")),
            (Some(tip.as_str()), None),
        ] {
            let l = fake(ls, Ok(8053));
            let (found, _) = known::find(&r, &state(&r), remote, std::slice::from_ref(&two), &["origin/main"], &l);
            assert!(found.is_none(), "{ls:?} {remote:?}");
            assert!(
                !l.asked.borrow().iter().any(|a| a.starts_with("ci ")),
                "no CI lookup without a confirmed tip"
            );
        }
        let red = fake(Some(&tip), Err("red or partial"));
        let (found, trail) = known::find(&r, &state(&r), Some("origin"), &[two], &["origin/main"], &red);
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
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5, &pass_for(&r, &two)).unwrap();
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

    /// ADR 0089 amendment (rows 3 and 7): the known commit is an outcome that names the commit and tree it stands on, exits 0 and
    /// says `not_rechecked`; a consumer asking about another tree (merge check) refuses it, and a record without evidence is none.
    #[test]
    fn the_known_commit_outcome_names_its_tree_and_a_consumer_about_another_tree_refuses_it() {
        use paddock_cli::outcome::{Sha, TreeId};
        use paddock_cli::{Outcome, Subject};
        let (_d, r, _one, two) = repo();
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5, &pass_for(&r, &two)).unwrap();
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
        let tree = git_text(&r, &["rev-parse", &format!("{two}^{{tree}}")]).unwrap().trim().to_string();
        let ask = Subject::Commit {
            sha: Sha::new(two.as_str()),
            tree: TreeId::new(tree.as_str()),
        };
        let wire = &rep.data["outcome"];
        assert_eq!(wire["status"], "known-commit");
        assert_eq!(wire["not_rechecked"], true);
        assert!(
            matches!(Outcome::from_wire(wire, &ask, true), Outcome::KnownCommit(_)),
            "the pre-push hook accepts it"
        );
        assert!(
            matches!(Outcome::from_wire(wire, &ask, false), Outcome::NotMeasured(_)),
            "merge check does not"
        );
        let other = Subject::Commit {
            sha: Sha::new("0".repeat(40)),
            tree: TreeId::new(tree),
        };
        assert!(
            matches!(Outcome::from_wire(wire, &other, true), Outcome::NotMeasured(_)),
            "another commit"
        );
        // A record written before evidence existed (no `evidence`) proves nothing.
        let rec = state(&r).join("records").join(format!("{two}.json"));
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("evidence");
        std::fs::write(&rec, v.to_string()).unwrap();
        let (found, _) = known::find(
            &r,
            &state(&r),
            None,
            std::slice::from_ref(&two),
            &["origin/main"],
            &fake(None, Err("x")),
        );
        assert!(found.is_none());
    }

    /// Only a `Clean` outcome can leave a record: findings, not measured and an empty run cannot.
    #[test]
    fn only_a_pass_leaves_a_record() {
        let (_d, r, _one, two) = repo();
        for not_a_pass in [
            paddock_cli::Outcome::NotMeasured(paddock_cli::Reason::new("x")),
            paddock_cli::Outcome::NotApplicable(paddock_cli::Reason::new("x")),
        ] {
            assert!(known::write_record(&r, &state(&r), &two, "b", &[], 1, &not_a_pass).is_err());
        }
        assert!(!state(&r).join("records").join(format!("{two}.json")).exists());
    }

    #[test]
    fn a_delete_only_push_and_an_explicit_base_never_use_evidence() {
        let (_d, r, _one, two) = repo();
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5, &pass_for(&r, &two)).unwrap();
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

    /// A record measured from a nearer base (a local train, a branch's old commit) checked less than a push from an older base
    /// needs: it counts only for a push measured from its own base or a newer one, or through the records of the bases between.
    #[test]
    fn a_pass_record_counts_only_for_a_push_measured_from_its_base_or_an_older_one() {
        let (_d, r, one, two) = repo();
        std::fs::write(r.join("a.txt"), "3\n").unwrap();
        git_text(&r, &["commit", "-q", "-am", "three"]).unwrap();
        let three = git_text(&r, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
        let l = fake(None, Err("no run"));
        known::write_record(&r, &state(&r), &three, &two, &[], 5, &pass_for(&r, &three)).unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[one.as_str()], &l);
        assert!(found.is_none(), "{trail:?}");
        assert!(
            trail[0].contains("measured from") && trail[0].contains(&two[..12]) && trail[0].contains("no valid pass record of its own"),
            "{trail:?}"
        );
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[two.as_str()], &l);
        assert!(found.is_some(), "its own base");
        // A base no remote-tracking ref holds (a local commit) takes no record, even a newer one.
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[three.as_str()], &l);
        assert!(found.is_none() && trail[0].contains("on no remote-tracking ref"), "{trail:?}");
        // Through the record of the base in between: two was measured from one, so three's record covers a push from one.
        known::write_record(&r, &state(&r), &two, &one, &[], 5, &pass_for(&r, &two)).unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[one.as_str()], &l);
        assert!(found.is_some(), "{trail:?}");
        // A record that does not name its base commit proves nothing.
        let rec = state(&r).join("records").join(format!("{three}.json"));
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("base_commit");
        std::fs::write(&rec, v.to_string()).unwrap();
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[two.as_str()], &l);
        assert!(found.is_none());
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
    let Some(Planned::NotMeasured { reason, next, .. }) = planned
        .iter()
        .find(|p| matches!(p, Planned::NotMeasured { id, .. } if id == "conformance"))
    else {
        panic!("conformance must be NotMeasured (exit 6, ADR 0089 A2), not run: {planned:?}");
    };
    assert!(reason.starts_with("environment not ready"), "{reason}");
    assert!(next.contains("npm ci --ignore-scripts -w conformance"), "{next}");
    assert!(!ids(&planned).contains(&"conformance".to_string()), "it does not run");
}

mod base_choice {
    use super::super::known::Lookups;
    use super::super::{PrePushArgs, Pushed, command_in};
    use crate::plan_repo::git_text;
    use std::path::{Path, PathBuf};

    /// A new ref `w` at `tip` pushed to `remote`: the shape most tests here need.
    fn choose_base(root: &Path, remote: Option<&str>, tip: &str, look: &dyn Lookups) -> (String, String) {
        let p = Pushed {
            local_ref: "refs/heads/w".into(),
            sha: tip.into(),
            remote_ref: "refs/heads/w".into(),
            remote_sha: "0".repeat(40),
        };
        super::super::choose_base(root, &p, remote, "origin/main", look)
    }

    /// `git ls-remote` as the remote would answer it: a fixed text or unreachable.
    pub struct Remote(pub Option<String>);
    impl Lookups for Remote {
        fn ls_remote(&self, _: &str) -> Option<String> {
            self.0.clone()
        }
        fn ci_green(&self, _: &str) -> Result<u64, String> {
            Err("no run".into())
        }
    }
    /// The remote's branches as `ls-remote` text, from refs/remotes/origin/* of the fixture (the sha each ref points at).
    pub fn live(r: &Path) -> Remote {
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
        assert!(
            why.contains("train/t") && why.contains("fewest new commits, 1") && why.contains("live"),
            "{why}"
        );
    }

    #[test]
    fn a_branch_made_from_main_is_measured_from_main() {
        let (_d, r) = repo();
        let main = sha(&r, "origin/main");
        git_text(&r, &["checkout", "-q", "-b", "w", "origin/main"]).unwrap();
        commit(&r, "w1");
        let (base, _) = choose_base(&r, Some("origin"), &sha(&r, "HEAD"), &live(&r));
        // Main and the train are equally far (one new commit): the train wins the tie, and both give main as the merge base.
        let mb = git_text(&r, &["merge-base", &base, "HEAD"]).unwrap();
        assert_eq!(mb.trim(), main);
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
    fn an_unreachable_remote_or_no_remote_uses_the_remote_tracking_refs_and_says_so() {
        let (_d, r) = repo();
        let train = sha(&r, "origin/train/t");
        commit(&r, "w1");
        let tip = sha(&r, "HEAD");
        let main = sha(&r, "origin/main");
        // Not checked live, a train ref main does not hold may be gone from the remote: only main (or the upstream) counts.
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(None));
        assert_eq!(base, main, "{why}");
        assert!(
            why.contains("could not be read, so the remote-tracking refs are not checked live")
                && why.contains("origin/main (remote-tracking ref, not checked live; main or the upstream holds it)")
                && why.contains("skipped 1 remote-tracking ref(s)"),
            "{why}"
        );
        let (base, why) = choose_base(&r, None, &tip, &live(&r));
        assert_eq!((base.as_str(), why.contains("no remote was named")), (main.as_str(), true), "{why}");
        // The train as the branch's upstream: it counts.
        git_text(&r, &["config", "remote.origin.url", "/nowhere"]).unwrap();
        git_text(&r, &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]).unwrap();
        git_text(&r, &["config", "branch.train.remote", "origin"]).unwrap();
        git_text(&r, &["config", "branch.train.merge", "refs/heads/train/t"]).unwrap();
        let p = Pushed {
            local_ref: "refs/heads/train".into(),
            sha: tip.clone(),
            remote_ref: "refs/heads/w".into(),
            remote_sha: "0".repeat(40),
        };
        let (base, why) = super::super::choose_base(&r, &p, Some("origin"), "origin/main", &Remote(None));
        assert_eq!(base, train, "{why}");
        git_text(&r, &["config", "--unset", "branch.train.merge"]).unwrap();
        // A remote-tracking ref the live list no longer has (deleted or force-pushed) is stale: skipped, said so; then origin/main.
        let (base, why) = choose_base(
            &r,
            Some("origin"),
            &tip,
            &Remote(Some(format!("{}\trefs/heads/x\n", "1".repeat(40)))),
        );
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
        assert!(
            why.contains("skipped 2 remote-tracking ref(s) the remote may no longer have"),
            "{why}"
        );
        // The remote moved a branch on to a commit this clone has: its older remote-tracking ref still counts.
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(Some(format!("{tip}\trefs/heads/train/t\n"))));
        assert_eq!(base, train, "{why}");
        assert!(why.contains("the remote has moved on from it (live)"), "{why}");
        // Nothing usable at all: origin/main, said so.
        git_text(&r, &["update-ref", "-d", "refs/remotes/origin/train/t"]).unwrap();
        git_text(&r, &["update-ref", "-d", "refs/remotes/origin/main"]).unwrap();
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(None));
        assert_eq!(base, "origin/main", "{why}");
        assert!(why.contains("measuring from origin/main"), "{why}");
    }

    /// A branch deleted on the remote keeps its remote-tracking ref until a prune. With the remote unreadable it must not be the
    /// base: its commits may be gone, and measuring from it would leave them unchecked.
    #[test]
    fn a_stale_ref_of_a_branch_deleted_on_the_remote_is_no_base_when_the_remote_cannot_be_read() {
        let (_d, r) = repo();
        commit(&r, "gone1");
        let gone = sha(&r, "HEAD");
        git_text(&r, &["update-ref", "refs/remotes/origin/feat/gone", &gone]).unwrap();
        commit(&r, "w1");
        let tip = sha(&r, "HEAD");
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(None));
        assert_ne!(base, gone, "{why}");
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
        assert!(why.contains("feat/gone"), "the skipped ref is named: {why}");
        // Live, the listing without it says the same.
        let listing = format!("{}\trefs/heads/main\n", sha(&r, "origin/main"));
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(Some(listing)));
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
    }

    #[test]
    fn every_pushed_commit_already_on_the_remote_is_never_an_empty_pass_it_measures_from_the_next_base() {
        let (_d, r) = repo();
        let two = sha(&r, "origin/train/t");
        let (base, why) = choose_base(&r, Some("origin"), &two, &live(&r));
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
        assert!(why.contains("already on origin/train/t") && why.contains("no evidence"), "{why}");
        // A commit already pushed under another branch name keeps the base that branch was measured from (its pass record stays
        // valid), never main.
        commit(&r, "w1");
        let w1 = sha(&r, "HEAD");
        git_text(&r, &["update-ref", "refs/remotes/origin/fix/x", &w1]).unwrap();
        let (base, why) = choose_base(&r, Some("origin"), &w1, &live(&r));
        assert_eq!(base, two, "{why}");
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

/// A scratch repository with a helper to commit and to fake remote-tracking refs (no network).
struct Repo {
    _d: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Repo {
    fn new() -> Repo {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        std::fs::create_dir(&root).unwrap();
        let r = Repo { _d: d, root };
        r.git(&["init", "-q", "-b", "main"]);
        r.git(&["config", "user.email", "t@t"]);
        r.git(&["config", "user.name", "t"]);
        r
    }

    fn git(&self, args: &[&str]) -> String {
        crate::plan_repo::git_text(&self.root, args).unwrap_or_else(|| panic!("git {args:?}"))
    }

    /// A commit that changes `file`, on the current branch; its sha.
    fn commit(&self, file: &str) -> String {
        std::fs::write(self.root.join(file), "x\n").unwrap();
        self.git(&["add", file]);
        self.git(&["commit", "-q", "-m", file]);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    fn remote(&self, name: &str, sha: &str) {
        self.git(&["update-ref", &format!("refs/remotes/origin/{name}"), sha]);
    }

    /// What `git ls-remote origin` would print for the branches faked with [`Repo::remote`].
    fn live(&self) -> base_choice::Remote {
        base_choice::live(&self.root)
    }

    fn choose(&self, p: &super::Pushed) -> (String, String) {
        super::choose_base(&self.root, p, Some("origin"), "origin/main", &self.live())
    }
}

fn new_ref(local: &str, sha: &str) -> super::Pushed {
    let zero = "0".repeat(40);
    super::Pushed {
        local_ref: format!("refs/heads/{local}"),
        sha: sha.into(),
        remote_ref: format!("refs/heads/{local}"),
        remote_sha: zero,
    }
}

#[test]
fn a_new_branch_cut_from_a_train_is_measured_against_the_train_not_main() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.git(&["checkout", "-q", "-b", "train"]);
    r.commit("t1");
    let t2 = r.commit("t2");
    r.remote("main", &c1);
    r.remote("train/2026-11-12-7", &t2);
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t2, "{why}");
    assert!(
        why.contains("train/2026-11-12-7") && why.contains("fewest new commits, 1") && why.contains(" ms"),
        "{why}"
    );
}

#[test]
fn an_existing_remote_ref_is_measured_from_its_old_commit() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    let f2 = r.commit("f2");
    let p = super::Pushed {
        local_ref: "refs/heads/fix".into(),
        sha: f2,
        remote_ref: "refs/heads/fix".into(),
        remote_sha: f1.clone(),
    };
    let (base, why) = r.choose(&p);
    assert_eq!(base, f1);
    assert!(why.contains("on the remote: its old commit"), "{why}");
    // Its old commit is not here: say so and measure from the nearest live remote branch tip (main here).
    let gone = super::Pushed {
        remote_sha: "1".repeat(40),
        ..p
    };
    let (base, why) = r.choose(&gone);
    assert_eq!(base, c1);
    assert!(why.contains("not here (fetch it)"), "{why}");
}

#[test]
fn with_no_usable_remote_branch_the_base_is_main_and_says_so() {
    let r = Repo::new();
    let c1 = r.commit("a");
    // Nothing listed by the remote: origin/main, said so.
    let (base, why) = r.choose(&new_ref("main", &c1));
    assert_eq!(base, "origin/main");
    assert!(
        why.contains("no commit of origin that the pushed commit contains is usable") && why.contains("measuring from origin/main"),
        "{why}"
    );
    // A sibling feature branch is no candidate either.
    r.git(&["checkout", "-q", "--orphan", "other"]);
    let o1 = r.commit("o");
    r.git(&["checkout", "-q", "main"]);
    r.remote("other", &o1);
    let (base, _) = r.choose(&new_ref("main", &c1));
    assert_eq!(base, "origin/main");
}

#[test]
fn a_tracking_upstream_wins_only_a_tie() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "train"]);
    let t1 = r.commit("t1");
    r.remote("train/x", &t1);
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    // Tracking origin/main, but the train is nearer: the nearest wins, the upstream only breaks a tie.
    r.git(&["config", "remote.origin.url", "/nowhere"]);
    r.git(&["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]);
    r.git(&["config", "branch.fix.remote", "origin"]);
    r.git(&["config", "branch.fix.merge", "refs/heads/main"]);
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t1, "an upstream that is main must not beat a nearer train: {why}");
    // At the same distance the tracking upstream wins.
    let r3 = Repo::new();
    let c3 = r3.commit("a");
    r3.remote("main", &c3);
    r3.remote("train/z", &c3);
    r3.git(&["config", "remote.origin.url", "/nowhere"]);
    r3.git(&["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]);
    r3.git(&["checkout", "-q", "-b", "fix"]);
    let f3 = r3.commit("f");
    r3.git(&["config", "branch.fix.remote", "origin"]);
    r3.git(&["config", "branch.fix.merge", "refs/heads/main"]);
    let (base, why) = r3.choose(&new_ref("fix", &f3));
    assert_eq!(
        (
            base.as_str(),
            why.contains("origin/main (") && why.contains("its tracking upstream")
        ),
        (c3.as_str(), true),
        "{why}"
    );
}

#[test]
fn git_pre_push_lines_become_pushed_refs_and_the_not_measured_ids_end_the_output() {
    let zero = "0".repeat(40);
    let input = format!("refs/heads/a aaa refs/heads/a {zero}\nrefs/heads/gone {zero} refs/heads/gone bbb\nbad line\n");
    let got = super::parse_push(&input);
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].sha.as_str(), got[0].remote_ref.as_str()), ("aaa", "refs/heads/a"));
    let nm = |id: &str| (id.to_string(), Row::not_measured("r", "n"));
    let report = render(
        "h",
        &[
            nm("clippy-windows apps/dyno"),
            ("fmt crates".into(), ran_ok("fmt crates", 1.0)),
            nm("clippy crates"),
        ],
    );
    let last = report.text.lines().last().unwrap();
    assert_eq!(last, "NOT MEASURED (2): clippy-windows apps/dyno; clippy crates");
}

#[test]
fn a_branch_that_merged_its_train_in_is_measured_from_the_train_not_its_own_old_tip() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    // The train moves on after the branch was pushed; the branch then merges it in and adds a commit.
    r.git(&["checkout", "-q", "-b", "train", &c1]);
    r.commit("t1");
    let t2 = r.commit("t2");
    r.remote("main", &c1);
    r.remote("train/9", &t2);
    r.git(&["checkout", "-q", "fix"]);
    r.git(&["merge", "-q", "--no-edit", "train"]);
    let m = r.commit("f2");
    let p = super::Pushed {
        local_ref: "refs/heads/fix".into(),
        sha: m,
        remote_ref: "refs/heads/fix".into(),
        remote_sha: f1,
    };
    let (base, why) = r.choose(&p);
    assert_eq!(base, t2, "the old tip would count t1 and t2 as new: {why}");
}

#[test]
fn queue_time_and_run_time_are_reported_apart_and_the_pass_counts_only_the_run() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let fake = Fake {
        timing: Some(Timing { queued: 12.0, ran: 3.0 }),
        ..Fake::default()
    };
    let (rows, timings) = run_all_timed(
        &planned,
        &fake,
        &RunOpts {
            workers: 2,
            budget: Duration::from_secs(900),
            queue: QUEUE_MAX,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
        },
    );
    assert!(
        rows.iter()
            .all(|(_, v)| v.kind() == Kind::Skipped || (v.kind() == Kind::Passed && (v.secs - 3.0).abs() < 1e-9)),
        "a pass shows the run time, not the wall time: {rows:?}"
    );
    assert_eq!(timings["fmt crates"], Timing { queued: 12.0, ran: 3.0 });
    let report = render_timed("h", &rows, &timings);
    assert!(
        report.text.contains("ok            fmt crates") && report.text.contains("3.0 s run, 12.0 s queued"),
        "{}",
        report.text
    );
    let gate = report.data["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "fmt crates")
        .unwrap()
        .clone();
    assert_eq!((gate["run_secs"].as_f64(), gate["queued_secs"].as_f64()), (Some(3.0), Some(12.0)));
}

#[test]
fn a_step_that_never_got_a_slot_is_not_measured_with_its_queue_time_never_passed() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let fake = Fake {
        no_slot: vec!["clippy crates".into()],
        ..Fake::default()
    };
    let (rows, timings) = run_all_timed(
        &planned,
        &fake,
        &RunOpts {
            workers: 2,
            budget: Duration::from_secs(900),
            queue: QUEUE_MAX,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
        },
    );
    let v = verdicts(&rows);
    let Some((reason, next)) = nm(&v["clippy crates"]) else {
        panic!("{:?}", v["clippy crates"])
    };
    assert!(reason.contains("never got a heavy slot") && reason.contains("31.0 s"), "{reason}");
    assert!(next.contains("heavy status") && next.contains("PADDOCK_QUEUE_MAX"), "{next}");
    let report = render_timed("h", &rows, &timings);
    assert_eq!(report.exit, Exit::NotMeasured);
    assert!(report.text.contains("NOT MEASURED  clippy crates"), "{}", report.text);
}

/// ADR 0089 amendment A2: an environment that is not ready is not measured (exit 6), not a broken check (exit 7).
#[test]
fn an_environment_not_ready_ends_the_run_with_exit_6_not_7() {
    let mut f = facts();
    f.conformance_env = Some("node_modules/yaml is missing".into());
    let planned = plan_gates(&plan(), &change(&["conformance/a.mjs"]), &f);
    let rows = run_all(
        &planned.into_iter().filter(|p| !matches!(p, Planned::Run(_))).collect::<Vec<_>>(),
        &Fake::default(),
        1,
    );
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::NotMeasured, "{}", report.text);
    assert!(
        report.text.contains("NOT MEASURED") && report.text.contains("not a finding in your change"),
        "{}",
        report.text
    );
}

/// A heavy step that waits `queued` for other agents (it moves the fake clock the budget is read on by that much, as the wait
/// would) and then runs at once; it records how many heavy steps of the push were in flight together and the limits each started
/// with.
struct Queued {
    queued: Duration,
    start: std::time::Instant,
    /// Microseconds the fake clock has moved.
    moved: std::sync::atomic::AtomicU64,
    in_flight: std::sync::atomic::AtomicUsize,
    most: std::sync::atomic::AtomicUsize,
    limits: Mutex<Vec<(String, Limits)>>,
    /// The steps in flight, and for each step the ones that were in flight when it entered.
    running: Mutex<Vec<String>>,
    beside: Mutex<Vec<(String, Vec<String>)>>,
    /// Steps that have entered `run`, and a signal for each entry.
    entered: Mutex<usize>,
    entry: std::sync::Condvar,
}

impl Queued {
    fn new(queued: Duration) -> Queued {
        Queued {
            queued,
            start: std::time::Instant::now(),
            moved: Default::default(),
            in_flight: Default::default(),
            most: Default::default(),
            limits: Mutex::new(Vec::new()),
            running: Mutex::new(Vec::new()),
            beside: Mutex::new(Vec::new()),
            entered: Mutex::new(0),
            entry: std::sync::Condvar::new(),
        }
    }
}

impl Exec for Queued {
    fn has_tool(&self, _: &str) -> bool {
        true
    }
    fn docker_up(&self) -> bool {
        true
    }
    fn load(&self) -> Option<f64> {
        Some(28.0)
    }
    fn now(&self) -> std::time::Instant {
        self.start + Duration::from_micros(self.moved.load(std::sync::atomic::Ordering::SeqCst))
    }
    fn run(&self, g: &Gate, limits: Limits) -> Result<Done, String> {
        use std::sync::atomic::Ordering::SeqCst;
        self.limits.lock().unwrap().push((g.id.clone(), limits));
        {
            let mut running = self.running.lock().unwrap();
            self.beside.lock().unwrap().push((g.id.clone(), running.clone()));
            running.push(g.id.clone());
        }
        let now = self.in_flight.fetch_add(1, SeqCst) + 1;
        self.most.fetch_max(now, SeqCst);
        // Wait for a second step to enter while this one is in flight: it does at once unless the push holds it back. The
        // bound only ends the wait when it is held back (the expected case), it settles nothing.
        let mut n = self.entered.lock().unwrap();
        *n += 1;
        self.entry.notify_all();
        let mine = *n;
        let _held = self
            .entry
            .wait_timeout_while(n, Duration::from_millis(200), |n| *n == mine)
            .unwrap();
        drop(_held);
        self.moved.fetch_add(u64::try_from(self.queued.as_micros()).unwrap(), SeqCst);
        self.running.lock().unwrap().retain(|id| *id != g.id);
        self.in_flight.fetch_sub(1, SeqCst);
        Ok(Done {
            success: true,
            how: "exit 0".into(),
            tail: String::new(),
            log: None,
            timing: Some(Timing {
                queued: self.queued.as_secs_f64(),
                ran: 0.0,
            }),
        })
    }
}

fn heavy_gate(id: &str, rank: u8) -> Planned {
    Planned::Run(Gate {
        id: id.into(),
        cwd: String::new(),
        program: "true".into(),
        args: vec![],
        env: Vec::new(),
        heavy: true,
        need: Need::Nothing,
        rank,
        next: "fix it".into(),
    })
}

/// 2026-11-14/15: rows of a push at load 28 were "not measured" because time spent waiting for the heavy slot was charged to the
/// 15-minute run budget. The run budget counts run time only: steps that queued for others three times the whole budget still get
/// all of it, and the time queued is reported apart.
#[test]
fn a_long_queue_for_other_agents_does_not_eat_the_run_budget() {
    let budget = Duration::from_millis(100);
    let fake = Queued::new(Duration::from_millis(150));
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 2), heavy_gate("clippy c", 3)];
    let (rows, timings) = run_all_timed(
        &planned,
        &fake,
        &RunOpts {
            workers: 3,
            budget,
            queue: Duration::from_secs(60),
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
        },
    );
    assert!(rows.iter().all(|(_, v)| v.kind() == Kind::Passed), "{rows:?}");
    for (id, l) in fake.limits.lock().unwrap().iter() {
        assert!(
            l.run > budget / 2,
            "{id} started with {:?} of run time: the queue ate the budget",
            l.run
        );
    }
    assert_eq!(timings.len(), 3, "every row's queue time is reported: {timings:?}");
}

/// Heavy has one machine-wide slot: two heavy steps of one push in flight together only queue behind each other, and that wait
/// would be credited as waiting for others (the budget would grow with the number of workers).
#[test]
fn a_push_has_one_heavy_step_in_flight_at_a_time() {
    let fake = Queued::new(Duration::from_millis(30));
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 1), heavy_gate("clippy c", 1)];
    let rows = run_all_with(
        &planned,
        &fake,
        &RunOpts {
            workers: 3,
            budget: Duration::from_secs(60),
            queue: Duration::from_secs(60),
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
        },
    );
    assert!(rows.iter().all(|(_, v)| v.kind() == Kind::Passed), "{rows:?}");
    assert_eq!(fake.most.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// The queue has its own bound, for the whole push: each step may wait only what is left of it, and a step that would start after
/// it is spent is not measured with the time queued and the load, and says what to do next.
#[test]
fn the_queue_allowance_bounds_the_whole_push_and_a_row_it_stops_says_queued_and_load() {
    let fake = Queued::new(Duration::from_millis(60));
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 2), heavy_gate("clippy c", 3)];
    let allowance = Duration::from_millis(100);
    let rows = run_all_with(
        &planned,
        &fake,
        &RunOpts {
            workers: 1,
            budget: Duration::from_secs(60),
            queue: allowance,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
        },
    );
    let v = verdicts(&rows);
    assert!(passed(&v["clippy a"]) && passed(&v["clippy b"]), "{rows:?}");
    let limits = fake.limits.lock().unwrap().clone();
    assert_eq!(limits[0].1.queue, allowance);
    assert!(
        limits[1].1.queue <= allowance - Duration::from_millis(60),
        "the second step waits only what is left: {limits:?}"
    );
    let Some((reason, next)) = nm(&v["clippy c"]) else {
        panic!("{:?}", v["clippy c"])
    };
    assert!(
        reason.starts_with("never started: the push's queue allowance") && reason.contains("queued 0.1 s in all, load 28.0"),
        "{reason}"
    );
    assert!(next.contains("heavy status") && next.contains("--only clippy"), "{next}");
}

/// The 2026-11-14/15 shape: train 10 is being built locally (partly pushed or not at all), a fix branch is stacked on it. The push
/// sends train 10's unpushed commits too, so the check covers them: the base is the nearest commit of the remote (origin's train 9,
/// or train 10 as far as it is pushed), never the local train, whatever a local ref is called (review of c4e32e462, P1-1).
#[test]
fn a_branch_stacked_on_a_local_train_checks_the_trains_unpushed_commits_too() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "train/2026-11-13-9"]);
    let t1 = r.commit("t1");
    r.remote("train/2026-11-13-9", &t1);
    r.git(&["checkout", "-q", "-b", "train/2026-11-14-10"]);
    let t2 = r.commit("t2");
    r.commit("t3");
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t1, "{why}");
    assert!(
        why.contains("origin/train/2026-11-13-9") && why.contains("fewest new commits, 3"),
        "{why}"
    );
    let (change, _, _) = super::discover(&r.root, &base, &[], std::slice::from_ref(&f1)).unwrap();
    assert_eq!(
        change.files,
        ["f1", "t2", "t3"].map(String::from).to_vec(),
        "the train's unpushed commits are checked"
    );
    // Partly pushed (origin has train 10 at t2): measured from there, t3 still checked.
    r.remote("train/2026-11-14-10", &t2);
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t2, "{why}");
    let (change, _, _) = super::discover(&r.root, &base, &[], std::slice::from_ref(&f1)).unwrap();
    assert_eq!(change.files, ["f1", "t3"].map(String::from).to_vec());
}

/// A branch stacked on another pushed branch is measured from that branch (a remote-tracking ref it contains), not from main.
#[test]
fn a_branch_stacked_on_a_pushed_sibling_is_measured_from_the_sibling() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "feat/a"]);
    r.commit("a1");
    let a2 = r.commit("a2");
    r.remote("feat/a", &a2);
    r.git(&["checkout", "-q", "-b", "feat/b"]);
    let b1 = r.commit("b1");
    let (base, why) = r.choose(&new_ref("feat/b", &b1));
    assert_eq!(base, a2, "{why}");
    assert!(why.contains("origin/feat/a (remote-tracking ref"), "{why}");
}

/// A person's run ahead of a push plans what the push will: HEAD's nearest commit of the remote, not always origin/main.
#[test]
fn a_persons_run_is_measured_from_the_nearest_base_like_its_push() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "train/2026-11-14-10"]);
    let t1 = r.commit("t1");
    r.remote("train/2026-11-14-10", &t1);
    r.git(&["checkout", "-q", "-b", "fix"]);
    r.commit("f1");
    let (base, why) = super::person_base(&r.root, None, "origin/main", &r.live());
    assert_eq!(base, t1, "{why}");
    assert!(
        why.starts_with("a person's run: HEAD of branch fix") && why.contains("origin/train/2026-11-14-10"),
        "{why}"
    );
    // Its own pushed copy at HEAD is the base (the uncommitted changes are what is checked), not a reason to measure from main.
    let f1 = r.git(&["rev-parse", "HEAD"]).trim().to_string();
    r.remote("fix", &f1);
    assert_eq!(super::person_base(&r.root, None, "origin/main", &r.live()).0, f1);
}

/// While a heavy step is in flight, a free worker runs the next light row instead of waiting on a heavy one (review P2-4).
#[test]
fn a_free_worker_runs_light_rows_while_a_heavy_step_is_in_flight() {
    let fake = Queued::new(Duration::from_millis(10));
    let light = Planned::Run(Gate {
        heavy: false,
        ..match heavy_gate("fmt x", 2) {
            Planned::Run(g) => g,
            _ => unreachable!(),
        }
    });
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 1), light];
    let rows = run_all_with(
        &planned,
        &fake,
        &RunOpts {
            workers: 2,
            budget: Duration::from_secs(60),
            queue: Duration::from_secs(60),
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
        },
    );
    assert!(rows.iter().all(|(_, v)| v.kind() == Kind::Passed), "{rows:?}");
    let beside = fake.beside.lock().unwrap().clone();
    // The first heavy step and the light row overlapped (whichever entered first); held behind the second heavy row, the
    // light row would start only after the first heavy step ended.
    let with = |a: &str, b: &str| beside.iter().any(|(id, w)| id == a && w.iter().any(|x| x == b));
    assert!(with("fmt x", "clippy a") || with("clippy a", "fmt x"), "{beside:?}");
    let heavy_beside: Vec<_> = beside.iter().filter(|(id, _)| id.starts_with("clippy")).collect();
    assert!(
        heavy_beside.iter().all(|(_, with)| !with.iter().any(|w| w.starts_with("clippy"))),
        "never two heavy steps at once: {beside:?}"
    );
}

/// One stack shape of the property test below.
struct Shape {
    /// Commits on the local train, of which the first `pushed` are on the remote's `train/x`.
    train: usize,
    pushed: usize,
    /// A pushed sibling branch at the last train commit (so the remote holds the train through it).
    sibling: bool,
    /// A file added and removed again on the branch.
    undone: bool,
    /// An unpushed local side branch merged in with `--no-ff`.
    merge: bool,
    /// The remote has an old `fix` the pushed commit does not contain (a force push).
    force: bool,
}

/// Property (review of c4e32e462, P1-1): for every stack shape (a local train unpushed, partly or wholly pushed, or reached
/// through a pushed sibling; a change made and undone; an unpushed side branch merged with `--no-ff`; a force push), live and with
/// `git ls-remote` unreadable, every commit the remote lacks (`git log <tip> --not --remotes=origin`) has its files in the planned
/// diff, and an unpushed train commit in a workspace the branch never touches still plans that workspace's Clippy. The shapes are
/// enumerated so each combination that matters runs once (pairwise): 2026-11-15 the 24 random cases took 1,651 git processes
/// and hit nextest's 60 s limit on the Windows runner (run 8138).
#[test]
fn every_commit_the_remote_lacks_is_in_the_planned_diff() {
    #[rustfmt::skip]
    let shapes = [
        Shape { train: 2, pushed: 0, sibling: false, undone: true, merge: true, force: true },
        Shape { train: 2, pushed: 1, sibling: false, undone: false, merge: false, force: false },
        Shape { train: 2, pushed: 2, sibling: false, undone: false, merge: true, force: false },
        Shape { train: 0, pushed: 0, sibling: false, undone: true, merge: false, force: true },
        Shape { train: 1, pushed: 0, sibling: true, undone: false, merge: true, force: false },
    ];
    for (case, sh) in shapes.iter().enumerate() {
        let r = Repo::new();
        let add = |path: &str| {
            let full = r.root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, "x\n").unwrap();
            r.git(&["add", path]);
            r.git(&["commit", "-q", "-m", path]);
            r.git(&["rev-parse", "HEAD"]).trim().to_string()
        };
        let c0 = add("README");
        r.remote("main", &c0);
        r.git(&["checkout", "-q", "-b", "train/x"]);
        let train: Vec<String> = (0..sh.train).map(|i| add(&format!("apps/dyno/src/t{i}.rs"))).collect();
        if sh.pushed > 0 {
            r.remote("train/x", &train[sh.pushed - 1]);
        }
        if sh.sibling {
            r.remote("feat/sibling", train.last().unwrap());
        }
        let train_tip = train.last().cloned().unwrap_or_else(|| c0.clone());
        r.git(&["checkout", "-q", "-b", "fix"]);
        add("tools/x/f0.rs");
        if sh.undone {
            add("apps/pitwall/src/undone.rs");
            r.git(&["rm", "-q", "apps/pitwall/src/undone.rs"]);
            r.git(&["commit", "-q", "-m", "undo"]);
        }
        if sh.merge {
            r.git(&["checkout", "-q", "-b", "side"]);
            add("apps/pitwall/src/side.rs");
            r.git(&["checkout", "-q", "fix"]);
            add("tools/x/before-merge.rs");
            r.git(&["merge", "-q", "--no-ff", "--no-edit", "side"]);
        }
        let mut push = new_ref("fix", "");
        if sh.force {
            r.git(&["checkout", "-q", "-b", "old-fix", &c0]);
            push.remote_sha = add("tools/x/old.rs");
            r.remote("fix", &push.remote_sha);
            r.git(&["checkout", "-q", "fix"]);
        }
        push.sha = r.git(&["rev-parse", "HEAD"]).trim().to_string();
        let tip = push.sha.clone();
        // Every commit the remote lacks with its files, in one git process: `commit <sha>` lines, then its files.
        let log = r.git(&[
            "log",
            "--no-merges",
            "--name-only",
            "--format=commit %H",
            &tip,
            "--not",
            "--remotes=origin",
        ]);
        let (mut lacks, mut wanted) = (Vec::new(), Vec::new());
        for l in log.lines().filter(|l| !l.is_empty()) {
            match l.strip_prefix("commit ") {
                Some(c) => lacks.push(c.to_string()),
                None => wanted.push((lacks.last().cloned().unwrap_or_default(), l.to_string())),
            }
        }
        let unpushed_train = train.iter().any(|t| lacks.contains(t));
        assert_eq!(
            unpushed_train,
            sh.pushed < sh.train && !sh.sibling,
            "case {case}: the shape is what it says"
        );
        for (how, look) in [("live", r.live()), ("unreadable", base_choice::Remote(None))] {
            let (base, why) = super::choose_base(&r.root, &push, Some("origin"), "origin/main", &look);
            let (change, _, _) = super::discover(&r.root, &base, &[], std::slice::from_ref(&tip)).unwrap();
            for (c, f) in &wanted {
                assert!(
                    change.files.contains(f),
                    "case {case} ({how}): {f} of {c} is not planned; base {base} ({why})"
                );
            }
            if unpushed_train {
                let got = ids(&plan_gates(&plan(), &change, &facts()));
                assert!(
                    got.contains(&"clippy apps/dyno".to_string()),
                    "case {case} ({how}): {got:?}; base {base} ({why})"
                );
            }
        }
        if unpushed_train {
            // The branch's own diff (from the train's local tip) does not reach apps/dyno; the push's check must.
            let (own, _, _) = super::discover(&r.root, &train_tip, &[], std::slice::from_ref(&tip)).unwrap();
            assert!(
                !ids(&plan_gates(&plan(), &own, &facts())).contains(&"clippy apps/dyno".to_string()),
                "case {case}"
            );
        }
    }
}
