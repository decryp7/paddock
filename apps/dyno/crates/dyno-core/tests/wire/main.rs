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

/// The source text of the comparison modules.
fn comparison_sources() -> String {
    let dir = manifest().join("tests/wire");
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

#[test]
fn every_wire_fixture_has_a_comparison() {
    let found = fixtures();
    assert!(found.len() >= 10, "the probe sees the fixture folder: {found:?}");
    let sources = comparison_sources();
    let unread: Vec<&String> = found.iter().filter(|f| !sources.contains(f.as_str())).collect();
    assert!(
        unread.is_empty(),
        "no module of tests/wire reads {unread:?}: add a module with a comparison (and `mod` line) for each, so it is compared with the Rust types"
    );
}
