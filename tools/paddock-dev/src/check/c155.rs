//! C155: the declared `hostTests` exceptions are consistent with the OS placement derived from the code (OS-scoped tests, user decision
//! 2026-11-22: "a test runs on an OS only if its code behaves differently there"; ADR 0117 §7, WP-6).
//!
//! The placement is derived, not declared ([`crate::os_placement`] reads the `os` edges of the repository graph: an OS `cfg` and
//! `cfg!`, an OS-API crate, an entry point of the portable layer, `CARGO_CFG_TARGET_OS` in a build script, `env::consts::OS`,
//! `process.platform`, an OS-behaviour dependency, and the import edge from a host module to the module it calls). A package or module
//! with a mark runs on the Windows and macOS legs, one without runs on Linux only, so "OS code that nobody listed" cannot happen any
//! more. What is declared are the exceptions (`hostTests.add`, `skip`, `only` of `crates/paddock.ci.json`,
//! `apps/<app>/paddock.ci.json`, `tools/paddock-dev/paddock.ci.json`, read through [`crate::host_tests`], ER-42), each with a reason,
//! and this check reports where an exception contradicts the code:
//!
//! 1. **bad-contract**: a `hostTests` the reader refuses (an unknown key, an exception without a reason).
//! 2. **stale-entry**: an exception that names no package of the workspace and no module of paddock-dev (a typo or a deleted crate).
//! 3. **redundant-exception**: an `add` the code already derives, or a `skip` that names code with no OS mark: the entry can go, the list
//!    only shrinks.
//! 4. **skipped-os-code**: a module that `skip.modules` keeps on Linux holds an OS signal: it is not pure logic.
//! 5. **os-code-windows**: a package or module that `only` gives to macOS holds Windows code: that code would never run on Windows.
//! 6. **os-removal**: an entry of `skip` or `only` that the contract at the merge base does not have. Taking an OS away from code that
//!    runs there is the unsafe direction and needs the user's approval (ER-46): the check compares every `skip` and `only` entry with the
//!    contract at the merge base (the one comparison [`super::baseline::growth`], the reader C53 and the allow lists use) and reports the
//!    added ones as one unwaivable finding at the contract. `add` is free and is not compared. The comparison runs on a checkout only: an
//!    in-memory tree has nothing to compare, and where git gives no merge base it warns (an error in CI), never silence. An entry the
//!    user approved is recorded in [`APPROVALS`] (`docs/os-removals.toml`: the contract, the removal keys, `by = "user"`, the date and
//!    why) and is not counted; an approval that is not the user's, or a register the check cannot read, is a finding of its own.
//!
//! Positive controls ([`Check::control`], ER-33): a `skip` over a module with an OS signal, an `only` over Windows code, a stale name
//! and a redundant `add` each go red, and `tests/it/os_scoped.rs` shows the same on the real tree and sources, and that the derived
//! placement is exactly what the contracts resolve to.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here and in docs/gates.toml (ER-41).** Recorded: not
//! yet.

use super::baseline::{self, Growth};
use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use crate::host_tests::{Derived, Exception, Exceptions, resolved_covers};
use crate::os_placement::{self, Placement, RustcLex};
use paddock_repo_graph::model::extract::BulkRead;
use paddock_repo_graph::model::os::{is_test_file, rust_sites, src_module};
use std::collections::{BTreeMap, BTreeSet};

const RULE: u32 = 3;

/// The files of a check's tree, in the one bulk call the extractor makes.
struct CtxBulk<'a>(&'a Ctx<'a>);

impl BulkRead for CtxBulk<'_> {
    fn read_all(&self, paths: &[String]) -> Vec<paddock_repo_graph::model::extract::Read> {
        // The check's tree reader answers present or not; it has no read error of its own to pass on.
        paths.iter().map(|p| Ok(self.0.read(p).map(|t| (*t).clone()))).collect()
    }
}

