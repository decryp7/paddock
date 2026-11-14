//! `./dev check new-id`: the next free check id across branches, against a scratch repository whose branches each add an id.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check_ids::{RESERVED_DIR, command_with, id_of_path, ids_of_gates, ids_of_index, ids_of_readme, slug};
use paddock_dev::input_hash::{Exec, Real};
use serde_json::Value;
use std::path::Path;

// Plain `git` and the tool under test, started and awaited here; nothing is killed by pid.
#[allow(clippy::disallowed_methods)]
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn reserve_file(dir: &Path, id: u32, branch: &str) {
    let d = dir.join(RESERVED_DIR);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join(format!("C{id}-{}", slug(branch))), format!("C{id} {branch} 2026-11-13\n")).unwrap();
}

fn add_check(dir: &Path, id: u32) {
    let d = dir.join("conformance/src/checks");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join(format!("C{id}-x.mjs")), "export default {};\n").unwrap();
    let idx = d.join("index.mjs");
    let mut t = std::fs::read_to_string(&idx).unwrap_or_default();
    t.push_str(&format!("import C{id} from './C{id}-x.mjs';\n"));
    std::fs::write(idx, t).unwrap();
}

/// `<tmp>/origin.git` and `<tmp>/work`: main has C5 and C7; local `a` adds C20, `b` (pushed, then deleted locally) adds C30,
/// `c` is only in the origin and reserves C40 without a file; `work` stays on main.
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let origin = d.path().join("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    let r = d.path().join("work");
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    add_check(&r, 5);
    add_check(&r, 7);
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "chore: base"]);
    git(&r, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(&r, &["push", "-q", "origin", "main"]);
    for (branch, id) in [("a", 20), ("b", 30)] {
        git(&r, &["checkout", "-q", "-b", branch, "main"]);
        add_check(&r, id);
        git(&r, &["add", "-A"]);
        git(&r, &["commit", "-q", "-m", "feat: check"]);
    }
    git(&r, &["push", "-q", "origin", "b"]);
    git(&r, &["checkout", "-q", "-b", "c", "main"]);
    reserve_file(&r, 40, "c");
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "chore: reserve"]);
    git(&r, &["push", "-q", "origin", "c"]);
    git(&r, &["checkout", "-q", "main"]);
    git(&r, &["branch", "-q", "-D", "b", "c"]);
    d
}

// Plain `git` and the tool under test, started and awaited here; nothing is killed by pid.
#[allow(clippy::disallowed_methods)]
fn dev(root: &Path, args: &[&str]) -> (i32, Value, String) {
    let out = paddock_testkit::TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["--json", "check", "new-id", "--root"])
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), v, String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn the_next_id_is_above_every_branch_local_and_origin_and_reservation() {
    let d = repo();
    let root = d.path().join("work");
    let (code, v, err) = dev(&root, &[]);
    assert_eq!(code, 0, "{err}");
    // C40 lives only on origin/c (a reservation), C30 only on origin/b, C20 only on local a.
    assert_eq!(v["id"], "C41");
    assert!(err.contains("check new-id: read"), "the scan is logged: {err}");
}

#[test]
fn each_source_alone_moves_the_id() {
    let d = repo();
    let root = d.path().join("work");
    git(&root, &["push", "-q", "origin", "--delete", "c"]);
    git(&root, &["update-ref", "-d", "refs/remotes/origin/c"]);
    assert_eq!(dev(&root, &[]).1["id"], "C31", "origin/b alone");
    // origin/b is read from the live remote, so the branch leaves the remote too, not only this clone's copy of it.
    git(&root, &["push", "-q", "origin", "--delete", "b"]);
    git(&root, &["update-ref", "-d", "refs/remotes/origin/b"]);
    assert_eq!(dev(&root, &[]).1["id"], "C21", "local a alone");
    git(&root, &["branch", "-q", "-D", "a"]);
    assert_eq!(dev(&root, &[]).1["id"], "C8", "main alone");
}

/// A branch another clone pushed after this clone's last fetch still counts: the scan reads the remote's branches live, not
/// this clone's possibly stale `origin/*` copies (on 2026-11-13 C119, C120 and C121 were each taken twice by parallel branches).
#[test]
fn a_branch_pushed_from_another_clone_since_the_last_fetch_counts() {
    let d = repo();
    let root = d.path().join("work");
    let other = d.path().join("other");
    git(d.path(), &["clone", "-q", d.path().join("origin.git").to_str().unwrap(), "other"]);
    git(&other, &["checkout", "-q", "-b", "e", "origin/main"]);
    reserve_file(&other, 60, "e");
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "chore: reserve"]);
    git(&other, &["push", "-q", "origin", "e"]);
    let (code, v, err) = dev(&root, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["id"], "C61", "origin's branch e, unknown to this clone's refs: {err}");
}

