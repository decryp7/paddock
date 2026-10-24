//! The plan format: parse, canonical writing, every refusal, the limits.

use paddock_testplan::*;

const PLAN: &str = include_str!("../../fixtures/settings.sighterplan");

fn parse(text: &str) -> Result<Plan, PlanError> {
    parse_plan(text.as_bytes())
}

fn err(text: &str) -> PlanError {
    parse(text).expect_err("must be refused")
}

/// The fixture with one text replaced (it must occur).
fn with(from: &str, to: &str) -> String {
    assert!(PLAN.contains(from), "fixture lacks {from}");
    PLAN.replacen(from, to, 1)
}

fn minimal(extra: &str) -> String {
    format!(
        "format = \"sighterplan\"\nversion = 1\nid = \"p\"\ntitle = \"t\"\n[app]\nname = \"a\"\n{extra}"
    )
}

fn case_with(steps: &str) -> String {
    minimal(&format!(
        "[[screens]]\nname = \"S\"\n[[cases]]\nid = \"c\"\ntitle = \"t\"\n{steps}"
    ))
}

#[test]
fn the_fixture_parses_and_every_part_is_typed() {
    let p = parse(PLAN).unwrap();
    assert_eq!(p.id.as_str(), "settings-smoke");
    assert_eq!(p.app.version.as_deref(), Some("2.4.0"));
    assert_eq!(p.generated_by.as_ref().unwrap().tool, "Pacenotes");
    assert_eq!(p.requirements.len(), 2);
    assert_eq!(p.requirements[0].acceptance.len(), 2);
    assert_eq!(
        p.requirements[0].source.as_ref().unwrap().as_str(),
        "https://tracker.example.test/issue/142"
    );
    assert_eq!(p.screens[0].contains, ["Notes", "Settings"]);
    let c = &p.cases[0];
    assert_eq!(c.priority, Priority::High);
    assert_eq!(c.requirements[1].to_string(), "REQ-142#AC-2");
    assert_eq!(c.steps.len(), 4);
    assert!(
        matches!(&c.steps[0], Step::Click { target } if target.class == Some(ControlClass::Button) && target.inside.as_ref().unwrap().as_str() == "Main")
    );
    assert!(
        matches!(&p.cases[1].steps[0], Step::Type { target, value: Value::Data(col) } if col.as_str() == "user" && target.near.is_some())
    );
    assert_eq!(p.cases[1].priority, Priority::Normal);
    assert_eq!(
        p.data[0].rows[0][1],
        Cell::Secret(SecretRef::new("ADA_PASSWORD").unwrap())
    );
    assert_eq!(
        p.case(&CaseId::new("TC-2").unwrap()).unwrap().title,
        "Sign in with each account"
    );
    assert!(p.case(&CaseId::new("nope").unwrap()).is_none());
}

#[test]
fn writing_is_canonical_and_reads_back_equal() {
    let p = parse(PLAN).unwrap();
    let text = p.to_toml();
    let again = parse(&text).unwrap();
    assert_eq!(again, p);
    assert_eq!(again.to_toml(), text, "a second write is byte-identical");
    assert_eq!(p.hash(), again.hash());
    assert_eq!(p.hash().len(), 64);
    let edited = parse(&with("Switch to the dark theme", "Switch to dark")).unwrap();
    assert_ne!(edited.hash(), p.hash());
}

#[test]
fn awkward_text_survives_a_round_trip() {
    let mut p = parse(PLAN).unwrap();
    p.title = "quote \" backslash \\ tab \t newline \n cr \r bell \u{7} del \u{7f} unicode é 日本"
        .to_owned();
    assert_eq!(parse(&p.to_toml()).unwrap().title, p.title);
}

#[test]
fn a_file_over_4_mib_is_refused_before_parsing() {
    let big = vec![b' '; MAX_FILE_BYTES + 1];
    assert_eq!(
        parse_plan(&big).unwrap_err(),
        PlanError::Limit {
            what: "file size in bytes".into(),
            max: MAX_FILE_BYTES
        }
    );
}

