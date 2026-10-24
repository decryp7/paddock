//! The result format, redaction, coverage, diff and JUnit.

use paddock_testplan::*;

const PLAN: &str = include_str!("../fixtures/settings.sighterplan");
const RESULT: &str = include_str!("../fixtures/settings.sighterresult");

fn plan() -> Plan {
    parse_plan(PLAN.as_bytes()).unwrap()
}

fn result_text(from: &str, to: &str) -> String {
    assert!(RESULT.contains(from), "fixture lacks {from}");
    RESULT.replacen(from, to, 1)
}

fn bad(text: &str) -> PlanError {
    parse_result(text.as_bytes()).expect_err("must be refused")
}

#[test]
fn the_result_fixture_parses_and_round_trips() {
    let r = parse_result(RESULT.as_bytes()).unwrap();
    assert_eq!(r.plan.as_str(), "settings-smoke");
    assert_eq!(r.env.os, "Windows 11");
    assert_eq!(r.env.dpi, Some(144));
    assert_eq!(r.cases.len(), 2);
    assert_eq!(r.cases[0].status, Status::Pass);
    assert_eq!(r.cases[0].steps[0].score, Some(0.98));
    assert_eq!(r.cases[0].steps[1].evidence[0].kind, EvidenceKind::OcrText);
    assert_eq!(r.cases[1].iteration, Some(1));
    assert_eq!(r.cases[1].reason.as_deref(), Some("Welcome never appeared"));
    let text = r.to_toml(&NoRedaction);
    assert_eq!(parse_result(text.as_bytes()).unwrap(), r);
    assert_eq!(
        parse_result(text.as_bytes()).unwrap().to_toml(&NoRedaction),
        text
    );
    assert!(text.contains("score = 1.0"), "{text}");
}

#[test]
fn a_result_for_this_exact_plan_is_recognised() {
    let p = plan();
    let mut r = parse_result(RESULT.as_bytes()).unwrap();
    assert!(
        !r.is_for(&p),
        "the fixture carries a placeholder hash: it is for an older plan"
    );
    r.plan_hash = p.hash();
    assert!(r.is_for(&p));
    r.plan = PlanId::new("other").unwrap();
    assert!(!r.is_for(&p));
}

#[test]
fn results_are_refused_for_the_same_reasons_as_plans() {
    assert!(matches!(
        bad(&result_text(
            "format = \"sighterresult\"",
            "format = \"sighterplan\""
        )),
        PlanError::WrongFormat { .. }
    ));
    assert!(matches!(
        bad(&result_text("version = 1", "version = 3")),
        PlanError::Version { found: 3, .. }
    ));
    assert!(matches!(
        bad(&result_text("run_id =", "bogus = 1\nrun_id =")),
        PlanError::UnknownKey { .. }
    ));
    assert!(matches!(
        bad(&result_text(
            "status = \"pass\"\nduration_ms = 4200",
            "status = \"maybe\"\nduration_ms = 4200"
        )),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("kind = \"screenshot\"", "kind = \"video\"")),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text(
            "started = \"2026-10-25T08:30:00Z\"",
            "started = \"yesterday\""
        )),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("plan_hash = \"0000", "plan_hash = \"zz00")),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text(
            "run_id = \"run-2026-10-25-001\"",
            "run_id = \"has space\""
        )),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("score = 0.98", "score = 1.5")),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("score = 0.98", "score = nan")),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("score = 0.98", "score = -0.1")),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("sha256 = \"1111", "sha256 = \"ABCD")),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("sha256 = \"2222", "sha256 = \"22")),
        PlanError::BadField { .. }
    ));
    assert!(matches!(
        bad(&result_text("case = \"TC-1\"", "case = \"T C\"")),
        PlanError::BadId { .. }
    ));
    assert!(matches!(
        bad(&result_text(
            "screen_seen = \"Main\"",
            "screen_seen = \"a b\""
        )),
        PlanError::BadId { .. }
    ));
    assert!(matches!(
        bad(&result_text("\"REQ-142\", ", "\"REQ#142\", ")),
        PlanError::BadId { .. }
    ));
    assert!(matches!(
        bad(&result_text("[env]", "[envv]")),
        PlanError::UnknownKey { .. } | PlanError::Syntax { .. }
    ));
    assert!(matches!(
        bad(&"x".repeat(MAX_FILE_BYTES + 1)),
        PlanError::Limit { .. }
    ));
}