/// Whether a test-name prefix (`ci_host::`) covers `module`, or `module` covers the prefix: the entry and the module are on one branch.
fn on_branch(prefix: &str, module: &str) -> bool {
    module.starts_with(prefix) || prefix.starts_with(module)
}

fn is_prefix(name: &str) -> bool {
    name.ends_with("::")
}

/// Whether an entry's name belongs to the workspace: a member package, or a prefix of some module (or a module below it).
fn known(name: &str, derived: &Derived) -> bool {
    if is_prefix(name) {
        derived.known_modules.iter().any(|m| on_branch(name, m))
    } else {
        derived.known_packages.contains(name)
    }
}

/// The marks of the derived names an entry covers: a package itself, or every derived module on its branch.
fn covered_marks<'a>(name: &str, p: &'a Placement) -> Vec<&'a (String, String)> {
    let keys: Vec<&String> = if is_prefix(name) {
        p.derived.modules.iter().filter(|m| m.starts_with(name)).collect()
    } else {
        p.derived.packages.iter().filter(|x| x.as_str() == name).collect()
    };
    keys.into_iter().flat_map(|k| p.why.get(k).into_iter().flatten()).collect()
}

/// The `skip` and `only` entries as the keys the merge-base comparison counts: an entry that is new takes an OS away.
fn removal_keys(ex: &Exceptions) -> Vec<String> {
    let mut out = Vec::new();
    for s in &ex.skip {
        if s.modules.is_empty() {
            out.push(format!("skip {}", s.name));
        }
        out.extend(s.modules.iter().map(|m| format!("skip {} {m}", s.name)));
    }
    for o in &ex.only {
        out.push(format!("only {} {}", o.os.as_deref().unwrap_or(""), o.name));
    }
    out.sort();
    out.dedup();
    out
}

/// The register of the user's approvals of `skip` and `only` entries (os-removal, ER-46).
pub const APPROVALS: &str = "docs/os-removals.toml";

/// The removal keys the user approved, per contract, from the register's text; `Err` names what is wrong (an unknown key, an approval
/// that is not the user's, a missing field), so a broken register approves nothing.
fn approvals(text: &str) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("not valid TOML: {e}"))?;
    if let Some(k) = t.keys().find(|k| !["version", "approved"].contains(&k.as_str())) {
        return Err(format!("unknown key {k:?} (version, approved)"));
    }
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let list = t.get("approved").and_then(toml::Value::as_array).cloned().unwrap_or_default();
    for (i, a) in list.iter().enumerate() {
        let what = format!("approved #{}", i + 1);
        let a = a.as_table().ok_or_else(|| format!("{what} is not a table"))?;
        if let Some(k) = a.keys().find(|k| !["contract", "by", "date", "why", "entries"].contains(&k.as_str())) {
            return Err(format!("{what} has an unknown key {k:?}"));
        }
        let text = |k: &str| {
            a.get(k)
                .and_then(toml::Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| format!("{what} has no {k}"))
        };
        let contract = text("contract")?;
        let by = text("by")?;
        text("date")?;
        text("why")?;
        if by != "user" {
            return Err(format!("{what} is approved by {by:?}: taking an OS away needs the user (ER-46), `by = \"user\"`"));
        }
        let entries = a
            .get("entries")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("{what} has no entries"))?;
        let set = out.entry(contract.to_string()).or_default();
        for e in entries {
            set.insert(e.as_str().ok_or_else(|| format!("{what} holds an entry that is not a string"))?.to_string());
        }
    }
    Ok(out)
}

/// One workspace under check.
struct Workspace<'a> {
    root: &'a str,
    contract: &'a str,
}

/// The check.
pub struct C155;

