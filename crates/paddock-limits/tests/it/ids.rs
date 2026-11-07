use paddock_limits::{LimitId, Quantity, Scope, SettingPath, Unit};

#[test]
fn limit_id_accepts_dotted_lower_case() {
    assert_eq!(
        LimitId::new("import.record_bytes").as_str(),
        "import.record_bytes"
    );
    assert_eq!(
        LimitId::parse("plugin.memory").unwrap(),
        LimitId::new("plugin.memory")
    );
}

#[test]
fn limit_id_parse_refuses_bad_keys() {
    for bad in [
        "",
        ".a",
        "a.",
        "a..b",
        "A.b",
        "a b",
        "1a",
        "a.B",
        &"a".repeat(65),
    ] {
        assert!(LimitId::parse(bad).is_err(), "{bad:?}");
    }
    assert!(LimitId::parse("a").is_ok());
    assert!(LimitId::parse(&"a".repeat(64)).is_ok());
}

#[test]
fn parse_interns_so_equal_keys_are_equal() {
    let a = LimitId::parse("intern.same").unwrap();
    let b = LimitId::parse("intern.same").unwrap();
    assert_eq!(a, b);
    assert_eq!(format!("{a} {a:?}"), "intern.same LimitId(intern.same)");
}

#[test]
fn bad_key_texts_name_the_problem() {
    assert!(
        LimitId::parse("A")
            .unwrap_err()
            .to_string()
            .contains("dotted lower-case")
    );
}

#[test]
fn intern_table_is_bounded() {
    let mut refused = None;
    for i in 0..5000 {
        if let Err(e) = LimitId::parse(&format!("flood.k{i}")) {
            refused = Some(e);
            break;
        }
    }
    assert!(refused.is_some_and(|e| e.to_string().contains("too many")));
}

#[test]
fn setting_path_allows_camel_case() {
    assert_eq!(
        SettingPath::new("performance.pluginMemoryMb").as_str(),
        "performance.pluginMemoryMb"
    );
    assert_eq!(
        format!("{} {:?}", SettingPath::new("a.b"), SettingPath::new("a.b")),
        "a.b SettingPath(a.b)"
    );
}

#[test]
fn quantities_read_as_written() {
    let cases = [
        (Quantity::Bytes(1024 * 1024 * 1024), "1 GiB"),
        (Quantity::Bytes(1024 << 20), "1 GiB"),
        (Quantity::Bytes(1_536 << 20), "1,536 MiB"),
        (Quantity::Bytes(16 << 20), "16 MiB"),
        (Quantity::Bytes(64 << 10), "64 KiB"),
        (Quantity::Bytes(1000), "1,000 B"),
        (Quantity::Bytes(0), "0 B"),
        (Quantity::Count(1_204), "1,204"),
        (Quantity::Count(5), "5"),
        (Quantity::Count(2_000_000), "2,000,000"),
        (Quantity::Ratio(200), "200:1"),
        (Quantity::Millis(108_000), "108 s"),
        (Quantity::Millis(250), "250 ms"),
        (Quantity::Millis(1_500), "1,500 ms"),
        (Quantity::Percent(80), "80 %"),
    ];
    for (q, text) in cases {
        assert_eq!(q.to_string(), text);
    }
}

#[test]
fn quantity_units_and_comparison() {
    assert_eq!(Quantity::Bytes(1).unit(), Unit::Bytes);
    assert_eq!(Quantity::Count(1).unit(), Unit::Count);
    assert_eq!(Quantity::Ratio(1).unit(), Unit::Ratio);
    assert_eq!(Quantity::Millis(1).unit(), Unit::Millis);
    assert_eq!(Quantity::Percent(1).unit(), Unit::Percent);
    assert_eq!(Quantity::Ratio(7).amount(), 7);
    assert_eq!(Quantity::Percent(7).amount(), 7);
    assert!(Quantity::Count(1).at_most(Quantity::Count(2)));
    assert!(!Quantity::Count(3).at_most(Quantity::Count(2)));
    assert!(!Quantity::Count(1).at_most(Quantity::Bytes(2)));
}

#[test]
fn scope_words() {
    let all = [
        Scope::File,
        Scope::Source,
        Scope::Import,
        Scope::Plugin,
        Scope::Query,
        Scope::Process,
    ];
    let words: Vec<_> = all.iter().map(|s| s.word()).collect();
    assert_eq!(
        words,
        ["file", "source", "import", "plugin", "query", "process"]
    );
}