#[test]
fn evidence_paths_cannot_escape_the_result_folder() {
    for path in [
        "../secrets.png",
        "/abs/x.png",
        "a\\\\b.png",
        "C:/x.png",
        "a//b",
    ] {
        let e = bad(&result_text(
            "path = \"evidence/tc-1/final.png\"",
            &format!("path = \"{path}\""),
        ));
        assert!(matches!(e, PlanError::BadField { .. }), "{path}: {e:?}");
    }
}

#[test]
fn limits_apply_to_results() {
    let one = "[[cases]]\ncase = \"c\"\nstatus = \"pass\"\nduration_ms = 1\n";
    let head = "format = \"sighterresult\"\nversion = 1\nplan = \"p\"\nplan_hash = \"0000000000000000000000000000000000000000000000000000000000000000\"\nrun_id = \"r\"\nstarted = \"2026-10-25T08:30:00Z\"\n[env]\nos = \"x\"\n";
    assert!(parse_result(format!("{head}{}", one.repeat(3)).as_bytes()).is_ok());
    assert!(matches!(
        bad(&format!("{head}{}", one.repeat(MAX_CASE_RESULTS + 1))),
        PlanError::Limit { .. }
    ));
    let steps = "[[cases.steps]]\nindex = 0\nstatus = \"pass\"\n".repeat(MAX_STEPS + 1);
    assert!(matches!(
        bad(&format!("{head}{one}{steps}")),
        PlanError::Limit { .. }
    ));
    let long = "r".repeat(MAX_STRING + 1);
    assert!(matches!(
        bad(&format!("{head}{one}reason = \"{long}\"\n")),
        PlanError::Limit { .. }
    ));
    assert!(matches!(
        bad(&head.replace("os = \"x\"", &format!("os = \"{long}\""))),
        PlanError::Limit { .. }
    ));
    assert!(matches!(
        bad(&format!("{head}theme = \"{long}\"\n")),
        PlanError::Limit { .. }
    ));
}

fn secret_result() -> RunResult {
    let mut r = parse_result(RESULT.as_bytes()).unwrap();
    r.cases[1].reason =
        Some("typed hunter2 and then hunter22 into the field; the key is s3cr3t-key".into());
    r.env.app_build = Some("build hunter2".into());
    r
}

#[test]
fn writers_replace_resolved_secrets_by_their_placeholder() {
    struct Keys;
    impl Resolver for Keys {
        fn resolve(&self, name: &SecretRef) -> Option<String> {
            match name.as_str() {
                "PW" => Some("hunter2".into()),
                "KEY" => Some("s3cr3t-key".into()),
                "SHORT" => Some(String::new()),
                _ => None,
            }
        }
    }
    let mut p = plan();
    p.data[0].rows[0][1] = Cell::Secret(SecretRef::new("PW").unwrap());
    p.data[0].rows[1][1] = Cell::Secret(SecretRef::new("KEY").unwrap());
    p.data[0].rows.push(vec![
        Cell::Literal("x".into()),
        Cell::Secret(SecretRef::new("SHORT").unwrap()),
    ]);
    p.data[0].rows.push(vec![
        Cell::Literal("y".into()),
        Cell::Secret(SecretRef::new("MISSING").unwrap()),
    ]);
    let redactor = SecretRedactor::for_plan(&p, &Keys);
    let text = secret_result().to_toml(&redactor);
    assert!(
        !text.contains("hunter2") && !text.contains("s3cr3t-key"),
        "{text}"
    );
    assert!(
        text.contains(
            "typed {{secret:PW}} and then {{secret:PW}}2 into the field; the key is {{secret:KEY}}"
        ),
        "{text}"
    );
    assert!(text.contains("build {{secret:PW}}"));
    // The unredacted writer keeps the text (for a result that holds no resolved value).
    assert!(secret_result().to_toml(&NoRedaction).contains("hunter2"));
    let junit = junit::write(&p, &secret_result(), &redactor);
    assert!(
        !junit.contains("hunter2") && !junit.contains("s3cr3t-key"),
        "{junit}"
    );
}