impl C155 {
    fn workspace(
        &self,
        ctx: &Ctx,
        w: &Workspace,
        ex: &Exceptions,
        approved: &BTreeMap<String, BTreeSet<String>>,
        out: &mut Vec<Finding>,
        probes: &mut usize,
    ) {
        let files = ctx.tree_files();
        let prefix = format!("{}/", w.root);
        *probes += files.iter().filter(|f| f.starts_with(&prefix) && f.ends_with(".rs")).count();
        let placement = match os_placement::derive(w.root, &files, &CtxBulk(ctx)) {
            Ok(p) => p,
            Err(why) => {
                out.push(Finding::error(
                    "C155",
                    RULE,
                    w.contract,
                    1,
                    format!("the OS placement of {} cannot be derived: {why}", w.root),
                    "fix the file the reason names (it must be readable text), then run the check again",
                ));
                return;
            }
        };
        let d = &placement.derived;
        let warn = |msg: String, fix: String| Finding::warn("C155", RULE, w.contract, 1, msg, fix);
        let all: Vec<(&str, &Exception)> = ex
            .add
            .iter()
            .map(|e| ("add", e))
            .chain(ex.skip.iter().map(|e| ("skip", e)))
            .chain(ex.only.iter().map(|e| ("only", e)))
            .collect();
        for (key, e) in &all {
            if !known(&e.name, d) {
                out.push(warn(
                    format!(
                        "[stale-entry] `hostTests.{key}` names `{}`, which is no member package of {} and no module of it",
                        e.name, w.root
                    ),
                    "Delete the entry or fix its spelling: a typo here silently drops that code's host tests.".into(),
                ));
            }
        }
        for (i, e) in ex.add.iter().enumerate() {
            // Redundant: without this entry the resolved placement already runs the name on every host.
            let mut without = ex.clone();
            without.add.remove(i);
            let derived_already = resolved_covers(&without.resolved(d), &e.name);
            if known(&e.name, d) && derived_already {
                out.push(warn(
                    format!("[redundant-exception] `hostTests.add` lists `{}`, which the code already marks as OS-specific", e.name),
                    format!(
                        "Delete the entry from {}: the placement is derived, only what the code cannot say is declared.",
                        w.contract
                    ),
                ));
            }
        }
        for e in ex.skip.iter().filter(|e| e.modules.is_empty()) {
            if known(&e.name, d) && covered_marks(&e.name, &placement).is_empty() {
                out.push(warn(
                    format!(
                        "[redundant-exception] `hostTests.skip` keeps `{}` on Linux, which the code does not mark as OS-specific",
                        e.name
                    ),
                    format!("Delete the entry from {}: unmarked code runs on Linux only already.", w.contract),
                ));
            }
        }
        // `skip.modules`: a module of a host package that holds an OS signal is not pure logic.
        for e in ex.skip.iter().filter(|e| !e.modules.is_empty()) {
            let Some(dir) = placement.dirs.get(&e.name) else { continue };
            for f in files.iter().filter(|f| f.starts_with(&format!("{dir}/")) && f.ends_with(".rs")) {
                let Some(module) = src_module(f, dir, true) else { continue };
                if !e.modules.iter().any(|p| format!("{module}::").starts_with(p.as_str())) {
                    continue;
                }
                let Some(text) = ctx.read(f) else { continue };
                *probes += 1;
                let sites = rust_sites(&text, is_test_file(f), f.ends_with("build.rs"), &RustcLex);
                if let Some(s) = sites.first() {
                    out.push(Finding::warn(
                        "C155",
                        RULE,
                        f,
                        s.line,
                        format!(
                            "[skipped-os-code] `{}` keeps `{module}::` on Linux (`hostTests.skip`) but it has an OS-specific site: {f}:{} ({})",
                            e.name,
                            s.line,
                            s.kind.name()
                        ),
                        format!("Remove `{module}::` from `hostTests.skip` for `{}` in {}.", e.name, w.contract),
                    ));
                }
            }
        }
        let resolved = ex.resolved(d);
        for e in &ex.only {
            if e.os.as_deref() == Some("windows-latest") {
                continue;
            }
            // Windows code a more specific entry puts back on every host (an `add` of the module the host code calls) is tested there.
            let on_windows = |m: &&String| !resolved_covers(&resolved, m);
            let holder = if is_prefix(&e.name) {
                placement.windows.iter().filter(on_windows).find(|m| m.starts_with(&e.name))
            } else {
                placement.windows.get(&e.name).filter(on_windows)
            };
            if let Some(holder) = holder {
                out.push(warn(
                    format!(
                        "[os-code-windows] `{}` is tested only on macOS (`hostTests.only`) but `{holder}` holds Windows code: it would never run on Windows",
                        e.name
                    ),
                    format!(
                        "Remove the `only` entry for `{}` in {} (it then runs on every host), or remove the Windows code.",
                        e.name, w.contract
                    ),
                ));
            }
        }
        // Taking an OS away needs the user (ER-46): every `skip` and `only` entry the merge base lacks and the user's register does not
        // approve.
        let approved = approved.get(w.contract).cloned().unwrap_or_default();
        let current: Vec<String> = removal_keys(ex).into_iter().filter(|k| !approved.contains(k)).collect();
        if current.is_empty() || ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
            return;
        }
        let entries_of = |text: &str| -> Option<Vec<String>> {
            let v: serde_json::Value = serde_json::from_str(text).ok()?;
            let old = Exceptions::parse(&v, w.contract).ok()?;
            Some(old.map(|e| removal_keys(&e)).unwrap_or_default())
        };
        let exact = |k: &String| k.clone();
        match baseline::growth(ctx.root(), &ctx.base_ref(), w.contract, &current, &entries_of, &exact, None) {
            Growth::Unmeasured(why) => out.push(baseline::unmeasured_finding(
                ctx.in_ci(),
                "C155",
                RULE,
                w.contract,
                "the OS removals of hostTests",
                &why,
            )),
            Growth::Added { added: 0, .. } => {}
            Growth::Added { base, added, .. } => out.push(
                warn(
                    format!(
                        "[os-removal] {added} `skip` or `only` entr{} not in {} at the merge base {}: it takes an operating system away from code the derivation puts there",
                        if added == 1 { "y is" } else { "ies are" },
                        w.contract,
                        &base[..base.len().min(9)]
                    ),
                    "Remove the entries, or get the user's approval for the removal (ER-46); `add` needs none.".into(),
                )
                .unwaivable(),
            ),
        }
    }
}

