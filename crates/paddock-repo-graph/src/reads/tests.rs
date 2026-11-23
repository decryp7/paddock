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
            (
                "crates/a/tests/ci-runs/timings".to_string(),
                How::ManifestDir
            ),
            (
                "crates/a/tests/fixtures/check".to_string(),
                How::ManifestDir
            ),
            ("crates/a/tests/inside-assert".to_string(), How::ManifestDir),
            ("crates/a/tests/recorded".to_string(), How::ManifestDir),
            ("crates/b/tests/fixtures/f.evtx".to_string(), How::Include),
            (
                "crates/b/tests/it/support/synth.rs".to_string(),
                How::Include
            ),
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
    assert!(matches!(
        rust_reads("x/src/lib.rs", "fn broken( {", "x"),
        Parsed::Fallback(_)
    ));
}