#[test]
fn the_longer_secret_wins_where_values_overlap() {
    let r = SecretRedactor::new([
        (SecretRef::new("A").unwrap(), "abc".into()),
        (SecretRef::new("B").unwrap(), "abcdef".into()),
    ]);
    assert_eq!(r.redact("x abcdef y abc"), "x {{secret:B}} y {{secret:A}}");
    assert_eq!(SecretRedactor::default().redact("abc"), "abc");
    assert_eq!(NoRedaction.redact("abc"), "abc");
}

fn run(started: &str, hash: &str, cases: &[(&str, Status, Option<u32>)]) -> RunResult {
    RunResult {
        plan: PlanId::new("settings-smoke").unwrap(),
        plan_hash: hash.to_owned(),
        run_id: "r".into(),
        started: Timestamp::new(started).unwrap(),
        env: Environment {
            os: "x".into(),
            ..Environment::default()
        },
        cases: cases
            .iter()
            .map(|(id, status, iteration)| CaseResult {
                case: CaseId::new(id).unwrap(),
                requirements: vec![],
                status: *status,
                iteration: *iteration,
                duration_ms: 1500,
                steps: vec![],
                evidence: vec![],
                reason: None,
            })
            .collect(),
    }
}

#[test]
fn statuses_roll_up_worst_first() {
    use Status::*;
    assert_eq!(Pass.worst(Fail), Fail);
    assert_eq!(Fail.worst(Pass), Fail);
    assert_eq!(Skipped.worst(Pass), Skipped);
    assert_eq!(Blocked.worst(NeedsRecording), Blocked);
    assert_eq!(NeedsRecording.worst(Skipped), NeedsRecording);
    for s in [Pass, Fail, Blocked, NeedsRecording, Skipped] {
        assert_eq!(Status::parse(s.as_str()), Some(s));
    }
    assert_eq!(Status::parse("PASS"), None);
    assert_eq!(
        EvidenceKind::parse("recording"),
        Some(EvidenceKind::Recording)
    );
    assert_eq!(EvidenceKind::parse("x"), None);
    assert_eq!(EvidenceKind::Screenshot.as_str(), "screenshot");
}

#[test]
fn coverage_is_requirement_by_case_by_last_status() {
    let p = plan();
    let h = p.hash();
    let old = run(
        "2026-10-24T08:00:00Z",
        &h,
        &[
            ("TC-1", Status::Fail, None),
            ("TC-2", Status::Pass, Some(0)),
            ("TC-2", Status::Pass, Some(1)),
        ],
    );
    let new = run(
        "2026-10-25T08:00:00Z",
        "ffff",
        &[("TC-1", Status::Pass, None)],
    );
    let mut other = run("2026-10-26T08:00:00Z", &h, &[("TC-1", Status::Fail, None)]);
    other.plan = PlanId::new("another").unwrap();
    // Input order does not matter: the later `started` decides.
    let m = Coverage::from(&p, &[new, other, old]);
    assert_eq!(m.rows.len(), 2);
    let r142 = &m.rows[0];
    assert_eq!(r142.requirement.as_str(), "REQ-142");
    assert_eq!(r142.cases.len(), 1);
    assert_eq!(r142.cases[0].status, Some(Status::Pass));
    assert!(
        r142.cases[0].stale,
        "the latest result is for another version of the plan"
    );
    assert_eq!(r142.status, Some(Status::Pass));
    assert!(!r142.uncovered());
    assert_eq!(r142.acceptance.len(), 2);
    assert_eq!(r142.acceptance[0].status, None, "no case is linked to AC-1");
    assert!(r142.acceptance[0].cases.is_empty());
    assert_eq!(r142.acceptance[1].status, Some(Status::Pass));
    assert_eq!(r142.acceptance[1].cases[0].as_str(), "TC-1");
    let r143 = &m.rows[1];
    assert_eq!(r143.cases[0].status, Some(Status::Pass));
    assert!(!r143.cases[0].stale);
}

