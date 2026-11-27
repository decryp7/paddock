//! The `syn` reads extractor with its positive controls (ADR 0117 §8).
#![allow(clippy::unwrap_used)]

use super::*;

fn paths(p: Parsed) -> Vec<(String, How)> {
    match p {
        Parsed::Reads(r) => r.into_iter().map(|r| (r.path, r.how)).collect(),
        Parsed::Fallback(e) => panic!("fallback: {e}"),
    }
}

#[test]
fn includes_module_paths_manifest_joins_concat_format_and_rerun_lines_are_reads() {
    let src = r#"
const WORDS: &str = include_str!("words.txt");
const F: &[u8] = include_bytes!("../../b/tests/fixtures/f.evtx");
const T: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/brief.md"));
#[path = "native/unix.rs"]
mod unix;
fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("recorded")
}
const TIMINGS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ci-runs/timings");
fn fixtures() -> String { format!("{}/tests/fixtures/check", env!("CARGO_MANIFEST_DIR")) }
#[test]
fn reads_in_a_macro() {
    assert!(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/inside-assert").exists());
}
fn main() { println!("cargo:rerun-if-changed=../ui/dist"); }
mod synth { include!("../../b/tests/it/support/synth.rs"); }
// include_str!("commented.txt")
const S: &str = "include_str!(\"in-a-string.txt\")";
"#;
    let got = paths(rust_reads("crates/a/src/lib.rs", src, "crates/a"));
    assert_eq!(
        got,
        [
            ("crates/a/src/native/unix.rs".to_string(), How::ModPath),
            ("crates/a/src/words.txt".to_string(), How::Include),
            ("crates/a/templates/brief.md".to_string(), How::Include),
            ("crates/a/tests/ci-runs/timings".to_string(), How::ManifestDir),
            ("crates/a/tests/fixtures/check".to_string(), How::ManifestDir),
            ("crates/a/tests/inside-assert".to_string(), How::ManifestDir),
            ("crates/a/tests/recorded".to_string(), How::ManifestDir),
            ("crates/b/tests/fixtures/f.evtx".to_string(), How::Include),
            ("crates/b/tests/it/support/synth.rs".to_string(), How::Include),
            ("crates/ui/dist".to_string(), How::Rerun),
        ]
    );
}

#[test]
fn a_literal_alone_reads_nothing_the_positive_control() {
    // The same literals without the macro, the attribute or the manifest folder are no read.
    let src = r#"
const WORDS: &str = "words.txt";
fn dir() -> std::path::PathBuf { std::path::Path::new("/somewhere").join("tests/recorded") }
const T: &str = concat!("a", "/templates/brief.md");
"#;
    assert!(paths(rust_reads("crates/a/src/lib.rs", src, "crates/a")).is_empty());
}

#[test]
fn a_source_syn_cannot_parse_falls_back() {
    assert!(matches!(rust_reads("x/src/lib.rs", "fn broken( {", "x"), Parsed::Fallback(_)));
}

/// The reads the extractor cannot resolve (ADR 0116 §2, audit U1-U13): each is a [`How::Blind`] read, so the crate is unresolved and
/// the test cache never reuses it.
fn blind(src: &str) -> Vec<String> {
    match rust_reads("crates/a/tests/it.rs", src, "crates/a") {
        Parsed::Reads(r) => r.into_iter().filter(|r| r.how == How::Blind).map(|r| r.path).collect(),
        Parsed::Fallback(e) => panic!("fallback: {e}"),
    }
}

