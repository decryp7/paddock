#![allow(clippy::unwrap_used, clippy::expect_used)]

//! No test double is declared outside a test (Dyno ADR 0033 section 2, "a declaration check").
//!
//! The scan parses the non-test sources of every shipped crate with `syn` and fails on, unless the item is under `cfg(test)` or
//! `cfg(any(test, feature = "test-support"))`:
//! - a public item named `Fake*`;
//! - a `mod fake` or `mod testing`;
//! - an `impl` of a trait for a type named `Fake*`, `Stub*` or `Scripted*`.
//!
//! The shipped set is not hand-kept: `dyno-app` plus the package of every `bundle.externalBin` of `tauri.conf.json` (the helper
//! included), plus their normal path dependencies inside `apps/dyno` (read from `cargo metadata`). Family crates are out of scope.
//! Known limits: it does not look into inherent `impl` blocks (`impl X { pub fn fake() }`) or `macro_rules!` output, and family
//! crates (`paddock_jobs::FakeClock`, `FakeSystemd`, `FakeWire`, `FakeToaster`) are a follow-up (ADR 0033 review P2-4).
//! This is a declaration check, not a grep for use sites: it catches a new ungated double on any OS, before anyone wires it. The
//! no-fake tests in each OS module and `scripts/check-shipped.test.mjs` cover the rest.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- the scan

/// What a `cfg` condition can be when `test` and the `test-support` feature are off (the build that ships).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tri {
    /// Never true in that build: the item is not compiled there.
    False,
    /// Always true.
    True,
    /// Depends on the target, another feature or a flag: it may be compiled in the build that ships.
    Maybe,
}

fn not(t: Tri) -> Tri {
    match t {
        Tri::False => Tri::True,
        Tri::True => Tri::False,
        Tri::Maybe => Tri::Maybe,
    }
}

/// Evaluates one `cfg` predicate: `test` and `feature = "test-support"` are off; `any`, `all` and `not` combine; anything else
/// (`windows`, `target_os`, another feature, `debug_assertions`) is `Maybe`.
fn eval_cfg(meta: &syn::Meta) -> Tri {
    match meta {
        syn::Meta::Path(p) if p.is_ident("test") => Tri::False,
        syn::Meta::NameValue(nv) if nv.path.is_ident("feature") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) if s.value() == "test-support" => Tri::False,
            _ => Tri::Maybe,
        },
        syn::Meta::List(l) if l.path.is_ident("any") || l.path.is_ident("all") || l.path.is_ident("not") => {
            use syn::punctuated::Punctuated;
            let Ok(args) = l.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated) else {
                return Tri::Maybe;
            };
            let vals: Vec<Tri> = args.iter().map(eval_cfg).collect();
            if l.path.is_ident("not") {
                return vals.first().map_or(Tri::Maybe, |t| not(*t));
            }
            let any = l.path.is_ident("any");
            let (decisive, neutral) = if any { (Tri::True, Tri::False) } else { (Tri::False, Tri::True) };
            if vals.contains(&decisive) {
                decisive
            } else if vals.iter().all(|t| *t == neutral) {
                neutral
            } else {
                Tri::Maybe
            }
        }
        _ => Tri::Maybe,
    }
}

/// Whether the attributes keep the item out of the build that ships: `#[test]`, or a `cfg` that is false with `test` and the
/// `test-support` feature off. `cfg(any(test, windows))` is not a gate (it is compiled on Windows); neither is `feature = "test"`.
fn gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if !a.path().is_ident("cfg") {
            return false;
        }
        a.parse_args::<syn::Meta>().is_ok_and(|m| eval_cfg(&m) == Tri::False)
    })
}

fn is_pub(v: &syn::Visibility) -> bool {
    !matches!(v, syn::Visibility::Inherited)
}

fn is_double_name(name: &str) -> bool {
    name.starts_with("Fake")
}

fn is_double_impl_name(name: &str) -> bool {
    ["Fake", "Stub", "Scripted"].iter().any(|p| name.starts_with(p))
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => None,
    }
}

