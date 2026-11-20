use super::*;
use crate::plan_since::{Leg, Rerun};

const NEXTEST_ID: &str = "paddock-service per_machine::tests::every_adopting_app_is_in_sync";
const PW_ID_TEXT: &str = "[webkit] › e2e/a11y.e2e.ts:6:7 › main screen has no serious accessibility violations (dark)";

fn rerun(id: u64, name: &str, key: &str, shard: Option<&str>) -> Rerun {
    Rerun {
        id,
        name: name.into(),
        key: Some(key.into()),
        leg: Leg {
            shard: shard.map(str::to_string),
            ..Leg::default()
        },
        scope: Scope::Job("test".into()),
        vitest: vec![],
    }
}

fn plan(workflow: &str, reruns: Vec<Rerun>) -> SincePlan {
    SincePlan {
        run: 8300,
        workflow: workflow.into(),
        head: "abc".into(),
        reruns,
        not_rerun: 0,
        everything: None,
        full_ran: false,
    }
}

fn only(pairs: &[(&str, &[&str])]) -> OnlyTests {
    OnlyTests::new(
        pairs
            .iter()
            .map(|(j, ids)| (j.to_string(), ids.iter().map(|s| s.to_string()).collect()))
            .collect(),
    )
}

#[test]
fn ids_are_told_by_their_shape() {
    assert_eq!(id_kind(NEXTEST_ID), Some(Kind::Nextest));
    assert_eq!(id_kind("src/app/x.component.spec.ts"), Some(Kind::Vitest));
    assert_eq!(id_kind(PW_ID_TEXT), Some(Kind::Playwright));
    assert_eq!(playwright_location(PW_ID_TEXT).as_deref(), Some("webkit=e2e/a11y.e2e.ts:6"));
    // A run with no project (one project in the config) names none: `=<file>:<line>`.
    assert_eq!(
        playwright_location("e2e/a.e2e.ts:3:5 › a title").as_deref(),
        Some("=e2e/a.e2e.ts:3")
    );
    assert_eq!(id_kind("(the job failed outside any test)"), None);
    assert_eq!(id_kind("one"), None);
}

