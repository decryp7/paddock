//! Declared reads (ADR 0116 §2, user decision 122): a crate lists the files its tests read in `[package.metadata.paddock] test-reads`.
//! These tests build a real fixture repository (cargo metadata, the model, the hash) and ask the cache for its candidates, so the
//! declaration is followed from the manifest to the key. Scratch folders only (ER-14).

use super::Candidate;
use super::cmd::{candidates_in, world};
use std::path::Path;

fn git(d: &Path, args: &[&str]) {
    let mut c = crate::git_env::git();
    c.arg("-C")
        .arg(d)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args);
    let out = c.output().unwrap();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn write(d: &Path, rel: &str, text: &str) {
    let p = d.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn commit(d: &Path, msg: &str) {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", msg]);
}

/// Crate `a` has a test that reads the folder it is started from (a blind site) and the manifest `metadata` text; `b` depends on `a`
/// and reads nothing; `c` is alone. Files `data/table.txt` and `other/x.txt` exist.
fn repo(metadata: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    write(
        d.path(),
        "docs/repo-graph.toml",
        &std::fs::read_to_string(root.join("docs/repo-graph.toml")).unwrap(),
    );
    write(
        d.path(),
        "crates/Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\nresolver = \"2\"\n",
    );
    write(
        d.path(),
        "crates/a/Cargo.toml",
        &format!("[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{metadata}"),
    );
    write(d.path(), "crates/a/src/lib.rs", "pub fn f() {}\n");
    write(
        d.path(),
        "crates/a/tests/it.rs",
        "#[test]\nfn reads_the_started_folder() {\n    let _ = std::env::current_dir();\n}\n",
    );
    write(
        d.path(),
        "crates/b/Cargo.toml",
        "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\na = { path = \"../a\" }\n",
    );
    write(d.path(), "crates/b/src/lib.rs", "pub fn g() {}\n");
    write(
        d.path(),
        "crates/c/Cargo.toml",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(d.path(), "crates/c/src/lib.rs", "pub fn h() {}\n");
    write(d.path(), "data/table.txt", "one\n");
    write(d.path(), "other/x.txt", "one\n");
    git(d.path(), &["init", "-q"]);
    commit(d.path(), "one");
    d
}

fn cands(d: &Path) -> Vec<Candidate> {
    let w = world(d).unwrap();
    candidates_in(d, &w, "crates", "linux").unwrap()
}

fn of<'a>(c: &'a [Candidate], dir: &str) -> &'a Candidate {
    c.iter().find(|c| c.target.node == format!("crate:{dir}")).unwrap()
}

const DECLARED: &str = "[package.metadata.paddock]\ntest-reads = [\"data/**\"]\n";

#[test]
fn graph_a_crate_with_a_blind_site_and_no_declaration_is_never_and_its_dependent_is_blind_through_it() {
    let d = repo("");
    let c = cands(d.path());
    assert!(
        !of(&c, "crates/a").blind.is_empty(),
        "a reads the started folder: {:?}",
        of(&c, "crates/a").blind
    );
    assert_eq!(of(&c, "crates/b").blind, vec!["blind through crates/a".to_string()]);
    assert!(of(&c, "crates/c").blind.is_empty(), "control: an unrelated crate is cacheable");
}

#[test]
fn graph_with_a_declaration_the_crate_is_cacheable_and_its_key_moves_when_a_declared_file_changes() {
    let d = repo(DECLARED);
    let before = cands(d.path());
    assert!(of(&before, "crates/a").blind.is_empty(), "{:?}", of(&before, "crates/a").blind);
    // A file the declaration does not name moves nothing (control: the key is not the whole tree).
    write(d.path(), "other/x.txt", "two\n");
    commit(d.path(), "undeclared");
    let same = cands(d.path());
    assert_eq!(of(&same, "crates/a").node_hash, of(&before, "crates/a").node_hash);
    write(d.path(), "data/table.txt", "two\n");
    commit(d.path(), "declared");
    let moved = cands(d.path());
    assert_ne!(
        of(&moved, "crates/a").node_hash,
        of(&before, "crates/a").node_hash,
        "the declared file is in a's key"
    );
    assert_eq!(
        of(&moved, "crates/c").node_hash,
        of(&before, "crates/c").node_hash,
        "control: an unrelated crate keeps its key"
    );
}

