//! The build cost class (`paddock_testkit::build_cost`, docs/benchmarks.md "test build cost class"): a test that starts a compiler or a
//! `cargo build` has the name prefix `build_`, and each Cargo workspace that has such a test gives the class its own time budget in the
//! default profile of its `.config/nextest.toml`.
//!
//! Two checks keep it true, both read with the tools' own parsers (`syn`, the TOML parser; ER-42):
//!
//! - **Every place that starts a build charges the class.** The places are the functions that call `TestCommand::cargo_build` (which
//!   only prepares the command: it counts when the function also runs one, with `status`, `output` or `spawn`) or the plugin kit's
//!   `build_with_cargo`; each must also call `paddock_testkit::build_cost::charge`, which fails a test under nextest whose
//!   name lacks the prefix (exact, at run time, warm or cold). Known false negatives: a build started by another way (a plain `Command` of
//!   `cargo` or `rustc`, a program that builds, such as `cargo paddock-plugin build`) is charged by hand where it is known (the test kit's
//!   own process test and the plugin CLI's `cli` helper); C123 keeps every such `Command` a `TestCommand`.
//! - **Every workspace with a charge has the class override** with the one filterset and a budget over the ci profile's default.
//!
//! The scan reads the whole tree, so its tests are class `tree` (ADR 0121); the positive controls are plain.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::build_cost::FILTER;
use std::path::{Path, PathBuf};
use syn::visit::Visit;

fn root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

/// One function of a source file, and what its code does.
#[derive(Debug, PartialEq, Eq)]
struct Func {
    name: String,
    /// Calls `TestCommand::cargo_build` (prepares a build command) or the plugin kit's `build_with_cargo` (runs one).
    prepares_build: bool,
    /// Runs a command (`status`, `output` or `spawn`).
    runs: bool,
    starts_build: bool,
    charges: bool,
}

/// Collects every function (nested ones apart, closures with their function) with what its own code calls.
#[derive(Default)]
struct Scan {
    frames: Vec<Func>,
    done: Vec<Func>,
}

