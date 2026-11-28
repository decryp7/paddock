//! The lists that name the Rust host's checks are generated from the checks' own declarations (user decision, 2026-11-28, ER-40): the
//! registry and the test module lines by build.rs (`check::scan`), `conformance.rust` and the README table by `./dev check lists`
//! (`check::lists`). A new check is one file with its declaration; these tests hold that, the stale-list error and the byte-exact
//! round trip of the committed lists.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::repo_root;
use paddock_dev::check::lists::{self, CONTRACT, README};
use paddock_dev::check::record::Precondition;
use paddock_dev::check::{self, Check, CheckOutput, CheckScope, Control, Ctx, Mode, Origin, scan};
use std::path::Path;

/// A check that exists only as its declaration: no logic beyond what the trait needs.
struct Declared;

impl Check for Declared {
    fn id(&self) -> &'static str {
        "C999"
    }
    fn rule(&self) -> u32 {
        1
    }
    fn title(&self) -> &'static str {
        "a new | check declared once"
    }
    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }
    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }
    fn origin(&self) -> Origin {
        Origin::BornInRust
    }
    fn control(&self) -> Control {
        Control {
            files: vec![],
            expect: vec![],
        }
    }
    fn run(&self, _: &Ctx) -> CheckOutput {
        CheckOutput {
            findings: vec![],
            probes: 0,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: "x".into(),
            },
        }
    }
}

/// A folder with copies of the committed lists and the JavaScript module names, as `generate` reads them.
fn copy_of_the_lists() -> tempfile::TempDir {
    let root = repo_root();
    let d = tempfile::tempdir().unwrap();
    for p in [CONTRACT, README] {
        let to = d.path().join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(root.join(p), to).unwrap();
    }
    let js = d.path().join(lists::JS_CHECKS);
    std::fs::create_dir_all(&js).unwrap();
    for id in lists::js_module_ids(&root).unwrap() {
        std::fs::write(js.join(format!("{id}-x.mjs")), "").unwrap();
    }
    d
}

fn touch(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), "").unwrap();
}

/// A new check file with only its declaration reaches the registry source, the sources list and the test module lines, with no other
/// file edited.
#[test]
fn a_new_check_file_reaches_the_registry_and_the_test_module_lines() {
    let d = tempfile::tempdir().unwrap();
    let (checks, tests) = (d.path().join("src/check"), d.path().join("tests/it"));
    for f in ["c39.rs", "c120.rs", "c53_liveness.rs", "mod.rs", "scan.rs"] {
        touch(&checks, f);
    }
    for f in ["check_c120.rs", "check_engine.rs", "main.rs"] {
        touch(&tests, f);
    }
    let before = scan::registry_source(&checks).unwrap();
    assert!(!before.contains("c999"), "{before}");
    touch(&checks, "c999.rs");
    touch(&tests, "check_c999.rs");
    let after = scan::registry_source(&checks).unwrap();
    assert!(after.contains("pub mod c999;") && after.contains("Box::new(c999::C999)"), "{after}");
    assert!(
        after.contains("(c999::C999.id(), \"tools/paddock-dev/src/check/c999.rs\")"),
        "{after}"
    );
    // Id order, not text order: c39 before c120 before c999; a helper file (c53_liveness.rs) is no check.
    let at = |s: &str| after.find(&format!("Box::new({s}")).unwrap();
    assert!(at("c39::") < at("c120::") && at("c120::") < at("c999::"), "{after}");
    assert!(!after.contains("c53_liveness") && !after.contains("scan"), "{after}");
    let mods = scan::test_mods_source(&tests).unwrap();
    assert!(
        mods.contains("mod check_c120;") && mods.contains("mod check_c999;") && !mods.contains("check_engine"),
        "{mods}"
    );
}

/// The same new check reaches the two committed lists through `generate` once it is in the registry; a check with a JavaScript module
/// is not in `conformance.rust`.
#[test]
fn a_new_check_declaration_reaches_conformance_rust_and_the_readme_table() {
    let d = copy_of_the_lists();
    let mut registry = check::registry();
    registry.push(Box::new(Declared));
    let out = lists::generate(d.path(), &registry).unwrap();
    let contract: serde_json::Value = serde_json::from_str(&out[0].1).unwrap();
    let rust: Vec<&str> = contract["conformance"]["rust"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(rust.last(), Some(&"C999"), "{rust:?}");
    assert!(!rust.contains(&"C139"), "C139 has a JavaScript module: {rust:?}");
    let row = out[1]
        .1
        .lines()
        .find(|l| l.starts_with("| C999 |"))
        .expect("a README row for the new check");
    assert_eq!(
        row,
        "| C999 | a new \\| check declared once | root | report-only | all | born in Rust |"
    );
}

/// The committed lists are what the generator writes, byte for byte (the JSON round trip keeps the file's own formatting), so
/// `./dev gen check` is quiet on a clean tree.
#[test]
fn the_committed_lists_are_what_the_generator_writes() {
    let root = repo_root();
    for (path, want) in lists::generate(&root, &check::registry()).unwrap() {
        let have = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(
            have == want,
            "{path} is stale: run `./dev check lists`; {:?}",
            lists::line_diff(&have, &want)
        );
    }
}

/// Positive control (ER-33): `--check` goes red on a stale README and on a stale contract, names the file, and writing makes it quiet.
#[test]
fn check_flags_a_stale_list_and_regenerating_fixes_it() {
    let d = copy_of_the_lists();
    let root = Some(d.path().to_path_buf());
    assert!(
        lists::command(root.clone(), true).is_ok(),
        "the copy of the committed lists is current"
    );
    let readme = d.path().join(README);
    let text = std::fs::read_to_string(&readme).unwrap();
    std::fs::write(&readme, text.replace("| C183 |", "| C18X |")).unwrap();
    let contract = d.path().join(CONTRACT);
    let text = std::fs::read_to_string(&contract).unwrap();
    std::fs::write(&contract, text.replace("\"C183\"", "\"C18X\"")).unwrap();
    let err = lists::command(root.clone(), true).expect_err("a stale list must fail --check");
    let said = err.text();
    assert!(
        said.contains(README) && said.contains(CONTRACT) && said.contains("./dev check lists"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(&readme).unwrap().contains("| C18X |"),
        true,
        "--check changes nothing"
    );
    lists::command(root.clone(), false).unwrap();
    assert!(std::fs::read_to_string(&readme).unwrap().contains("| C183 |"));
    assert!(lists::command(root, true).is_ok(), "after writing, the lists are current");
}

/// Missing markers or a contract without `conformance` are errors that name the file, never a silent pass.
#[test]
fn a_list_that_cannot_be_written_is_an_error_naming_the_file() {
    let e = lists::readme_text("no markers here\n", &[]).unwrap_err();
    assert!(e.contains(README) && e.contains("rust-checks:begin"), "{e}");
    let e = lists::contract_text("{\"x\": 1}", &["C1"]).unwrap_err();
    assert!(e.contains(CONTRACT) && e.contains("conformance"), "{e}");
}

/// Every check declares its origin once: a check with a JavaScript module is `Ported`, and one without is `BornInRust` or has recorded
/// JavaScript answers (the parity test in `check_engine.rs`); here, only that nothing is both.
#[test]
fn no_check_is_born_in_rust_and_has_a_javascript_module() {
    let js = lists::js_module_ids(&repo_root()).unwrap();
    for c in check::registry() {
        assert!(
            !(c.origin() == Origin::BornInRust && js.contains(c.id())),
            "{} has a JavaScript module, so it was not born in Rust: drop `fn origin` from its impl",
            c.id()
        );
    }
}
