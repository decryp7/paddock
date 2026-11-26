//! C173 (no new non-Rust tooling file outside the allow list and its shrink-only baseline): a good and a bad fixture per file kind, a
//! planted `.sh` and a planted `.mjs` going red (ER-33), baseline growth going red, an allow-listed file passing, and this working
//! tree holding no finding but the growth warning that goes away once the list is on the target branch.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, repo_root};
use paddock_dev::check::allow_list::Spec;
use paddock_dev::check::c173::{ALLOW, C173};
use paddock_dev::check::config::Config;
use paddock_dev::check::finding::Severity;
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{Ctx, RunOptions, registry, run_checks, run_checks_in};
use std::path::Path;

const KINDS: [&str; 7] = ["sh", "ps1", "py", "mjs", "js", "cjs", "ts"];

fn options(root: &Path) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: Config::bare("fixture"),
        scope: None,
        only: Some(vec!["C173".into()]),
        files: None,
        today: "2026-11-26".into(),
        base_ref: Some("main".into()),
        ci: Some(false),
    }
}

fn entry(path: &str, reason: &str) -> String {
    format!("{{\"path\": \"{path}\", \"site\": \"tooling file\", \"reason\": \"{reason}\"}}")
}

fn allow_text(paths: &[&str]) -> String {
    let list: Vec<String> = paths.iter().map(|p| entry(p, "baseline: ported later")).collect();
    format!("{{\"allow\": [{}]}}", list.join(", "))
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

fn git(root: &Path, args: &[&str]) {
    let out = paddock_dev::git_env::git()
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
}

fn flagged(files: &[(&str, &str)]) -> Vec<String> {
    let tree = tree_of(files);
    let r = run_checks(&options(tree.path()), &registry()).unwrap();
    assert!(
        r.errors() == r.findings.iter().filter(|f| f.message.starts_with("new non-Rust")).count(),
        "{:?}",
        r.findings
    );
    r.findings
        .iter()
        .filter(|f| f.message.starts_with("new non-Rust"))
        .map(|f| f.file.clone())
        .collect()
}

/// A bad and a good fixture per file kind: the new file is an error at its own path, the allow-listed one passes.
#[test]
fn c173_flags_a_new_file_of_every_kind_and_passes_the_allow_listed_one() {
    for kind in KINDS {
        let (bad, good) = (format!("scripts/new.{kind}"), format!("tools/old.{kind}"));
        let allow = allow_text(&[&good]);
        let got = flagged(&[(ALLOW, &allow), (&bad, "x\n"), (&good, "x\n")]);
        assert_eq!(got, [bad], "kind {kind}");
    }
}

/// ER-33: a planted `.sh` and a planted `.mjs` go red, as errors (blocking), each in its own tooling path.
#[test]
fn c173_a_planted_sh_and_a_planted_mjs_are_errors() {
    let allow = allow_text(&[]);
    let tree = tree_of(&[
        (ALLOW, &allow),
        ("scripts/planted.sh", "#!/bin/sh\n"),
        ("apps/dyno/scripts/planted.mjs", "export {};\n"),
        (".gitea/actions/x/planted.py", "x\n"),
    ]);
    let r = run_checks(&options(tree.path()), &registry()).unwrap();
    let got: Vec<&str> = r.findings.iter().map(|f| f.file.as_str()).collect();
    assert_eq!(
        got,
        [".gitea/actions/x/planted.py", "apps/dyno/scripts/planted.mjs", "scripts/planted.sh"],
        "{:?}",
        r.findings
    );
    assert_eq!(r.errors(), 3);
}

/// What is not tooling is never flagged: other extensions, other paths, data under fixtures and goldens, Playwright and Vitest suites.
#[test]
fn c173_leaves_data_suites_other_kinds_and_other_paths_alone() {
    // one named tooling file, so the run has looked at something (a run that read nothing is not a pass)
    let allow = allow_text(&["scripts/named.sh"]);
    let got = flagged(&[
        (ALLOW, &allow),
        ("scripts/named.sh", "x"),
        ("scripts/a.json", "{}"),
        ("scripts/a.rs", "fn main() {}"),
        ("ui/projects/x/a.mjs", "x"),
        ("apps/dyno/app/e2e/a.ts", "x"),
        ("tools/paddock-dev/tests/fixtures/f.sh", "x"),
        ("tools/paddock-dev/tests/golden/g.mjs", "x"),
        ("tools/x/run.spec.ts", "x"),
        ("scripts/playwright.config.ts", "x"),
    ]);
    assert!(got.is_empty(), "{got:?}");
}

/// The tracked list is the one read (ER-42): an untracked file in a work tree is not flagged, a tracked one is.
#[test]
fn c173_reads_the_tracked_list_in_a_work_tree() {
    let allow = allow_text(&[]);
    let d = tree_of(&[(ALLOW, &allow), ("scripts/tracked.sh", "x\n")]);
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "base"]);
    std::fs::write(d.path().join("scripts/untracked.sh"), "x\n").unwrap();
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    let got: Vec<&str> = r
        .findings
        .iter()
        .filter(|f| f.message.starts_with("new non-Rust"))
        .map(|f| f.file.as_str())
        .collect();
    assert_eq!(got, ["scripts/tracked.sh"], "{:?}", r.findings);
}

static BLOCKING: Spec = Spec {
    id: "C173",
    rule: 83,
    file: ALLOW,
    unseen: "a tooling file",
    restore: "Restore it.",
    growth_blocks: true,
};