#[test]
fn invalid_utf8_names_the_position() {
    let mut bytes = b"format = \"sighterplan\"\ntitle = \"".to_vec();
    bytes.push(0xFF);
    assert!(matches!(
        parse_plan(&bytes).unwrap_err(),
        PlanError::Syntax {
            line: 2,
            col: 10,
            ..
        }
    ));
}

#[test]
fn a_byte_order_mark_is_accepted() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(PLAN.as_bytes());
    assert!(parse_plan(&bytes).is_ok());
}

#[test]
fn syntax_errors_carry_line_and_column() {
    let e = err("format = \"sighterplan\"\nversion = 1\nid = \n");
    assert!(matches!(e, PlanError::Syntax { line: 3, .. }), "{e:?}");
    assert!(e.to_string().starts_with("line 3"));
}

#[test]
fn a_missing_required_key_is_a_syntax_error() {
    assert!(matches!(
        err("format = \"sighterplan\"\nversion = 1\nid = \"p\"\n"),
        PlanError::Syntax { .. }
    ));
}

#[test]
fn unknown_keys_name_the_key_and_line_at_every_level() {
    for (text, key, line) in [
        (
            with(
                "title = \"Settings smoke test\"",
                "title = \"Settings smoke test\"\nbogus = 1",
            ),
            "bogus",
            5,
        ),
        (
            with(
                "name = \"Demo Notes\"",
                "name = \"Demo Notes\"\nextra = true",
            ),
            "extra",
            8,
        ),
        (
            with(
                "priority = \"high\"",
                "priority = \"high\"\ncolour = \"red\"",
            ),
            "colour",
            0,
        ),
    ] {
        match err(&text) {
            PlanError::UnknownKey { path, line: l } => {
                assert_eq!(path, key);
                if line > 0 {
                    assert_eq!(l, line);
                }
            }
            other => panic!("{other:?}"),
        }
    }
    let e = err(&with("ms = 250", "ms = 250\nnap = 1"));
    assert!(e.to_string().contains("unknown key `nap`"));
}

#[test]
fn format_and_version_are_checked_first() {
    assert_eq!(
        err("version = 1"),
        PlanError::WrongFormat {
            expected: "sighterplan",
            found: String::new()
        }
    );
    assert!(
        err("version = 1")
            .to_string()
            .contains("`format` is missing")
    );
    let e = err("format = \"sighterresult\"\nversion = 1");
    assert_eq!(
        e,
        PlanError::WrongFormat {
            expected: "sighterplan",
            found: "sighterresult".into()
        }
    );
    assert!(e.to_string().contains("expected here"));
    assert!(matches!(
        err("format = \"sighterplan\""),
        PlanError::BadField { .. }
    ));
}

#[test]
fn a_newer_or_older_major_is_refused_with_advice() {
    let e = err(&with("version = 1", "version = 2"));
    assert_eq!(
        e,
        PlanError::Version {
            found: 2,
            min: MIN_READABLE_VERSION,
            max: FORMAT_VERSION
        }
    );
    assert!(e.to_string().contains("needs Sighter/Pacenotes 2 or newer"));
    // The newer file is refused even though it also carries keys this reader does not know.
    assert!(matches!(
        err("format = \"sighterplan\"\nversion = 9\nfuture = true"),
        PlanError::Version { found: 9, .. }
    ));
    let old = err(&with("version = 1", "version = 0"));
    assert!(matches!(old, PlanError::Version { found: 0, .. }));
    assert!(old.to_string().contains("older"));
    assert!(matches!(
        err(&with("version = 1", "version = -4")),
        PlanError::Version { found: 0, .. }
    ));
    assert!(matches!(
        err(&with("version = 1", "version = 99999999999")),
        PlanError::Version {
            found: u32::MAX,
            ..
        }
    ));
    assert_eq!(std::hint::black_box(FORMAT_VERSION), 1);
}