#[test]
fn the_input_round_trips_and_every_bad_input_is_refused() {
    let o = only(&[("crate (ubuntu-latest)", &[NEXTEST_ID, NEXTEST_ID])]);
    assert_eq!(OnlyTests::parse(&o.encode()).unwrap(), o, "the encoding round trips");
    assert_eq!(o.count(), 1, "an id given twice is one test");
    // The parse de-duplicates.
    let p = OnlyTests::parse(r#"{"j":["a b","a b"]}"#).unwrap();
    assert_eq!(p.count(), 1);
    for bad in [
        "",
        "[]",
        "{}",
        r#"{"j":[]}"#,
        r#"{"j":"a b"}"#,
        r#"{"j":[1]}"#,
        r#"{"j":["nonsense"]}"#,
        r#"{"j":["(the job failed outside any test)"]}"#,
    ] {
        assert!(OnlyTests::parse(bad).is_err(), "{bad}");
    }
    let many: Vec<String> = (0..=MAX_IDS).map(|i| format!("bin t{i}")).collect();
    let big = serde_json::json!({ "j": many }).to_string();
    assert!(OnlyTests::parse(&big).unwrap_err().contains("more than"));
}

#[test]
fn nextest_vitest_and_playwright_jobs_are_narrowed_to_their_named_tests_and_the_rest_do_not_rerun() {
    let mut p = plan(
        "debrief.yml",
        vec![
            rerun(1, "rust tests", "rust-test", None),
            rerun(2, "frontend", "frontend", None),
            rerun(3, "end-to-end (shard 2/4)", "e2e", Some("2")),
            rerun(4, "rust tests (macOS)", "rust-test-macos", None),
        ],
    );
    let o = only(&[
        ("rust tests", &[NEXTEST_ID]),
        ("frontend", &["src/app/x.component.spec.ts"]),
        ("end-to-end (shard 2/4)", &[PW_ID_TEXT]),
    ]);
    p.apply_only_tests(&o).unwrap();
    let names: Vec<&str> = p.reruns.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        ["rust tests", "frontend", "end-to-end (shard 2/4)"],
        "the macOS job is not named: it does not rerun"
    );
    let out: BTreeMap<String, String> = p.app_outputs().into_iter().collect();
    assert!(out["rust_test_filter"].contains("test(=per_machine::tests::every_adopting_app_is_in_sync)"));
    assert_eq!(out["frontend_include"], "src/app/x.component.spec.ts");
    assert_eq!(out["e2e_shards"], r#"[{"shard":2,"tests":"webkit=e2e/a11y.e2e.ts:6"}]"#);
    assert_eq!(out["rerun_jobs"], r#"["e2e","frontend","rust-test"]"#);
    assert!(!out.contains_key("rust_test_macos_filter"));
}

#[test]
fn a_name_that_cannot_be_honoured_is_an_error_and_changes_nothing() {
    let base = || {
        plan(
            "ci.yml",
            vec![
                rerun(1, "crate (ubuntu-latest)", "crate", None),
                rerun(2, "conformance", "conformance", None),
            ],
        )
    };
    // A job that did not fail in the run.
    let mut p = base();
    assert!(
        p.apply_only_tests(&only(&[("crate (macos-latest)", &[NEXTEST_ID])]))
            .unwrap_err()
            .contains("did not fail")
    );
    // A job whose runner takes no test list: it reruns whole, in the next rung.
    assert!(
        p.apply_only_tests(&only(&[("conformance", &[NEXTEST_ID])]))
            .unwrap_err()
            .contains("takes a test list")
    );
    // The wrong runner's id.
    assert!(
        p.apply_only_tests(&only(&[("crate (ubuntu-latest)", &["src/x.spec.ts"])]))
            .unwrap_err()
            .contains("is not a Nextest test id")
    );
    assert_eq!(p.reruns.len(), 2, "an error changes nothing");
    assert!(matches!(p.reruns[0].scope, Scope::Job(_)));
    // A plan that already reruns everything cannot be narrowed.
    let mut e = base();
    e.everything = Some("the plan job failed".into());
    assert!(
        e.apply_only_tests(&only(&[("crate (ubuntu-latest)", &[NEXTEST_ID])]))
            .unwrap_err()
            .contains("reruns everything")
    );
}

#[test]
fn a_ci_yml_crate_leg_gets_its_filter_through_the_existing_leg_output() {
    let mut r = rerun(1, "crate (ubuntu-latest)", "crate", None);
    r.leg.os = Some("ubuntu-latest".into());
    let mut p = plan("ci.yml", vec![r, rerun(2, "paddock-dev (macos-latest)", "paddock-dev", None)]);
    p.apply_only_tests(&only(&[("crate (ubuntu-latest)", &[NEXTEST_ID])])).unwrap();
    let out: BTreeMap<String, String> = p.ci_outputs(&[], &mut |_| {}).into_iter().collect();
    assert!(out["crate_legs"].contains(r#""os":"ubuntu-latest""#) && out["crate_legs"].contains("test(=per_machine::tests"));
    assert_eq!(out["rerun_jobs"], r#"["crate"]"#, "paddock-dev is not named");
    assert!(!out.contains_key("paddock_dev_legs"));
}

#[test]
fn a_failure_in_each_playwright_project_is_run_in_that_project() {
    let mut p = plan("debrief.yml", vec![rerun(3, "end-to-end (shard 2/4)", "e2e", Some("2"))]);
    let both = [
        "[chromium] › e2e/a.e2e.ts:3:5 › one",
        "[webkit] › e2e/a.e2e.ts:3:5 › one",
        "[webkit] › e2e/b.e2e.ts:9:1 › two",
    ];
    p.apply_only_tests(&only(&[("end-to-end (shard 2/4)", &both)])).unwrap();
    let out: BTreeMap<String, String> = p.app_outputs().into_iter().collect();
    assert_eq!(
        out["e2e_shards"],
        r#"[{"shard":2,"tests":"chromium=e2e/a.e2e.ts:3 webkit=e2e/a.e2e.ts:3 webkit=e2e/b.e2e.ts:9"}]"#
    );
}

#[test]
fn a_test_binary_a_filterset_cannot_write_is_selected_whole_and_says_so() {
    let mut p = plan("ci.yml", vec![rerun(1, "crate (ubuntu-latest)", "crate", None)]);
    let odd = "paddock-service weird::tests::name with spaces";
    // A name with a space is not an id (it would split into binary and test); a name with a `.` in it is not plain to a filterset.
    assert!(id_kind(odd).is_none());
    let tricky = "paddock-service tests::it::a-b";
    let warnings = p
        .apply_only_tests(&only(&[("crate (ubuntu-latest)", &[tricky, NEXTEST_ID])]))
        .unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("binary paddock-service") && warnings[0].contains("whole binary"),
        "{warnings:?}"
    );
    // Plain names: no warning (the positive control of the note).
    let mut q = plan("ci.yml", vec![rerun(1, "crate (ubuntu-latest)", "crate", None)]);
    assert!(
        q.apply_only_tests(&only(&[("crate (ubuntu-latest)", &[NEXTEST_ID])]))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn only_jobs_keeps_the_named_jobs_whole_and_refuses_what_it_cannot_honour() {
    let mk = || {
        plan(
            "ci.yml",
            vec![
                rerun(1, "crate (ubuntu-latest)", "crate", None),
                rerun(2, "conformance", "conformance", None),
            ],
        )
    };
    let mut p = mk();
    p.apply_only_jobs(&OnlyJobs::parse(r#"["conformance"]"#).unwrap()).unwrap();
    assert_eq!(p.reruns.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["conformance"]);
    assert!(matches!(p.reruns[0].scope, Scope::Job(_)), "a named job reruns whole");
    // A job that did not fail, and a plan that reruns everything, are errors and change nothing.
    let mut p = mk();
    assert!(
        p.apply_only_jobs(&OnlyJobs::parse(r#"["icons"]"#).unwrap())
            .unwrap_err()
            .contains("did not fail")
    );
    assert_eq!(p.reruns.len(), 2);
    let mut e = mk();
    e.everything = Some("the plan job failed".into());
    assert!(
        e.apply_only_jobs(&OnlyJobs::parse(r#"["conformance"]"#).unwrap())
            .unwrap_err()
            .contains("reruns everything")
    );
    for bad in ["", "{}", "[]", r#"[1]"#, r#"[""]"#, "nonsense"] {
        assert!(OnlyJobs::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(OnlyJobs::parse(r#"["b","a","a"]"#).unwrap().names(), ["a", "b"]);
}
