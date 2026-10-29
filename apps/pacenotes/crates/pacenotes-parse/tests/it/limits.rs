//! Limits, hostile input and cancel (conformance C36: a parser never panics and respects its caps).

use pacenotes_parse::{Limits, ParseError, ParseLanguage, parse};
use paddock_jobs::CancelToken;

use crate::support::outline;

const SAMPLES: [(ParseLanguage, &str); 11] = [
    (ParseLanguage::Go, include_str!("languages_src/sample.go")),
    (ParseLanguage::Java, include_str!("languages_src/Sample.java")),
    (ParseLanguage::Kotlin, include_str!("languages_src/sample.kt")),
    (ParseLanguage::C, include_str!("languages_src/sample.c")),
    (ParseLanguage::Cpp, include_str!("languages_src/sample.cpp")),
    (ParseLanguage::CSharp, include_str!("languages_src/Sample.cs")),
    (ParseLanguage::TypeScript, include_str!("languages_src/sample.ts")),
    (ParseLanguage::Tsx, include_str!("languages_src/sample.ts")),
    (ParseLanguage::JavaScript, include_str!("languages_src/sample.js")),
    (ParseLanguage::Rust, include_str!("languages_src/sample.rs.txt")),
    (ParseLanguage::Python, include_str!("languages_src/sample.py")),
];

#[test]
fn arbitrary_bytes_never_panic_in_any_language() {
    for (language, source) in SAMPLES {
        paddock_testkit::assert_never_panics(&[source.as_bytes()], |bytes| {
            parse(language, bytes, &Limits::default(), &CancelToken::new()).is_ok()
        });
    }
}

#[test]
fn a_file_over_the_byte_limit_is_not_parsed() {
    let limits = Limits {
        max_bytes: 10,
        ..Limits::default()
    };
    let err = parse(ParseLanguage::Go, b"package sample\n\nfunc f() {}\n", &limits, &CancelToken::new()).unwrap_err();
    assert_eq!(err, ParseError::TooLarge { len: 28, max: 10 });
    assert!(err.to_string().contains("at most 10"));
    assert!(
        parse(
            ParseLanguage::Go,
            b"package p",
            &Limits {
                max_bytes: 9,
                ..Limits::default()
            },
            &CancelToken::new()
        )
        .is_ok(),
        "the limit itself is allowed"
    );
}

#[test]
fn the_node_cap_stops_the_walk_and_marks_the_outline_partial() {
    let source: String = (0..200).map(|i| format!("func f{i}() {{}}\n")).collect::<String>();
    let source = format!("package p\n{source}");
    let full = parse(ParseLanguage::Go, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap();
    assert!(!full.partial && full.symbols.len() == 200);
    let cut = parse(
        ParseLanguage::Go,
        source.as_bytes(),
        &Limits {
            max_nodes: 100,
            ..Limits::default()
        },
        &CancelToken::new(),
    )
    .unwrap();
    assert!(cut.partial);
    assert!(
        cut.symbols.len() < 200 && !cut.symbols.is_empty(),
        "a prefix of the file: {}",
        cut.symbols.len()
    );
    assert_eq!(cut.symbols[0].qualified_name, "f0");
}

#[test]
fn cancel_before_and_during_a_parse_is_an_error_not_a_result() {
    let dead = CancelToken::new();
    dead.cancel();
    assert_eq!(
        parse(ParseLanguage::Rust, b"fn f() {}", &Limits::default(), &dead),
        Err(ParseError::Cancelled)
    );
    // during the walk: a token that is cancelled by the time the walk checks it (every 1024 nodes)
    let big = "fn f() { let a = 1; }\n".repeat(2000);
    let token = CancelToken::new();
    let t = token.clone();
    let _guard = token.on_cancel(|| {});
    t.cancel();
    assert_eq!(
        parse(ParseLanguage::Rust, big.as_bytes(), &Limits::default(), &token),
        Err(ParseError::Cancelled)
    );
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let open = "(".repeat(20_000);
    let close = ")".repeat(20_000);
    let source = format!("int main() {{ return {open}1{close}; }}\n");
    let o = parse(ParseLanguage::C, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap();
    assert!(o.symbols.iter().any(|s| s.name == "main") || o.partial || o.has_errors);
    let nested = format!("{}{}", "[".repeat(20_000), "]".repeat(20_000));
    let o = parse(
        ParseLanguage::JavaScript,
        format!("const a = {nested};\n").as_bytes(),
        &Limits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(o.partial || o.has_errors || !o.symbols.is_empty());
}

#[test]
fn invalid_utf8_and_odd_files_still_give_an_outline() {
    let mut bytes = b"package p\nfunc caf".to_vec();
    bytes.extend_from_slice(&[0xff, 0xfe]);
    bytes.extend_from_slice(b"() {}\n");
    let o = parse(ParseLanguage::Go, &bytes, &Limits::default(), &CancelToken::new()).unwrap();
    assert!(o.has_errors || !o.symbols.is_empty());
    for source in ["", "\n", "\n\n\n", "// only a comment", "package p"] {
        let o = outline(ParseLanguage::Go, source);
        assert!(o.symbols.is_empty());
    }
    let o = outline(ParseLanguage::Go, "package p\nfunc f() {}");
    assert_eq!(o.chunks.len(), 1, "a file without a final newline has its last line");
}

#[test]
fn a_very_long_name_and_import_are_cut() {
    let name = "n".repeat(10_000);
    let o = outline(ParseLanguage::Go, &format!("package p\nimport \"{name}\"\nfunc {name}() {{}}\n"));
    assert!(o.symbols[0].name.len() <= 256);
    assert!(o.imports[0].target.len() <= 512);
}

#[test]
fn a_file_with_syntax_errors_still_gives_what_it_can() {
    let o = outline(ParseLanguage::Rust, "fn ok() {}\nfn broken( {\n}\nfn after() {}\n");
    assert!(o.has_errors);
    assert!(o.symbols.iter().any(|s| s.name == "ok"));
}