#[test]
fn bad_ids_are_named() {
    for (from, to) in [
        ("id = \"settings-smoke\"", "id = \"has space\""),
        ("id = \"TC-1\"", "id = \"\""),
        ("id = \"REQ-143\"", "id = \"REQ#143\""),
        (
            "name = \"Settings\"\ncontains",
            "name = \"Sett\\u00efngs\"\ncontains",
        ),
    ] {
        assert!(
            matches!(err(&with(from, to)), PlanError::BadId { .. }),
            "{to}"
        );
    }
    let long = "x".repeat(65);
    let e = err(&with(
        "id = \"settings-smoke\"",
        &format!("id = \"{long}\""),
    ));
    assert!(matches!(&e, PlanError::BadId { kind: "plan id", value } if value.len() == 65));
    assert!(e.to_string().contains("not a valid plan id"));
    let huge = "y".repeat(500);
    assert!(
        matches!(err(&with("id = \"settings-smoke\"", &format!("id = \"{huge}\""))), PlanError::BadId { value, .. } if value.len() == 80)
    );
}

#[test]
fn id_types_accept_exactly_their_charset() {
    assert!(CaseId::new("a.b_c:d/e#f-g").is_ok());
    assert!(
        RequirementId::new("a#b").is_err(),
        "# is reserved for acceptance references"
    );
    assert!(Column::new("user_1").is_ok());
    assert!(Column::new("a-b").is_err());
    assert!(SecretRef::new("API_KEY_2").is_ok());
    assert!(SecretRef::new("api").is_err());
    assert_eq!(Tag::new("smoke").unwrap().to_string(), "smoke");
    assert_eq!(DataName::new("rows").unwrap().as_str(), "rows");
    assert_eq!(ScreenName::new("Main").unwrap().as_str(), "Main");
    assert_eq!(PlanId::new("p").unwrap().to_string(), "p");
    assert_eq!(std::hint::black_box(MAX_ID_LEN), 64);
}

#[test]
fn acceptance_and_requirement_references() {
    let a = AcceptanceRef::new("REQ-142#AC-3").unwrap();
    assert_eq!((a.requirement().as_str(), a.number()), ("REQ-142", 3));
    assert_eq!(a.to_string(), "REQ-142#AC-3");
    for bad in [
        "REQ-142",
        "REQ-142#AC-0",
        "REQ-142#AC-01",
        "REQ-142#X-1",
        "REQ-142#AC-",
        "#AC-1",
        "a b#AC-1",
        "R#AC-1#AC-2",
    ] {
        assert!(AcceptanceRef::new(bad).is_err(), "{bad}");
    }
    assert!(AcceptanceRef::new(&format!("{}#AC-1", "r".repeat(64))).is_ok());
    assert!(AcceptanceRef::new(&format!("{}#AC-1", "r".repeat(90))).is_err());
    let whole = ReqRef::new("REQ-1").unwrap();
    let part = ReqRef::new("REQ-1#AC-2").unwrap();
    assert_eq!(whole.requirement(), part.requirement());
    assert_eq!(
        (whole.to_string(), part.to_string()),
        ("REQ-1".to_owned(), "REQ-1#AC-2".to_owned())
    );
    assert!(ReqRef::new("REQ-1#nope").is_err());
}

#[test]
fn relative_paths_cannot_leave_the_folder() {
    assert_eq!(
        RelPath::new("evidence/a.png").unwrap().to_string(),
        "evidence/a.png"
    );
    for bad in [
        "",
        "/etc/passwd",
        "C:/x",
        "c:\\x",
        "a\\b",
        "../x",
        "a/../b",
        "a//b",
        "./a",
        "a/.",
        "a\0b",
        "a\nb",
    ] {
        let e = RelPath::new(bad).unwrap_err();
        assert!(matches!(e, PlanError::BadField { .. }), "{bad:?}");
    }
    assert!(RelPath::new(&"a/".repeat(600)).is_err());
    assert!(RelPath::new("..hidden/ok..").is_ok());
}

