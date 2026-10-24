mod common;
use common::*;
use paddock_packs::{Limits, Origin, PackError, parse_pack};

fn parse(text: &str) -> Result<paddock_packs::Parsed<Vec<Category>>, PackError> {
    parse_pack::<Cleanup>(text.as_bytes(), Origin::User, &Limits::default())
}

#[test]
fn a_good_pack_parses_into_the_apps_value() {
    let p = parse(GOOD_CLEANUP).unwrap();
    assert_eq!(
        p.pack,
        vec![Category {
            id: "logs".into(),
            name: "Old logs".into(),
            paths: vec!["{cache}/logs".into()]
        }]
    );
    assert!(p.warnings.is_empty());
}

#[test]
fn unknown_keys_are_warnings_that_name_the_key() {
    let p = parse("format = 1\nextra = 1\n[[category]]\nid='a'\nname='A'\nflavour='x'\n").unwrap();
    assert_eq!(p.pack.len(), 1);
    let mut w = p.warnings.clone();
    w.sort();
    assert_eq!(
        w,
        [
            "unknown key \"category.0.flavour\" is ignored",
            "unknown key \"extra\" is ignored"
        ]
    );
}

#[test]
fn format_must_be_present_a_number_and_the_one_this_build_reads() {
    for (text, want) in [
        ("[[category]]\nid='a'\nname='A'\n", "\"format\" is missing"),
        ("format = \"1\"\n", "must be a whole number"),
        ("format = 2\n", "newer version"),
        ("format = 0\n", "older format"),
    ] {
        let e = parse(text).unwrap_err();
        assert!(matches!(e, PackError::Format(_)), "{e:?}");
        assert!(e.to_string().contains(want), "{e}");
    }
}

#[test]
fn syntax_errors_name_the_line_and_column() {
    let e = parse("format = 1\n[[category]\nid = 'a'\n").unwrap_err();
    assert!(matches!(e, PackError::Syntax(_)));
    assert!(e.to_string().contains("line 2"), "{e}");
}

#[test]
fn an_oversized_file_is_refused_before_it_is_parsed() {
    let limits = Limits {
        max_file_bytes: 64,
        ..Limits::default()
    };
    let big = format!("format = 1\n# {}\n", "x".repeat(200));
    let e = parse_pack::<Cleanup>(big.as_bytes(), Origin::User, &limits).unwrap_err();
    assert!(matches!(e, PackError::TooLarge { max: 64, .. }), "{e:?}");
    assert!(e.to_string().contains("too large"));
}

#[test]
fn a_file_that_is_not_text_is_refused() {
    let e = parse_pack::<Cleanup>(&[0xff, 0xfe, 0x00, 0x80], Origin::User, &Limits::default())
        .unwrap_err();
    assert_eq!(e, PackError::NotText);
}

#[test]
fn too_many_entries_a_long_string_deep_nesting_and_too_many_nodes_are_refused() {
    let many = format!(
        "format = 1\n{}",
        "[[category]]\nid='a'\nname='A'\n".repeat(6)
    );
    let tight = Limits {
        max_entries: 5,
        ..Limits::default()
    };
    let e = parse_pack::<Cleanup>(many.as_bytes(), Origin::User, &tight).unwrap_err();
    assert!(
        matches!(&e, PackError::Limit(m) if m.contains("category") && m.contains("6 entries")),
        "{e:?}"
    );

    let long = format!(
        "format = 1\n[[category]]\nid='a'\nname='{}'\n",
        "n".repeat(100)
    );
    let tight = Limits {
        max_string_len: 50,
        ..Limits::default()
    };
    let e = parse_pack::<Cleanup>(long.as_bytes(), Origin::User, &tight).unwrap_err();
    assert!(
        matches!(&e, PackError::Limit(m) if m.contains("category[0].name")),
        "{e:?}"
    );

    let long_key = format!("format = 1\n{} = 1\n", "k".repeat(100));
    let e = parse_pack::<Cleanup>(long_key.as_bytes(), Origin::User, &tight).unwrap_err();
    assert!(
        matches!(&e, PackError::Limit(m) if m.contains("a key")),
        "{e:?}"
    );

    let deep = "format = 1\na = [[[[[[[[[[[[[[[[1]]]]]]]]]]]]]]]]\n";
    let e = parse_pack::<Cleanup>(deep.as_bytes(), Origin::User, &Limits::default()).unwrap_err();
    assert!(
        matches!(&e, PackError::Limit(m) if m.contains("nested")),
        "{e:?}"
    );

    let nodes = Limits {
        max_nodes: 10,
        ..Limits::default()
    };
    let e = parse_pack::<Cleanup>(many.as_bytes(), Origin::User, &nodes).unwrap_err();
    assert!(
        matches!(&e, PackError::Limit(m) if m.contains("more than 10 values")),
        "{e:?}"
    );
}

#[test]
fn a_wrong_shape_is_refused_with_the_parsers_words() {
    let e = parse("format = 1\n[[category]]\nid = 5\nname = 'A'\n").unwrap_err();
    assert!(matches!(e, PackError::Shape(_)), "{e:?}");
    let e = parse("format = 1\n[[category]]\nname = 'A'\n").unwrap_err();
    assert!(e.to_string().contains("id"), "{e}");
    let e = parse("format = 1\n[[category]]\nid='a'\nname='A'\nrisk='wild'\n").unwrap_err();
    assert!(matches!(e, PackError::Shape(_)));
}

#[test]
fn the_apps_rules_see_who_wrote_the_pack() {
    let safe = "format = 1\n[[category]]\nid='a'\nname='A'\nrisk='safe'\n";
    assert!(matches!(parse(safe), Err(PackError::Rejected(m)) if m.contains("cannot mark")));
    assert!(parse_pack::<Cleanup>(safe.as_bytes(), Origin::Builtin, &Limits::default()).is_ok());
}

#[test]
fn nothing_in_a_pack_is_executed_or_followed() {
    // An include-looking key, a path and shell-looking text are data: ignored with a warning, never opened or run.
    let p = parse("format = 1\ninclude = '/etc/passwd'\nrun = '$(touch /tmp/x)'\n").unwrap();
    assert!(p.pack.is_empty());
    assert_eq!(p.warnings.len(), 2);
}

#[test]
fn error_messages_read_as_sentences() {
    assert_eq!(
        PackError::NotText.to_string(),
        "the pack file is not UTF-8 text"
    );
    assert!(
        PackError::Io("could not read a.toml".into())
            .to_string()
            .contains("a.toml")
    );
    assert!(
        PackError::TooLarge { len: 9, max: 4 }
            .to_string()
            .contains("limit is 4")
    );
}