#[test]
fn coverage_shows_what_never_ran_and_what_has_no_case() {
    let mut p = plan();
    p.requirements.push(Requirement {
        id: RequirementId::new("REQ-9").unwrap(),
        title: "Lonely".into(),
        acceptance: vec![],
        source: None,
    });
    let m = Coverage::from(&p, &[]);
    assert!(
        m.rows[0]
            .cases
            .iter()
            .all(|c| c.status.is_none() && !c.stale)
    );
    assert_eq!(m.rows[0].status, None);
    assert!(m.rows[2].uncovered());
    assert_eq!(m.rows[2].status, None);
    let failing = run(
        "2026-10-25T08:00:00Z",
        &p.hash(),
        &[
            ("TC-1", Status::Fail, None),
            ("TC-1", Status::Pass, Some(1)),
        ],
    );
    assert_eq!(
        Coverage::from(&p, &[failing]).rows[0].status,
        Some(Status::Fail),
        "the worst iteration decides"
    );
}

#[test]
fn diff_keeps_links_by_case_and_step() {
    let old = plan();
    assert!(diff(&old, &old).is_empty());
    assert_eq!(diff(&old, &old).unchanged.len(), 2);
    let mut new = old.clone();
    new.requirements[0].title = "Renamed".into();
    new.requirements.remove(1);
    new.requirements.push(Requirement {
        id: RequirementId::new("REQ-7").unwrap(),
        title: "New".into(),
        acceptance: vec![],
        source: None,
    });
    new.cases[0].title = "Dark theme".into();
    new.cases[0].steps[1] = Step::AssertScreen {
        expect_screen: ScreenName::new("Settings").unwrap(),
        timeout_ms: 9000,
    };
    new.cases[0].steps.remove(2);
    new.cases[0].steps.push(Step::Wait { ms: 5 });
    new.cases.remove(1);
    new.cases.push(Case {
        id: CaseId::new("TC-9").unwrap(),
        title: "New case".into(),
        requirements: vec![],
        priority: Priority::Low,
        tags: vec![],
        data: None,
        steps: vec![Step::Wait { ms: 1 }, Step::Press { keys: "Esc".into() }],
    });
    let d = diff(&old, &new);
    assert!(!d.is_empty());
    assert_eq!(
        d.requirements,
        [
            (RequirementId::new("REQ-142").unwrap(), ChangeKind::Changed),
            (RequirementId::new("REQ-7").unwrap(), ChangeKind::Added),
            (RequirementId::new("REQ-143").unwrap(), ChangeKind::Removed),
        ]
    );
    assert!(d.unchanged.is_empty());
    let changed = &d.cases[0];
    assert_eq!(
        (changed.case.as_str(), changed.kind, changed.meta_changed),
        ("TC-1", ChangeKind::Changed, true)
    );
    assert!(matches!(
        &changed.steps[0],
        StepChange::Kept { old: 0, new: 0 }
    ));
    assert!(
        matches!(&changed.steps[1], StepChange::Changed { old: 1, new: 1, from, to } if from.ends_with("timeout_ms=3000") && to.ends_with("timeout_ms=9000"))
    );
    assert!(matches!(
        &changed.steps[2],
        StepChange::Removed { old: 2, .. }
    ));
    assert!(matches!(
        &changed.steps[3],
        StepChange::Kept { old: 3, new: 2 }
    ));
    assert!(matches!(
        &changed.steps[4],
        StepChange::Added { new: 3, .. }
    ));
    let added = &d.cases[2];
    assert_eq!((added.kind, added.steps.len()), (ChangeKind::Removed, 5));
    assert!(matches!(
        &added.steps[0],
        StepChange::Removed { old: 0, .. }
    ));
    let new_case = &d.cases[1];
    assert_eq!(new_case.kind, ChangeKind::Added);
    assert!(
        matches!(&new_case.steps[1], StepChange::Added { new: 1, text } if text == "press \"Esc\"")
    );
}

