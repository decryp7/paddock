//! The push gate with a plan, a list of files and a fake machine: no process starts (ER-14).

use super::exec::{Done, Exec, Verdict, decide, run_all};
use super::gates::{Change, Facts, Gate, Need, Planned, plan_gates};
use super::render;
use paddock_cli::Exit;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Mutex;

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
        range: Some("abc..HEAD".into()),
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
        range: None,
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
    ran: Mutex<Vec<String>>,
}

impl Exec for Fake {
    fn has_tool(&self, p: &str) -> bool {
        !self.no_tools.iter().any(|t| t == p)
    }
    fn docker_up(&self) -> bool {
        !self.no_docker
    }
    fn run(&self, g: &Gate) -> Result<Done, String> {
        self.ran.lock().unwrap().push(g.id.clone());
        let fail = self.fail.iter().any(|f| f == &g.id);
        Ok(Done {
            success: !fail,
            how: if fail { "exit 1".into() } else { "exit 0".into() },
            tail: if fail { "error: boom".into() } else { String::new() },
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
        fn run(&self, _: &Gate) -> Result<Done, String> {
            Ok(Done {
                success: false,
                how: "exit 137".into(),
                tail: String::new(),
            })
        }
    }
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let g = run_gates(&planned).into_iter().find(|g| g.id == "clippy-linux crates").unwrap();
    assert!(matches!(decide(g, &Oom), Verdict::Failed { next, .. } if next.contains("PADDOCK_LINUX_MEMORY")));
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