#[test]
fn timestamps_are_rfc_3339() {
    for ok in [
        "2026-10-25T08:30:00Z",
        "2026-10-25T08:30:00.123Z",
        "2026-10-25T08:30:00+02:00",
        "2026-10-25T23:59:60-11:30",
    ] {
        assert_eq!(Timestamp::new(ok).unwrap().to_string(), ok);
    }
    for bad in [
        "",
        "2026-10-25",
        "2026-10-25 08:30:00Z",
        "2026-13-25T08:30:00Z",
        "2026-00-25T08:30:00Z",
        "2026-10-32T08:30:00Z",
        "2026-10-25T24:30:00Z",
        "2026-10-25T08:60:00Z",
        "2026-10-25T08:30:61Z",
        "2026-10-25T08:30:00",
        "2026-10-25T08:30:00.Z",
        "2026-10-25T08:30:00+0200",
        "2026-10-25T08:30:00+24:00",
        "2026-10-25T08:30:00+02:60",
        "2026-10-25T08:30:00+0é:00",
        "20x6-10-25T08:30:00Z",
        "2026/10/25T08:30:00Z",
        "2026-10-25T08:30:00Zjunk",
    ] {
        assert!(Timestamp::new(bad).is_err(), "{bad}");
    }
}

#[test]
fn source_links_are_http_only() {
    assert!(SourceLink::new("http://x.test/a").is_ok());
    for bad in [
        "file:///etc/passwd",
        "https://a b",
        "ftp://x",
        "https://x\n",
    ] {
        assert!(SourceLink::new(bad).is_err(), "{bad}");
    }
    assert!(SourceLink::new(&format!("https://x/{}", "a".repeat(MAX_STRING))).is_err());
    assert!(matches!(
        err(&with(
            "source = \"https://tracker.example.test/issue/142\"",
            "source = \"/etc/passwd\""
        )),
        PlanError::BadField { .. }
    ));
}

#[test]
fn control_classes_and_priorities() {
    for name in [
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
    ] {
        assert_eq!(ControlClass::parse(name).unwrap().as_str(), name);
    }
    assert!(
        ControlClass::parse("slider")
            .unwrap_err()
            .to_string()
            .contains("expected one of")
    );
    assert!(matches!(
        err(&with("class = \"button\"", "class = \"slider\"")),
        PlanError::BadField { .. }
    ));
    for (p, name) in [
        (Priority::Low, "low"),
        (Priority::Normal, "normal"),
        (Priority::High, "high"),
        (Priority::Critical, "critical"),
    ] {
        let text = with("priority = \"high\"", &format!("priority = \"{name}\""));
        assert_eq!(parse(&text).unwrap().cases[0].priority, p);
        assert_eq!(p.as_str(), name);
    }
    assert!(matches!(
        err(&with("priority = \"high\"", "priority = \"urgent\"")),
        PlanError::BadField { .. }
    ));
    assert!(Priority::Low < Priority::Critical);
}

#[test]
fn values_parse_once_into_their_kind() {
    assert_eq!(
        Value::parse("plain").unwrap(),
        Value::Literal("plain".into())
    );
    assert_eq!(
        Value::parse("a } b").unwrap(),
        Value::Literal("a } b".into())
    );
    assert_eq!(
        Value::parse("{{data.user_1}}").unwrap().to_source(),
        "{{data.user_1}}"
    );
    assert_eq!(
        Value::parse("{{secret:API_KEY}}").unwrap().to_source(),
        "{{secret:API_KEY}}"
    );
    assert_eq!(Value::parse("x").unwrap().to_source(), "x");
    for bad in [
        "{{nope}}",
        "a {{data.x}}",
        "{{data.x}} b",
        "{{data.x}}{{data.y}}",
        "{{ data.x }}",
        "{{{{data.x}}}}",
        "{{data.x",
    ] {
        assert!(
            matches!(Value::parse(bad), Err(PlanError::BadValue { .. })),
            "{bad}"
        );
    }
    assert!(matches!(
        Value::parse("{{secret:lower}}"),
        Err(PlanError::BadSecret { .. })
    ));
    assert!(matches!(
        Value::parse("{{data.a-b}}"),
        Err(PlanError::BadId { .. })
    ));
    assert!(
        err(&with("option = \"Dark\"", "option = \"{{oops}}\""))
            .to_string()
            .contains("not a valid value")
    );
    assert!(
        err(&with("option = \"Dark\"", "option = \"{{secret:no\""))
            .to_string()
            .contains("not a valid value")
    );
    assert!(
        err(&with("option = \"Dark\"", "option = \"{{secret:no}}\""))
            .to_string()
            .contains("secret `no`")
    );
}