impl Scan {
    fn enter(&mut self, name: &syn::Ident) {
        self.frames.push(Func {
            name: name.to_string(),
            prepares_build: false,
            runs: false,
            starts_build: false,
            charges: false,
        });
    }
    fn leave(&mut self) {
        let mut f = self.frames.pop().expect("a frame was entered");
        f.starts_build |= f.prepares_build && f.runs;
        self.done.push(f);
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.enter(&f.sig.ident);
        syn::visit::visit_item_fn(self, f);
        self.leave();
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.enter(&f.sig.ident);
        syn::visit::visit_impl_item_fn(self, f);
        self.leave();
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if let Some(f) = self.frames.last_mut() {
            if m.method == "cargo_build" {
                f.prepares_build = true;
            }
            if ["status", "output", "spawn"].iter().any(|n| m.method == n) {
                f.runs = true;
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*c.func
            && let Some(f) = self.frames.last_mut()
        {
            let names: Vec<String> = p.path.segments.iter().map(|s| s.ident.to_string()).collect();
            let last = names.last().map(String::as_str);
            if last == Some("build_with_cargo") {
                f.starts_build = true;
            }
            if last == Some("charge") && names.iter().any(|n| n == "build_cost") {
                f.charges = true;
            }
        }
        syn::visit::visit_expr_call(self, c);
    }
}

/// The functions of `text` (a Rust file), or why it does not parse.
fn functions(text: &str) -> Result<Vec<Func>, String> {
    let file = syn::parse_file(text).map_err(|e| e.to_string())?;
    let mut scan = Scan::default();
    scan.visit_file(&file);
    Ok(scan.done)
}

/// The findings of one file: a function that starts a build and does not charge the class.
fn uncharged(path: &str, text: &str) -> Vec<String> {
    match functions(text) {
        Ok(fs) => fs
            .into_iter()
            .filter(|f| f.starts_build && !f.charges)
            .map(|f| {
                format!(
                    "{path}: `{}` starts a build and does not call `paddock_testkit::build_cost::charge`",
                    f.name
                )
            })
            .collect(),
        Err(e) => vec![format!("{path}: does not parse as Rust ({e}): the audit cannot see its builds")],
    }
}

/// What a workspace's nextest configuration says about the class.
#[derive(Debug, PartialEq, Eq)]
enum Class {
    /// No override of the default profile has the class's filterset.
    Missing,
    /// The override has no `slow-timeout`, or one that is not over the ci profile's.
    NoRoom { class: u64, ci: u64 },
    /// The class has its budget (seconds).
    Budget(u64),
}

fn budget(t: Option<&toml::Value>) -> Option<u64> {
    let t = t?.as_table()?;
    Some(t.get("period")?.as_str()?.strip_suffix('s')?.parse::<u64>().ok()? * u64::try_from(t.get("terminate-after")?.as_integer()?).ok()?)
}

/// The class's state in the text of a `.config/nextest.toml`.
fn class_of(config: &str) -> Class {
    let doc: toml::Table = config.parse().expect("the nextest configuration parses");
    let profile = |name: &str| doc.get("profile").and_then(|p| p.get(name));
    let ci = budget(profile("ci").and_then(|p| p.get("slow-timeout"))).unwrap_or(0);
    let found = profile("default")
        .and_then(|p| p.get("overrides"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .find(|o| o.get("filter").and_then(toml::Value::as_str) == Some(FILTER));
    let Some(found) = found else { return Class::Missing };
    match budget(found.get("slow-timeout")) {
        Some(class) if class > ci => Class::Budget(class),
        other => Class::NoRoom {
            class: other.unwrap_or(0),
            ci,
        },
    }
}

/// The nearest `.config/nextest.toml` above `file` (a path relative to `root`), as a path relative to `root`.
fn config_of(root: &Path, file: &str) -> Option<String> {
    let mut dir = Path::new(file).parent();
    while let Some(d) = dir {
        let candidate = d.join(".config/nextest.toml");
        if root.join(&candidate).is_file() {
            return Some(candidate.to_string_lossy().replace('\\', "/"));
        }
        dir = d.parent();
    }
    None
}

/// The tracked Rust files of the code (not the fixtures of tests, which hold samples that need not parse).
fn code_files(root: &Path) -> Vec<String> {
    let out = paddock_proc::command("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--", "*.rs"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty() && !f.contains("/fixtures/") && !f.starts_with("templates/") && !f.starts_with("spikes/"))
        .map(String::from)
        .collect()
}

/// Every code file with the functions that start a build, and the files that call `charge` anywhere.
fn scan_tree(root: &Path) -> (Vec<String>, Vec<(String, usize)>) {
    let (mut findings, mut sites) = (Vec::new(), Vec::new());
    for f in code_files(root) {
        let text = std::fs::read_to_string(root.join(&f)).unwrap();
        // A cheap pre-filter: a file that names none of the three cannot hold a site.
        if !(text.contains("cargo_build") || text.contains("build_with_cargo") || text.contains("build_cost")) {
            continue;
        }
        findings.extend(uncharged(&f, &text));
        if let Ok(fs) = functions(&text) {
            let n = fs.iter().filter(|x| x.charges).count();
            if n > 0 {
                sites.push((f, n));
            }
        }
    }
    (findings, sites)
}

#[test]
fn tree_every_function_that_starts_a_build_charges_the_build_class() {
    let root = root();
    let (findings, sites) = scan_tree(&root);
    assert!(
        findings.is_empty(),
        "the build class is charged where a build starts (paddock_testkit::build_cost):\n{}",
        findings.join("\n")
    );
    // The precondition (ER-33): the scan saw the places it exists for, in the kit and in the apps that start builds.
    for needle in [
        "crates/paddock-plugin-test/src/fixture.rs",
        "apps/pitcrew/",
        "apps/debrief/app/src-tauri/",
    ] {
        assert!(
            sites.iter().any(|(f, _)| f.contains(needle)),
            "the scan saw no charge in {needle}: {sites:?}"
        );
    }
}

#[test]
fn tree_every_workspace_that_charges_the_build_class_has_its_override() {
    let root = root();
    let (_, sites) = scan_tree(&root);
    let mut configs: Vec<String> = Vec::new();
    let mut findings = Vec::new();
    for (file, _) in &sites {
        match config_of(&root, file) {
            Some(c) if !configs.contains(&c) => configs.push(c),
            Some(_) => {}
            None => findings.push(format!("{file}: no .config/nextest.toml above it")),
        }
    }
    for c in &configs {
        match class_of(&std::fs::read_to_string(root.join(c)).unwrap()) {
            Class::Budget(_) => {}
            other => findings.push(format!(
                "{c}: the default profile has no override with `filter = '{FILTER}'` and a slow-timeout over the ci profile's ({other:?})"
            )),
        }
    }
    assert!(findings.is_empty(), "{}", findings.join("\n"));
    assert!(configs.len() >= 3, "the crates and two apps charge the class: {configs:?}");
}

/// Positive control: a function that starts a build without the charge is found, with it is not, in every shape the scan reads.
#[test]
fn the_scan_finds_a_build_started_without_the_charge_and_accepts_one_with_it() {
    let bare = "fn f() { TestCommand::new(x).cargo_build().status(); }";
    assert_eq!(uncharged("a.rs", bare).len(), 1);
    let split = "fn f() { let mut b = x; b.cargo_build().args(a); b.output(); }";
    assert_eq!(
        uncharged("a.rs", split).len(),
        1,
        "preparing in one statement and running in another is one build"
    );
    assert!(
        uncharged("a.rs", "fn f() { let e = c.cargo_build().environment(); }").is_empty(),
        "a prepared command that never runs starts no build"
    );
    let helper = "fn locate() { find(|| build_with_cargo(n)); }";
    assert_eq!(uncharged("a.rs", helper).len(), 1);
    let nested = "fn outer() { build_cost::charge(\"x\"); fn inner() { c.cargo_build().status(); } }";
    assert_eq!(
        uncharged("a.rs", nested).len(),
        1,
        "an inner function is its own: the outer charge does not cover it"
    );
    let charged = "fn f() { paddock_testkit::build_cost::charge(\"x\"); get_or_init(|| { c.cargo_build().status(); }); }";
    assert!(uncharged("a.rs", charged).is_empty(), "a closure belongs to its function");
    let method = "impl T { fn m(&self) { build_cost::charge(\"x\"); self.c.cargo_build().spawn(); } }";
    assert!(uncharged("a.rs", method).is_empty());
    assert!(
        uncharged("a.rs", "fn f() { other::charge(1); x.build(); }").is_empty(),
        "no build, no finding"
    );
    assert_eq!(
        uncharged("a.rs", "fn (").len(),
        1,
        "a file that does not parse is a finding, never a pass"
    );
}

/// Positive control: the class state of a configuration is read with the TOML parser.
#[test]
fn the_class_override_is_read_from_the_configuration_and_a_budget_under_the_ci_profile_is_refused() {
    let ci = "[profile.ci]\nslow-timeout = { period = \"30s\", terminate-after = 2 }\n";
    let with = |budget: &str| format!("{ci}\n[[profile.default.overrides]]\nfilter = '{FILTER}'\nslow-timeout = {budget}\n");
    assert_eq!(class_of(ci), Class::Missing);
    assert_eq!(class_of(&with("{ period = \"60s\", terminate-after = 5 }")), Class::Budget(300));
    assert_eq!(
        class_of(&with("{ period = \"30s\", terminate-after = 2 }")),
        Class::NoRoom { class: 60, ci: 60 }
    );
    // The same filter in the ci profile's own overrides is not the default profile's.
    let other =
        format!("{ci}\n[[profile.ci.overrides]]\nfilter = '{FILTER}'\nslow-timeout = {{ period = \"60s\", terminate-after = 5 }}\n");
    assert_eq!(class_of(&other), Class::Missing);
    let no_budget = format!("{ci}\n[[profile.default.overrides]]\nfilter = '{FILTER}'\n");
    assert_eq!(class_of(&no_budget), Class::NoRoom { class: 0, ci: 60 });
}
