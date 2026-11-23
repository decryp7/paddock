//! C158 (no code reads a workflow's text outside the CI model): a tree of the shapes it reads, through the host. The check's own test file: C158 does not read it.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};

/// `(file, line, unwaivable)` of every finding of C158 on a tree of `files`.
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
        only: Some(vec!["C158".into()]),
        files: None,
        today: "2026-11-23".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    };
    let r = run_checks(&opts, &registry()).unwrap();
    r.findings.iter().map(|f| (f.file.clone(), f.line, f.unwaivable)).collect()
}

const ALLOW: &str = "conformance/c158-allow.json";

#[test]
fn a_read_of_a_workflow_is_a_finding_unless_the_allow_list_names_it_and_the_model_may_read_it() {
    let allow = "{\"allow\":[{\"path\":\"scripts/listed.mjs\",\"site\":\"ci.yml\",\"reason\":\"baseline: predates the check\"}]}";
    let read = "const t = readFileSync('.gitea/workflows/ci.yml', 'utf8');\n";
    let f = found(&[
        (ALLOW, allow),
        ("scripts/listed.mjs", read),
        ("scripts/unlisted.mjs", read),
        (
            "tools/paddock-dev/src/gates/ci.rs",
            "let t = std::fs::read_to_string(\".gitea/workflows/ci.yml\");\n",
        ),
        ("scripts/mention.mjs", "const doc = 'see .gitea/workflows/ci.yml';\n"),
    ]);
    assert_eq!(f, vec![("scripts/unlisted.mjs".to_string(), 1, false)], "{f:?}");
}

#[test]
fn removing_the_read_of_a_listed_site_makes_its_entry_stale() {
    let allow = "{\"allow\":[{\"path\":\"scripts/listed.mjs\",\"site\":\"ci.yml\",\"reason\":\"baseline: predates the check\"}]}";
    let f = found(&[(ALLOW, allow), ("scripts/listed.mjs", "const a = 1;\n")]);
    assert_eq!(
        f,
        vec![(ALLOW.to_string(), 1, false)],
        "a listed site that is gone is a stale-entry warning: {f:?}"
    );
}
