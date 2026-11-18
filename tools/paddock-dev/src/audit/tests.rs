//! The audit's tests: a finding the change adds fails, one main has warns, a tool that did not look is not measured, and each
//! check shows it can see its signal (ER-33): the same fake tool with its advisory removed finds nothing.

use super::*;
use serde_json::Value;
use std::cell::Cell;
use std::sync::Mutex;

/// A `cargo deny --format json` run, trimmed from a real one (cargo-deny 0.20.2) for `time 0.1.43`.
fn deny_json(advisory: Option<&str>) -> String {
    let mut s = String::from(r#"{"fields":{"level":"WARN","message":"unable to find a config path"},"type":"log"}"#);
    s.push('\n');
    let errors = if let Some(id) = advisory {
        s.push_str(&format!(
            r#"{{"fields":{{"advisory":{{"id":"{id}","package":"time"}},"code":"vulnerability","graphs":[{{"Krate":{{"name":"time","version":"0.1.43"}},"parents":[{{"Krate":{{"name":"app","version":"0.1.0"}}}}]}}],"message":"Potential segfault in the time crate","severity":"error"}},"type":"diagnostic"}}"#
        ));
        s.push('\n');
        1
    } else {
        0
    };
    s.push_str(&format!(
        r#"{{"fields":{{"advisories":{{"errors":{errors},"helps":0,"notes":0,"warnings":0}}}},"type":"summary"}}"#
    ));
    s
}

/// An `npm audit --json` answer, trimmed from a real one (npm 11) for picomatch below `@angular-devkit/core` below `@angular/cli`.
fn npm_json(severity: &str) -> String {
    format!(
        r#"{{"auditReportVersion":2,"vulnerabilities":{{
        "picomatch":{{"name":"picomatch","severity":"{severity}","isDirect":false,"via":[{{"source":1,"name":"picomatch","dependency":"picomatch","title":"picomatch vulnerable to ReDoS","url":"https://github.com/advisories/GHSA-fc5q-39xc-vjpf","severity":"{severity}","range":"<=4.0.7"}}],"effects":["@angular-devkit/core"]}},
        "@angular-devkit/core":{{"name":"@angular-devkit/core","severity":"{severity}","isDirect":false,"via":["picomatch"],"effects":["@angular/cli"]}},
        "@angular/cli":{{"name":"@angular/cli","severity":"{severity}","isDirect":true,"via":["@angular-devkit/core"],"effects":[]}}
        }},"metadata":{{}}}}"#
    )
}

fn finding(id: &str, package: &str) -> Finding {
    Finding {
        source: "cargo crates/Cargo.lock".into(),
        id: id.into(),
        package: package.into(),
        version: "1.0.0".into(),
        path: format!("app 0.1.0 > {package} 1.0.0"),
        title: "a title".into(),
    }
}

#[test]
fn cargo_deny_json_becomes_a_finding_with_its_dependency_path() {
    let m = parse_deny("cargo crates/Cargo.lock", &deny_json(Some("RUSTSEC-2020-0071")));
    let Measure::Found(f) = m else { panic!("measured") };
    assert_eq!(f.len(), 1);
    assert_eq!(
        (f[0].id.as_str(), f[0].package.as_str(), f[0].version.as_str()),
        ("RUSTSEC-2020-0071", "time", "0.1.43")
    );
    assert_eq!(f[0].path, "app 0.1.0 > time 0.1.43");
}

#[test]
fn positive_control_the_same_deny_output_without_the_advisory_finds_nothing() {
    assert_eq!(parse_deny("s", &deny_json(None)), Measure::Found(vec![]));
    assert_ne!(parse_deny("s", &deny_json(Some("RUSTSEC-2020-0071"))), Measure::Found(vec![]));
}

#[test]
fn cargo_deny_that_printed_no_summary_is_not_measured() {
    // What an offline run prints: an error log and no summary.
    let text = r#"{"fields":{"level":"ERROR","message":"failed to fetch the advisory database: network is unreachable"},"type":"log"}"#;
    let Measure::NotMeasured(why) = parse_deny("s", text) else {
        panic!("looked")
    };
    assert!(why.contains("network is unreachable"), "{why}");
    assert!(
        matches!(parse_deny("s", ""), Measure::NotMeasured(_)),
        "no output at all is not a pass"
    );
}

