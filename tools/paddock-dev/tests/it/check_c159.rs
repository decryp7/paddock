//! C159 (no file-wide clippy::disallowed_methods allow where a bare-pid kill is called): through the host. The check's own test file: C159 does not read it.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};

/// `(file, line, unwaivable)` of every finding of C159 on a tree of `files`.
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
        only: Some(vec!["C159".into()]),
        files: None,
        today: "2026-11-23".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    };
    let r = run_checks(&opts, &registry()).unwrap();
    r.findings.iter().map(|f| (f.file.clone(), f.line, f.unwaivable)).collect()
}

#[test]
fn a_blanket_allow_over_a_kill_is_one_unwaivable_finding_and_a_per_site_expect_is_clean() {
    let f = found(&[
        (
            "apps/a/tests/sweep.rs",
            "#![allow(clippy::unwrap_used, clippy::disallowed_methods)]\nuse sysinfo::System;\nfn k(s: &System, p: Pid) {\n    s.process(p).unwrap().kill();\n}\n",
        ),
        (
            "apps/a/tests/site.rs",
            "fn k(p: i32) {\n    #[expect(clippy::disallowed_methods, reason = \"our own child\")]\n    unsafe { libc::kill(p, 9) };\n}\n",
        ),
        ("apps/a/tests/no_kill.rs", "#![allow(clippy::disallowed_methods)]\nfn f() {}\n"),
    ]);
    assert_eq!(f, vec![("apps/a/tests/sweep.rs".to_string(), 1, true)], "{f:?}");
}

#[test]
fn a_paddock_waiver_does_not_cover_it() {
    // paddock.waivers.toml covers C53's blanket-allow finding; this finding is unwaivable, so the entry is no way round it.
    let waivers =
        "[[waiver]]\ncheck = \"C159\"\npath = \"apps/a/tests/sweep.rs\"\nreason = \"a long enough reason\"\nexpires = \"2099-01-01\"\n";
    let f = found(&[
        ("paddock.waivers.toml", waivers),
        (
            "apps/a/tests/sweep.rs",
            "#![allow(clippy::disallowed_methods)]\nfn k(p: i32) { unsafe { libc::kill(p, 9) }; }\n",
        ),
    ]);
    assert!(
        f.iter().any(|(file, _, unwaivable)| file == "apps/a/tests/sweep.rs" && *unwaivable),
        "{f:?}"
    );
}
