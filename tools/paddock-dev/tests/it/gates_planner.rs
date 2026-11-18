//! C100 asks `./dev plan`'s own planner, in this process, and never a script (ADR 0022 R4; the user retired `affected.mjs`
//! on 2026-11-18). The JS planner used to answer here; a repository whose `scripts/affected-marks.mjs` answers differently shows
//! which one decides.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::gates::edge;
use paddock_dev::gates::gating::Planner;

#[test]
fn a_script_that_answers_differently_does_not_decide_what_c100_reads() {
    let (_keep, root) = crate::golden::fixture_repo();
    // A script of the old kind that disagrees with the planner on purpose: it says a change of crates/a marks only the ui.
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(
        root.join("scripts/affected-marks.mjs"),
        "process.stdout.write(JSON.stringify({'crates/a/Cargo.toml': ['ui']}));\n",
    )
    .unwrap();
    let planner = edge::planner(&root);
    planner.prefetch(&["crates/a/Cargo.toml".to_string(), "ui/package.json".to_string()]);
    assert_eq!(planner.failure(), None);
    let marks = planner.marks("crates/a/Cargo.toml").expect("an answer");
    assert!(
        marks.contains("crates"),
        "the Rust planner marks the crates build for a crate manifest: {marks:?}"
    );
    assert!(!marks.contains("ui"), "the script said ui; the planner does not: {marks:?}");
}

#[test]
fn the_planner_needs_no_script_and_no_node() {
    let (_keep, root) = crate::golden::fixture_repo();
    assert!(
        !root.join("scripts/affected-marks.mjs").exists(),
        "a positive control: the repository has no script"
    );
    let planner = edge::planner(&root);
    planner.prefetch(&["crates/a/Cargo.toml".to_string()]);
    assert_eq!(planner.failure(), None, "no script, still an answer");
    assert!(planner.marks("crates/a/Cargo.toml").is_some_and(|m| m.contains("crates")));
}