#[test]
fn steps_need_exactly_their_fields() {
    let e = err(&case_with("[[cases.steps]]\naction = \"click\"\n"));
    assert!(
        matches!(&e, PlanError::BadStep { case, index: 0, msg } if case == "c" && msg.contains("needs `target`"))
    );
    assert!(e.to_string().contains("step 1"));
    let e = err(&case_with(
        "[[cases.steps]]\naction = \"wait\"\nms = 5\ntext = \"x\"\n",
    ));
    assert!(matches!(&e, PlanError::BadStep { msg, .. } if msg.contains("does not take `text`")));
    let e = err(&case_with("[[cases.steps]]\naction = \"dance\"\n"));
    assert!(matches!(&e, PlanError::BadStep { msg, .. } if msg.contains("unknown action `dance`")));
    for (step, needs) in [
        ("action = \"type\"\ntarget = { text = \"x\" }", "value"),
        ("action = \"select\"\ntarget = { text = \"x\" }", "option"),
        (
            "action = \"wait_for\"\ntarget = { text = \"x\" }",
            "timeout_ms",
        ),
        (
            "action = \"assert_screen\"\nexpect_screen = \"S\"",
            "timeout_ms",
        ),
        ("action = \"assert_text\"", "text"),
        ("action = \"press\"", "keys"),
        ("action = \"wait\"", "ms"),
    ] {
        let e = err(&case_with(&format!("[[cases.steps]]\n{step}\n")));
        assert!(
            matches!(&e, PlanError::BadStep { msg, .. } if msg.contains(&format!("needs `{needs}`"))),
            "{step}: {e:?}"
        );
    }
}

#[test]
fn step_values_are_checked_against_the_whole_plan() {
    let ok = |step: &str| parse(&case_with(&format!("[[cases.steps]]\n{step}\n")));
    assert!(ok("action = \"click\"\ntarget = { class = \"button\" }").is_ok());
    assert!(ok("action = \"assert_screen\"\nexpect_screen = \"S\"\ntimeout_ms = 10").is_ok());
    assert!(matches!(
        ok("action = \"click\"\ntarget = { inside = \"S\" }"),
        Err(PlanError::BadStep { .. })
    ));
    assert!(matches!(
        ok("action = \"click\"\ntarget = { text = \"a\", inside = \"Nowhere\" }"),
        Err(PlanError::DanglingRef { kind: "screen", .. })
    ));
    assert!(matches!(
        ok("action = \"assert_screen\"\nexpect_screen = \"Nowhere\"\ntimeout_ms = 1"),
        Err(PlanError::DanglingRef { kind: "screen", .. })
    ));
    assert!(matches!(
        ok("action = \"press\"\nkeys = \"  \""),
        Err(PlanError::BadStep { .. })
    ));
    assert!(matches!(
        ok("action = \"wait\"\nms = 3600001"),
        Err(PlanError::Limit { .. })
    ));
    assert!(ok("action = \"wait\"\nms = 3600000").is_ok());
    assert!(matches!(
        ok("action = \"wait_for\"\ntarget = { text = \"a\" }\ntimeout_ms = 3600001"),
        Err(PlanError::Limit { .. })
    ));
    assert!(matches!(
        ok("action = \"type\"\ntarget = { text = \"a\" }\nvalue = \"{{data.x}}\""),
        Err(PlanError::DanglingRef { .. })
    ));
    assert!(matches!(
        ok("action = \"click\"\ntarget = { text = \"a\", near = { inside = \"S\" } }"),
        Err(PlanError::BadStep { .. })
    ));
    let deep = (0..10).fold("{ text = \"x\" }".to_owned(), |inner, _| {
        format!("{{ text = \"y\", near = {inner} }}")
    });
    assert!(matches!(
        ok(&format!("action = \"click\"\ntarget = {deep}")),
        Err(PlanError::Limit { .. })
    ));
}

