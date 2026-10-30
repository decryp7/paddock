//! Structural patterns.

use pacenotes_parse::ParseLanguage;
use pacenotes_rules::pattern::{MAX_PATTERN_BYTES, MAX_PLACEHOLDERS, MatchLimits, Pattern, PatternError, Truncated};
use paddock_jobs::CancelToken;

fn find(language: ParseLanguage, pattern: &str, code: &str) -> Vec<(String, Vec<(String, String)>)> {
    let p = Pattern::compile(language, pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"));
    p.find_all(code.as_bytes(), &MatchLimits::default(), &CancelToken::new())
        .matches
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
fn bounds_hold_on_hostile_input_counted_in_steps_and_nodes_not_seconds() {
    let p = Pattern::compile(ParseLanguage::Rust, "$X.unwrap()").unwrap();
    let code = format!("fn f() {{ {} }}", "a.unwrap(); ".repeat(5000));
    let live = CancelToken::new();
    let found = p.find_all(
        code.as_bytes(),
        &MatchLimits {
            max_matches: 10,
            ..MatchLimits::default()
        },
        &live,
    );
    assert_eq!((found.matches.len(), found.truncated), (10, Some(Truncated::Matches)));
    let found = p.find_all(
        code.as_bytes(),
        &MatchLimits {
            max_nodes: 5,
            ..MatchLimits::default()
        },
        &live,
    );
    assert_eq!((found.matches.len() < 10, found.truncated), (true, Some(Truncated::Nodes)));
    // the whole file shares one step budget, however many attempts it holds
    let found = p.find_all(
        code.as_bytes(),
        &MatchLimits {
            max_steps: 100,
            ..MatchLimits::default()
        },
        &live,
    );
    assert_eq!(found.truncated, Some(Truncated::Steps));
    assert!(found.matches.len() < 100);
    let found = p.find_all(&vec![b' '; 2 * 1024 * 1024 + 1], &MatchLimits::default(), &live);
    assert_eq!(
        found.truncated,
        Some(Truncated::TooLarge {
            len: 2 * 1024 * 1024 + 1,
            max: 2 * 1024 * 1024
        })
    );
    assert!(
        p.find_all(&[0xff, 0xfe, 0x00], &MatchLimits::default(), &live).matches.is_empty(),
        "bytes that are not text match nothing"
    );
    // a cancel ends the walk (checked every 256 nodes)
    let stopped = CancelToken::new();
    stopped.cancel();
    assert_eq!(
        p.find_all(code.as_bytes(), &MatchLimits::default(), &stopped).truncated,
        Some(Truncated::Cancelled)
    );
    // deep nesting and runs next to runs end by the step budget of the attempt
    let nested = format!("{}x{}", "f(".repeat(2000), ")".repeat(2000));
    let _ = p.find_all(format!("fn g() {{ {nested} }}").as_bytes(), &MatchLimits::default(), &live);
    let q = Pattern::compile(ParseLanguage::Rust, "f($$$A, $$$B, $$$C, $$$D, x)").unwrap();
    let args = (0..40).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    let found = q.find_all(
        format!("fn g() {{ f({args}, y); }}").as_bytes(),
        &MatchLimits {
            max_steps: 50_000,
            ..MatchLimits::default()
        },
        &live,
    );
    assert!(found.matches.is_empty());
}

#[test]
fn member_access_patterns_work_in_java_and_csharp_and_tsx_compiles() {
    assert_eq!(
        find(ParseLanguage::Java, "$X.length", "class A { int m(int[] a) { return a.length; } }").len(),
        1
    );
    assert_eq!(
        find(
            ParseLanguage::CSharp,
            "$X.Length",
            "class A { int M(int[] a) { return a.Length; } }"
        )
        .len(),
        1
    );
    assert_eq!(find(ParseLanguage::Rust, "$X.len", "fn f() { let n = v.len; }").len(), 1);
    assert_eq!(
        find(
            ParseLanguage::Tsx,
            "console.log($X)",
            "function F() { console.log(1); return <div/>; }"
        )
        .len(),
        1
    );
}

#[test]
fn tokens_are_compared_not_text_and_a_dollar_in_a_string_is_literal() {
    let hits = find(ParseLanguage::Rust, "$A == $A", "fn f() { if a.b == a . b {} if a.b == a.c {} }");
    assert_eq!(hits.len(), 1, "{hits:?}");
    let lit = find(ParseLanguage::Rust, "say(\"$X\")", "fn f() { say(\"$X\"); say(\"hello\"); }");
    assert_eq!(lit.len(), 1, "a placeholder inside a string matches only itself: {lit:?}");
    assert_eq!(find(ParseLanguage::Python, "say('$X')", "say('$X')\nsay('hi')\n").len(), 1);
}

#[test]
fn a_pattern_is_one_statement_or_expression() {
    let err = Pattern::compile(ParseLanguage::Rust, "a(); b();").unwrap_err();
    assert!(
        matches!(err, PatternError::Syntax { .. }) && err.to_string().contains("one statement or one expression"),
        "{err}"
    );
}

/// A measurement helper, not a test (rule 48): `cargo test --release -p pacenotes-rules --test it -- --ignored --nocapture measure_matcher`.
/// Times a typical rule over a realistic file, and the hostile cases against their budgets.
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_matcher() {
    use std::time::Instant;
    let live = CancelToken::new();
    let file = (0..2000)
        .map(|i| format!("fn f{i}(a: Option<u32>) -> u32 {{ let b = a.unwrap(); let c = g(b, {i}); c + 1 }}\n"))
        .collect::<String>();
    println!("typical file: {} bytes", file.len());
    for (name, pattern) in [("unwrap", "$X.unwrap()"), ("plus", "$A + $B"), ("run g", "g($$$A)")] {
        let p = Pattern::compile(ParseLanguage::Rust, pattern).unwrap();
        let t = Instant::now();
        let found = p.find_all(
            file.as_bytes(),
            &MatchLimits {
                max_matches: 100_000,
                ..MatchLimits::default()
            },
            &live,
        );
        println!(
            "typical {name}: {:?} for {} matches ({:?})",
            t.elapsed(),
            found.matches.len(),
            found.truncated
        );
    }
    let q = Pattern::compile(ParseLanguage::Rust, "f($$$A, $$$B, $$$C, $$$D, x)").unwrap();
    let args = (0..40).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    let hostile = format!("fn g() {{ f({args}, y); }}");
    let t = Instant::now();
    let found = q.find_all(hostile.as_bytes(), &MatchLimits::default(), &live);
    println!("hostile run-against-run: {:?} ({:?})", t.elapsed(), found.truncated);
    let nested = format!("fn g() {{ {}x{} }}", "f(".repeat(2000), ")".repeat(2000));
    let p = Pattern::compile(ParseLanguage::Rust, "$X.unwrap()").unwrap();
    let t = Instant::now();
    let found = p.find_all(nested.as_bytes(), &MatchLimits::default(), &live);
    println!("hostile 2000-deep nesting: {:?} ({:?})", t.elapsed(), found.truncated);
}

/// The reviewer's shape: a repeated run placeholder against a call with many large arguments compared tokens without counting them.
/// Every token compared is a step now, so the search ends by its budget and says so.
#[test]
fn a_repeated_run_placeholder_cannot_escape_the_step_budget() {
    let arg = (0..100).map(|i| format!("v{i}")).collect::<Vec<_>>().join(" + ");
    let args = (0..60).map(|i| format!("({arg} + {i})")).collect::<Vec<_>>().join(", ");
    let code = format!("fn g() {{ f({args}, y); }}");
    let p = Pattern::compile(ParseLanguage::Rust, "f($$$A, $$$A, z)").unwrap();
    let found = p.find_all(
        code.as_bytes(),
        &MatchLimits {
            max_steps: 200_000,
            ..MatchLimits::default()
        },
        &CancelToken::new(),
    );
    assert!(found.matches.is_empty());
    assert_eq!(found.truncated, Some(Truncated::Steps), "the result must say it is partial");
}

#[test]
fn a_cancel_is_noticed_inside_a_match_attempt() {
    let arg = (0..100).map(|i| format!("v{i}")).collect::<Vec<_>>().join(" + ");
    let args = (0..60).map(|i| format!("({arg} + {i})")).collect::<Vec<_>>().join(", ");
    let code = format!("fn g() {{ f({args}, y); }}");
    let p = Pattern::compile(ParseLanguage::Rust, "f($$$A, $$$A, z)").unwrap();
    let stopped = CancelToken::new();
    stopped.cancel();
    let found = p.find_all(code.as_bytes(), &MatchLimits::default(), &stopped);
    assert_eq!(found.truncated, Some(Truncated::Cancelled));
}

/// Binding a run shares the sibling list instead of copying it, and the work is counted: a run over n arguments takes a number of
/// steps linear in n (a copy per binding would be quadratic work that no step counted).
#[test]
fn binding_a_run_is_linear_in_counted_steps() {
    let steps = |n: usize| {
        let args = (0..n).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        let p = Pattern::compile(ParseLanguage::Rust, "f($$$A, z)").unwrap();
        let found = p.find_all(
            format!("fn g() {{ f({args}, z); }}").as_bytes(),
            &MatchLimits::default(),
            &CancelToken::new(),
        );
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].captures["A"].split(", ").count(), n);
        found.steps
    };
    let (small, large) = (steps(500), steps(2000));
    assert!(large <= small * 5, "4x the arguments took {large} steps against {small}");
    assert!(large <= 2000 * 8, "{large} steps for 2000 arguments");
}

