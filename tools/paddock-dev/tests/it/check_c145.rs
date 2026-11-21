//! C145 (`docs/lessons.md` keeps its format): the Rust host over fixture trees (the union-merge defect of 9ec43be3c and b9bf5fc4b planted
//! in a copy of the file's shape), the positive control, a missing file and this working tree. Rust only: there is no JavaScript twin.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, repo_root};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use std::path::Path;

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C145".into()]),
        files: None,
        today: "2026-11-21".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    }
}

fn tree_of(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    d
}

const GOOD: &str = "# Paddock lessons\n\nIntro.\n\n## One (2026-11-20)\n- a\n\n## Two\n- b\n";
/// What a union merge made twice: the blank line between two sections is gone, so `## Two` sits under the last line of `## One`.
const GLUED: &str = "# Paddock lessons\n\nIntro.\n\n## One (2026-11-20)\n- a\n## Two\n- b\n";

#[test]
fn c145_flags_a_heading_glued_to_the_section_above_as_a_warning() {
    let tree = tree_of(&[("docs/lessons.md", GLUED)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize, &str)> = r.findings.iter().map(|f| (f.file.as_str(), f.line, f.message.as_str())).collect();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].0, got[0].1), ("docs/lessons.md", 7), "{got:?}");
    assert!(got[0].2.contains("not preceded by a blank line"), "{got:?}");
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
    assert_eq!(r.warnings(), 1);
}

#[test]
fn c145_passes_the_good_file_and_flags_each_other_defect_once() {
    let good = tree_of(&[("docs/lessons.md", GOOD)]);
    let r = run_checks(&options(good.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    for (text, line, what) in [
        ("## One\n\nx\n", 1, "first line"),
        ("# T\n\n## A\n\nx\n\n## A\n\ny\n", 7, "twice"),
        ("# T\n\n## A\n\nx", 5, "newline"),
    ] {
        let tree = tree_of(&[("docs/lessons.md", text)]);
        let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
        let got: Vec<(usize, &str)> = r.findings.iter().map(|f| (f.line, f.message.as_str())).collect();
        assert_eq!(got.len(), 1, "{text:?}: {got:?}");
        assert_eq!(got[0].0, line, "{text:?}");
        assert!(got[0].1.contains(what), "{text:?}: {got:?}");
    }
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the glued heading.
#[test]
fn c145_control_sees_a_glued_heading() {
    let tree = tree_of(&[("docs/lessons.md", GOOD)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C145").expect("a run record").record;
    assert!(record.control, "the control found its planted heading");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the file": a whole-tree run that finds no lessons file did not look at anything, which is never a pass.
#[test]
fn c145_without_the_lessons_file_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "x\n## glued\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
    assert!(
        r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
        "{:?}",
        r.findings
    );
}

/// This working tree's own file is in the format (the real run, as the conformance job makes it).
#[test]
fn live_c145_the_real_lessons_file_has_no_finding() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C145 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert!(r.findings.is_empty(), "docs/lessons.md breaks its format: {:?}", r.findings);
}
