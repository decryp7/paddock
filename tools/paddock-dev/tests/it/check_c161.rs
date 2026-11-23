//! C161 (every register under docs/ declares its policy; growth is measured by it): the policy rules against a merge base in a real
//! git repository, and the positive controls that remove the thing the check watches.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};
use std::path::Path;

const REGISTERS: &str = "docs/registers.toml";

fn registers(policy: &str, extra: &str) -> String {
    format!(
        "version = 1\n\n[[register]]\nfile = \"docs/registers.toml\"\ntable = \"register\"\npolicy = \"free\"\nreader = \"\"\nstrict = \"check\"\nfields = [\"file\", \"table\", \"policy\", \"reader\", \"strict\", \"key\", \"members\", \"fields\", \"scalars\", \"why\"]\nscalars = [\"version\"]\nwhy = \"the declaration itself\"\n\n[[register]]\nfile = \"docs/list.toml\"\ntable = \"entry\"\npolicy = \"{policy}\"\nreader = \"\"\nstrict = \"check\"\nkey = [\"id\"]\nfields = [\"id\", \"approved\"]\nwhy = \"a list of the test\"\n{extra}"
    )
}

fn list(ids: &[(&str, bool)]) -> String {
    ids.iter()
        .map(|(id, approved)| format!("[[entry]]\nid = \"{id}\"\n{}", if *approved { "approved = \"user\"\n" } else { "" }))
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::disallowed_methods, reason = "a test spawns git; it ends it itself")]
fn git(root: &Path, args: &[&str]) {
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
}

/// A repository whose merge base (`origin/main`) holds `(registers, list)` and whose `topic` commit holds `(registers, list)` of `now`.
fn repo(base: (&str, &str), now: (&str, &str)) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join(REGISTERS), base.0).unwrap();
    std::fs::write(root.join("docs/list.toml"), base.1).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(root, &["checkout", "-q", "-b", "topic"]);
    std::fs::write(root.join(REGISTERS), now.0).unwrap();
    std::fs::write(root.join("docs/list.toml"), now.1).unwrap();
    git(root, &["commit", "-q", "-a", "--allow-empty", "-m", "topic"]);
    d
}

fn findings(root: &Path) -> Vec<(String, bool, String)> {
    let opts = RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare("fixture"),
        scope: None,
        only: Some(vec!["C161".into()]),
        files: None,
        today: "2026-11-23".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    };
    run_checks(&opts, &registry())
        .unwrap()
        .findings
        .iter()
        .map(|f| (f.file.clone(), f.unwaivable, f.message.clone()))
        .collect()
}

#[test]
fn a_new_entry_in_a_shrink_only_table_is_one_unwaivable_finding_and_shrinking_is_silent() {
    let reg = registers("shrink-only", "");
    let two = list(&[("a", false), ("b", false)]);
    let one = list(&[("a", false)]);
    let grew = findings(repo((&reg, &one), (&reg, &two)).path());
    assert_eq!(grew.len(), 1, "{grew:?}");
    assert!(
        grew[0].1 && grew[0].2.contains("`b`") && grew[0].2.contains("shrink-only"),
        "{grew:?}"
    );
    assert!(
        findings(repo((&reg, &two), (&reg, &one)).path()).is_empty(),
        "removing an entry is shrinking"
    );
    assert!(findings(repo((&reg, &two), (&reg, &two)).path()).is_empty());
}

#[test]
fn a_new_entry_of_an_approval_required_table_needs_its_approved_field() {
    let reg = registers("approval-required", "");
    let one = list(&[("a", true)]);
    let unapproved = list(&[("a", true), ("b", false)]);
    let approved = list(&[("a", true), ("b", true)]);
    let bad = findings(repo((&reg, &one), (&reg, &unapproved)).path());
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].2.contains("`b`") && bad[0].2.contains("no `approved`"), "{bad:?}");
    assert!(findings(repo((&reg, &one), (&reg, &approved)).path()).is_empty());
}

#[test]
fn a_free_table_may_grow() {
    let reg = registers("free", "");
    let (one, two) = (list(&[("a", false)]), list(&[("a", false), ("b", false)]));
    assert!(findings(repo((&reg, &one), (&reg, &two)).path()).is_empty());
}

#[test]
fn a_policy_that_loosened_against_the_merge_base_is_a_finding_and_a_tightened_one_is_not() {
    let (shrink, free) = (registers("shrink-only", ""), registers("free", ""));
    let one = list(&[("a", false)]);
    let loosened = findings(repo((&shrink, &one), (&free, &one)).path());
    assert_eq!(loosened.len(), 1, "{loosened:?}");
    assert!(loosened[0].1 && loosened[0].2.contains("loosened"), "{loosened:?}");
    assert!(findings(repo((&free, &one), (&shrink, &one)).path()).is_empty());
}

#[test]
fn a_register_file_nobody_declared_and_an_unknown_key_are_found_and_removing_the_declaration_is_what_makes_them() {
    let reg = registers("free", "");
    let ok = list(&[("a", false)]);
    assert!(findings(repo((&reg, &ok), (&reg, &ok)).path()).is_empty());
    let d = repo((&reg, &ok), (&reg, &ok));
    std::fs::write(d.path().join("docs/other.toml"), "[[x]]\n").unwrap();
    std::fs::write(d.path().join("docs/list.toml"), format!("{ok}\nmisspelt = 1\n")).unwrap();
    let f = findings(d.path());
    assert!(
        f.iter()
            .any(|(file, _, m)| file == "docs/other.toml" && m.contains("no declared policy")),
        "{f:?}"
    );
    assert!(
        f.iter().any(|(file, _, m)| file == "docs/list.toml" && m.contains("`misspelt`")),
        "{f:?}"
    );
}

#[test]
fn a_declaration_that_cannot_be_read_makes_the_check_broken_not_clean() {
    let ok = list(&[("a", false)]);
    let d = repo((&registers("free", ""), &ok), (&registers("free", ""), &ok));
    std::fs::write(d.path().join(REGISTERS), "version = 1\n[[register]]\nfile = \"docs/list.toml\"\n").unwrap();
    let f = findings(d.path());
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].1 && f[0].2.contains("cannot look"), "{f:?}");
}

#[test]
fn the_real_tree_declares_every_register_and_has_no_unknown_key() {
    let root = super::check::repo_root();
    let f = findings(&root);
    // Growth and an unmeasurable base depend on the checkout's refs; the declaration and the fields do not.
    let own: Vec<_> = f
        .iter()
        .filter(|(_, _, m)| !m.contains("grew") && !m.contains("not measured") && !m.contains("loosened") && !m.contains("merge base"))
        .collect();
    assert!(own.is_empty(), "{own:?}");
}
