//! Allow lists follow a pure rename (ER-21: moving files tripped the path-keyed registers on the throttle move and would on every batch of
//! the paddock-dev split). A fixture repository (`paddock_testkit::git_fixture`, rule 102) holds a golden with an allow-listed value; a
//! branch `git mv`s the file. The reader reads git's own rename detection (`git diff -M`) between the merge base and the tree, so the entry
//! counts for the new path, whether or not it was rewritten, and a rewrite is not growth. A moved file's changed content is judged as
//! usual: a newly added offending line is still a finding. List growth (a new entry) is unchanged.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::config::Config;
use paddock_dev::check::finding::Severity;
use paddock_dev::check::{RunOptions, registry, run_checks};
use paddock_testkit::TestCommand;
use paddock_testkit::git_fixture::{copy_tree, template};
use std::path::Path;

const ALLOW: &str = "conformance/c131-allow.json";
const OLD: &str = "x/tests/golden/g.json";
const NEW: &str = "y/tests/golden/g.json";
const GOLDEN: &str = "{\n  \"home\": \"/Users/ana/x\",\n  \"note\": \"a longer body keeps the two files similar for git's rename detection\",\n  \"more\": 1,\n  \"lines\": 2\n}\n";

fn allow(path: &str, extra: &str) -> String {
    let e = |p: &str| format!("{{\"path\": \"{p}\", \"site\": \"/Users/ana\", \"reason\": \"invented test user\"}}");
    let list: Vec<String> = std::iter::once(e(path)).chain((!extra.is_empty()).then(|| e(extra))).collect();
    format!("{{\"allow\": [{}]}}", list.join(", "))
}

#[allow(clippy::disallowed_methods)]
fn git(dir: &Path, args: &[&str]) {
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = TestCommand::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repository on `main` with the golden at its old path and its allow entry, copied for the test; the test branches from it.
fn repo() -> tempfile::TempDir {
    let t = template("renames-allow", include_str!("check_renames.rs"), |b, _| {
        git(b, &["init", "-q", "-b", "main"]);
        paddock_testkit::git_fixture::lf_only(b);
        std::fs::create_dir_all(b.join("conformance")).unwrap();
        std::fs::create_dir_all(b.join(OLD).parent().unwrap()).unwrap();
        std::fs::write(b.join(ALLOW), allow(OLD, "")).unwrap();
        std::fs::write(b.join(OLD), GOLDEN).unwrap();
        git(b, &["add", "--all"]);
        git(b, &["commit", "-q", "-m", "base"]);
    });
    let d = tempfile::tempdir().unwrap();
    copy_tree(&t, d.path());
    git(d.path(), &["checkout", "-q", "-b", "topic"]);
    d
}

fn mv(d: &Path) {
    std::fs::create_dir_all(d.join(NEW).parent().unwrap()).unwrap();
    git(d, &["mv", OLD, NEW]);
}

fn commit(d: &Path) {
    git(d, &["add", "--all"]);
    git(d, &["commit", "-q", "-m", "topic"]);
}

/// The C131 findings of `root` against `main`: `(severity, file, message)`.
fn findings(root: &Path) -> Vec<(Severity, String, String)> {
    let o = RunOptions {
        root: root.to_path_buf(),
        config: Config::bare("fixture"),
        scope: None,
        only: Some(vec!["C131".into()]),
        files: None,
        today: "2026-11-27".into(),
        base_ref: Some("main".into()),
        ci: Some(false),
    };
    let r = run_checks(&o, &registry()).unwrap();
    r.findings.iter().map(|f| (f.severity, f.file.clone(), f.message.clone())).collect()
}

fn errors(root: &Path) -> Vec<String> {
    findings(root)
        .into_iter()
        .filter(|f| f.0 == Severity::Error)
        .map(|f| format!("{}: {}", f.1, f.2))
        .collect()
}

#[test]
fn a_moved_golden_keeps_its_allow_entry_when_the_entry_still_names_the_old_path() {
    let d = repo();
    mv(d.path());
    commit(d.path());
    assert_eq!(errors(d.path()), Vec::<String>::new());
    // Following is a warning to rewrite the entry, never silence.
    let said = findings(d.path());
    assert!(said.iter().any(|f| f.2.contains(OLD) && f.2.contains(NEW)), "{said:?}");
}

#[test]
fn an_entry_rewritten_to_the_new_path_is_not_growth() {
    let d = repo();
    mv(d.path());
    std::fs::write(d.path().join(ALLOW), allow(NEW, "")).unwrap();
    commit(d.path());
    assert_eq!(errors(d.path()), Vec::<String>::new());
}

#[test]
fn the_rename_is_seen_before_it_is_committed_too() {
    let d = repo();
    mv(d.path());
    std::fs::write(d.path().join(ALLOW), allow(NEW, "")).unwrap();
    assert_eq!(errors(d.path()), Vec::<String>::new());
}

#[test]
fn a_moved_file_with_a_new_offending_line_still_fails() {
    let d = repo();
    mv(d.path());
    std::fs::write(d.path().join(ALLOW), allow(NEW, "")).unwrap();
    let text = GOLDEN.replace("\"more\": 1,", "\"more\": 1,\n  \"home2\": \"/Users/bob/x\",");
    std::fs::write(d.path().join(NEW), text).unwrap();
    commit(d.path());
    let e = errors(d.path());
    assert!(e.len() == 1 && e[0].contains("/Users/bob"), "{e:?}");
}

#[test]
fn a_new_entry_is_still_growth_in_a_branch_that_moves_files() {
    let d = repo();
    mv(d.path());
    std::fs::write(d.path().join(ALLOW), allow(NEW, "z/tests/golden/other.json")).unwrap();
    commit(d.path());
    let e = errors(d.path());
    assert!(e.iter().any(|m| m.contains("the allow list grew: 1 entry")), "{e:?}");
}

#[test]
fn a_file_that_is_not_moved_gets_no_help_from_another_files_rename() {
    let d = repo();
    // The entry names the old path but the file is deleted and a different one added: not a rename (content differs), so no following.
    std::fs::remove_file(d.path().join(OLD)).unwrap();
    std::fs::create_dir_all(d.path().join(NEW).parent().unwrap()).unwrap();
    std::fs::write(d.path().join(NEW), "{\n  \"home\": \"/Users/ana/q\"\n}\n").unwrap();
    commit(d.path());
    assert_eq!(errors(d.path()).len(), 1);
}