#[test]
fn references_and_duplicates_are_checked() {
    assert!(matches!(
        err(&with(
            "requirements = [\"REQ-143\"]",
            "requirements = [\"REQ-999\"]"
        )),
        PlanError::DanglingRef {
            kind: "requirement",
            ..
        }
    ));
    assert!(matches!(
        err(&with("\"REQ-142#AC-2\"", "\"REQ-142#AC-3\"")),
        PlanError::DanglingRef {
            kind: "acceptance criterion",
            ..
        }
    ));
    assert!(matches!(
        err(&with("data = \"logins\"", "data = \"nope\"")),
        PlanError::DanglingRef {
            kind: "data table",
            ..
        }
    ));
    assert!(matches!(
        err(&with("{{data.user}}", "{{data.nope}}")),
        PlanError::DanglingRef { .. }
    ));
    assert!(matches!(
        err(&with("id = \"TC-2\"", "id = \"TC-1\"")),
        PlanError::Duplicate { kind: "case", .. }
    ));
    assert!(matches!(
        err(&with("id = \"REQ-143\"", "id = \"REQ-142\"")),
        PlanError::Duplicate {
            kind: "requirement",
            ..
        }
    ));
    assert!(matches!(
        err(&with(
            "name = \"Settings\"\ncontains",
            "name = \"Main\"\ncontains"
        )),
        PlanError::Duplicate { kind: "screen", .. }
    ));
    assert!(matches!(
        err(&with(
            "name = \"logins\"",
            "name = \"logins\"\ncolumns = []\n[[data]]\nname = \"logins\""
        )),
        PlanError::Syntax { .. }
            | PlanError::Duplicate { .. }
            | PlanError::UnknownKey { .. }
            | PlanError::BadField { .. }
    ));
    assert!(
        err(&with("id = \"TC-2\"", "id = \"TC-1\""))
            .to_string()
            .contains("declared twice")
    );
    assert!(
        err(&with("data = \"logins\"", "data = \"nope\""))
            .to_string()
            .contains("does not declare")
    );
}

#[test]
fn data_tables_are_checked() {
    let table = |body: &str| parse(&minimal(&format!("[[data]]\nname = \"t\"\n{body}")));
    assert!(table("columns = [{ name = \"a\" }]\nrows = [[\"1\"], [\"2\"]]").is_ok());
    assert!(table("columns = [{ name = \"a\" }]").is_ok());
    assert!(matches!(
        table("columns = []"),
        Err(PlanError::BadField { .. })
    ));
    assert!(matches!(
        table("rows = []"),
        Err(PlanError::BadField { .. })
    ));
    assert!(matches!(
        table("columns = [{ name = \"a\" }, { name = \"a\" }]"),
        Err(PlanError::Duplicate { kind: "column", .. })
    ));
    assert!(matches!(
        table("columns = [{ name = \"a\" }]\nrows = [[\"1\", \"2\"]]"),
        Err(PlanError::BadField { .. })
    ));
    assert!(matches!(
        table("columns = [{ name = \"a\", secret = true }]\nrows = [[\"not a name\"]]"),
        Err(PlanError::BadSecret { .. })
    ));
    assert!(matches!(
        table("columns = [{ name = \"a\" }]\nrows = [[\"x\"]]\nbogus = 1"),
        Err(PlanError::UnknownKey { .. })
    ));
    assert!(matches!(
        table("columns = [{ name = \"a-b\" }]"),
        Err(PlanError::BadId { .. })
    ));
    let cols = (0..=MAX_COLUMNS)
        .map(|i| format!("{{ name = \"c{i}\" }}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert!(matches!(
        table(&format!("columns = [{cols}]")),
        Err(PlanError::BadField { .. })
    ));
    let rows = vec!["[\"x\"]"; MAX_ROWS + 1].join(",");
    assert!(matches!(
        table(&format!("columns = [{{ name = \"a\" }}]\nrows = [{rows}]")),
        Err(PlanError::Limit { .. })
    ));
}

#[test]
fn built_plans_are_validated_too() {
    use paddock_testplan::{Cell as C, DataColumn, DataTable};
    let mut p = parse(PLAN).unwrap();
    p.data[0].rows[0][0] = C::Secret(SecretRef::new("X").unwrap());
    assert!(matches!(p.validate(), Err(PlanError::BadSecret { .. })));
    let mut p = parse(PLAN).unwrap();
    p.data[0].rows[0][1] = C::Literal("plain".into());
    assert!(matches!(p.validate(), Err(PlanError::BadSecret { .. })));
    let mut p = parse(PLAN).unwrap();
    p.data.push(DataTable {
        name: DataName::new("logins").unwrap(),
        columns: vec![DataColumn {
            name: Column::new("a").unwrap(),
            secret: false,
        }],
        rows: vec![],
    });
    assert!(matches!(p.validate(), Err(PlanError::Duplicate { .. })));
    let mut p = parse(PLAN).unwrap();
    p.title = "x".repeat(MAX_STRING + 1);
    assert!(matches!(p.validate(), Err(PlanError::Limit { .. })));
}

