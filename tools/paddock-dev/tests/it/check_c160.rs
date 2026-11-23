//! C160 (every ci.yml input is in the declared run scope or non-narrowing): through the host, on a ci.yml with an input the table does not name.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};

/// `(file, line, unwaivable)` of every finding of C160 on a tree of `files`.
fn found(files: &[(&str, &str)]) -> Vec<(String, usize, bool)> {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    let opts = RunOptions {
        root: d.path().to_path_buf(),
        config: paddock_dev::check::config::Config::bare("fixture"),
        scope: None,
        only: Some(vec!["C160".into()]),
        files: None,
        today: "2026-11-23".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    };
    let r = run_checks(&opts, &registry()).unwrap();
    r.findings.iter().map(|f| (f.file.clone(), f.line, f.unwaivable)).collect()
}

const CI: &str = ".gitea/workflows/ci.yml";

#[test]
fn an_input_the_declaration_does_not_name_is_a_finding_and_the_declared_ones_are_clean() {
    let clean = "name: ci\non:\n  workflow_dispatch:\n    inputs:\n      base:\n        default: \"\"\n      since_run:\n        default: \"\"\n  workflow_call:\n    inputs:\n      webkit:\n        type: boolean\n";
    let f = found(&[(CI, clean)]);
    // The control tree has none of the other declared inputs, so the table's other entries read as stale at the table's file only
    // on a real tree; in memory the stale entries are not reported.
    assert!(f.iter().all(|(file, _, _)| file != CI), "{f:?}");
    let with_new = format!("{clean}      tree:\n        default: \"\"\n");
    let f = found(&[(CI, &with_new)]);
    assert!(
        f.iter().any(|(file, _, _)| file == CI),
        "an undeclared input `tree` is found at ci.yml: {f:?}"
    );
}