/// Without the remote the scan cannot see the other branches: that is an error naming the remote, never a guess from the cache.
#[test]
fn an_unreachable_remote_is_an_error() {
    let d = repo();
    let root = d.path().join("work");
    git(&root, &["remote", "set-url", "origin", d.path().join("gone.git").to_str().unwrap()]);
    let (code, _, err) = dev(&root, &[]);
    assert_ne!(code, 0, "{err}");
    assert!(err.contains("origin"), "{err}");
}

#[test]
fn an_uncommitted_file_of_the_working_tree_counts() {
    let d = repo();
    let root = d.path().join("work");
    add_check(&root, 55);
    assert_eq!(dev(&root, &[]).1["id"], "C56");
}

#[test]
fn reserve_appends_one_line_and_is_idempotent_per_branch() {
    let d = repo();
    let root = d.path().join("work");
    let (code, v, err) = dev(&root, &["--reserve", "feat/new"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["id"], "C41");
    let names = |root: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(root.join(RESERVED_DIR))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(&root), vec!["C41-feat-new"]);
    // The same branch asks again: the same id, no second file. Another branch gets the next one.
    assert_eq!(dev(&root, &["--reserve", "feat/new"]).1["id"], "C41");
    assert_eq!(dev(&root, &["--reserve", "feat/other"]).1["id"], "C42");
    assert_eq!(names(&root), vec!["C41-feat-new", "C42-feat-other"]);
}

/// Two branches that reserved the same id at the same time touch different files (no merge conflict); the next scan names both.
#[test]
fn two_branches_reserving_the_same_id_are_detected() {
    let d = repo();
    let root = d.path().join("work");
    for branch in ["x", "y"] {
        git(&root, &["checkout", "-q", "-b", branch, "main"]);
        reserve_file(&root, 50, branch);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "chore: reserve"]);
    }
    git(&root, &["checkout", "-q", "main"]);
    let (code, _, err) = dev(&root, &[]);
    assert_eq!(code, 4, "{err}");
    assert!(err.contains("C50 is reserved by more than one branch (x, y)"), "{err}");
    // Merging both is conflict free: the files differ.
    git(&root, &["merge", "-q", "--no-edit", "x"]);
    git(&root, &["merge", "-q", "--no-edit", "y"]);
}

#[test]
fn rust_check_files_and_gate_register_ids_count() {
    let d = repo();
    let root = d.path().join("work");
    let rust = root.join("tools/paddock-dev/src/check");
    std::fs::create_dir_all(&rust).unwrap();
    std::fs::write(rust.join("c77.rs"), "").unwrap();
    assert_eq!(dev(&root, &[]).1["id"], "C78", "a Rust check file");
    git(&root, &["checkout", "-q", "-b", "g", "main"]);
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/gates.toml"), "[[x]]\nid = \"C90\"\nreason = \"r\"\n").unwrap();
    git(&root, &["add", "docs/gates.toml"]);
    git(&root, &["commit", "-q", "-m", "chore: gates"]);
    git(&root, &["checkout", "-q", "main"]);
    assert_eq!(dev(&root, &[]).1["id"], "C91", "a gates.toml id on another branch");
}

/// A git that returns nothing for `cat-file` must fail the scan: a skipped blob would hand out a taken id.
struct NoCatFile;
impl Exec for NoCatFile {
    fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, paddock_cli::CliError> {
        if args.first() == Some(&"cat-file") {
            Ok(String::new())
        } else {
            Real.run(program, args, cwd, stdin)
        }
    }
}

#[test]
fn a_blob_that_cat_file_does_not_return_is_an_error() {
    let d = repo();
    let root = d.path().join("work");
    let err = command_with(&NoCatFile, Some(root.clone()), None, "2026-11-13").unwrap_err();
    assert!(err.to_string().contains("git cat-file returned nothing"), "{err}");
    assert!(command_with(&Real, Some(root), None, "2026-11-13").is_ok());
}

#[test]
fn the_parsers_read_each_shape() {
    assert_eq!(id_of_path("conformance/src/checks/C121-name.mjs"), Some(121));
    assert_eq!(id_of_path("conformance/src/checks/index.mjs"), None);
    assert_eq!(id_of_path("conformance/src/other/C9-x.mjs"), None);
    assert_eq!(ids_of_index("import C3 from './C3-a.mjs';\nimport x from 'y';\n"), vec![3]);
    assert_eq!(ids_of_readme("| C102 | a | |\n|---|---|\n| not | b |\n"), vec![102]);
    assert_eq!(id_of_path("tools/paddock-dev/src/check/c119.rs"), Some(119));
    assert_eq!(id_of_path("tools/paddock-dev/src/check/c53_liveness.rs"), Some(53));
    assert_eq!(id_of_path("tools/paddock-dev/src/check/mod.rs"), None);
    assert_eq!(id_of_path("conformance/reserved/C121-feat-x"), Some(121));
    assert_eq!(ids_of_gates("[[a]]\nid = \"C7\"\nid = \"other\"\nreason = \"id = C8\"\n"), vec![7]);
    assert_eq!(slug("Feat/New_thing"), "feat-new-thing");
}
