//! The ref a push's growth checks measure against (`PADDOCK_BASE_REF` of the conformance gate): the branch's target, never the
//! incremental diff base. On the second push of a branch the diff base is the remote's old tip, from which the first push's growth
//! is "already there": measured from it, growth hides itself (reproduced on C53, ADR 0098 amendment).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};
use paddock_dev::pre_push::growth_target;
use std::path::Path;

#[allow(clippy::disallowed_methods, reason = "a test spawns git; it ends it itself")]
fn git(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn allow(count: usize) -> String {
    format!(
        "{{\"allow\":[{{\"path\":\"tools/t/tests/it/ok.rs\",\"site\":\"#!/bin/sh\",\"reason\":\"baseline: planted before StandIn\",\"count\":{count}}}]}}"
    )
}

/// main (list 1) <- train/x (one empty commit) <- feature: F1 raises the list to 3 (first push, remote tip), F2 is the second push.
struct Repo {
    dir: tempfile::TempDir,
    f1: String,
    f2: String,
}

fn repo() -> Repo {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    std::fs::create_dir_all(r.join("conformance")).unwrap();
    std::fs::create_dir_all(r.join("tools/t/tests/it")).unwrap();
    std::fs::write(r.join("conformance/c130-allow.json"), allow(1)).unwrap();
    std::fs::write(r.join("tools/t/tests/it/ok.rs"), "let s = \"#!/bin/sh\\n\";\n".repeat(4)).unwrap();
    git(r, &["init", "-q", "-b", "main"]);
    git(r, &["add", "."]);
    git(r, &["commit", "-q", "-m", "main"]);
    git(r, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(r, &["checkout", "-q", "-b", "train/x"]);
    git(r, &["commit", "-q", "--allow-empty", "-m", "train"]);
    git(r, &["update-ref", "refs/remotes/origin/train/x", "HEAD"]);
    git(r, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(r.join("conformance/c130-allow.json"), allow(3)).unwrap();
    git(r, &["commit", "-q", "-a", "-m", "F1 grows the list"]);
    let f1 = git(r, &["rev-parse", "HEAD"]);
    git(r, &["update-ref", "refs/remotes/origin/feature", &f1]);
    git(r, &["update-ref", "refs/remotes/origin/other-feature", &f1]);
    git(r, &["commit", "-q", "--allow-empty", "-m", "F2"]);
    let f2 = git(r, &["rev-parse", "HEAD"]);
    Repo { dir: d, f1, f2 }
}

fn grew(root: &Path, base_ref: &str) -> usize {
    let o = RunOptions {
        only: Some(vec!["C130".into()]),
        config: paddock_dev::check::config::Config::bare("fixture"),
        root: root.to_path_buf(),
        scope: None,
        files: None,
        today: "2026-11-16".into(),
        base_ref: Some(base_ref.into()),
        ci: Some(false),
    };
    run_checks(&o, &registry())
        .unwrap()
        .findings
        .iter()
        .filter(|f| f.message.contains("allow list grew"))
        .count()
}

#[test]
fn the_target_is_the_nearest_main_or_train_the_pushed_commit_contains_never_the_remotes_old_tip() {
    let r = repo();
    let (t, why) = growth_target(r.dir.path(), Some(&r.f2), Some("origin"), None, None);
    assert_eq!(t, "origin/train/x", "{why}");
    assert!(why.contains("1 commit(s) behind") || why.contains("2 commit(s) behind"), "{why}");
    // Neither the remote ref's own old tip (origin/feature) nor another feature branch is ever the target.
    assert_ne!(t, "origin/feature");
    assert_ne!(t, "origin/other-feature");
    // A branch off main only measures against main; a commit contained in neither falls back to origin/main, said.
    git(r.dir.path(), &["checkout", "-q", "-b", "off-main", "main"]);
    git(r.dir.path(), &["commit", "-q", "--allow-empty", "-m", "x"]);
    assert_eq!(
        growth_target(r.dir.path(), Some("HEAD"), Some("origin"), None, None).0,
        "origin/main"
    );
    let orphan = git(r.dir.path(), &["commit-tree", "-m", "orphan", "HEAD^{tree}"]);
    let (t, why) = growth_target(r.dir.path(), Some(&orphan), Some("origin"), None, None);
    assert_eq!(t, "origin/main");
    assert!(why.contains("none of"), "{why}");
    // An explicit --base (the merge check) is kept; no commit at all says so.
    assert_eq!(growth_target(r.dir.path(), Some(&r.f2), None, Some("some-sha"), None).0, "some-sha");
    assert!(growth_target(r.dir.path(), None, None, None, None).1.contains("no commit"));
}

#[test]
fn a_second_push_after_growth_still_reports_it_against_the_target_and_not_against_the_incremental_base() {
    let r = repo();
    let _ = &r.f1;
    let (target, _) = growth_target(r.dir.path(), Some(&r.f2), Some("origin"), None, None);
    assert_eq!(
        grew(r.dir.path(), &target),
        1,
        "F1 raised the list: against the train it is the branch's growth, on the second push too"
    );
    // The incremental diff base of the second push is the remote's old tip, which already holds the growth: it would hide it.
    assert_eq!(grew(r.dir.path(), "origin/feature"), 0);
}

/// The two ci.yml lines that give the growth checks their target (ER-42: read through the workflow accessor, not by matching text).
#[test]
fn ci_yml_sets_the_target_in_the_conformance_and_train_gates_jobs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(".gitea/workflows/ci.yml"))
        .unwrap()
        .replace("\r\n", "\n");
    let wf = paddock_dev::check::util::workflows::read(".gitea/workflows/ci.yml", &text);
    let value = |job: &str| {
        let j = wf
            .jobs
            .iter()
            .find(|j| j.id == job)
            .unwrap_or_else(|| panic!("ci.yml has no job {job}"));
        let env = paddock_dev::check::util::workflows::job_env(j);
        let mine: Vec<_> = env.iter().filter(|(k, _)| k == "PADDOCK_BASE_REF").collect();
        assert_eq!(mine.len(), 1, "{job} sets PADDOCK_BASE_REF once: {env:?}");
        mine[0].1.clone()
    };
    assert_eq!(value("conformance"), "${{ inputs.base || 'origin/main' }}");
    assert_eq!(value("train-gates"), "${{ inputs.base }}");
}

/// Nothing but `check::baseline::target_ref` reads the variable (ER-21: five checks each read it or forgot it). A source audit over this
/// tool's own code: a read of it by `env::var`, `var_os`, `env.get` or `env(` outside baseline.rs fails. Setting it (the pre-push gate's
/// env) is not a read.
#[test]
fn only_target_ref_reads_the_variable() {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    let read = regex::Regex::new(r#"(?:var|var_os|get|env|remove)\s*\(\s*"PADDOCK_BASE_REF""#).unwrap();
    let mut offenders = Vec::new();
    for f in files {
        if f.ends_with("check/baseline.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, l) in text.lines().enumerate() {
            let code = l.trim_start();
            if !code.starts_with("//") && read.is_match(l) {
                offenders.push(format!("{}:{}", f.strip_prefix(&src).unwrap().display(), i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "reads PADDOCK_BASE_REF outside check::baseline::target_ref: {offenders:?}"
    );
}

#[test]
fn a_landing_push_is_measured_from_its_previous_tip_a_landed_train_is_not_named_and_a_url_remote_uses_origin() {
    let r = repo();
    let d = r.dir.path();
    // A push to a train (or main) is measured from the remote ref's previous tip.
    let (t, why) = growth_target(d, Some(&r.f2), Some("origin"), None, Some(&r.f1));
    assert_eq!(t, r.f1);
    assert!(why.contains("previous tip"), "{why}");
    // A tip the repository does not have is not a target: the usual choice applies.
    assert_eq!(
        growth_target(d, Some(&r.f2), Some("origin"), None, Some(&"1".repeat(40))).0,
        "origin/train/x"
    );
    // A train that points at main's commit (landed) is not named: main wins the tie.
    git(d, &["update-ref", "refs/remotes/origin/train/landed", "refs/remotes/origin/main"]);
    git(d, &["checkout", "-q", "-b", "on-main", "main"]);
    git(d, &["commit", "-q", "--allow-empty", "-m", "y"]);
    assert_eq!(growth_target(d, Some("HEAD"), Some("origin"), None, None).0, "origin/main");
    // A remote given as a URL falls back to origin for the tracking refs.
    assert_eq!(
        growth_target(d, Some(&r.f2), Some("https://example.invalid/x.git"), None, None).0,
        "origin/train/x"
    );
    assert_eq!(
        growth_target(d, Some(&r.f2), Some("git@host:x/y.git"), None, None).0,
        "origin/train/x"
    );
}
