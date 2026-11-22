//! OS-scoped tests (`hostTests`, docs/research/2026-11-22-os-scoped-tests.md, user decision 2026-11-22), on the real contracts and the
//! real sources: conformance C155 finds nothing on this tree, and it goes red when the entries that matter are taken out (ER-33): the
//! two cases that made the import rule (`ci_setup_rust` imports `local_sccache`, `ci_run` imports `pre_push::exec`) and a package with OS code.
//! The Linux leg is never scoped: what leaves a host leg still runs there.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};
use paddock_dev::host_tests::{HostTests, LINUX};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

/// The sources and contracts C155 reads, copied into a scratch tree: `.rs`, `Cargo.toml` and `paddock.ci.json` files of the folders
/// that declare `hostTests`, without build output.
fn copy_tree(from: &Path, to: &Path, rel: &str) {
    for e in std::fs::read_dir(from.join(rel)).unwrap().flatten() {
        let name = e.file_name().into_string().unwrap();
        let sub = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let ty = e.file_type().unwrap();
        if ty.is_dir() {
            if ["target", "node_modules", ".git", "dist", ".angular", "fixtures", "golden"].contains(&name.as_str()) {
                continue;
            }
            copy_tree(from, to, &sub);
        } else if ty.is_file() && (name.ends_with(".rs") || name == "Cargo.toml" || name == "paddock.ci.json") {
            let dest = to.join(&sub);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn scratch() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = root();
    for top in ["crates", "apps", "tools/paddock-dev"] {
        copy_tree(&root, d.path(), top);
    }
    d
}

fn run(root: &Path) -> (Vec<(String, String)>, usize) {
    let opts = RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare("paddock"),
        scope: None,
        only: Some(vec!["C155".into()]),
        files: None,
        today: "2026-11-22".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    };
    let r = run_checks(&opts, &registry()).unwrap();
    let probes = r.runs.iter().map(|x| x.record.probes).sum();
    (r.findings.iter().map(|f| (f.file.clone(), f.message.clone())).collect(), probes)
}

fn edit_contract(root: &Path, rel: &str, f: impl Fn(&mut Value)) {
    let path = root.join(rel);
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    f(&mut v);
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

fn without(list: &mut Value, entry: &str) {
    let a = list.as_array_mut().unwrap();
    let before = a.len();
    a.retain(|e| e != entry);
    assert_eq!(a.len() + 1, before, "`{entry}` is in the list the control edits");
}

#[test]
fn the_real_tree_has_no_os_code_outside_host_tests_and_the_check_read_many_files() {
    let d = scratch();
    let (findings, probes) = run(d.path());
    assert!(findings.is_empty(), "C155 on the real tree:\n{findings:#?}");
    assert!(
        probes > 1500,
        "a check that read almost nothing proves nothing (ER-33): {probes} source files"
    );
}

#[test]
fn unlisting_a_package_with_os_code_goes_red() {
    let d = scratch();
    edit_contract(d.path(), "crates/paddock.ci.json", |v| {
        without(&mut v["hostTests"]["packages"], "paddock-proc")
    });
    let (findings, _) = run(d.path());
    assert!(
        findings
            .iter()
            .any(|(f, m)| f.starts_with("crates/paddock-proc/") && m.contains("[os-code] `paddock-proc`")),
        "{findings:#?}"
    );
}

#[test]
fn unlisting_local_sccache_goes_red_through_the_import_of_ci_setup_rust() {
    let d = scratch();
    edit_contract(d.path(), "tools/paddock-dev/paddock.ci.json", |v| {
        without(&mut v["hostTests"]["modules"], "local_sccache::")
    });
    let (findings, _) = run(d.path());
    assert!(
        findings
            .iter()
            .any(|(f, m)| f.ends_with("src/ci_setup_rust.rs") && m.contains("[import-edge]") && m.contains("crate::local_sccache")),
        "{findings:#?}"
    );
}

#[test]
fn unlisting_pre_push_exec_goes_red_through_the_import_of_ci_run() {
    let d = scratch();
    edit_contract(d.path(), "tools/paddock-dev/paddock.ci.json", |v| {
        without(&mut v["hostTests"]["modules"], "pre_push::exec::")
    });
    let (findings, _) = run(d.path());
    assert!(
        findings
            .iter()
            .any(|(f, m)| f.ends_with("src/ci_run/mod.rs") && m.contains("[import-edge]") && m.contains("crate::pre_push::exec")),
        "{findings:#?}"
    );
}

#[test]
fn every_declared_scope_is_unscoped_on_linux_and_scoped_on_the_hosts() {
    let root = root();
    let mut declared = 0;
    for rel in [
        "crates",
        "apps/dyno",
        "apps/pitcrew",
        "apps/pitwall",
        "apps/spotter",
        "apps/debrief",
        "tools/paddock-dev",
    ] {
        let text = std::fs::read_to_string(root.join(rel).join("paddock.ci.json")).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let h = HostTests::parse(&v, rel)
            .unwrap()
            .unwrap_or_else(|| panic!("{rel} declares hostTests"));
        declared += 1;
        let linux = h.scope(LINUX);
        assert!(linux.packages.is_empty() && linux.filter.is_empty(), "{rel}: Linux runs everything");
        for os in ["windows-latest", "macos-latest"] {
            let s = h.scope(os);
            assert!(!s.packages.is_empty() || !s.filter.is_empty(), "{rel} on {os} is scoped");
        }
    }
    assert_eq!(declared, 7);
}