#[test]
fn cargo_deny_that_counts_errors_but_prints_none_is_not_measured() {
    let text = r#"{"fields":{"advisories":{"errors":2,"helps":0,"notes":0,"warnings":0}},"type":"summary"}"#;
    assert!(matches!(parse_deny("s", text), Measure::NotMeasured(_)));
}

#[test]
fn a_warning_severity_advisory_is_not_a_finding() {
    let text = deny_json(Some("RUSTSEC-2020-0071"))
        .replace(r#""severity":"error""#, r#""severity":"warning""#)
        .replace(r#""errors":1"#, r#""errors":0"#);
    assert_eq!(parse_deny("s", &text), Measure::Found(vec![]));
}

#[test]
fn npm_audit_json_becomes_a_finding_with_the_path_from_the_direct_dependency() {
    let Measure::Found(f) = parse_npm("npm package-lock.json", &npm_json("high"), "high") else {
        panic!("measured")
    };
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].id, "GHSA-fc5q-39xc-vjpf");
    assert_eq!(f[0].package, "picomatch");
    assert_eq!(f[0].path, "@angular/cli > @angular-devkit/core > picomatch");
}

#[test]
fn npm_findings_below_the_level_do_not_count() {
    assert_eq!(parse_npm("n", &npm_json("moderate"), "high"), Measure::Found(vec![]));
    assert!(matches!(parse_npm("n", &npm_json("moderate"), "moderate"), Measure::Found(f) if f.len() == 1));
}

#[test]
fn npm_that_answered_with_an_error_or_no_json_is_not_measured() {
    let offline = r#"{"error":{"code":"ENOTFOUND","summary":"request to https://registry.npmjs.org failed"}}"#;
    let Measure::NotMeasured(why) = parse_npm("n", offline, "high") else {
        panic!("looked")
    };
    assert!(why.contains("registry.npmjs.org"), "{why}");
    assert!(matches!(parse_npm("n", "npm error network", "high"), Measure::NotMeasured(_)));
    assert!(matches!(parse_npm("n", "{}", "high"), Measure::NotMeasured(_)));
}

fn npm() -> Npm {
    Npm {
        level: "high".into(),
        omit_dev: true,
    }
}

fn found(v: Vec<Finding>) -> Measure {
    Measure::Found(v)
}

#[test]
fn an_advisory_the_change_introduces_is_new() {
    let head = found(vec![finding("RUSTSEC-2026-0001", "left")]);
    let v = Verdict::compare("cargo crates/Cargo.lock", &head, &mut || found(vec![]));
    assert_eq!(v.new.len(), 1);
    assert!(v.existing.is_empty() && !v.clean());
}

#[test]
fn an_advisory_main_has_too_is_a_warning_naming_main() {
    let head = found(vec![finding("RUSTSEC-2026-0543", "chrono")]);
    let v = Verdict::compare("cargo crates/Cargo.lock", &head, &mut || {
        found(vec![finding("RUSTSEC-2026-0543", "chrono")])
    });
    assert!(v.new.is_empty() && v.clean());
    let text = lines(&v, Mode::Compare, true);
    assert_eq!(text.len(), 1);
    assert!(
        text[0].starts_with("::warning::") && text[0].contains("RUSTSEC-2026-0543") && text[0].contains("already present on main"),
        "{}",
        text[0]
    );
}

#[test]
fn the_same_advisory_on_another_version_is_still_main_s_and_on_another_dependency_is_new() {
    let mut on_base = finding("RUSTSEC-2026-0543", "chrono");
    on_base.version = "0.4.1".into();
    let head = found(vec![
        finding("RUSTSEC-2026-0543", "chrono"),
        finding("RUSTSEC-2026-0543", "chrono-tz"),
    ]);
    let v = Verdict::compare("s", &head, &mut || found(vec![on_base.clone()]));
    assert_eq!(v.existing.iter().map(|f| f.package.as_str()).collect::<Vec<_>>(), ["chrono"]);
    assert_eq!(v.new.iter().map(|f| f.package.as_str()).collect::<Vec<_>>(), ["chrono-tz"]);
}

