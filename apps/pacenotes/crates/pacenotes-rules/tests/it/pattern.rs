//! Structural patterns.

use pacenotes_parse::ParseLanguage;
use pacenotes_rules::pattern::{MAX_PATTERN_BYTES, MAX_PLACEHOLDERS, MatchLimits, Pattern, PatternError};

fn find(language: ParseLanguage, pattern: &str, code: &str) -> Vec<(String, Vec<(String, String)>)> {
    let p = Pattern::compile(language, pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"));
    p.find_all(code.as_bytes(), &MatchLimits::default())
        .into_iter()
        .map(|m| (m.text, m.captures.into_iter().collect()))
        .collect()
}

#[test]
fn a_placeholder_matches_one_node_and_every_match_is_reported_with_its_captures() {
    let hits = find(
        ParseLanguage::Rust,
        "$X.unwrap()",
        "fn f() { a.unwrap(); b.c().unwrap(); d.expect(\"x\"); e.unwrap_or(1); }",
    );
    let texts: Vec<_> = hits.iter().map(|h| h.0.as_str()).collect();
    assert_eq!(texts, ["a.unwrap()", "b.c().unwrap()"]);
    assert_eq!(hits[1].1, [("X".to_owned(), "b.c()".to_owned())]);
}

#[test]
fn operators_and_other_tokens_must_match_and_layout_and_comments_do_not_matter() {
    assert_eq!(
        find(ParseLanguage::Rust, "$A + $B", "fn f() { let x = a + b; let y = a - b; }").len(),
        1
    );
    assert_eq!(
        find(ParseLanguage::Rust, "$X.unwrap()", "fn f() {\n  a\n    // note\n    .unwrap()\n}").len(),
        1
    );
}

#[test]
fn the_same_name_twice_must_match_the_same_text() {
    let hits = find(
        ParseLanguage::Rust,
        "$A == $A",
        "fn f() { if x == x {} if x == y {} if  a.b  ==  a.b {} }",
    );
    assert_eq!(hits.len(), 2, "{hits:?}");
}

#[test]
fn a_run_placeholder_matches_zero_or_more_siblings() {
    let hits = find(
        ParseLanguage::Rust,
        "log($$$ARGS)",
        "fn f() { log(); log(1); log(1, two(), 3); other(1); }",
    );
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[2].1, [("ARGS".to_owned(), "1, two(), 3".to_owned())]);
    let any = find(ParseLanguage::Rust, "run($_, $_)", "fn f() { run(1, 2); run(1); }");
    assert_eq!(any.len(), 1);
}

#[test]
fn the_languages_with_wrappers_and_without_compile_their_patterns() {
    assert_eq!(
        find(ParseLanguage::Python, "print($X)", "def f():\n    print(1)\n    other(2)\n").len(),
        1
    );
    assert_eq!(
        find(
            ParseLanguage::JavaScript,
            "console.log($X)",
            "function f() { console.log(1); console.error(2); }"
        )
        .len(),
        1
    );
    assert_eq!(
        find(
            ParseLanguage::TypeScript,
            "console.log($X)",
            "function f(): void { console.log(1); }"
        )
        .len(),
        1
    );
    assert_eq!(
        find(
            ParseLanguage::Go,
            "fmt.Println($$$A)",
            "package p\nfunc f() { fmt.Println(1, 2) }\n"
        )
        .len(),
        1
    );
    assert_eq!(
        find(
            ParseLanguage::Java,
            "System.out.println($X)",
            "class A { void m() { System.out.println(1); } }"
        )
        .len(),
        1
    );
    assert_eq!(
        find(
            ParseLanguage::CSharp,
            "Console.WriteLine($X)",
            "class A { void M() { Console.WriteLine(1); } }"
        )
        .len(),
        1
    );
    assert_eq!(find(ParseLanguage::Kotlin, "println($X)", "fun m() { println(1) }").len(), 1);
    assert_eq!(find(ParseLanguage::C, "free($X)", "void m(void) { free(p); }").len(), 1);
    assert_eq!(find(ParseLanguage::Cpp, "delete $X", "void m() { delete p; }").len(), 1);
}

#[test]
fn bad_patterns_are_refused_with_a_message_that_says_what_to_fix() {
    let rust = ParseLanguage::Rust;
    assert!(matches!(Pattern::compile(rust, "  "), Err(PatternError::Empty)));
    assert!(matches!(Pattern::compile(rust, "$X"), Err(PatternError::TooGeneral)));
    assert!(matches!(Pattern::compile(rust, "foo((("), Err(PatternError::Syntax { .. })));
    assert!(matches!(
        Pattern::compile(rust, &"a".repeat(MAX_PATTERN_BYTES + 1)),
        Err(PatternError::TooLong(_))
    ));
    let many = (0..=MAX_PLACEHOLDERS).map(|i| format!("$V{i}")).collect::<Vec<_>>().join(" + ");
    assert!(matches!(Pattern::compile(rust, &many), Err(PatternError::TooManyPlaceholders)));
    let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert!(matches!(Pattern::compile(rust, &deep), Err(PatternError::TooDeep)));
    assert!(
        Pattern::compile(rust, "foo((")
            .unwrap_err()
            .to_string()
            .contains("not valid rust code")
    );
}

#[test]
fn bounds_hold_on_hostile_input() {
    let p = Pattern::compile(ParseLanguage::Rust, "$X.unwrap()").unwrap();
    let code = format!("fn f() {{ {} }}", "a.unwrap(); ".repeat(5000));
    assert_eq!(
        p.find_all(
            code.as_bytes(),
            &MatchLimits {
                max_nodes: 500_000,
                max_matches: 10
            }
        )
        .len(),
        10
    );
    assert!(
        p.find_all(
            code.as_bytes(),
            &MatchLimits {
                max_nodes: 5,
                max_matches: 10
            }
        )
        .len()
            < 10
    );
    assert!(
        p.find_all(&[0xff, 0xfe, 0x00], &MatchLimits::default()).is_empty(),
        "bytes that are not text match nothing"
    );
    let nested = format!("{}x{}", "f(".repeat(2000), ")".repeat(2000));
    let _ = p.find_all(format!("fn g() {{ {nested} }}").as_bytes(), &MatchLimits::default());
    // a run placeholder next to another must not blow up: the step budget ends the attempt
    let q = Pattern::compile(ParseLanguage::Rust, "f($$$A, $$$B, $$$C, $$$D, x)").unwrap();
    let args = (0..40).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    let _ = q.find_all(format!("fn g() {{ f({args}, y); }}").as_bytes(), &MatchLimits::default());
}
