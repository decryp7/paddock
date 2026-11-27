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
//!    why) and is not counted; an approval that is not the user's, or a register the check cannot read, is a finding of its own. The
//!    register is itself shrink-only (ER-46): an approval its merge-base copy lacks is one unwaivable finding (so writing `by = "user"`
//!    approves nothing unseen), and an approval of an entry the merge base already has, or the contract no longer has, is stale.
//!
//! Positive controls ([`Check::control`], ER-33): a `skip` over a module with an OS signal, an `only` over Windows code, a stale name
//! and a redundant `add` each go red, and `tests/it/os_scoped.rs` shows the same on the real tree and sources, and that the derived
//! placement is exactly what the contracts resolve to.
//!
//! **Blocking (errors, mode gate) since its first green run on the real tree, ci.yml run 9065 (11cd82593, 2026-11-25; ER-41).**

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
        if let Some(k) = a
            .keys()
            .find(|k| !["contract", "by", "date", "why", "entries"].contains(&k.as_str()))
        {
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
            return Err(format!(
                "{what} is approved by {by:?}: taking an OS away needs the user (ER-46), `by = \"user\"`"
            ));
        }
        let entries = a
            .get("entries")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("{what} has no entries"))?;
        let set = out.entry(contract.to_string()).or_default();
        for e in entries {
            set.insert(
                e.as_str()
                    .ok_or_else(|| format!("{what} holds an entry that is not a string"))?
                    .to_string(),
            );
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
        let finding = |msg: String, fix: String| Finding::error("C155", RULE, w.contract, 1, msg, fix);
        let all: Vec<(&str, &Exception)> = ex
            .add
            .iter()
            .map(|e| ("add", e))
            .chain(ex.skip.iter().map(|e| ("skip", e)))
            .chain(ex.only.iter().map(|e| ("only", e)))
            .collect();
        for (key, e) in &all {
            if !known(&e.name, d) {
                out.push(finding(
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
                out.push(finding(
                    format!(
                        "[redundant-exception] `hostTests.add` lists `{}`, which the code already marks as OS-specific",
                        e.name
                    ),
                    format!(
                        "Delete the entry from {}: the placement is derived, only what the code cannot say is declared.",
                        w.contract
                    ),
                ));
            }
        }
        for e in ex.skip.iter().filter(|e| e.modules.is_empty()) {
            if known(&e.name, d) && covered_marks(&e.name, &placement).is_empty() {
                out.push(finding(
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
                    out.push(Finding::error(
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
                out.push(finding(
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
        // approve. The merge base's entries come from the one comparison (`baseline::growth`); an approval the merge base already
        // covers, or of an entry the contract no longer has, is stale: the register only shrinks.
        let approved = approved.get(w.contract).cloned().unwrap_or_default();
        let current = removal_keys(ex);
        if (current.is_empty() && approved.is_empty()) || ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
            return;
        }
        let old: std::cell::RefCell<BTreeSet<String>> = Default::default();
        let entries_of = |text: &str| -> Option<Vec<String>> {
            let v: serde_json::Value = serde_json::from_str(text).ok()?;
            let parsed = Exceptions::parse(&v, w.contract).ok()?;
            let keys = parsed.map(|e| removal_keys(&e)).unwrap_or_default();
            *old.borrow_mut() = keys.iter().cloned().collect();
            Some(keys)
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
            Growth::Added { base, .. } => {
                let old = old.into_inner();
                let short = &base[..base.len().min(9)];
                let added = current.iter().filter(|k| !old.contains(*k) && !approved.contains(*k)).count();
                if added > 0 {
                    out.push(
                        finding(
                            format!(
                                "[os-removal] {added} `skip` or `only` entr{} not in {} at the merge base {short}: it takes an operating system away from code the derivation puts there",
                                if added == 1 { "y is" } else { "ies are" },
                                w.contract,
                            ),
                            "Remove the entries, or get the user's approval for the removal (ER-46); `add` needs none.".into(),
                        )
                        .unwaivable(),
                    );
                }
                for k in &approved {
                    let why = if old.contains(k) {
                        format!("the merge base {short} already has it in {}", w.contract)
                    } else if !current.contains(k) {
                        format!("{} has no such entry", w.contract)
                    } else {
                        continue;
                    };
                    out.push(stale_approval(w.contract, k, &why));
                }
            }
        }
    }
}

/// The finding for an approval the register no longer needs (it only shrinks, ER-46).
fn stale_approval(contract: &str, key: &str, why: &str) -> Finding {
    Finding::error(
        "C155",
        RULE,
        APPROVALS,
        1,
        format!("[os-removal] stale approval `{key}` of {contract}: {why}"),
        format!("Remove `{key}` from {APPROVALS} (the register only shrinks; an approval leaves once the merge base has its entry)."),
    )
}

/// The register's approvals as one key each (`<contract> <removal key>`), for its own shrink-only comparison.
fn approval_keys(approved: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    approved
        .iter()
        .flat_map(|(c, keys)| keys.iter().map(move |k| format!("{c} {k}")))
        .collect()
}

/// The register of the user's approvals of growth of a shrink-only list: docs/gates.toml.
const GROWTH_REGISTER: &str = "docs/gates.toml";

/// The digest an approval of growth names: SHA-256 (hex) of the added approval keys, sorted and joined by a newline, so the approval
/// covers exactly the additions the user saw (a count alone would approve any other additions of the same number).
pub fn growth_digest(added: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut keys: Vec<&str> = added.iter().map(String::as_str).collect();
    keys.sort_unstable();
    Sha256::digest(keys.join("\n").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What the register of growth approvals says about the additions of the approval register.
#[derive(Debug, PartialEq, Eq)]
pub enum GrowthApproval {
    /// An entry of the user names these additions exactly (count and digest).
    Approved,
    /// No entry for C155 (or the file cannot be read: a broken register approves nothing).
    Missing,
    /// An entry for C155 exists but does not match; the reason.
    Mismatch(String),
}

/// The user's approval of `added` (the approvals new against the merge base), read from `gates` (the text of docs/gates.toml). The line is
/// `[[register_growth]]` with `check = "C155"`, `by = "user"`, a `date`, the `count` of added approvals and their `digest`
/// ([`growth_digest`]): it cannot approve additions the user did not see.
pub fn growth_approval(gates: Option<&str>, added: &[String]) -> GrowthApproval {
    let Some(table) = gates.and_then(|t| t.parse::<toml::Table>().ok()) else {
        return GrowthApproval::Missing;
    };
    let entries: Vec<&toml::Table> = table
        .get("register_growth")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_table))
        .into_iter()
        .flatten()
        .filter(|e| e.get("check").and_then(toml::Value::as_str) == Some("C155"))
        .collect();
    if entries.is_empty() {
        return GrowthApproval::Missing;
    }
    let digest = growth_digest(added);
    let mut why = Vec::new();
    for e in entries {
        let s = |k: &str| e.get(k).and_then(toml::Value::as_str).filter(|v| !v.trim().is_empty());
        if s("by") != Some("user") {
            why.push("an entry is not `by = \"user\"`".to_string());
        } else if s("date").is_none() {
            why.push("an entry has no `date`".to_string());
        } else if e.get("count").and_then(toml::Value::as_integer) != i64::try_from(added.len()).ok() {
            why.push(format!("an entry's `count` is not {}", added.len()));
        } else if s("digest") != Some(digest.as_str()) {
            why.push(format!("an entry's `digest` is not {digest}"));
        } else {
            return GrowthApproval::Approved;
        }
    }
    GrowthApproval::Mismatch(why.join("; "))
}

/// The register is itself a shrink-only list (ER-46, final review P1-2): an approval the merge base's register lacks is growth, one
/// unwaivable finding, so no agent can approve a removal by writing `by = "user"`. The user approves the growth with one line in
/// docs/gates.toml ([`growth_approval`]) that names the date, the count and the digest of exactly the added approvals.
fn register_growth(ctx: &Ctx, approved: &BTreeMap<String, BTreeSet<String>>) -> Option<Finding> {
    let current = approval_keys(approved);
    if current.is_empty() || ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
        return None;
    }
    let old: std::cell::RefCell<BTreeSet<String>> = Default::default();
    let entries_of = |text: &str| {
        let keys = approvals(text).ok().map(|a| approval_keys(&a))?;
        *old.borrow_mut() = keys.iter().cloned().collect();
        Some(keys)
    };
    let exact = |k: &String| k.clone();
    match baseline::growth(ctx.root(), &ctx.base_ref(), APPROVALS, &current, &entries_of, &exact, None) {
        Growth::Unmeasured(why) => Some(baseline::unmeasured_finding(
            ctx.in_ci(),
            "C155",
            RULE,
            APPROVALS,
            "the register of OS-removal approvals",
            &why,
        )),
        Growth::Added { added: 0, .. } => None,
        Growth::Added { base, added, .. } => {
            let old = old.into_inner();
            let new: Vec<String> = current.iter().filter(|k| !old.contains(*k)).cloned().collect();
            let digest = growth_digest(&new);
            let gates = ctx.read(GROWTH_REGISTER);
            let detail = match growth_approval(gates.as_deref().map(String::as_str), &new) {
                GrowthApproval::Approved => return None,
                GrowthApproval::Missing => String::new(),
                GrowthApproval::Mismatch(why) => {
                    format!(" ({GROWTH_REGISTER} has a [[register_growth]] entry for C155 that does not match: {why})")
                }
            };
            Some(
                Finding::error(
                    "C155",
                    RULE,
                    APPROVALS,
                    1,
                    format!(
                        "[os-removal] the register of approvals grew: {added} approval{} not in {APPROVALS} at the merge base {}; only the user approves a removal{detail}",
                        if added == 1 { " is" } else { "s are" },
                        &base[..base.len().min(9)]
                    ),
                    format!(
                        "Remove the added approvals, or the user adds to {GROWTH_REGISTER}: [[register_growth]] check = \"C155\", by = \"user\", date = \"<day>\", count = {}, digest = \"{digest}\" (ER-46).",
                        new.len()
                    ),
                )
                .unwaivable(),
            )
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
        Mode::Gate
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
                (
                    "crates/Cargo.toml",
                    "[workspace]\nmembers = [\"paddock-a\", \"paddock-b\", \"paddock-c\", \"paddock-d\"]\n",
                ),
                ("crates/paddock-a/Cargo.toml", "[package]\nname = \"paddock-a\"\n"),
                ("crates/paddock-a/src/lib.rs", "mod model;\n"),
                ("crates/paddock-a/src/model.rs", "fn pure() {}\n#[cfg(windows)]\nfn win() {}\n"),
                ("crates/paddock-b/Cargo.toml", "[package]\nname = \"paddock-b\"\n"),
                (
                    "crates/paddock-b/src/lib.rs",
                    "fn f() {\n    let _ = windows_sys::Win32::Foundation::CloseHandle;\n}\n",
                ),
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
                    Finding::error(
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
        findings.extend(register_growth(ctx, &approved));
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for (root, contract) in os_placement::workspaces(&files) {
            let Some(text) = ctx.read(&contract) else { continue };
            let value: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    findings.push(Finding::error(
                        "C155",
                        RULE,
                        &contract,
                        1,
                        format!("[bad-contract] not valid JSON: {e}"),
                        "Fix the file.",
                    ));
                    continue;
                }
            };
            let ex = match Exceptions::parse(&value, &contract) {
                Ok(Some(e)) => e,
                Ok(None) => continue,
                Err(e) => {
                    findings.push(Finding::error(
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
            seen.insert(contract.clone());
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
        // An approval of a contract that declares no hostTests approves nothing any more.
        if !ctx.is_in_memory() {
            for (contract, keys) in approved.iter().filter(|(c, _)| !seen.contains(*c)) {
                for k in keys {
                    findings.push(stale_approval(contract, k, "the contract declares no hostTests"));
                }
            }
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
        assert!(
            known("ci_host::kept_clone::deep::", &d),
            "a prefix below a file module names its items"
        );
        assert!(!known("other::", &d));
        assert!(known("p", &d) && !known("q", &d));
    }

    fn gates_with(count: usize, digest: &str, by: &str) -> String {
        format!(
            "version = 1\n[[register_growth]]\ncheck = \"C155\"\nby = \"{by}\"\ndate = \"2026-11-27\"\ncount = {count}\ndigest = \"{digest}\"\n"
        )
    }

    fn two() -> Vec<String> {
        vec!["tools/x.json skip b::".to_string(), "tools/x.json skip a::".to_string()]
    }

    #[test]
    fn growth_with_the_users_matching_approval_is_approved_whatever_the_order_of_the_additions() {
        let digest = growth_digest(&two());
        assert_eq!(
            growth_approval(Some(&gates_with(2, &digest, "user")), &two()),
            GrowthApproval::Approved
        );
    }

    #[test]
    fn growth_without_an_approval_is_missing_so_the_finding_stands() {
        assert_eq!(growth_approval(None, &two()), GrowthApproval::Missing);
        assert_eq!(growth_approval(Some("version = 1\n"), &two()), GrowthApproval::Missing);
        assert_eq!(
            growth_approval(Some("not toml ["), &two()),
            GrowthApproval::Missing,
            "a broken register approves nothing"
        );
    }

    #[test]
    fn an_approval_that_does_not_match_the_additions_or_is_not_the_users_approves_nothing() {
        let digest = growth_digest(&two());
        // One more addition than the user saw (same count is not enough: the digest differs too).
        let mut more = two();
        more.push("tools/x.json skip c::".into());
        assert!(matches!(
            growth_approval(Some(&gates_with(2, &digest, "user")), &more),
            GrowthApproval::Mismatch(_)
        ));
        // The same count with another content.
        let mut other = two();
        other[0] = "tools/x.json skip z::".into();
        assert!(matches!(
            growth_approval(Some(&gates_with(2, &digest, "user")), &other),
            GrowthApproval::Mismatch(_)
        ));
        // A wrong count with the right digest.
        assert!(matches!(
            growth_approval(Some(&gates_with(3, &digest, "user")), &two()),
            GrowthApproval::Mismatch(_)
        ));
        // Not the user's.
        assert!(matches!(
            growth_approval(Some(&gates_with(2, &digest, "coordinator")), &two()),
            GrowthApproval::Mismatch(_)
        ));
    }
}