#[test]
fn a_lockfile_in_another_workspace_does_not_borrow_main_s_decision() {
    let mut other = finding("RUSTSEC-2026-0543", "chrono");
    other.source = "cargo apps/dyno/Cargo.lock".into();
    let head = found(vec![finding("RUSTSEC-2026-0543", "chrono")]);
    let v = Verdict::compare("cargo crates/Cargo.lock", &head, &mut || found(vec![other.clone()]));
    assert_eq!(v.new.len(), 1);
}

#[test]
fn a_clean_change_never_audits_the_merge_base() {
    let v = Verdict::compare("s", &found(vec![]), &mut || {
        panic!("the merge base is audited only when the change has findings")
    });
    assert!(v.clean() && v.existing.is_empty());
}

#[test]
fn a_tool_that_did_not_look_is_not_measured_on_the_change_or_on_the_base() {
    let v = Verdict::compare("s", &Measure::NotMeasured("offline".into()), &mut || {
        panic!("no base without a head")
    });
    assert!(!v.clean());
    let head = found(vec![finding("RUSTSEC-2026-0543", "chrono")]);
    let v = Verdict::compare("s", &head, &mut || Measure::NotMeasured("no history".into()));
    assert!(v.new.is_empty() && v.existing.is_empty(), "no guess about the finding");
    assert_eq!(v.not_measured.len(), 1);
    assert!(v.not_measured[0].1.contains("no history") && v.not_measured[0].1.contains("cannot be compared"));
}

#[test]
fn nightly_makes_every_finding_new_and_names_the_advisory_path_and_decision() {
    let v = Verdict::nightly("cargo crates/Cargo.lock", &found(vec![finding("RUSTSEC-2026-0543", "chrono")]));
    let text = lines(&v, Mode::Nightly, true);
    assert_eq!(text.len(), 1);
    for part in [
        "::error::",
        "RUSTSEC-2026-0543",
        "app 0.1.0 > chrono 1.0.0",
        "reason and a review date",
        "crates/deny.toml",
        "cargo update -p chrono",
    ] {
        assert!(text[0].contains(part), "{part} missing from {}", text[0]);
    }
    let npm = Finding {
        source: "npm package-lock.json".into(),
        ..finding("GHSA-fc5q-39xc-vjpf", "picomatch")
    };
    let t = lines(&Verdict::nightly("n", &found(vec![npm])), Mode::Nightly, true);
    assert!(
        t[0].contains("scripts/npm-audit-allow.json") && t[0].contains("reviewBy"),
        "{}",
        t[0]
    );
}

#[test]
fn report_only_turns_every_error_into_a_warning_that_says_it_would_fail() {
    let mut v = Verdict::nightly("s", &found(vec![finding("RUSTSEC-2026-0543", "chrono")]));
    v.merge(Verdict::nightly("t", &Measure::NotMeasured("offline".into())));
    let text = lines(&v, Mode::Nightly, false);
    assert_eq!(text.len(), 2);
    assert!(
        text.iter().all(|l| l.starts_with("::warning::report-only, would fail: ")),
        "{text:?}"
    );
    assert!(lines(&v, Mode::Nightly, true).iter().all(|l| l.starts_with("::error::")));
}

fn exit_of(v: &Verdict, enforce: bool) -> i32 {
    super::report(v, Mode::Compare, enforce, 1).exit.code()
}

#[test]
fn exit_codes_new_findings_fail_not_measured_is_its_own_code_and_report_only_passes() {
    let new = Verdict::nightly("s", &found(vec![finding("A", "x")]));
    let missing = Verdict::nightly("s", &Measure::NotMeasured("offline".into()));
    let existing = Verdict {
        existing: vec![finding("A", "x")],
        ..Verdict::default()
    };
    assert_eq!(exit_of(&new, true), Exit::Findings.code());
    assert_eq!(exit_of(&missing, true), Exit::NotMeasured.code());
    assert_eq!(exit_of(&existing, true), Exit::Ok.code());
    assert_eq!(exit_of(&Verdict::default(), true), Exit::Ok.code());
    assert_eq!(exit_of(&new, false), Exit::Ok.code(), "report-only");
    assert_eq!(exit_of(&missing, false), Exit::Ok.code(), "report-only");
}

