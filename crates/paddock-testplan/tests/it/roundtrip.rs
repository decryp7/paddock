//! Properties: `parse(write(p)) == p` for generated plans and results, and no input makes the parsers panic.

use paddock_testplan::*;
use proptest::prelude::*;

fn id() -> impl Strategy<Value = String> {
    "[A-Za-z0-9._:/-]{1,12}"
}

fn text() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[ -~\\n\\t\u{e9}\u{65e5}]{0,24}")
        .unwrap()
        .prop_filter("no placeholders", |s| !s.contains("{{"))
}

fn target() -> impl Strategy<Value = Target> {
    (
        proptest::option::of(text()),
        proptest::option::of(text()),
        proptest::option::of(0usize..15),
        proptest::option::of(0u16..9),
    )
        .prop_map(|(t, n, c, nth)| {
            let class = c.map(|i| {
                [
                    "button",
                    "link",
                    "text_box",
                    "check_box",
                    "radio_button",
                    "combo_box",
                    "list_item",
                    "menu_item",
                    "tab",
                    "tree_item",
                    "table",
                    "label",
                    "image",
                    "window",
                    "other",
                ][i]
            });
            Target {
                text: t.or_else(|| Some("anchor".into())),
                name: n,
                class: class.map(|c| ControlClass::parse(c).unwrap()),
                inside: None,
                near: None,
                nth,
            }
        })
}

fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        text().prop_map(Value::Literal),
        Just(Value::Data(Column::new("c").unwrap())),
        "[A-Z0-9_]{1,10}".prop_map(|s| Value::Secret(SecretRef::new(&s).unwrap()))
    ]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        target().prop_map(|target| Step::Click { target }),
        (target(), value()).prop_map(|(target, value)| Step::Type { target, value }),
        "[A-Za-z+]{1,8}".prop_map(|keys| Step::Press { keys }),
        (target(), value()).prop_map(|(target, option)| Step::Select { target, option }),
        (target(), 0u32..100_000)
            .prop_map(|(target, timeout_ms)| Step::WaitFor { target, timeout_ms }),
        (0u32..100_000).prop_map(|timeout_ms| Step::AssertScreen {
            expect_screen: ScreenName::new("S").unwrap(),
            timeout_ms
        }),
        (proptest::option::of(target()), value())
            .prop_map(|(target, text)| Step::AssertText { target, text }),
        (0u32..100_000).prop_map(|ms| Step::Wait { ms }),
    ]
}

fn plan() -> impl Strategy<Value = Plan> {
    (
        id(),
        text(),
        proptest::collection::vec((text(), proptest::collection::vec(step(), 0..6)), 0..4),
        proptest::option::of(text()),
    )
        .prop_map(|(pid, title, cases, version)| Plan {
            id: PlanId::new(&pid).unwrap(),
            title,
            app: AppRef {
                name: "App".into(),
                version,
            },
            generated_by: None,
            requirements: vec![Requirement {
                id: RequirementId::new("R").unwrap(),
                title: "r".into(),
                acceptance: vec!["a".into()],
                source: None,
            }],
            screens: vec![ScreenExpect {
                name: ScreenName::new("S").unwrap(),
                title: None,
                contains: vec![],
            }],
            cases: cases
                .into_iter()
                .enumerate()
                .map(|(i, (title, steps))| Case {
                    id: CaseId::new(&format!("C{i}")).unwrap(),
                    title,
                    requirements: vec![ReqRef::new("R#AC-1").unwrap()],
                    priority: Priority::High,
                    tags: vec![],
                    data: Some(DataName::new("d").unwrap()),
                    steps,
                })
                .collect(),
            data: vec![DataTable {
                name: DataName::new("d").unwrap(),
                columns: vec![DataColumn {
                    name: Column::new("c").unwrap(),
                    secret: false,
                }],
                rows: vec![vec![Cell::Literal("v".into())]],
            }],
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn a_plan_reads_back_equal(p in plan()) {
        prop_assert!(p.validate().is_ok());
        let text = p.to_toml();
        let back = parse_plan(text.as_bytes()).unwrap();
        prop_assert_eq!(&back, &p);
        prop_assert_eq!(back.to_toml(), text);
    }

    #[test]
    fn a_result_reads_back_equal(reason in text(), score in proptest::option::of(0u32..=100), dur in 0u64..1_000_000, n in 0usize..4) {
        let r = RunResult {
            plan: PlanId::new("p").unwrap(),
            plan_hash: "a".repeat(64),
            run_id: "r1".into(),
            started: Timestamp::new("2026-10-25T08:30:00Z").unwrap(),
            env: Environment { os: reason.clone(), ..Environment::default() },
            cases: (0..n).map(|i| CaseResult {
                case: CaseId::new("c").unwrap(),
                requirements: vec![],
                status: Status::Fail,
                iteration: Some(i as u32),
                duration_ms: dur,
                steps: vec![StepResult { index: 0, status: Status::Pass, screen_seen: None, score: score.map(|s| s as f32 / 100.0), evidence: vec![] }],
                evidence: vec![],
                reason: Some(reason.clone()),
            }).collect(),
        };
        let text = r.to_toml(&NoRedaction);
        prop_assert_eq!(parse_result(text.as_bytes()).unwrap(), r);
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = parse_plan(&bytes);
        let _ = parse_result(&bytes);
    }

    #[test]
    fn mutated_fixtures_never_panic(cut in 0usize..2000, byte in any::<u8>()) {
        for fixture in [include_str!("../../fixtures/settings.sighterplan"), include_str!("../../fixtures/settings.sighterresult")] {
            let mut bytes = fixture.as_bytes().to_vec();
            let at = cut % bytes.len();
            bytes[at] = byte;
            let _ = parse_plan(&bytes);
            let _ = parse_result(&bytes);
            let _ = parse_plan(&bytes[..at]);
        }
    }
}