/// A step count cannot see a copy; the bytes allocated can. Binding a run shares the sibling list, so 4x the arguments allocate about 4x
/// the bytes (the parse, the matches), not 16x (a copy of every prefix is quadratic). A mutant that copies the prefix in `seq` fails this.
#[test]
fn binding_a_run_allocates_linearly_in_the_arguments() {
    let bytes = |n: usize| {
        let args = (0..n).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        let code = format!("fn g() {{ f({args}, z); }}");
        let p = Pattern::compile(ParseLanguage::Rust, "f($$$A, z)").unwrap();
        let (found, bytes) = crate::alloc::allocated_by(|| p.find_all(code.as_bytes(), &MatchLimits::default(), &CancelToken::new()));
        assert_eq!(found.matches.len(), 1);
        bytes
    };
    let (small, large) = (bytes(500), bytes(2000));
    assert!(large <= small * 5, "4x the arguments allocated {large} bytes against {small}");
}

/// What the Rust side of one search may allocate in the rebinding test below. tree-sitter allocates its tree through C `malloc`, which
/// the counting allocator cannot see, so what is counted is the matcher's own work: the sibling lists it builds (64,000 arguments are
/// 128,000 children, built once per node it descends into) and the captures. Measured: about 8.4 MB (2026-10-30). The budget is a flat
/// 16 MiB, twice that. The eager version, which collected the whole capture on every comparison, allocated 40 GB on this input, so the
/// margin against the bug it guards is more than three orders of magnitude.
const REBIND_BYTES_BUDGET: usize = 16 * 1024 * 1024;

/// Starting to compare an already bound run must not cost the size of the run: `f($$$A, $$$A)` against 64,000 arguments with a small step
/// budget ends by its steps, and neither the steps nor the bytes allocated run away.
#[test]
fn rebinding_a_run_is_bounded_by_steps_in_work_and_in_bytes() {
    let args = (0..64_000).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    let code = format!("fn g() {{ f({args}); }}");
    let p = Pattern::compile(ParseLanguage::Rust, "f($$$A, $$$A)").unwrap();
    let limits = MatchLimits {
        max_steps: 100_000,
        ..MatchLimits::default()
    };
    let (found, bytes) = crate::alloc::allocated_by(|| p.find_all(code.as_bytes(), &limits, &CancelToken::new()));
    assert_eq!(found.truncated, Some(Truncated::Steps));
    assert!(found.steps <= limits.max_steps + 1_000, "{} steps", found.steps);
    assert!(
        bytes <= REBIND_BYTES_BUDGET,
        "{bytes} bytes allocated against a budget of {REBIND_BYTES_BUDGET}"
    );
}