#[test]
fn diff_aligns_inserted_and_deleted_steps() {
    let old = plan();
    let mut new = old.clone();
    new.cases[0].steps.insert(1, Step::Wait { ms: 7 });
    let d = diff(&old, &new);
    let steps = &d.cases[0].steps;
    assert!(!d.cases[0].meta_changed);
    assert!(matches!(steps[0], StepChange::Kept { old: 0, new: 0 }));
    assert!(matches!(&steps[1], StepChange::Added { new: 1, .. }));
    assert!(matches!(steps[2], StepChange::Kept { old: 1, new: 2 }));
    let back = diff(&new, &old);
    assert!(matches!(
        &back.cases[0].steps[1],
        StepChange::Removed { old: 1, .. }
    ));
    assert!(matches!(
        back.cases[0].steps[2],
        StepChange::Kept { old: 2, new: 1 }
    ));
}

#[test]
fn junit_carries_requirement_ids_and_maps_statuses() {
    let p = plan();
    let mut r = run(
        "2026-10-25T08:30:00Z",
        &p.hash(),
        &[
            ("TC-1", Status::Pass, None),
            ("TC-2", Status::Fail, Some(0)),
            ("TC-2", Status::Blocked, Some(1)),
            ("TC-2", Status::NeedsRecording, Some(2)),
            ("TC-2", Status::Skipped, Some(3)),
            ("ghost", Status::Pass, None),
        ],
    );
    r.cases[0].requirements = vec![
        ReqRef::new("REQ-142").unwrap(),
        ReqRef::new("REQ-142#AC-2").unwrap(),
    ];
    r.cases[1].reason = Some("a <b> & \"c\" 'd' \u{1}x\n\ty".into());
    r.cases[0].steps = vec![StepResult {
        index: 0,
        status: Status::Pass,
        screen_seen: None,
        score: None,
        evidence: vec![EvidenceRef {
            kind: EvidenceKind::Screenshot,
            path: RelPath::new("e/a.png").unwrap(),
            sha256: "0".repeat(64),
        }],
    }];
    r.cases[0].evidence = r.cases[0].steps[0].evidence.clone();
    let xml = junit::write(&p, &r, &NoRedaction);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(xml.contains("tests=\"6\" failures=\"1\" errors=\"1\" skipped=\"2\" time=\"9.000\" timestamp=\"2026-10-25T08:30:00Z\""), "{xml}");
    assert!(
        xml.contains("<property name=\"requirement\" value=\"REQ-142\"/>")
            && xml.contains("value=\"REQ-142#AC-2\"")
    );
    assert!(xml.contains("name=\"Switch to the dark theme\""));
    assert!(xml.contains("name=\"Sign in with each account [row 1]\""));
    assert!(
        xml.contains("name=\"ghost\""),
        "a case the plan lacks is named by its id"
    );
    assert!(
        xml.contains("<failure message=\"a &lt;b&gt; &amp; &quot;c&quot; &apos;d&apos; x\n\ty\"/>"),
        "{xml}"
    );
    assert!(
        xml.contains("<error message=\"\"/>")
            && xml.contains("<skipped message=\"needs recording. \"/>")
            && xml.contains("<skipped message=\"\"/>")
    );
    assert!(xml.contains("evidence screenshot: e/a.png") && xml.contains("step 1: pass"));
    assert!(xml.contains("<property name=\"plan_hash\"") && xml.ends_with("</testsuites>\n"));
}
