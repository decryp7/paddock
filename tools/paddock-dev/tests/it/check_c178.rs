//! C178 (paddock-dev's modules refer only to their own layer or a lower one): the Rust host over fixture trees, the positive control,
//! the missing and broken table, and this working tree. Rust only: there is no JavaScript twin.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, repo_root};
use paddock_dev::check::c178::{LAYERS, Layers};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use std::path::Path;

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C178".into()]),
        files: None,
        today: "2026-11-27".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    }
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

const TABLE: &str =
    "[[layer]]\nname = \"low\"\nmodules = [\"a\", \"lib\"]\n\n[[layer]]\nname = \"high\"\nuses = [\"low\"]\nmodules = [\"b\"]\n";
const LIB: &str = "pub mod a;\npub mod b;\n";

/// Positive control for the fixture: an upward reference is an error at its line (the check blocks since 2026-11-28), and the same
/// reference downward is green.
#[test]
fn c178_reports_an_upward_reference_as_an_error() {
    let tree = tree_of(&[
        (LAYERS, TABLE),
        ("tools/paddock-dev/src/lib.rs", LIB),
        ("tools/paddock-dev/src/a.rs", "pub fn f() {\n    crate::b::g();\n}\n"),
        ("tools/paddock-dev/src/b.rs", "pub fn g() {\n    crate::a::f();\n}\n"),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(got, vec![("tools/paddock-dev/src/a.rs", 2)], "{:?}", r.findings);
    let m = &r.findings[0].message;
    assert!(
        m.contains("`a`") && m.contains("`crate::b`") && m.contains("`high`"),
        "the edge and the layers are named: {m}"
    );
    assert_eq!((r.errors(), r.warnings()), (1, 0));
}

/// A module the table does not list, and an entry with no module, are reported; a tree with no upward reference is otherwise green.
#[test]
fn c178_reports_a_module_the_table_misses_and_a_stale_entry() {
    let tree = tree_of(&[
        (LAYERS, &TABLE.replace("\"b\"", "\"b\", \"gone\"")),
        ("tools/paddock-dev/src/lib.rs", LIB),
        ("tools/paddock-dev/src/a.rs", "pub fn f() {}\n"),
        ("tools/paddock-dev/src/b.rs", "pub fn g() {}\n"),
        ("tools/paddock-dev/src/extra/mod.rs", "pub fn h() {}\n"),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let msgs: Vec<&str> = r.findings.iter().map(|f| f.message.as_str()).collect();
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("module `extra` is in no layer")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("lists module `gone`")), "{msgs:?}");
}

/// The shapes the extractor must read: a use group, a path in a type, a macro's tokens; and the ones it must not: test code, comments,
/// strings, `self::` and `super::` paths, an item of lib.rs.
#[test]
fn c178_reads_use_groups_types_and_macros_but_not_test_code() {
    let src = "use crate::b::{X, Y};\nfn f(_: crate::b::T) {\n    m!(crate::b::Z);\n    // crate::b::C\n    let _ = \"crate::b::S\";\n    self::q();\n    super::r();\n    let _ = crate::ROOT;\n}\n#[cfg(test)]\nmod t {\n    use crate::b::U;\n}\n";
    let tree = tree_of(&[
        (LAYERS, TABLE),
        ("tools/paddock-dev/src/lib.rs", LIB),
        ("tools/paddock-dev/src/a.rs", src),
        ("tools/paddock-dev/src/b.rs", "pub fn g() {}\n"),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let lines: Vec<usize> = r.findings.iter().map(|f| f.line).collect();
    assert_eq!(lines, vec![1, 2, 3], "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the upward references.
#[test]
fn c178_control_sees_a_builder() {
    let tree = tree_of(&[
        (LAYERS, TABLE),
        ("tools/paddock-dev/src/lib.rs", "pub mod a;\n"),
        ("tools/paddock-dev/src/a.rs", "fn a() {}\n"),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C178").expect("a run record").record;
    assert!(record.control, "the control found its planted references");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the table": without it nothing was judged, which is never a pass (ER-33); a table that does not parse or lists a module
/// twice says so.
#[test]
fn c178_without_a_table_or_with_a_broken_one_is_not_measured() {
    let src = ("tools/paddock-dev/src/a.rs", "pub fn f() {}\n");
    for files in [
        vec![src],
        vec![src, (LAYERS, "[[layer]\nname = ")],
        vec![src, (LAYERS, "# no layers\n")],
        vec![
            src,
            (
                LAYERS,
                "[[layer]]\nname = \"x\"\nmodules = [\"a\"]\n[[layer]]\nname = \"y\"\nmodules = [\"a\"]\n",
            ),
        ],
    ] {
        let tree = tree_of(&files);
        let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
        assert!(r.errors() >= 1, "{:?}", r.findings);
        assert!(
            r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
            "{:?}",
            r.findings
        );
    }
}

/// A tree with a table and no source reads nothing, which is not a pass either.
#[test]
fn c178_without_source_files_is_not_measured() {
    let tree = tree_of(&[(LAYERS, TABLE)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
}

#[test]
fn the_table_parses_to_layers_and_what_they_use() {
    let l = Layers::parse(TABLE).unwrap();
    assert_eq!((l.layer("a"), l.layer("b"), l.layer("zz")), (Some(0), Some(1), None));
    assert_eq!(l.name(1), "high");
    assert!(l.may_use(1, 0) && l.may_use(1, 1) && !l.may_use(0, 1));
}

/// A diamond: two layers that use the same base and not each other may not refer to each other; the layer above both uses both, and
/// what a used layer uses (a chain through the diamond) is used too. A layer that uses an undeclared one is a broken table.
#[test]
fn a_diamond_keeps_its_siblings_apart() {
    let t = "[[layer]]\nname = \"core\"\nmodules = [\"c\"]\n[[layer]]\nname = \"left\"\nuses = [\"core\"]\nmodules = [\"l\"]\n[[layer]]\nname = \"right\"\nuses = [\"core\"]\nmodules = [\"r\"]\n[[layer]]\nname = \"top\"\nuses = [\"left\", \"right\"]\nmodules = [\"t\"]\n";
    let l = Layers::parse(t).unwrap();
    let at = |m: &str| l.layer(m).unwrap();
    assert!(!l.may_use(at("l"), at("r")) && !l.may_use(at("r"), at("l")), "siblings");
    assert!(
        l.may_use(at("t"), at("l")) && l.may_use(at("t"), at("r")) && l.may_use(at("t"), at("c")),
        "the top uses all, core through both"
    );
    assert!(l.may_use(at("l"), at("c")) && !l.may_use(at("c"), at("l")));
    let bad = "[[layer]]\nname = \"a\"\nuses = [\"b\"]\nmodules = []\n[[layer]]\nname = \"b\"\nmodules = []\n";
    assert!(Layers::parse(bad).unwrap_err().contains("not declared above"));
}

/// This working tree: the check runs over every production file and finds nothing: no module refers to a layer its own does not use, no
/// module of src/ is missing from the table and no entry is stale.
#[test]
fn live_c178_the_real_tree_has_no_back_edge_and_the_table_is_complete() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C178 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "{:?}", r.findings);
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    let table: Vec<_> = r
        .findings
        .iter()
        .filter(|f| f.message.contains("is in no layer") || f.message.contains("does not have"))
        .collect();
    assert!(table.is_empty(), "the layer table lags src/: {table:?}");
}
