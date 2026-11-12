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
        windows_clippy: BTreeSet::from(["apps/dyno".to_string(), "apps/pitwall".to_string()]),
        image: Some(Ok("registry/paddock-ci:v7".into())),
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
    let planned = plan_gates(&plan(), &change(&["crates/paddock-proc/src/lib.rs"]), &facts());
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
    assert!(ci.contains(&args.join(" ")));
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
    assert!(matches!(&v["fmt apps/dyno"], Verdict::Passed { .. }), "the other gates still ran");
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
    accept_not_measured(&mut rows, &["clippy".into()], state.path());
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    accept_not_measured(&mut rows, &["clippy-linux".into()], state.path());
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::Ok);
    assert!(report.text.contains("ACCEPTED      clippy-linux crates") && report.text.contains("logged"));
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