/// A runner answering from a table, matching on the folder it runs in.
struct Fake {
    answers: Vec<(&'static str, Ran)>,
    ran_in: Mutex<Vec<String>>,
}

impl Runner for Fake {
    fn run(&self, dir: &Path, argv: &[String]) -> Ran {
        self.ran_in.lock().unwrap().push(format!("{} @ {}", argv[0], dir.display()));
        let d = dir.display().to_string();
        self.answers
            .iter()
            .find(|(k, _)| d.contains(k))
            .map(|(_, r)| r.clone())
            .unwrap_or(Ran {
                code: None,
                stdout: String::new(),
                stderr: "not found".into(),
            })
    }
}

struct FakeBase {
    prepared: Cell<u32>,
    released: Cell<u32>,
    result: Result<PathBuf, String>,
}

impl Base for FakeBase {
    fn prepare(&self) -> Result<PathBuf, String> {
        self.prepared.set(self.prepared.get() + 1);
        self.result.clone()
    }
    fn release(&self) {
        self.released.set(self.released.get() + 1);
    }
}

fn deny_ran(advisory: Option<&str>) -> Ran {
    Ran {
        code: Some(i32::from(advisory.is_some())),
        stdout: String::new(),
        stderr: deny_json(advisory),
    }
}

fn cargo_target(dir: &str) -> Target {
    Target {
        kind: Kind::Cargo,
        dir: dir.into(),
        file: format!("{dir}/Cargo.lock"),
    }
}

/// A merge base folder holding the lockfiles the base has (the extraction's shape), under a folder the test removes.
fn base_with(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let p = dir.path().join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "").unwrap();
    }
    dir
}

#[test]
fn judge_all_audits_the_tree_and_then_the_merge_base_once_and_cleans_up() {
    let dir = base_with(&["crates/Cargo.lock", "apps/dyno/Cargo.lock"]);
    let key: &'static str = Box::leak(dir.path().display().to_string().into_boxed_str());
    let runner = Fake {
        answers: vec![
            ("/head/", deny_ran(Some("RUSTSEC-2026-0543"))),
            (key, deny_ran(Some("RUSTSEC-2026-0543"))),
        ],
        ran_in: Mutex::new(vec![]),
    };
    let base = FakeBase {
        prepared: Cell::new(0),
        released: Cell::new(0),
        result: Ok(dir.path().to_path_buf()),
    };
    let targets = [cargo_target("crates"), cargo_target("apps/dyno")];
    let v = judge_all(&runner, Path::new("/head/"), &targets, &npm(), Some(&base));
    assert_eq!(v.existing.len(), 2, "both workspaces have it on main too");
    assert!(v.clean());
    assert_eq!((base.prepared.get(), base.released.get()), (1, 1), "extracted once, removed once");
}

#[test]
fn a_lockfile_the_change_adds_has_no_base_so_its_findings_are_new_not_unmeasured() {
    // The base folder holds crates/ only: apps/dyno/Cargo.lock is added by this change.
    let dir = base_with(&["crates/Cargo.lock"]);
    let key: &'static str = Box::leak(dir.path().display().to_string().into_boxed_str());
    let runner = Fake {
        answers: vec![
            ("/head/", deny_ran(Some("RUSTSEC-2026-0543"))),
            (key, deny_ran(Some("RUSTSEC-2026-0543"))),
        ],
        ran_in: Mutex::new(vec![]),
    };
    let base = FakeBase {
        prepared: Cell::new(0),
        released: Cell::new(0),
        result: Ok(dir.path().to_path_buf()),
    };
    let targets = [cargo_target("crates"), cargo_target("apps/dyno")];
    let v = judge_all(&runner, Path::new("/head/"), &targets, &npm(), Some(&base));
    assert!(v.not_measured.is_empty(), "{:?}", v.not_measured);
    assert_eq!(
        (v.new.len(), v.existing.len()),
        (1, 1),
        "the added lockfile's finding is new; the old one is main's"
    );
    assert!(v.new[0].source.contains("apps/dyno"));
    assert_eq!(
        runner.ran_in.lock().unwrap().iter().filter(|r| r.contains(key)).count(),
        1,
        "the base's missing workspace is not audited"
    );
}

#[test]
fn judge_all_does_not_extract_the_merge_base_when_nothing_is_found() {
    let runner = Fake {
        answers: vec![("/head/", deny_ran(None))],
        ran_in: Mutex::new(vec![]),
    };
    let base = FakeBase {
        prepared: Cell::new(0),
        released: Cell::new(0),
        result: Err("unused".into()),
    };
    let v = judge_all(&runner, Path::new("/head/"), &[cargo_target("crates")], &npm(), Some(&base));
    assert!(v.clean());
    assert_eq!((base.prepared.get(), base.released.get()), (0, 0));
}

