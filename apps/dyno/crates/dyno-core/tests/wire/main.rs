#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Every wire fixture of the UI contract, compared in one test target: `cargo test -p dyno-core --test wire`.
//!
//! Each `*.wire.json` under `app/src/app/core/` is the JSON the Rust types serialise to, and its `*.contract.spec.ts` holds the
//! same values as TypeScript. A fixture went stale twice because a wire type changed and only some of the comparisons ran;
//! now they are modules of this one target, so a run of `wire` is a run of all of them, and
//! [`every_wire_fixture_has_a_comparison`] fails for a fixture no module reads. After a deliberate change to a wire type or to
//! `protocol.rs`, regenerate them all with `scripts/regen-wire-fixtures.sh` (the pre-push hook runs it) and update the specs.

mod counters;
mod disk;
mod extras;
mod health;
mod jobs;
mod power;
mod read_via;
mod startup;
mod tray_host;
mod wake;
mod wol;

use std::path::PathBuf;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The wire fixture files of the app, by file name.
fn fixtures() -> Vec<String> {
    let dir = manifest().join("../../app/src/app/core");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".wire.json"))
        .collect();
    names.sort();
    names
}

/// The modules `main.rs` declares (`mod x;` lines): only these are compiled and run, so only these can compare a fixture.
fn declared_modules(main_src: &str) -> Vec<String> {
    main_src
        .lines()
        .filter_map(|l| l.trim().strip_prefix("mod ")?.strip_suffix(';'))
        .map(|m| m.trim().to_string())
        .collect()
}

/// The source of the declared modules among `files` (name without `.rs`, text); a file without its `mod` line is ignored.
fn declared_sources(main_src: &str, files: &[(String, String)]) -> String {
    let declared = declared_modules(main_src);
    files
        .iter()
        .filter(|(name, _)| declared.contains(name))
        .map(|(_, text)| text.as_str())
        .collect()
}

/// The fixtures that no declared module names.
fn unread<'a>(fixtures: &'a [String], main_src: &str, files: &[(String, String)]) -> Vec<&'a String> {
    let sources = declared_sources(main_src, files);
    fixtures.iter().filter(|f| !sources.contains(f.as_str())).collect()
}

/// Every `.rs` file of `tests/wire` as (name, text).
fn module_files() -> Vec<(String, String)> {
    std::fs::read_dir(manifest().join("tests/wire"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .map(|p| {
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(p).unwrap(),
            )
        })
        .collect()
}

#[test]
fn every_wire_fixture_has_a_comparison() {
    let found = fixtures();
    assert!(found.len() >= 10, "the probe sees the fixture folder: {found:?}");
    let unread = unread(&found, include_str!("main.rs"), &module_files());
    assert!(
        unread.is_empty(),
        "no declared module of tests/wire reads {unread:?}: add a module with a comparison and its `mod` line for each, so it is compared with the Rust types"
    );
}

#[test]
fn a_module_file_without_its_mod_line_does_not_count() {
    let files = vec![
        ("a".to_string(), "reads x.wire.json".to_string()),
        ("b".to_string(), "reads y.wire.json".to_string()),
    ];
    let fixtures = vec!["x.wire.json".to_string(), "y.wire.json".to_string()];
    // b.rs exists but is never compiled: its comparison never runs.
    assert_eq!(unread(&fixtures, "mod a;\nmod c;", &files), vec!["y.wire.json"]);
    assert!(unread(&fixtures, "mod a;\nmod b;", &files).is_empty());
}