impl Check for C155 {
    fn id(&self) -> &'static str {
        "C155"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "hostTests exceptions agree with the OS placement derived from the code"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    "crates/paddock.ci.json",
                    r#"{"hostTests": {
  "add": [{"name": "paddock-ghost", "reason": "a name that is no package"}, {"name": "paddock-c", "reason": "already marked by its cfg"}],
  "skip": [{"name": "paddock-a", "modules": ["model::"], "reason": "claimed to be pure logic"}],
  "only": [{"os": "macos-latest", "name": "paddock-b", "reason": "claimed to be a dev Mac tool"}]
}}"#,
                ),
                ("crates/paddock-a/Cargo.toml", "[package]\nname = \"paddock-a\"\n"),
                ("crates/paddock-a/src/lib.rs", "mod model;\n"),
                ("crates/paddock-a/src/model.rs", "fn pure() {}\n#[cfg(windows)]\nfn win() {}\n"),
                ("crates/paddock-b/Cargo.toml", "[package]\nname = \"paddock-b\"\n"),
                ("crates/paddock-b/src/lib.rs", "fn f() {\n    let _ = windows_sys::Win32::Foundation::CloseHandle;\n}\n"),
                ("crates/paddock-c/Cargo.toml", "[package]\nname = \"paddock-c\"\n"),
                ("crates/paddock-c/src/lib.rs", "#[cfg(unix)]\nfn u() {}\n"),
                ("crates/paddock-d/Cargo.toml", "[package]\nname = \"paddock-d\"\n"),
                (
                    "crates/paddock-d/src/lib.rs",
                    "// cfg(windows) in a comment\nfn f() { let _ = \"std::process::Command\"; }\n",
                ),
            ],
            // (file, line) of each finding: the module with a cfg (skipped-os-code), and at the contract the stale entry, the redundant
            // add and the macOS-only package with Windows code.
            expect: vec![
                ("crates/paddock-a/src/model.rs", 2),
                ("crates/paddock.ci.json", 1),
                ("crates/paddock.ci.json", 1),
                ("crates/paddock.ci.json", 1),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        let files = ctx.tree_files();
        let mut declared = 0;
        // The user's approvals; a register that cannot be read approves nothing and is a finding (fail closed).
        let approved = match ctx.read(APPROVALS).map(|t| approvals(&t)) {
            None => BTreeMap::new(),
            Some(Ok(a)) => a,
            Some(Err(why)) => {
                findings.push(
                    Finding::warn(
                        "C155",
                        RULE,
                        APPROVALS,
                        1,
                        format!("[os-removal] the register of approvals cannot be used: {why}"),
                        "Fix the register: each [[approved]] names contract, by = \"user\", date, why and entries (the keys C155 reports).",
                    )
                    .unwaivable(),
                );
                BTreeMap::new()
            }
        };
        for (root, contract) in os_placement::workspaces(&files) {
            let Some(text) = ctx.read(&contract) else { continue };
            let value: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    findings.push(Finding::warn("C155", RULE, &contract, 1, format!("[bad-contract] not valid JSON: {e}"), "Fix the file."));
                    continue;
                }
            };
            let ex = match Exceptions::parse(&value, &contract) {
                Ok(Some(e)) => e,
                Ok(None) => continue,
                Err(e) => {
                    findings.push(Finding::warn(
                        "C155",
                        RULE,
                        &contract,
                        1,
                        format!("[bad-contract] {e}"),
                        "Fix `hostTests` (docs/research/2026-11-22-os-scoped-tests.md; the shape is in tools/paddock-dev/src/host_tests.rs).",
                    ));
                    continue;
                }
            };
            declared += 1;
            self.workspace(
                ctx,
                &Workspace {
                    root: &root,
                    contract: &contract,
                },
                &ex,
                &approved,
                &mut findings,
                &mut probes,
            );
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: declared > 0 || ctx.is_in_memory(),
                detail: format!("{declared} hostTests declared, {probes} source files read"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ex(v: serde_json::Value) -> Exceptions {
        Exceptions::parse(&json!({ "hostTests": v }), "x.json").unwrap().unwrap()
    }

    #[test]
    fn removal_keys_name_every_skip_and_only_entry_and_never_an_add() {
        let e = ex(json!({
            "add": [{"name": "a", "reason": "a reason here"}],
            "skip": [{"name": "p", "modules": ["m::", "n::"], "reason": "pure logic"}, {"name": "q", "reason": "no tests at all"}],
            "only": [{"os": "macos-latest", "name": "t::", "reason": "a dev Mac tool"}]
        }));
        assert_eq!(removal_keys(&e), ["only macos-latest t::", "skip p m::", "skip p n::", "skip q"]);
        assert!(removal_keys(&ex(json!({"add": [{"name": "a", "reason": "a reason here"}]}))).is_empty());
    }

    #[test]
    fn a_prefix_is_known_when_it_is_on_a_branch_with_a_module() {
        let d = Derived {
            known_modules: ["ci_host::kept_clone::".to_string()].into(),
            known_packages: ["p".to_string()].into(),
            ..Derived::default()
        };
        assert!(known("ci_host::", &d), "a folder prefix above a module");
        assert!(known("ci_host::kept_clone::", &d));
        assert!(known("ci_host::kept_clone::deep::", &d), "a prefix below a file module names its items");
        assert!(!known("other::", &d));
        assert!(known("p", &d) && !known("q", &d));
    }
}