#[test]
fn judge_all_without_a_merge_base_is_not_measured_and_a_missing_tool_is_not_a_pass() {
    let runner = Fake {
        answers: vec![("/head/", deny_ran(Some("RUSTSEC-2026-0543")))],
        ran_in: Mutex::new(vec![]),
    };
    let base = FakeBase {
        prepared: Cell::new(0),
        released: Cell::new(0),
        result: Err("origin/main is not fetched".into()),
    };
    let v = judge_all(&runner, Path::new("/head/"), &[cargo_target("crates")], &npm(), Some(&base));
    assert!(!v.clean() && v.not_measured[0].1.contains("origin/main is not fetched"));
    // cargo-deny not installed: the runner could not start it.
    let none = Fake {
        answers: vec![],
        ran_in: Mutex::new(vec![]),
    };
    let v = judge_all(&none, Path::new("/head/"), &[cargo_target("crates")], &npm(), None);
    assert!(!v.clean() && v.new.is_empty());
}

#[test]
fn judge_all_in_a_nightly_run_fails_on_a_new_advisory_and_passes_without_it() {
    let with = Fake {
        answers: vec![("/head/", deny_ran(Some("RUSTSEC-2026-0543")))],
        ran_in: Mutex::new(vec![]),
    };
    let without = Fake {
        answers: vec![("/head/", deny_ran(None))],
        ran_in: Mutex::new(vec![]),
    };
    let t = [cargo_target("crates")];
    assert_eq!(judge_all(&with, Path::new("/head/"), &t, &npm(), None).new.len(), 1);
    assert!(judge_all(&without, Path::new("/head/"), &t, &npm(), None).clean());
}