/// The audit's sites, one fixture each, named by the shape that hid the read.
const SITES: &[(&str, &str)] = &[
    (
        "U1 manifest dir held in a variable",
        r#"fn t() { let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")); let p = root.join("docs/x.toml"); let _ = std::fs::read(p); }"#,
    ),
    (
        "U2 manifest dir behind a helper fn",
        r#"fn crate_dir() -> std::path::PathBuf { std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")) } fn t() { let _ = crate_dir().join("x"); }"#,
    ),
    (
        "U3 non-literal join",
        r#"fn t(name: &str) { let _ = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(name); }"#,
    ),
    (
        "U4 parent of the manifest dir",
        r#"fn t() { let _ = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("docs"); }"#,
    ),
    (
        "U5 a root join that climbs out of the crate to the repository root",
        r#"fn t() { let _ = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."); }"#,
    ),
    (
        "U6 ancestors walk up from a path",
        r#"fn t(p: &std::path::Path) { for a in p.ancestors() { let _ = a.join("docs/ipc-endpoints.toml"); } }"#,
    ),
    (
        "U7 current_dir",
        r#"fn t() { let _ = std::env::current_dir().unwrap().join("docs"); }"#,
    ),
    (
        "U8 manifest dir from the environment at run time",
        r#"fn t() { let d = std::env::var("CARGO_MANIFEST_DIR").unwrap(); let _ = std::fs::read_dir(d); }"#,
    ),
    (
        "U9 manifest path",
        r#"fn t() { let d = env!("CARGO_MANIFEST_PATH"); let _ = std::fs::read(d); }"#,
    ),
    (
        "U10 git ls-files",
        r#"fn t(prog: &str) { let _ = std::process::Command::new(prog).args(["ls-files", "-z"]).output(); }"#,
    ),
    (
        "U11 an array of segments joined later",
        r#"fn t() { let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")); let mut p = base.to_path_buf(); for s in ["a", "b"] { p = p.join(s); } let _ = p; }"#,
    ),
    (
        "U12 a manifest dir inside a macro that is not a path macro",
        r#"fn t() { my_macro!(env!("CARGO_MANIFEST_DIR"), "x"); }"#,
    ),
    (
        "U13 a manifest dir in a macro whose tokens do not parse as expressions",
        r#"fn t() { my_macro! { dir => "CARGO_MANIFEST_DIR"; => } }"#,
    ),
];

#[test]
fn every_audit_site_is_a_blind_read_the_positive_control() {
    for (name, src) in SITES {
        assert!(!blind(src).is_empty(), "{name}: not unresolved");
    }
}

#[test]
fn a_fully_resolved_literal_chain_is_not_blind_the_negative_control() {
    let src = r#"
fn t() {
    let _ = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("x").to_path_buf();
    let _ = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/a.txt"));
    let _ = format!("{}/tests/y", env!("CARGO_MANIFEST_DIR"));
    let _ = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../b/tests/z");
}
"#;
    assert!(blind(src).is_empty(), "{:?}", blind(src));
}

#[test]
fn a_zero_argument_method_is_not_the_identity() {
    // `.parent()`, `.canonicalize()` and `.with_file_name(..)` move the path; only `to_path_buf`/`as_path` keep it.
    for m in ["parent", "canonicalize", "with_file_name"] {
        let src = format!(r#"fn t() {{ let _ = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").{m}(); }}"#)
            .replace("with_file_name()", r#"with_file_name("y")"#);
        assert!(!blind(&src).is_empty(), "{m} treated as the identity");
    }
}

#[test]
fn deleting_the_guard_turns_the_controls_green_again_the_guard_is_load_bearing() {
    // The reads without the blind ones are what the extractor reported before the guard: for every site the plain reads are
    // empty or incomplete, so only the Blind read stands between the site and a cached pass (ER-33).
    for (name, src) in SITES {
        let all = match rust_reads("crates/a/tests/it.rs", src, "crates/a") {
            Parsed::Reads(r) => r,
            Parsed::Fallback(e) => panic!("fallback: {e}"),
        };
        let resolved = all.iter().filter(|r| r.how != How::Blind).count();
        let blind = all.iter().filter(|r| r.how == How::Blind).count();
        assert!(blind >= 1, "{name}");
        assert!(
            resolved == 0,
            "{name}: the site also resolved to {resolved} read(s); it is not a pure blind site"
        );
    }
}

#[test]
fn a_read_that_leaves_the_repository_is_blind_not_dropped() {
    let src = r#"fn t() { let _ = include_str!("../../../../../outside.txt"); }"#;
    assert!(!blind(src).is_empty());
}