#[test]
fn graph_a_dependent_blind_only_through_the_declared_crate_is_cacheable_and_its_key_moves_with_the_declared_file() {
    let d = repo(DECLARED);
    let before = cands(d.path());
    assert!(of(&before, "crates/b").blind.is_empty(), "{:?}", of(&before, "crates/b").blind);
    write(d.path(), "data/table.txt", "two\n");
    commit(d.path(), "declared");
    let moved = cands(d.path());
    assert_ne!(
        of(&moved, "crates/b").node_hash,
        of(&before, "crates/b").node_hash,
        "b runs a's tests' data: its key moves too"
    );
}

#[test]
fn graph_a_declared_glob_that_matches_nothing_is_a_finding_and_the_crate_stays_never() {
    let d = repo("[package.metadata.paddock]\ntest-reads = [\"data/**\", \"gone/**\"]\n");
    let c = cands(d.path());
    let a = &of(&c, "crates/a").blind;
    assert!(
        a.iter().any(|r| r.contains("gone/**") && r.contains("matches no tracked file")),
        "{a:?}"
    );
    assert_eq!(
        of(&c, "crates/b").blind,
        vec!["blind through crates/a".to_string()],
        "the dependent follows"
    );
}

#[test]
fn graph_a_declaration_that_matches_nothing_makes_even_a_crate_without_blind_sites_never() {
    let d = repo("");
    write(
        d.path(),
        "crates/c/Cargo.toml",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[package.metadata.paddock]\ntest-reads = [\"gone/**\"]\n",
    );
    commit(d.path(), "stale declaration");
    let c = cands(d.path());
    assert!(
        of(&c, "crates/c").blind.iter().any(|r| r.contains("matches no tracked file")),
        "{:?}",
        of(&c, "crates/c").blind
    );
}

#[test]
fn graph_a_declaration_that_is_not_a_list_of_strings_is_refused() {
    let d = repo("[package.metadata.paddock]\ntest-reads = \"data/**\"\n");
    let e = world(d.path()).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("test-reads"), "{e}");
}

#[test]
fn cover_blind_splits_sites_by_a_declaration_that_holds() {
    use crate::repo_model::test_reads::resolve;
    let site = |s: &str| vec![s.to_string()];
    let own = std::collections::BTreeMap::from([
        ("crates/ok".to_string(), site("ok: current_dir")),
        ("crates/none".to_string(), site("none: current_dir")),
        ("crates/stale".to_string(), site("stale: current_dir")),
        ("crates/empty".to_string(), site("empty: current_dir")),
    ]);
    let tracked = vec!["data/a.txt".to_string()];
    let declared = resolve(
        &tracked,
        &std::collections::BTreeMap::from([
            ("crates/ok".to_string(), vec!["data/*".to_string()]),
            ("crates/stale".to_string(), vec!["gone/*".to_string()]),
            ("crates/quiet".to_string(), vec!["gone/*".to_string()]),
            ("crates/empty".to_string(), vec![]),
        ]),
    );
    let got = super::cover_blind(own, &declared);
    assert_eq!(
        got.covered.keys().collect::<Vec<_>>(),
        ["crates/empty", "crates/ok"],
        "an empty list is a declaration: the blind reads touch no repository file"
    );
    assert_eq!(
        got.open["crates/none"],
        site("none: current_dir"),
        "no declaration: stays blind, never silently cacheable"
    );
    assert_eq!(
        got.open["crates/stale"].len(),
        2,
        "the finding first, then its own site: {:?}",
        got.open["crates/stale"]
    );
    assert!(got.open["crates/stale"][0].contains("matches no tracked file"));
    assert!(
        got.open["crates/quiet"][0].contains("matches no tracked file"),
        "a stale declaration without a site is a finding too"
    );
}