#[test]
fn the_plan_audits_a_lockfile_only_with_its_manifest_and_deny_toml_and_skips_golden_test_data() {
    let tracked: Vec<String> = [
        "crates/Cargo.lock",
        "crates/Cargo.toml",
        "crates/deny.toml",
        "apps/pacenotes/deny.toml",
        "apps/pacenotes/Cargo.toml",
        "crates/paddock-ipc/fuzz/Cargo.lock",
        "crates/paddock-ipc/fuzz/Cargo.toml",
        "package-lock.json",
        "package.json",
        "tools/paddock-dev/tests/golden/fixture/Cargo.lock",
        "tools/paddock-dev/tests/golden/fixture/Cargo.toml",
        "tools/paddock-dev/tests/golden/fixture/deny.toml",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    let sources: Vec<String> = plan(&tracked).iter().map(Target::source).collect();
    assert_eq!(sources, ["cargo crates/Cargo.lock", "npm package-lock.json"]);
}

#[test]
fn npm_audit_leaves_dev_dependencies_out_by_default_and_audits_them_when_asked() {
    struct Args(Mutex<Vec<String>>);
    impl Runner for Args {
        fn run(&self, _: &Path, argv: &[String]) -> Ran {
            *self.0.lock().unwrap() = argv.to_vec();
            Ran {
                code: Some(0),
                stdout: npm_json("moderate"),
                stderr: String::new(),
            }
        }
    }
    let t = Target {
        kind: Kind::Npm,
        dir: "".into(),
        file: "package-lock.json".into(),
    };
    let r = Args(Mutex::new(vec![]));
    audit_target(&r, Path::new("/x"), &t, &npm());
    assert!(r.0.lock().unwrap().contains(&"--omit=dev".to_string()));
    audit_target(
        &r,
        Path::new("/x"),
        &t,
        &Npm {
            level: "high".into(),
            omit_dev: false,
        },
    );
    assert!(!r.0.lock().unwrap().contains(&"--omit=dev".to_string()));
}

fn npm_finding() -> Finding {
    Finding {
        source: "npm package-lock.json".into(),
        ..finding("GHSA-fc5q-39xc-vjpf", "picomatch")
    }
}

fn exception(review_by: &str) -> Exception {
    Exception {
        id: "GHSA-fc5q-39xc-vjpf".into(),
        reason: "build-time glob".into(),
        approved_by: "user".into(),
        approved: "2026-11-18".into(),
        review_by: review_by.into(),
    }
}

#[test]
fn an_approved_exception_warns_with_its_review_date_and_does_not_fail() {
    let mut v = Verdict::nightly("npm package-lock.json", &found(vec![npm_finding()]));
    v.apply_exceptions(&[exception("2026-12-18")], "2026-11-20");
    assert!(v.clean() && v.new.is_empty() && v.allowed.len() == 1);
    let text = lines(&v, Mode::Nightly, true);
    assert_eq!(text.len(), 1);
    assert!(
        text[0].starts_with("::warning::") && text[0].contains("allowed until 2026-12-18") && text[0].contains("approved by user"),
        "{}",
        text[0]
    );
}

#[test]
fn an_expired_exception_counts_as_a_finding_again_and_says_it_expired() {
    // The review day itself no longer counts, as in npm-audit.mjs.
    for today in ["2026-12-18", "2027-01-01"] {
        let mut v = Verdict::nightly("npm package-lock.json", &found(vec![npm_finding()]));
        v.apply_exceptions(&[exception("2026-12-18")], today);
        assert!(!v.clean() && v.allowed.is_empty() && v.new.len() == 1, "{today}");
        let text = lines(&v, Mode::Nightly, true);
        assert!(
            text.iter()
                .any(|l| l.starts_with("::error::") && l.contains("expired on 2026-12-18")),
            "{text:?}"
        );
    }
}

#[test]
fn an_expired_exception_for_a_finding_main_has_too_is_an_error_and_fails_an_enforced_run() {
    let mut v = Verdict::compare("n", &found(vec![npm_finding()]), &mut || found(vec![npm_finding()]));
    v.apply_exceptions(&[exception("2026-12-18")], "2026-12-18");
    assert!(v.new.is_empty() && v.existing.len() == 1 && v.expired.len() == 1);
    let text = lines(&v, Mode::Compare, true);
    assert!(
        text.iter()
            .any(|l| l.starts_with("::error::") && l.contains("expired on 2026-12-18")),
        "{text:?}"
    );
    assert!(!v.clean());
    assert_eq!(exit_of(&v, true), Exit::Findings.code());
    assert_eq!(exit_of(&v, false), Exit::Ok.code(), "report-only");
    assert!(
        lines(&v, Mode::Compare, false)
            .iter()
            .any(|l| l.starts_with("::warning::report-only, would fail: the exception"))
    );
    // The control: the same finding before the review day is allowed and the run is clean.
    let mut live = Verdict::compare("n", &found(vec![npm_finding()]), &mut || found(vec![npm_finding()]));
    live.apply_exceptions(&[exception("2026-12-18")], "2026-12-17");
    assert!(live.clean() && live.expired.is_empty());
}

#[test]
fn an_exception_covers_only_its_advisory_and_only_npm_findings() {
    let mut other = npm_finding();
    other.id = "GHSA-aaaa-bbbb-cccc".into();
    let cargo = Finding {
        id: "GHSA-fc5q-39xc-vjpf".into(),
        ..finding("x", "y")
    };
    let mut v = Verdict::nightly("s", &found(vec![other, cargo]));
    v.apply_exceptions(&[exception("2026-12-18")], "2026-11-20");
    assert_eq!(v.new.len(), 2);
    assert!(v.allowed.is_empty());
}

#[test]
fn an_exception_also_covers_a_finding_main_has() {
    let mut v = Verdict::compare("n", &found(vec![npm_finding()]), &mut || found(vec![npm_finding()]));
    v.apply_exceptions(&[exception("2026-12-18")], "2026-11-20");
    assert!(v.existing.is_empty() && v.allowed.len() == 1);
}

#[test]
fn today_is_read_as_a_utc_day() {
    assert_eq!(allow::day_of(0), "1970-01-01");
    assert_eq!(allow::day_of(20_775), "2026-11-18");
    assert_eq!(allow::day_of(11_016), "2000-02-29");
}

/// The cases both readers answer (scripts/npm-audit-fixtures.json: the script's own test reads the same file, rule 55).
fn fixtures() -> Value {
    assert!(
        crate::plan::is_contract_input("scripts/npm-audit-fixtures.json"),
        "the planner must mark paddock-dev for the file this test reads"
    );
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/npm-audit-fixtures.json");
    serde_json::from_str(&std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{} cannot be read: {e}", file.display()))).unwrap()
}

fn text_of(case: &Value) -> String {
    match case.get("raw").and_then(Value::as_str) {
        Some(raw) => raw.to_string(),
        None => case.get("report").or_else(|| case.get("doc")).unwrap().to_string(),
    }
}

fn cases(kind: &str) -> Vec<Value> {
    let all = fixtures();
    let list = all.get(kind).and_then(Value::as_array).cloned().unwrap();
    assert!(
        !list.is_empty(),
        "the shared {kind} fixtures are not empty: a loop over none proves nothing"
    );
    list
}

#[test]
fn every_shared_report_fixture_is_read_as_the_script_reads_it() {
    for c in cases("reports") {
        let name = c["name"].as_str().unwrap();
        let m = parse_npm("n", &text_of(&c), "high");
        match (c.get("notMeasured").and_then(Value::as_str), m) {
            (Some(why), Measure::NotMeasured(got)) => assert!(got.contains(why), "{name}: wanted {why:?}, got {got:?}"),
            (Some(why), Measure::Found(f)) => panic!("{name}: wanted not measured ({why}), found {f:?}"),
            (None, Measure::Found(f)) => {
                let want: Vec<String> = c["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                assert_eq!(f.iter().map(|x| x.id.clone()).collect::<Vec<_>>(), want, "{name}");
            }
            (None, Measure::NotMeasured(got)) => panic!("{name}: should have been measured: {got}"),
        }
    }
}

#[test]
fn every_shared_allow_fixture_is_read_as_the_script_reads_it() {
    for c in cases("allow") {
        let name = c["name"].as_str().unwrap();
        let got = parse_allow(&text_of(&c));
        match (c.get("error").and_then(Value::as_str), got) {
            (Some(why), Err(e)) => assert!(e.contains(why), "{name}: wanted {why:?}, got {e:?}"),
            (Some(why), Ok(v)) => panic!("{name}: wanted a refusal ({why}), accepted {} entries", v.len()),
            (None, Ok(v)) => {
                let want: Vec<String> = c["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                assert_eq!(v.into_iter().map(|e| e.id).collect::<Vec<_>>(), want, "{name}");
            }
            (None, Err(e)) => panic!("{name}: should have been accepted: {e}"),
        }
    }
}

#[test]
fn an_exception_may_run_at_most_90_days_and_days_are_real_days() {
    assert_eq!(allow::MAX_DAYS, 90);
    assert_eq!(allow::day_number("2026-11-18").map(allow::day_of), Some("2026-11-18".to_string()));
    for bad in ["2026-02-29", "2026-13-01", "2026-00-10", "2026-04-31", "2026-1-01", "20260101"] {
        assert_eq!(allow::day_number(bad), None, "{bad}");
    }
    assert!(allow::day_number("2028-02-29").is_some(), "a leap day");
}

/// Runs the script's own `parseAllow` on a text: `Some(Ok(ids))` or `Some(Err)` when it throws, `None` (said out loud) without node.
fn script_reads(text: &str) -> Option<Result<Vec<String>, String>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/npm-audit.mjs");
    let code = "import(process.argv[1]).then((m) => { try { console.log(JSON.stringify(m.parseAllow(process.argv[2]).map((e) => e.id))); } catch (e) { console.log('THROWS ' + e.message); } });";
    // conformance: test-env — node is found through the caller's PATH and runs a read-only script of the repository on its argv
    let mut cmd = paddock_proc::command("node");
    cmd.args(["-e", code, &script.display().to_string(), text]);
    let out = match paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(Duration::from_secs(60))) {
        Ok(o) if o.success() => o,
        other => {
            eprintln!(
                "NOT MEASURED: node did not run ({other:?}), so the script's own reader was not compared; the shared fixtures above still hold for both readers"
            );
            return None;
        }
    };
    let o = out.stdout_text();
    Some(match o.trim().strip_prefix("THROWS ") {
        Some(why) => Err(why.to_string()),
        None => Ok(serde_json::from_str(o.trim()).expect("ids")),
    })
}

#[test]
fn the_rust_reader_and_the_script_agree_on_every_shared_allow_fixture_when_node_runs() {
    for c in cases("allow") {
        let text = text_of(&c);
        let Some(js) = script_reads(&text) else { return };
        let rust = parse_allow(&text).map(|v| v.into_iter().map(|e| e.id).collect::<Vec<_>>());
        assert_eq!(rust.is_ok(), js.is_ok(), "{}: rust {rust:?}, script {js:?}", c["name"]);
    }
}

#[test]
fn the_families_real_list_is_read_by_the_accessor_and_by_the_script_whatever_it_holds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(ALLOW_FILE)).unwrap();
    let from_file = allow::load(&root).unwrap();
    // No entry is required (removing the last exception is the aim): the two readers agree on whatever is there.
    let ids: Vec<String> = from_file.iter().map(|e| e.id.clone()).collect();
    if let Some(js) = script_reads(&text) {
        assert_eq!(Ok(ids), js);
    }
}

