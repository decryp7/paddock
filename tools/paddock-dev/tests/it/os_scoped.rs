//! OS-scoped tests (`hostTests`, docs/research/2026-11-22-os-scoped-tests.md, user decision 2026-11-22; ADR 0117 §7, WP-6), on the real
//! contracts and the real sources. The placement is derived from the code and the contracts hold only exceptions: conformance C155
//! finds nothing on this tree; the derived placement holds the cases that made the old import rule (`ci_setup_rust` imports
//! `local_sccache`, `ci_run` imports `pre_push::exec`) and a package with OS code; each exception that takes an OS away goes red when
//! it is new (ER-46) or contradicts the code (ER-33). The Linux leg is never scoped: what leaves a host leg still runs there.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};
use paddock_dev::host_tests::{LINUX, read};
use paddock_dev::os_placement::{DiskBulk, derive, workspaces};
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
    run_with(root, Some("origin/main"))
}

fn run_with(root: &Path, base_ref: Option<&str>) -> (Vec<(String, String)>, usize) {
    let opts = RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare("paddock"),
        scope: None,
        only: Some(vec!["C155".into()]),
        files: None,
        today: "2026-11-22".into(),
        base_ref: base_ref.map(str::to_string),
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

/// `git <args>` in `dir` as a throwaway committer (no hook, no signing, no global config).
fn git(dir: &Path, args: &[&str]) {
    let out = paddock_proc::command("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn tracked(root: &Path) -> Vec<String> {
    let out = paddock_proc::command("git").args(["ls-files"]).current_dir(root).output().unwrap();
    String::from_utf8(out.stdout).unwrap().lines().map(str::to_string).collect()
}

fn real_files() -> (PathBuf, Vec<String>) {
    let root = root();
    let files = tracked(&root);
    (root, files)
}

#[test]
fn the_real_tree_has_no_exception_that_contradicts_the_code_and_the_check_read_many_files() {
    let d = scratch();
    let (findings, probes) = run(d.path());
    assert!(findings.is_empty(), "C155 on the real tree:\n{findings:#?}");
    assert!(
        probes > 1500,
        "a check that read almost nothing proves nothing (ER-33): {probes} source files"
    );
}

#[test]
fn the_derived_placement_holds_the_packages_and_modules_the_hand_lists_named() {
    let (root, files) = real_files();
    let bulk = DiskBulk(root.clone());
    let crates = derive("crates", &files, &bulk).unwrap();
    for p in ["paddock-proc", "paddock-env", "paddock-service"] {
        assert!(crates.derived.packages.contains(p), "{p} has OS code: {:?}", crates.why.get(p));
    }
    assert!(
        crates.derived.known_packages.len() > crates.derived.packages.len(),
        "some crates are portable"
    );
    assert!(
        !crates.derived.modules.is_empty() || crates.derived.known_modules.is_empty(),
        "crates are judged by package"
    );
    let dev = derive("tools/paddock-dev", &files, &bulk).unwrap();
    assert!(dev.derived.packages.len() <= 1, "paddock-dev is judged by module, not by package");
    // The two cases that made the old import rule: a host module calls these, so their tests run where the caller's do.
    for m in ["local_sccache::", "pre_push::exec::"] {
        assert!(dev.derived.modules.contains(m), "{m}: {:?}", dev.why.get(m));
    }
    // Planner modules have no OS code and no host caller: Linux only.
    for m in ["audit::judge::", "check::c121::"] {
        assert!(
            dev.derived.known_modules.contains(m) && !dev.derived.modules.contains(m),
            "{m} is portable: {:?}",
            dev.why.get(m)
        );
    }
}

#[test]
fn a_module_that_a_host_module_calls_is_derived_and_a_pure_one_nobody_calls_is_not() {
    let d = scratch();
    let src = d.path().join("tools/paddock-dev/src");
    std::fs::write(src.join("zz_callee.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(src.join("zz_pure.rs"), "pub fn g() {}\n").unwrap();
    std::fs::write(
        src.join("zz_host.rs"),
        "use crate::zz_callee::f;\n#[cfg(windows)]\nfn w() { f(); }\n",
    )
    .unwrap();
    let files = tracked_plus(
        d.path(),
        &[
            "tools/paddock-dev/src/zz_callee.rs",
            "tools/paddock-dev/src/zz_pure.rs",
            "tools/paddock-dev/src/zz_host.rs",
        ],
    );
    let with = derive("tools/paddock-dev", &files, &DiskBulk(d.path().to_path_buf())).unwrap();
    assert!(with.derived.modules.contains("zz_host::") && with.derived.modules.contains("zz_callee::"));
    assert!(with.derived.known_modules.contains("zz_pure::") && !with.derived.modules.contains("zz_pure::"));
    // Positive control: without the import line the callee is portable again.
    std::fs::write(src.join("zz_host.rs"), "#[cfg(windows)]\nfn w() {}\n").unwrap();
    let without = derive("tools/paddock-dev", &files, &DiskBulk(d.path().to_path_buf())).unwrap();
    assert!(without.derived.modules.contains("zz_host::") && !without.derived.modules.contains("zz_callee::"));
}

/// The real tree's tracked files plus new ones of the scratch tree (the scratch is not a checkout).
fn tracked_plus(scratch: &Path, extra: &[&str]) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    fn walk(base: &Path, rel: &str, out: &mut Vec<String>) {
        for e in std::fs::read_dir(base.join(rel)).unwrap().flatten() {
            let name = e.file_name().into_string().unwrap();
            let sub = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            if e.file_type().unwrap().is_dir() {
                walk(base, &sub, out);
            } else {
                out.push(sub);
            }
        }
    }
    walk(scratch, "", &mut files);
    for x in extra {
        assert!(files.iter().any(|f| f == x), "{x} is in the scratch tree");
    }
    files
}

#[test]
fn a_new_package_with_an_os_cfg_is_derived_and_without_it_is_not() {
    let d = scratch();
    let dir = d.path().join("crates/zz-new");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"zz-new\"\n").unwrap();
    std::fs::write(dir.join("src/lib.rs"), "#[cfg(windows)]\nfn w() {}\n").unwrap();
    let files = tracked_plus(d.path(), &["crates/zz-new/src/lib.rs"]);
    let with = derive("crates", &files, &DiskBulk(d.path().to_path_buf())).unwrap();
    assert!(with.derived.packages.contains("zz-new"));
    std::fs::write(dir.join("src/lib.rs"), "fn w() {}\n").unwrap();
    let without = derive("crates", &files, &DiskBulk(d.path().to_path_buf())).unwrap();
    assert!(without.derived.known_packages.contains("zz-new") && !without.derived.packages.contains("zz-new"));
}

#[test]
fn a_tauri_shell_in_last_packages_stays_off_the_host_legs_though_the_code_marks_it() {
    let (root, files) = real_files();
    let p = derive("apps/pitwall", &files, &DiskBulk(root.clone())).unwrap();
    assert!(
        p.derived.packages.contains("pitwall-app"),
        "the shell has OS code: {:?}",
        p.why.get("pitwall-app")
    );
    let v: Value = serde_json::from_str(&std::fs::read_to_string(root.join("apps/pitwall/paddock.ci.json")).unwrap()).unwrap();
    let h = read(&v, "apps/pitwall/paddock.ci.json", &p.derived).unwrap().unwrap();
    assert!(!h.scope("windows-latest").packages.contains(&"pitwall-app".to_string()));
    assert!(h.scope("windows-latest").packages.contains(&"pitwall-server".to_string()));
}

#[test]
fn a_skip_over_a_module_with_os_code_goes_red_and_a_skip_over_pure_logic_does_not() {
    let d = scratch();
    std::fs::write(d.path().join("crates/paddock-proc/src/zz_probe.rs"), "#[cfg(windows)]\nfn w() {}\n").unwrap();
    std::fs::write(d.path().join("crates/paddock-proc/src/zz_pure.rs"), "fn p() {}\n").unwrap();
    let skip = |modules: &[&str]| {
        let m: Vec<Value> = modules.iter().map(|m| Value::from(*m)).collect();
        edit_contract(d.path(), "crates/paddock.ci.json", |v| {
            let list = v["hostTests"]["skip"].as_array_mut().unwrap();
            list.retain(|e| e["name"] != "paddock-proc");
            list.push(serde_json::json!({"name": "paddock-proc", "modules": m, "reason": "a module the control claims is pure"}));
        });
    };
    skip(&["zz_pure::"]);
    let (findings, _) = run(d.path());
    assert!(findings.is_empty(), "{findings:#?}");
    skip(&["zz_probe::"]);
    let (findings, _) = run(d.path());
    assert!(
        findings
            .iter()
            .any(|(f, m)| f.ends_with("zz_probe.rs") && m.contains("[skipped-os-code]")),
        "{findings:#?}"
    );
}

#[test]
fn a_stale_and_a_redundant_exception_go_red() {
    let d = scratch();
    edit_contract(d.path(), "crates/paddock.ci.json", |v| {
        let add = serde_json::json!([
            {"name": "paddock-no-such-crate", "reason": "a typo the control makes"},
            {"name": "paddock-proc", "reason": "already derived from its code"}
        ]);
        v["hostTests"]["add"] = add;
    });
    let (findings, _) = run(d.path());
    assert!(
        findings
            .iter()
            .any(|(_, m)| m.contains("[stale-entry]") && m.contains("paddock-no-such-crate")),
        "{findings:#?}"
    );
    assert!(
        findings
            .iter()
            .any(|(_, m)| m.contains("[redundant-exception]") && m.contains("`paddock-proc`")),
        "{findings:#?}"
    );
}

#[test]
fn a_new_skip_or_only_entry_is_an_os_removal_that_needs_the_user_and_an_add_is_free() {
    let d = scratch();
    let root = d.path();
    git(root, &["init", "-q", "-b", "work"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chore: base"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    // An add that is no removal: the code does not mark it, so it is an exception that is free, and no finding is about removal.
    edit_contract(root, "crates/paddock.ci.json", |v| {
        v["hostTests"]["add"] = serde_json::json!([{"name": "paddock-names", "reason": "a control for the free direction"}]);
    });
    git(root, &["commit", "-qam", "chore: an add"]);
    let (findings, _) = run(root);
    assert!(!findings.iter().any(|(_, m)| m.contains("[os-removal]")), "{findings:#?}");
    // A new `only`: it takes Windows away from a package the code puts there.
    edit_contract(root, "crates/paddock.ci.json", |v| {
        v["hostTests"]["only"] =
            serde_json::json!([{"os": "macos-latest", "name": "paddock-proc", "reason": "a control for the unsafe direction"}]);
    });
    git(root, &["commit", "-qam", "chore: an only"]);
    let (findings, _) = run(root);
    assert!(
        findings
            .iter()
            .any(|(f, m)| f == "crates/paddock.ci.json" && m.contains("[os-removal]") && m.contains("1 `skip` or `only` entry")),
        "{findings:#?}"
    );
}

#[test]
fn every_declared_scope_is_unscoped_on_linux_and_scoped_on_the_hosts() {
    let (root, files) = real_files();
    let bulk = DiskBulk(root.clone());
    let mut declared = 0;
    for (ws, contract) in workspaces(&files) {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(root.join(&contract)).unwrap()).unwrap();
        if v.get("hostTests").is_none() {
            continue;
        }
        let p = derive(&ws, &files, &bulk).unwrap();
        let h = read(&v, &contract, &p.derived)
            .unwrap()
            .unwrap_or_else(|| panic!("{ws} declares hostTests and has OS code"));
        declared += 1;
        let linux = h.scope(LINUX);
        assert!(linux.packages.is_empty() && linux.filter.is_empty(), "{ws}: Linux runs everything");
        for os in ["windows-latest", "macos-latest"] {
            let s = h.scope(os);
            assert!(!s.packages.is_empty() || !s.filter.is_empty(), "{ws} on {os} is scoped");
        }
    }
    assert_eq!(declared, 7);
}