#[test]
fn counts_and_string_sizes_are_limited() {
    let long = "z".repeat(MAX_STRING + 1);
    for from in [
        "title = \"Settings smoke test\"",
        "name = \"Demo Notes\"",
        "version = \"2.4.0\"",
        "tool = \"Pacenotes\"",
        "title = \"The user can change the theme\"",
        "title = \"Switch to the dark theme\"",
        "\"Choosing Dark applies at once\"",
        "contains = [\"Appearance\"]",
        "title = \"Demo Notes\"",
        "keys = \"Enter\"",
        "option = \"Dark\"",
        "text = \"Welcome\"",
    ] {
        let to = from.replacen(from.split('"').nth(1).unwrap(), &long, 1);
        assert!(
            matches!(err(&with(from, &to)), PlanError::Limit { .. }),
            "{from}"
        );
    }
    let many = |n: usize| minimal(&"[[cases]]\nid = \"c\"\ntitle = \"t\"\n".repeat(n));
    assert!(matches!(
        parse(&many(MAX_CASES + 1)).unwrap_err(),
        PlanError::Limit { .. }
    ));
    let steps = "[[cases.steps]]\naction = \"wait\"\nms = 1\n".repeat(MAX_STEPS + 1);
    assert!(matches!(err(&case_with(&steps)), PlanError::Limit { .. }));
    let steps = "[[cases.steps]]\naction = \"wait\"\nms = 1\n".repeat(MAX_STEPS);
    assert!(parse(&case_with(&steps)).is_ok());
    let reqs = "[[requirements]]\nid = \"R\"\ntitle = \"t\"\n";
    assert!(parse(&minimal(reqs)).is_ok());
    assert!(matches!(
        parse(&minimal(&format!(
            "[[requirements]]\nid = \"R\"\ntitle = \"t\"\nacceptance = [{}]",
            vec!["\"a\""; MAX_ENTRIES + 1].join(",")
        )))
        .unwrap_err(),
        PlanError::Limit { .. }
    ));
    assert!(
        std::hint::black_box(MAX_STRING) == 4096 && std::hint::black_box(MAX_FILE_BYTES) == 4 << 20
    );
}

#[test]
fn the_secret_lint_warns_without_failing() {
    let p = parse(&case_with(
        "[[cases.steps]]\naction = \"type\"\ntarget = { name = \"Password\" }\nvalue = \"hunter2\"\n\
         [[cases.steps]]\naction = \"type\"\ntarget = { text = \"x\", near = { text = \"API token\" } }\nvalue = \"abc\"\n\
         [[cases.steps]]\naction = \"type\"\ntarget = { name = \"Password\" }\nvalue = \"{{secret:PW}}\"\n\
         [[cases.steps]]\naction = \"type\"\ntarget = { name = \"Name\" }\nvalue = \"Ada\"\n\
         [[cases.steps]]\naction = \"type\"\ntarget = { name = \"Card number\" }\nvalue = \"\"\n\
         [[data]]\nname = \"d\"\ncolumns = [{ name = \"card_number\" }, { name = \"api_key\", secret = true }, { name = \"user\" }]\n",
    ));
    let p = p.unwrap_or_else(|e| panic!("{e}"));
    let lints = lint(&p);
    assert_eq!(lints.len(), 3, "{lints:?}");
    assert!(lints[0].at.contains("step 1") && lints[0].message.contains("{{secret:NAME}}"));
    assert!(lints[1].at.contains("step 2"));
    assert!(lints[2].at.contains("card_number"));
    assert!(lint(&parse(PLAN).unwrap()).is_empty());
}