#[test]
fn report_only_a_run_that_cannot_start_is_a_warning_and_exit_0_and_enforced_it_is_an_error() {
    let e = || CliError::failure("No lockfile with an audit found under /x.").with_hint("Run it in the repository root.");
    let r = super::cannot_start(e(), false).unwrap();
    assert_eq!(r.exit.code(), Exit::Ok.code());
    assert!(
        r.text
            .starts_with("::warning::report-only, would fail: audit did not run: No lockfile"),
        "{}",
        r.text
    );
    assert!(r.text.contains("Next step: Run it in the repository root."), "{}", r.text);
    assert!(super::cannot_start(e(), true).is_err(), "enforced it stays an error");
}

#[test]
fn report_only_a_broken_allow_file_and_an_unlistable_repository_do_not_turn_the_job_red() {
    // A broken list, read through the real loader.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(dir.path().join(ALLOW_FILE), r#"{"allow":[{"id":"nope"}]}"#).unwrap();
    let broken = allow::load(dir.path()).unwrap_err();
    assert!(broken.contains("is broken"), "{broken}");
    let r = super::cannot_start(CliError::failure(broken), false).unwrap();
    assert_eq!(r.exit.code(), Exit::Ok.code());
    // A root where `git ls-files` fails, through the real `run`: report-only warns, enforced fails.
    let none = Fake {
        answers: vec![],
        ran_in: Mutex::new(vec![]),
    };
    let gone = dir.path().join("no-such-folder");
    let r = run(&none, &gone, &npm(), "2026-11-20", false, None).unwrap();
    assert_eq!(r.exit.code(), Exit::Ok.code());
    assert!(
        r.text.starts_with("::warning::report-only, would fail: audit did not run"),
        "{}",
        r.text
    );
    assert!(run(&none, &gone, &npm(), "2026-11-20", true, None).is_err());
}

