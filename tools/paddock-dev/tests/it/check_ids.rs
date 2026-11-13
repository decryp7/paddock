//! `./dev check new-id`: the next free check id across branches, against a scratch repository whose branches each add an id.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
use paddock_dev::check_ids::{RESERVED, id_of_path, ids_of_index, ids_of_readme, reservations};
use serde_json::Value;
use std::path::Path;

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
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
    std::fs::write(r.join(RESERVED), "# header\nC40 c 2026-11-13\n").unwrap();
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "chore: reserve"]);
    git(&r, &["push", "-q", "origin", "c"]);
    git(&r, &["checkout", "-q", "main"]);
    git(&r, &["branch", "-q", "-D", "b", "c"]);
    d
}

fn dev(root: &Path, args: &[&str]) -> (i32, Value, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
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
    git(&root, &["update-ref", "-d", "refs/remotes/origin/b"]);
    assert_eq!(dev(&root, &[]).1["id"], "C21", "local a alone");
    git(&root, &["branch", "-q", "-D", "a"]);
    assert_eq!(dev(&root, &[]).1["id"], "C8", "main alone");
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
    let text = std::fs::read_to_string(root.join(RESERVED)).unwrap();
    assert_eq!(text.lines().filter(|l| l.starts_with("C41 feat/new ")).count(), 1, "{text}");
    // The same branch asks again: the same id, no second line. Another branch gets the next one.
    assert_eq!(dev(&root, &["--reserve", "feat/new"]).1["id"], "C41");
    assert_eq!(dev(&root, &["--reserve", "feat/other"]).1["id"], "C42");
    let text = std::fs::read_to_string(root.join(RESERVED)).unwrap();
    assert_eq!(text.lines().filter(|l| l.starts_with('C')).count(), 2, "{text}");
}

#[test]
fn the_parsers_read_each_shape() {
    assert_eq!(id_of_path("conformance/src/checks/C121-name.mjs"), Some(121));
    assert_eq!(id_of_path("conformance/src/checks/index.mjs"), None);
    assert_eq!(id_of_path("conformance/src/other/C9-x.mjs"), None);
    assert_eq!(ids_of_index("import C3 from './C3-a.mjs';\nimport x from 'y';\n"), vec![3]);
    assert_eq!(ids_of_readme("| C102 | a | |\n|---|---|\n| not | b |\n"), vec![102]);
    assert_eq!(
        reservations("# c\n\nC9 feat/x 2026-11-13\nbad line\n"),
        vec![(9, "feat/x".to_string())]
    );
}