fn run_with(root: &Path, spec: &'static Spec) -> paddock_dev::check::CheckOutput {
    let tree = Tree::open(root);
    let config = Config::bare("fixture");
    let ctx = Ctx::new(&tree, tree.files(), Some("main".into()), &config, None, "2026-11-26".into());
    C173.run_with(&ctx, spec)
}

/// A repository whose merge base `main` names one file, and whose branch adds an entry that hides a new file.
fn grown_repo() -> tempfile::TempDir {
    let base = allow_text(&["scripts/old.sh"]);
    let d = tree_of(&[(ALLOW, &base), ("scripts/old.sh", "x\n")]);
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "base"]);
    git(d.path(), &["checkout", "-q", "-b", "topic"]);
    let grown = allow_text(&["scripts/old.sh", "scripts/hidden.sh"]);
    std::fs::write(d.path().join(ALLOW), grown).unwrap();
    std::fs::write(d.path().join("scripts/hidden.sh"), "x\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "hide a new file in the list"]);
    d
}

/// ER-46: an entry the merge base lacks is growth. Blocking, it is an unwaivable error at the list; as shipped (`growth_blocks` false until
/// the list is on main) the same finding is a warning.
#[test]
fn c173_baseline_growth_is_red_when_blocking_and_a_warning_until_then() {
    let d = grown_repo();
    let out = run_with(d.path(), &BLOCKING);
    let growth: Vec<_> = out.findings.iter().filter(|f| f.message.contains("the allow list grew")).collect();
    assert_eq!(growth.len(), 1, "{:?}", out.findings);
    assert_eq!(growth[0].file, ALLOW);
    assert!(growth[0].unwaivable);
    assert_eq!(growth[0].severity, Severity::Error);
    let shipped = paddock_dev::check::c173::SPEC.growth_blocks;
    let out = run_with(d.path(), &paddock_dev::check::c173::SPEC);
    let growth = out
        .findings
        .iter()
        .find(|f| f.message.contains("the allow list grew"))
        .expect("growth is reported");
    assert_eq!(growth.severity, if shipped { Severity::Error } else { Severity::Warn });
}

/// Shrinking is fine: a list that dropped an entry and its file has no finding at all.
#[test]
fn c173_a_shrunk_baseline_is_clean() {
    let base = allow_text(&["scripts/old.sh", "scripts/gone.sh"]);
    let d = tree_of(&[(ALLOW, &base), ("scripts/old.sh", "x\n"), ("scripts/gone.sh", "x\n")]);
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "base"]);
    git(d.path(), &["checkout", "-q", "-b", "topic"]);
    std::fs::write(d.path().join(ALLOW), allow_text(&["scripts/old.sh"])).unwrap();
    git(d.path(), &["rm", "-q", "scripts/gone.sh"]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "port gone.sh"]);
    let out = run_with(d.path(), &BLOCKING);
    assert!(out.findings.is_empty(), "{:?}", out.findings);
}

/// A file deleted while its entry stays is a stale warning: the baseline shrinks in the change that ports the file.
#[test]
fn c173_an_entry_of_a_deleted_file_is_a_stale_warning() {
    let allow = allow_text(&["scripts/gone.sh", "scripts/kept.sh"]);
    let d = tree_of(&[(ALLOW, &allow), ("scripts/kept.sh", "x\n")]);
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    assert_eq!((r.errors(), r.warnings()), (0, 1), "{:?}", r.findings);
    assert!(r.findings[0].message.starts_with("stale allow entry (scripts/gone.sh"));
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the planted files.
#[test]
fn c173_control_sees_a_planted_sh_and_mjs() {
    let allow = allow_text(&[]);
    let d = tree_of(&[(ALLOW, &allow)]);
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C173").expect("a run record").record;
    assert!(record.control, "the control found its planted files");
}

/// "Delete the tooling files": a whole-tree run that finds none looked at nothing, which is never a pass.
#[test]
fn c173_a_run_that_finds_no_tooling_file_is_not_measured() {
    let allow = allow_text(&[]);
    let d = tree_of(&[(ALLOW, &allow)]);
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    assert!(
        r.findings.iter().any(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
        "{:?}",
        r.findings
    );
}

/// "Delete the check's list": a missing, non-JSON or list-less allow file is one check-broken finding, never a pass.
#[test]
fn c173_with_a_broken_allow_file_is_one_check_broken_finding() {
    for allow in [None, Some("nope"), Some("{}"), Some("{\"allow\": [{\"path\": \"a\"}]}")] {
        let mut files = vec![("scripts/new.sh", "x\n")];
        if let Some(text) = allow {
            files.push((ALLOW, text));
        }
        let tree = tree_of(&files);
        let r = run_checks(&options(tree.path()), &registry()).unwrap();
        let got: Vec<(&str, Option<&str>)> = r.findings.iter().map(|f| (f.file.as_str(), f.outcome.map(|o| o.name()))).collect();
        assert_eq!(got, [(ALLOW, Some("check-broken"))], "{allow:?}");
    }
}

/// The real tree: every tooling file it tracks is named by the list (no error), and the one thing a branch may add is the growth warning.
#[test]
fn live_c173_every_tooling_file_of_the_tree_is_named() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C173 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "{:?}", r.findings);
    assert!(
        r.findings.iter().all(|f| f.message.contains("the allow list grew")),
        "only the growth of a list not yet on the target branch may be reported: {:?}",
        r.findings
    );
}
