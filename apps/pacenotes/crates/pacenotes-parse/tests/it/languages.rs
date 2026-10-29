//! A golden outline per language, from generated source (no real code), in the order of ADR 0003. A golden is reviewed when
//! it is written (`PADDOCK_UPDATE_GOLDEN=1`); a later change shows up as a diff.

use std::path::Path;

use pacenotes_parse::ParseLanguage;

use crate::support::{outline, render};

fn golden(language: ParseLanguage, source: &str, name: &str) {
    let o = outline(language, source);
    assert!(!o.has_errors && !o.partial, "the generated {name} source parses cleanly");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/it/golden")
        .join(format!("{name}.txt"));
    paddock_testkit::assert_golden(&path, &(render(&o) + "\n"));
}

#[test]
fn go() {
    golden(ParseLanguage::Go, include_str!("languages_src/sample.go"), "go");
}

#[test]
fn java() {
    golden(ParseLanguage::Java, include_str!("languages_src/Sample.java"), "java");
}

#[test]
fn kotlin() {
    golden(ParseLanguage::Kotlin, include_str!("languages_src/sample.kt"), "kotlin");
}

#[test]
fn c() {
    golden(ParseLanguage::C, include_str!("languages_src/sample.c"), "c");
}

#[test]
fn cpp() {
    golden(ParseLanguage::Cpp, include_str!("languages_src/sample.cpp"), "cpp");
}

#[test]
fn csharp() {
    golden(ParseLanguage::CSharp, include_str!("languages_src/Sample.cs"), "csharp");
}

#[test]
fn typescript() {
    golden(ParseLanguage::TypeScript, include_str!("languages_src/sample.ts"), "typescript");
}

#[test]
fn javascript() {
    golden(ParseLanguage::JavaScript, include_str!("languages_src/sample.js"), "javascript");
}

#[test]
fn rust() {
    golden(ParseLanguage::Rust, include_str!("languages_src/sample.rs.txt"), "rust");
}

#[test]
fn python() {
    golden(ParseLanguage::Python, include_str!("languages_src/sample.py"), "python");
}

#[test]
fn tsx_reads_jsx_and_typescript_together() {
    let o = outline(
        ParseLanguage::Tsx,
        "export function View(p: { n: number }) {\n  return <div>{p.n}</div>;\n}\n",
    );
    assert!(!o.has_errors);
    assert_eq!(o.symbols.len(), 1);
    assert_eq!(o.symbols[0].qualified_name, "View");
}

#[test]
fn every_symbol_has_a_name_a_span_inside_the_file_and_a_qualified_name_ending_in_it() {
    for (language, source) in [
        (ParseLanguage::Go, include_str!("languages_src/sample.go")),
        (ParseLanguage::Java, include_str!("languages_src/Sample.java")),
        (ParseLanguage::Rust, include_str!("languages_src/sample.rs.txt")),
        (ParseLanguage::Python, include_str!("languages_src/sample.py")),
        (ParseLanguage::Cpp, include_str!("languages_src/sample.cpp")),
    ] {
        let lines = u32::try_from(source.lines().count()).unwrap();
        for s in &outline(language, source).symbols {
            assert!(
                !s.name.is_empty() && s.qualified_name.ends_with(&s.name),
                "{}: {s:?}",
                language.id()
            );
            assert!(
                s.span.start().line() >= 1 && s.span.end().line() <= lines + 1,
                "{}: {s:?}",
                language.id()
            );
        }
    }
}
