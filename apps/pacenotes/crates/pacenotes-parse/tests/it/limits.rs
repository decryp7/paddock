//! Limits, hostile input and cancel (conformance C36: a parser never panics and respects its caps).

use std::cell::Cell;

use pacenotes_parse::{Limits, ParseError, ParseLanguage, Stop, parse, parse_with};
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

/// Stops on the (n+1)th check, and counts the checks.
struct StopAfter {
    allowed: usize,
    checks: Cell<usize>,
}

impl Stop for StopAfter {
    fn stopped(&self) -> bool {
        self.checks.set(self.checks.get() + 1);
        self.checks.get() > self.allowed
    }
}

#[test]
fn cancel_at_every_check_point_of_the_parse_and_the_walk_is_an_error() {
    // enough nodes that the walk checks several times (every 1024 nodes) and the parser reports progress
    let source = "fn f() { let a = 1; }\n".repeat(1500);
    let never = StopAfter {
        allowed: usize::MAX,
        checks: Cell::new(0),
    };
    let ok = parse_with(ParseLanguage::Rust, source.as_bytes(), &Limits::default(), &never).unwrap();
    assert_eq!(ok.symbols.len(), 1500);
    let total = never.checks.get();
    assert!(total >= 4, "the parse and the walk both check: {total} checks");
    for allowed in 0..total {
        let stop = StopAfter {
            allowed,
            checks: Cell::new(0),
        };
        let r = parse_with(ParseLanguage::Rust, source.as_bytes(), &Limits::default(), &stop);
        assert_eq!(r, Err(ParseError::Cancelled), "stopping after {allowed} of {total} checks");
    }
    // a parser that was stopped mid-file is reset: the next parse on this thread is whole
    let again = parse(ParseLanguage::Rust, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap();
    assert_eq!(again.symbols.len(), 1500);
}

#[test]
fn deeply_nested_containers_are_capped_and_the_output_stays_small() {
    // 50,000 nested classes in about half a megabyte: unbounded, every qualified name would hold all its ancestors
    let n = 50_000;
    let source = format!("{}{}", "class A{".repeat(n), "}".repeat(n));
    assert!(source.len() > 400_000 && source.len() < Limits::default().max_bytes);
    let o = parse(ParseLanguage::Java, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap();
    assert!(o.partial, "symbols below the depth cap were skipped, and the outline says so");
    assert!(
        o.symbols.len() <= pacenotes_parse::extract::MAX_CONTAINER_DEPTH + 1,
        "{} symbols",
        o.symbols.len()
    );
    let bytes: usize = o.symbols.iter().map(|s| s.qualified_name.len()).sum();
    assert!(
        bytes <= o.symbols.len() * pacenotes_parse::extract::MAX_QUALIFIED_BYTES,
        "{bytes} bytes of names"
    );
    assert!(
        o.symbols
            .iter()
            .all(|s| s.depth as usize <= pacenotes_parse::extract::MAX_CONTAINER_DEPTH)
    );
}

#[test]
fn a_qualified_name_is_cut_on_a_character_boundary() {
    let name = "é".repeat(120);
    let mut source = String::new();
    for _ in 0..12 {
        source.push_str(&format!("class {name} {{ "));
    }
    source.push_str("void m() {}");
    source.push_str(&"}".repeat(12));
    let o = outline(ParseLanguage::Java, &source);
    let longest = o.symbols.iter().map(|s| s.qualified_name.len()).max().unwrap();
    assert!(longest <= pacenotes_parse::extract::MAX_QUALIFIED_BYTES, "{longest}");
    assert!(o.symbols.iter().all(|s| std::str::from_utf8(s.qualified_name.as_bytes()).is_ok()));
}

#[test]
fn symbols_and_calls_below_a_function_inherit_it_without_rescanning() {
    // methods are methods, locals inside them are skipped, and calls name the right caller, at any depth
    let mut source = String::from("class Outer {\n");
    for i in 0..300 {
        source.push_str(&format!("  void m{i}() {{ helper(); }}\n"));
    }
    source.push_str("}\n");
    let o = outline(ParseLanguage::Java, &source);
    assert_eq!(
        o.symbols.iter().filter(|s| s.kind == pacenotes_core::SymbolKind::Method).count(),
        300
    );
    assert_eq!(o.calls.len(), 300);
    assert_eq!(o.calls[299].caller.as_deref(), Some("Outer.m299"));
}

#[test]
fn the_per_thread_parser_is_reused_and_threads_do_not_share_one() {
    let source = include_str!("languages_src/sample.go");
    let first = parse(ParseLanguage::Go, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap();
    let handles: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(move || parse(ParseLanguage::Go, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap()))
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), first);
    }
    // the same thread parsing a different language and coming back gives the same outline
    let other = parse(
        ParseLanguage::Python,
        include_str!("languages_src/sample.py").as_bytes(),
        &Limits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(!other.symbols.is_empty());
    assert_eq!(
        parse(ParseLanguage::Go, source.as_bytes(), &Limits::default(), &CancelToken::new()).unwrap(),
        first
    );
}
