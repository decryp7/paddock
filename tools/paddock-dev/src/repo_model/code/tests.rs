//! The code adapter with its positive controls (ADR 0117 §8).
#![allow(clippy::unwrap_used)]

use super::*;

fn tracked(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn read_paths_finds_include_and_manifest_dir_reads_and_the_control_finds_none_without_them() {
    let t = tracked(&[
        "crates/a/tests/recorded/x.txt",
        "crates/a/src/words.txt",
        "crates/b/tests/fixtures/f.evtx",
        "tools/paddock-dev/tests/ci-runs/timings/a.json",
    ]);
    let src = r#"
const WORDS: &str = include_str!("words.txt");
fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/recorded")
}
const F: &[u8] = include_bytes!("../../b/tests/fixtures/f.evtx");
// a comment naming "tests/recorded" is no read
let label = "tests/recorded";
"#;
    let got = read_paths("crates/a/src/lib.rs", src, "crates/a", &t);
    assert_eq!(
        got,
        tracked(&[
            "crates/a/src/words.txt",
            "crates/a/tests/recorded",
            "crates/b/tests/fixtures/f.evtx"
        ])
    );
    let concat = r#"const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ci-runs/timings");"#;
    assert_eq!(
        read_paths("tools/paddock-dev/src/ci_runs/timings.rs", concat, "tools/paddock-dev", &t),
        tracked(&["tools/paddock-dev/tests/ci-runs/timings"])
    );
    // Control: the same literal outside an include or a manifest-dir statement reads nothing.
    assert!(read_paths("crates/a/src/lib.rs", "let x = \"tests/recorded\";\n", "crates/a", &t).is_empty());
}

#[test]
fn literals_split_quoted_strings() {
    assert_eq!(literals(r#"a("x", "y\"z")"#), ["x", "y\"z"]);
}