#[test]
fn the_json_report_lists_the_allowed_and_the_expired() {
    let mut v = Verdict::nightly("npm package-lock.json", &found(vec![npm_finding()]));
    v.apply_exceptions(&[exception("2026-12-18")], "2026-11-20");
    let data = super::report(&v, Mode::Nightly, true, 1).data;
    assert_eq!(data["allowed"][0]["id"], "GHSA-fc5q-39xc-vjpf");
    assert_eq!(data["allowed"][0]["reviewBy"], "2026-12-18");
    assert_eq!(data["expired"], json!([]));
    let mut late = Verdict::nightly("npm package-lock.json", &found(vec![npm_finding()]));
    late.apply_exceptions(&[exception("2026-12-18")], "2026-12-18");
    let data = super::report(&late, Mode::Nightly, true, 1).data;
    assert_eq!(data["expired"][0]["id"], "GHSA-fc5q-39xc-vjpf");
    assert_eq!(data["allowed"], json!([]));
}

#[test]
fn an_npm_severity_npm_does_not_define_is_not_measured_and_a_missing_one_too() {
    let unknown = npm_json("catastrophic");
    assert!(matches!(parse_npm("n", &unknown, "high"), Measure::NotMeasured(w) if w.contains("severity")));
    let none = npm_json("high")
        .replace(r#""severity":"high","isDirect":false"#, r#""isDirect":false"#)
        .replace(r#","severity":"high","range""#, r#","range""#);
    assert!(
        matches!(parse_npm("n", &none, "high"), Measure::NotMeasured(w) if w.contains("severity")),
        "{none}"
    );
}