fn pub_item_name(item: &syn::Item) -> Option<(&syn::Visibility, String)> {
    match item {
        syn::Item::Struct(i) => Some((&i.vis, i.ident.to_string())),
        syn::Item::Enum(i) => Some((&i.vis, i.ident.to_string())),
        syn::Item::Trait(i) => Some((&i.vis, i.ident.to_string())),
        syn::Item::Type(i) => Some((&i.vis, i.ident.to_string())),
        syn::Item::Fn(i) => Some((&i.vis, i.sig.ident.to_string())),
        syn::Item::Const(i) => Some((&i.vis, i.ident.to_string())),
        syn::Item::Static(i) => Some((&i.vis, i.ident.to_string())),
        syn::Item::Mod(i) => Some((&i.vis, i.ident.to_string())),
        _ => None,
    }
}

/// Where the `mod name;` declarations of a file are found: the folder holding `name.rs` or `name/mod.rs`.
fn child_dir(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    match file.file_stem().and_then(|s| s.to_str()) {
        Some("lib" | "main" | "mod") | None => dir.to_path_buf(),
        Some(stem) => dir.join(stem),
    }
}

/// Reads the file a `mod x;` names: `(folder of x.rs, folder of the declaring file, name, #[path])` to `(file, source)`. A fixture
/// test supplies its own.
type Loader = dyn Fn(&Path, &Path, &str, Option<&str>) -> Option<(PathBuf, String)>;

/// What a scan found and what it saw (the counts are the positive control, ER-33).
#[derive(Default)]
struct Report {
    offences: Vec<String>,
    /// Source files parsed.
    files: usize,
    /// The files that declare a `mod fake` or `mod testing` that is gated, i.e. doubles the scan met and passed.
    gated_double_mods: Vec<String>,
}

struct Scan<'a> {
    load: &'a Loader,
    report: Report,
}

fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find(|a| a.path().is_ident("path")).and_then(|a| match &a.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

impl Scan<'_> {
    fn items(&mut self, items: &[syn::Item], file: &Path, dir: &Path) {
        for item in items {
            self.item(item, file, dir);
        }
    }

    fn item(&mut self, item: &syn::Item, file: &Path, dir: &Path) {
        let attrs: &[syn::Attribute] = match item {
            syn::Item::Struct(i) => &i.attrs,
            syn::Item::Enum(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            syn::Item::Type(i) => &i.attrs,
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Use(i) => &i.attrs,
            _ => &[],
        };
        if gated(attrs) {
            if let syn::Item::Mod(m) = item {
                if m.ident == "fake" || m.ident == "testing" {
                    self.report.gated_double_mods.push(file.display().to_string());
                }
            }
            return;
        }
        let at = file.display();
        if let Some((vis, name)) = pub_item_name(item) {
            if is_pub(vis) && is_double_name(&name) {
                self.report.offences.push(format!(
                    "{at}: public item {name} is a test double outside cfg(test) or the test-support feature"
                ));
            }
        }
        match item {
            syn::Item::Mod(m) => {
                let name = m.ident.to_string();
                if name == "fake" || name == "testing" {
                    self.report
                        .offences
                        .push(format!("{at}: `mod {name}` is not gated by cfg(test) or the test-support feature"));
                    return;
                }
                if let Some((_, inner)) = &m.content {
                    self.items(inner, file, &dir.join(&name));
                } else {
                    let file_dir = file.parent().unwrap_or(Path::new("."));
                    match (self.load)(dir, file_dir, &name, path_attr(&m.attrs).as_deref()) {
                        Some((path, src)) => self.file(&src, &path),
                        None => self.report.offences.push(format!(
                            "{at}: `mod {name};` has no source file, so the scan cannot see what it declares"
                        )),
                    }
                }
            }
            syn::Item::Impl(i) if i.trait_.is_some() => {
                if let Some(name) = type_name(&i.self_ty) {
                    if is_double_impl_name(&name) {
                        self.report.offences.push(format!(
                            "{at}: trait impl for {name}, a test double outside cfg(test) or the test-support feature"
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

impl Scan<'_> {
    /// Parses one file and scans its items, following its `mod x;` declarations.
    fn file(&mut self, src: &str, path: &Path) {
        self.report.files += 1;
        match syn::parse_file(src) {
            Ok(f) => self.items(&f.items, path, &child_dir(path)),
            Err(e) => self.report.offences.push(format!("{}: cannot parse: {e}", path.display())),
        }
    }
}

/// The report for one source text, where `mod x;` is resolved by `load`.
fn report_in(src: &str, file: &Path, load: &Loader) -> Report {
    let mut scan = Scan {
        load,
        report: Report::default(),
    };
    scan.file(src, file);
    scan.report
}

fn offences_in(src: &str, file: &Path, load: &Loader) -> Vec<String> {
    report_in(src, file, load).offences
}

/// The disk loader: `#[path]` relative to the declaring file's folder, else `dir/name.rs`, else `dir/name/mod.rs`.
fn load_from_disk(dir: &Path, file_dir: &Path, name: &str, path: Option<&str>) -> Option<(PathBuf, String)> {
    let candidates = match path {
        Some(p) => vec![file_dir.join(p)],
        None => vec![dir.join(format!("{name}.rs")), dir.join(name).join("mod.rs")],
    };
    candidates
        .into_iter()
        .find_map(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s)))
}

fn report_of_root(root: &Path) -> Report {
    let src = std::fs::read_to_string(root).unwrap();
    report_in(&src, root, &load_from_disk)
}

// ------------------------------------------------------- the fixtures (they run every time)

fn none(_: &Path, _: &Path, _: &str, _: Option<&str>) -> Option<(PathBuf, String)> {
    None
}

fn scan(src: &str) -> Vec<String> {
    offences_in(src, Path::new("fixture/lib.rs"), &none)
}

#[test]
fn an_ungated_public_fake_is_found() {
    let found = scan("pub struct FakeThing; impl Port for FakeThing {}");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("public item FakeThing"));
    assert!(found[1].contains("trait impl for FakeThing"));
    assert_eq!(scan("pub enum FakeOp { A }").len(), 1);
    assert_eq!(scan("pub(crate) fn FakeMaker() {}").len(), 1);
}

#[test]
fn an_ungated_mod_fake_or_testing_is_found() {
    assert_eq!(scan("pub mod fake;").len(), 1);
    assert_eq!(scan("mod testing { pub fn helper() {} }").len(), 1);
    assert_eq!(scan("pub mod fake { }").len(), 1);
}

#[test]
fn a_trait_impl_for_a_stub_or_scripted_type_is_found() {
    assert_eq!(scan("struct StubSource; impl Source for StubSource {}").len(), 1);
    assert_eq!(scan("struct Scripted; impl Source for Scripted {}").len(), 1);
    // A plain impl with no trait is not an adapter of a port.
    assert!(scan("struct Scripted; impl Scripted { fn new() -> Self { Scripted } }").is_empty());
}

#[test]
fn a_double_inside_an_ungated_inline_mod_is_found() {
    assert_eq!(scan("mod inner { pub struct FakeWire; }").len(), 1);
}

#[test]
fn gated_doubles_pass() {
    let ok = r#"
        #[cfg(test)] pub struct FakeA;
        #[cfg(any(test, feature = "test-support"))] pub struct FakeB;
        #[cfg(any(test, feature = "test-support"))] pub mod fake;
        #[cfg(all(test, unix))] mod testing { pub struct FakeC; }
        #[cfg(feature = "test-support")] impl Port for FakeD {}
        #[cfg(test)] mod tests { pub struct FakeE; impl Port for FakeE {} }
        pub struct Real; impl Port for Real {}
        struct private_fake_named_lower;
    "#;
    assert!(scan(ok).is_empty(), "{:?}", scan(ok));
}

#[test]
fn a_cfg_that_can_be_true_in_the_shipped_build_is_not_a_gate() {
    for cfg in [
        "any(test, windows)",
        "any(test, target_os = \"macos\")",
        "any(test, feature = \"testdb\")",
        "feature = \"test\"",
        "feature = \"testing\"",
        "not(feature = \"test-support\")",
        "debug_assertions",
        "any(test, not(test))",
    ] {
        let found = scan(&format!("#[cfg({cfg})] pub struct FakeA;"));
        assert_eq!(found.len(), 1, "cfg({cfg}) must not hide a double: {found:?}");
    }
}

#[test]
fn a_cfg_that_is_false_without_test_and_test_support_is_a_gate() {
    for cfg in [
        "test",
        "feature = \"test-support\"",
        "any(test, feature = \"test-support\")",
        "all(test, windows)",
        "all(feature = \"test-support\", unix)",
        "not(not(test))",
    ] {
        assert!(scan(&format!("#[cfg({cfg})] pub struct FakeA;")).is_empty(), "cfg({cfg}) is a gate");
    }
}

#[test]
fn a_missing_mod_file_is_an_offence_and_a_path_attribute_is_followed() {
    let found = scan("mod ghost;");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("`mod ghost;` has no source file"));
    let load = |_: &Path, file_dir: &Path, name: &str, path: Option<&str>| -> Option<(PathBuf, String)> {
        match (name, path) {
            ("rig", Some("common/rig.rs")) => Some((file_dir.join("common/rig.rs"), "pub struct FakeRig;".into())),
            _ => None,
        }
    };
    let found = offences_in("#[path = \"common/rig.rs\"] mod rig;", Path::new("fixture/lib.rs"), &load);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("rig.rs") && found[0].contains("FakeRig"), "{found:?}");
    // A gated mod with no file is not an offence: it is not compiled.
    assert!(scan("#[cfg(test)] mod ghost;").is_empty());
}

#[test]
fn the_report_counts_files_and_the_gated_double_modules_it_met() {
    let load = |dir: &Path, _: &Path, name: &str, _: Option<&str>| -> Option<(PathBuf, String)> {
        (name == "inner").then(|| (dir.join("inner.rs"), "pub fn f() {}".into()))
    };
    let r = report_in(
        "mod inner; #[cfg(any(test, feature = \"test-support\"))] pub mod fake;",
        Path::new("fixture/lib.rs"),
        &load,
    );
    assert!(r.offences.is_empty(), "{:?}", r.offences);
    assert_eq!(r.files, 2);
    assert_eq!(r.gated_double_mods, vec!["fixture/lib.rs".to_string()]);
}

#[test]
fn a_negated_gate_is_not_a_gate() {
    assert_eq!(scan("#[cfg(not(test))] pub struct FakeA;").len(), 1);
}

#[test]
fn a_file_module_is_followed_and_a_gated_one_is_not() {
    let load = |dir: &Path, _: &Path, name: &str, _: Option<&str>| -> Option<(PathBuf, String)> {
        match name {
            "bad" => Some((dir.join("bad.rs"), "pub struct FakeInFile;".into())),
            "good" => Some((dir.join("good.rs"), "pub struct FakeNeverRead;".into())),
            _ => None,
        }
    };
    let found = offences_in("mod bad; #[cfg(test)] mod good;", Path::new("fixture/lib.rs"), &load);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("bad.rs") && found[0].contains("FakeInFile"));
}

// ---------------------------------------------------------------- the real tree

/// `(name, manifest dir, normal path dependencies' names, root source files)` of every package of the workspace.
struct Package {
    dir: PathBuf,
    path_deps: Vec<String>,
    roots: Vec<PathBuf>,
    bins: Vec<String>,
}

fn workspace() -> (PathBuf, BTreeMap<String, Package>) {
    let dyno = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let mut cmd = dyno_platform::process::command(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(["metadata", "--no-deps", "--format-version", "1", "--offline"])
        .current_dir(&dyno);
    let out = dyno_platform::process::run_with_timeout(cmd, std::time::Duration::from_secs(120)).expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut packages = BTreeMap::new();
    for p in meta["packages"].as_array().unwrap() {
        let name = p["name"].as_str().unwrap().to_string();
        let dir = Path::new(p["manifest_path"].as_str().unwrap()).parent().unwrap().to_path_buf();
        let path_deps = p["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["kind"].is_null() && d["path"].is_string())
            .map(|d| d["name"].as_str().unwrap().to_string())
            .collect();
        let mut roots = Vec::new();
        let mut bins = Vec::new();
        for t in p["targets"].as_array().unwrap() {
            let kinds: Vec<_> = t["kind"].as_array().unwrap().iter().filter_map(|k| k.as_str()).collect();
            if kinds.iter().any(|k| ["lib", "rlib", "bin", "proc-macro"].contains(k)) {
                roots.push(PathBuf::from(t["src_path"].as_str().unwrap()));
            }
            if kinds.contains(&"bin") {
                bins.push(t["name"].as_str().unwrap().to_string());
            }
        }
        packages.insert(
            name,
            Package {
                dir,
                path_deps,
                roots,
                bins,
            },
        );
    }
    (dyno, packages)
}

/// `dyno-app` plus the package of each `bundle.externalBin`, and every normal path dependency inside `apps/dyno`.
fn shipped_set() -> BTreeSet<String> {
    let (dyno, packages) = workspace();
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dyno.join("app/src-tauri/tauri.conf.json")).unwrap()).unwrap();
    let mut todo = vec!["dyno-app".to_string()];
    for e in conf["bundle"]["externalBin"].as_array().unwrap() {
        let bin = e.as_str().unwrap().rsplit('/').next().unwrap();
        let owner = packages.iter().find(|(_, p)| p.bins.iter().any(|b| b == bin));
        todo.push(
            owner
                .unwrap_or_else(|| panic!("no package builds the bundled binary {bin}"))
                .0
                .clone(),
        );
    }
    let inside = |n: &str| {
        packages
            .get(n)
            .is_some_and(|p| p.dir.canonicalize().is_ok_and(|d| d.starts_with(&dyno)))
    };
    let mut seen = BTreeSet::new();
    while let Some(n) = todo.pop() {
        if !inside(&n) || !seen.insert(n.clone()) {
            continue;
        }
        todo.extend(packages[&n].path_deps.iter().cloned());
    }
    seen
}

#[test]
fn the_shipped_set_includes_the_helper_and_its_path_dependencies() {
    let set = shipped_set();
    for want in [
        "dyno-app",
        "dyno-agent",
        "dyno-cli",
        "dyno-bench",
        "dyno-bench-gpu",
        "dyno-helper",
        "dyno-core",
        "dyno-platform",
    ] {
        assert!(set.contains(want), "{want} missing from {set:?}");
    }
    assert!(!set.contains("paddock-power"), "family crates are out of scope");
}

/// Fewer files than this means the scan lost most of the tree (today about 180; the margin is for deletions).
const MIN_FILES: usize = 100;

#[test]
fn no_shipped_crate_declares_an_ungated_test_double() {
    let (_, packages) = workspace();
    let mut offences = Vec::new();
    let mut files = 0;
    let mut gated = Vec::new();
    for name in shipped_set() {
        for root in &packages[&name].roots {
            let r = report_of_root(root);
            offences.extend(r.offences);
            files += r.files;
            gated.extend(r.gated_double_mods);
        }
    }
    assert!(
        offences.is_empty(),
        "test doubles outside cfg(test) or the test-support feature:\n{}",
        offences.join("\n")
    );
    // The positive control (ER-33): a clean result counts only if the scan read the tree and met the doubles it must pass.
    assert!(
        files >= MIN_FILES,
        "check broken: the scan parsed only {files} files (expected at least {MIN_FILES})"
    );
    for want in ["dyno-platform/src/lib.rs", "dyno-core/src/wol/mod.rs"] {
        assert!(
            gated.iter().any(|f| f.replace('\\', "/").ends_with(want)),
            "check broken: the scan never met the gated `mod fake` in {want}; met {gated:?}"
        );
    }
}
