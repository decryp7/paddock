//! The language tokenizers of the Rust host (rustc's lexer, tree-sitter-bash, tree-sitter-yaml): the 36 probes of the WP-10 batch 2
//! review (`tests/fixtures/lex/probes.json`), each with the answer the language gives: a marker is a waiver only where the language has a
//! comment. The JavaScript hand lexers these were compared with (`shell-lexer.mjs`, `yaml-lexer.mjs`, `inline-waiver.mjs`) were retired
//! with the last JS check that used them (pair P3, ADR 0022 amendment 2026-11-19); the Rust-against-JS comparisons of generated sources
//! went with them. The mask of Rust text is still compared with the JS `mask-rust.mjs` in `check.rs` (every tracked `.rs` file).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::inline_waiver::{CommentView, InlineWaiver, Kind};
use serde_json::Value;
use std::path::Path;

fn kind_of(lang: &str) -> Kind {
    match lang {
        "rust" => Kind::Rust,
        "shell" => Kind::Shell,
        _ => Kind::Yaml,
    }
}

#[test]
fn the_review_probes_give_the_language_answer() {
    let probes: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lex/probes.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(probes.len(), 36);
    let state = |w: InlineWaiver| if matches!(w, InlineWaiver::Reason(_)) { "reason" } else { "none" };
    for p in &probes {
        let (text, lang, tag, line) = (
            p["text"].as_str().unwrap(),
            p["lang"].as_str().unwrap(),
            p["tag"].as_str().unwrap(),
            p["line"].as_u64().unwrap() as usize,
        );
        let view = CommentView::of(text, kind_of(lang));
        assert_eq!(
            state(view.waiver(line, tag)),
            p["expect"].as_str().unwrap(),
            "{}: the language's answer",
            p["name"]
        );
    }
    // The control: a probe that expects a waiver exists and one that expects none, so the loop can fail both ways.
    assert!(probes.iter().any(|p| p["expect"] == "reason") && probes.iter().any(|p| p["expect"] == "none"));
}