#[test]
fn secret_refs_lists_steps_and_cells() {
    let p = parse(&with(
        "value = \"{{data.pass}}\"",
        "value = \"{{secret:LOGIN_TOKEN}}\"",
    ))
    .unwrap();
    let names: Vec<_> = p.secret_refs().iter().map(ToString::to_string).collect();
    assert_eq!(names, ["ADA_PASSWORD", "GRACE_PASSWORD", "LOGIN_TOKEN"]);
}

#[test]
fn step_text_is_canonical_and_shows_secrets_as_placeholders() {
    let p = parse(PLAN).unwrap();
    let text: Vec<String> = p
        .cases
        .iter()
        .flat_map(|c| c.steps.iter().map(ToString::to_string))
        .collect();
    assert_eq!(text[0], "click text=\"Settings\" class=button inside=Main");
    assert_eq!(text[1], "assert_screen Settings timeout_ms=3000");
    assert_eq!(
        text[2],
        "select name=\"Theme\" class=combo_box nth=0 option=\"Dark\""
    );
    assert_eq!(text[3], "assert_text text=\"Dark\"");
    assert_eq!(
        text[4],
        "type name=\"User\" near=(text=\"Sign in\") value=\"{{data.user}}\""
    );
    assert_eq!(text[6], "press \"Enter\"");
    assert_eq!(text[7], "wait_for text=\"Welcome\" timeout_ms=5000");
    assert_eq!(text[8], "wait ms=250");
    let secret = parse(&with(
        "value = \"{{data.pass}}\"",
        "value = \"{{secret:PW}}\"",
    ))
    .unwrap();
    assert!(
        secret.cases[1].steps[1]
            .to_string()
            .ends_with("value=\"{{secret:PW}}\"")
    );
    let with_target = parse(&with(
        "text = \"Dark\"",
        "target = { name = \"Theme\" }\ntext = \"Dark\"",
    ))
    .unwrap();
    assert_eq!(
        with_target.cases[0].steps[3].to_string(),
        "assert_text name=\"Theme\" text=\"Dark\""
    );
    assert!(Target::default().is_empty());
}

#[test]
fn the_error_messages_all_tell_the_user_what_to_fix() {
    use PlanError::*;
    let all = [
        Syntax {
            line: 1,
            col: 2,
            msg: "m".into(),
        },
        UnknownKey {
            path: "k".into(),
            line: 3,
        },
        Version {
            found: 5,
            min: 1,
            max: 1,
        },
        Version {
            found: 0,
            min: 1,
            max: 1,
        },
        WrongFormat {
            expected: "sighterplan",
            found: String::new(),
        },
        WrongFormat {
            expected: "sighterplan",
            found: "x".into(),
        },
        Limit {
            what: "w".into(),
            max: 1,
        },
        BadId {
            kind: "case id",
            value: "v".into(),
        },
        BadSecret {
            value: "v".into(),
            msg: "m".into(),
        },
        BadValue { value: "v".into() },
        BadStep {
            case: "c".into(),
            index: 0,
            msg: "m".into(),
        },
        BadField {
            field: "f".into(),
            msg: "m".into(),
        },
        DanglingRef {
            kind: "screen",
            id: "i".into(),
        },
        Duplicate {
            kind: "case",
            id: "i".into(),
        },
    ];
    for e in all {
        let text = e.to_string();
        assert!(text.len() > 5 && !text.contains("{{{{"), "{text}");
        let _: &dyn std::error::Error = &e;
    }
}

#[test]
fn empty_optional_parts_read_back_equal() {
    let p = parse(&minimal("[[data]]\nname = \"t\"\ncolumns = [{ name = \"a\" }]\n[[cases]]\nid = \"c\"\ntitle = \"t\"\n")).unwrap();
    assert!(p.data[0].rows.is_empty() && p.cases[0].requirements.is_empty());
    assert_eq!(parse(&p.to_toml()).unwrap(), p);
}

#[test]
fn a_built_plan_with_too_many_entries_is_refused() {
    let mut p = parse(PLAN).unwrap();
    let screen = p.screens[0].clone();
    p.screens = vec![screen; MAX_ENTRIES + 1];
    assert!(matches!(p.validate(), Err(PlanError::Limit { .. })));
}
