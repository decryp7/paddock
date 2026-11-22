//! C155: OS-specific code is only in packages and modules that `hostTests` runs on Windows and macOS (OS-scoped tests, user decision
//! 2026-11-22: "a test runs on an OS only if its code behaves differently there"; docs/research/2026-11-22-os-scoped-tests.md).
//!
//! `hostTests` (`crates/paddock.ci.json`, `apps/<app>/paddock.ci.json`, `tools/paddock-dev/paddock.ci.json`, read through
//! [`crate::host_tests`], ER-42) says which packages and which paddock-dev test-name prefixes the Windows and macOS legs test; every
//! other test runs on Linux only. A package or module left out of the list while its code behaves differently per OS would silently
//! lose its Windows and macOS tests, so this check reads the sources and reports:
//!
//! 1. **os-code**: a package of a scoped workspace (or a paddock-dev module, a file of `src/` or `tests/it/`) that is not in the list
//!    and holds one of: an OS `cfg` (`cfg(windows)`, `cfg(unix)`, `target_os`, `target_family`, also `cfg!` and `cfg_attr`), a call into an
//!    OS-API crate (`windows_sys`, `libc`, `objc2`, `core_foundation`, `security_framework`, `zbus`), or an entry point of the portable
//!    layer of ADR 0109 (`paddock_proc::command`, `paddock_env::winpath`, `paddock_env::atomic`, `std::process::Command`, a file lock).
//!    Comments and string contents are masked, so a word in a comment is not a use. One finding per package or module, naming its first
//!    site and the number of sites, with the line to add.
//! 2. **skipped-os-code**: a module that `hostTests.skip` keeps on Linux holds such a site: it is not pure logic.
//! 3. **import-edge** (paddock-dev only): a source file that a listed module covers imports (`crate::<module>`, in code that is not a
//!    test) a module that no listed prefix covers: that host code calls it, so its tests must run where the caller runs. The target
//!    is the longest module path that names a file (`crate::pre_push::exec::heavy_prefix` is `pre_push::exec`); an item of a folder's
//!    `mod.rs` (`crate::pre_push::stop_on_signals`) is covered when some module of that folder is listed. The cases that made this
//!    rule: `ci_setup_rust` imports `local_sccache` and `ci_run` imports `pre_push::exec`, so those keep their Windows and macOS tests.
//!    A package in the contract's `lastPackages` is kept off the host legs by that older decision and is not read.
//! 4. **stale-entry**: a name in `hostTests` that is no package of the workspace and no module of paddock-dev (a typo or a deleted crate).
//! 5. **bad-contract**: a `hostTests` the reader refuses.
//!
//! Positive controls ([`Check::control`], ER-33): an unlisted package with `cfg(windows)` and an unlisted module that a listed one
//! imports each go red; `tests/it/check_c155.rs` unlists `paddock-proc` and `local_sccache` in the real contracts and shows the same.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here and in docs/gates.toml (ER-41).** Recorded: not
//! yet. First count on the real tree (this branch): see CHANGELOG. Lists only shrink in effect (ER-46): the check keeps no list of its
//! own, an entry in `hostTests` is the list, and the lists are in the contracts under review.

use super::finding::Finding;
use super::jsre::rx;
use super::lex::rust::{mask, without_test_items};
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use crate::host_tests::{HOST_OSES, HostTests};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

const RULE: u32 = 3;
const CRATES: &str = "crates/paddock.ci.json";
const PADDOCK_DEV: &str = "tools/paddock-dev/paddock.ci.json";
const PD_DIR: &str = "tools/paddock-dev/";

static CFG: LazyLock<Regex> = LazyLock::new(|| rx(r"\bcfg(?:_attr)?!?\s*\([^\n]*?\b(?:windows|unix|target_os|target_family)\b"));
static API: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:windows_sys|libc|objc2|core_foundation|security_framework|zbus)\s*::"));
static LAYER: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"\bpaddock_proc\s*::\s*command\b|\bpaddock_env\s*::\s*(?:winpath|atomic)\b|\bprocess\s*::\s*Command\b|\bfd_lock\b|\bfs2\s*::|\b(?:try_lock|lock_exclusive|flock)\b",
    )
});
static PACKAGE_NAME: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?m)^name\s*=\s*"([^"]+)""#));
static CRATE_PATH: LazyLock<Regex> = LazyLock::new(|| rx(r"\bcrate\s*::\s*([A-Za-z_][A-Za-z0-9_]*(?:\s*::\s*[A-Za-z_][A-Za-z0-9_]*)*)"));

/// One OS-specific site: its 1-based line and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// `cfg`, `os-api` or `layer`.
    pub kind: &'static str,
}

/// Whether a file is test code: under a `tests/` folder, or named `tests.rs` or `*_tests.rs` (a module only `#[cfg(test)]` includes).
pub fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel.split('/').any(|s| s == "tests") || name == "tests.rs" || name.ends_with("_tests.rs")
}

/// The OS-specific sites of one source file, `text` with comments and string contents masked first. An OS `cfg` and a call into an OS-API
/// crate count anywhere (a `cfg(windows)` test is an OS-specific test); an entry point of the portable layer (a process, a lock) counts
/// only in production code: a test that starts `node` or `git` to run a check is not code that behaves differently per OS, the code it
/// drives is judged where it is. `test_file`: [`is_test_file`].
pub fn sites(text: &str, test_file: bool) -> Vec<Site> {
    let m = mask(text);
    let production = if test_file { String::new() } else { without_test_items(&m) };
    let line = |s: &str, at: usize| s[..at].bytes().filter(|b| *b == b'\n').count() + 1;
    let mut out = Vec::new();
    for (re, kind, text) in [(&*CFG, "cfg", &m), (&*API, "os-api", &m), (&*LAYER, "layer", &production)] {
        for hit in re.find_iter(text) {
            out.push(Site {
                line: line(text, hit.start()),
                kind,
            });
        }
    }
    out.sort_by_key(|s| (s.line, s.kind));
    out.dedup();
    out
}

/// The `crate::` paths a source file's production code names (test items removed), one per use: `crate::a::b` is `a::b`, and a group
/// `crate::{a, b::c}` is `a` and `b::c`. With the line of each.
pub fn crate_paths(text: &str) -> Vec<(usize, String)> {
    let m = without_test_items(&mask(text));
    let mut out = Vec::new();
    let line_of = |at: usize| m[..at].bytes().filter(|b| *b == b'\n').count() + 1;
    for cap in CRATE_PATH.captures_iter(&m) {
        let Some(g) = cap.get(1) else { continue };
        out.push((
            line_of(g.start()),
            g.as_str().split("::").map(str::trim).collect::<Vec<_>>().join("::"),
        ));
    }
    // `use crate::{a, b::c, d::{e, f}};`
    let mut from = 0;
    while let Some(i) = m[from..].find("crate::{") {
        let open = from + i + "crate::".len();
        let Some(body) = braced(&m[open..]) else { break };
        for p in group_paths(body, "") {
            out.push((line_of(open), p));
        }
        from = open + body.len();
    }
    out
}

/// The text inside the `{ .. }` that starts `s` (without the braces), or `None` when it never closes.
fn braced(s: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The leaf paths of a use group body (`a, b::c, d::{e, f}` under `prefix`): `a`, `b::c`, `d::e`, `d::f`. `self` leaves are the prefix.
fn group_paths(body: &str, prefix: &str) -> Vec<String> {
    let mut parts: Vec<&str> = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    let mut out = Vec::new();
    for p in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let joined = |tail: &str| {
            if prefix.is_empty() {
                tail.to_string()
            } else {
                format!("{prefix}::{tail}")
            }
        };
        if let Some(open) = p.find('{') {
            let head = p[..open].trim_end_matches([':', ' ']);
            let inner = braced(&p[open..]).unwrap_or("");
            out.extend(group_paths(inner, &joined(head)));
        } else {
            let leaf = p.split(" as ").next().unwrap_or(p).trim();
            out.push(if leaf == "self" { prefix.to_string() } else { joined(leaf) });
        }
    }
    out.retain(|p| !p.is_empty());
    out
}

/// The module path of a file of paddock-dev's `src/` (`ci_host/kept_clone.rs` is `ci_host::kept_clone`, `x/mod.rs` is `x`); `None` for
/// `lib.rs`, `main.rs` and a test-only file (`tests.rs`, a `tests/` folder).
fn src_module(rel: &str) -> Option<String> {
    let inner = rel.strip_prefix(&format!("{PD_DIR}src/"))?.strip_suffix(".rs")?;
    if inner == "lib" || inner == "main" || inner.rsplit('/').next() == Some("tests") || inner.split('/').any(|s| s == "tests") {
        return None;
    }
    let inner = inner.strip_suffix("/mod").unwrap_or(inner);
    Some(inner.replace('/', "::"))
}

/// The test-name prefix of a file of `tests/it/` (`worktree/mod.rs` is `worktree`, `golden/planner.rs` is `golden::planner`).
fn it_module(rel: &str) -> Option<String> {
    let inner = rel.strip_prefix(&format!("{PD_DIR}tests/it/"))?.strip_suffix(".rs")?;
    if inner == "main" {
        return None;
    }
    Some(inner.strip_suffix("/mod").unwrap_or(inner).replace('/', "::"))
}

/// Whether `module` (`pre_push::exec`) is covered by the listed prefixes (`pre_push::exec::`): it is one of them or below one, or (an
/// item of a folder's `mod.rs`) some listed prefix is below it.
fn covered(module: &str, listed: &[String]) -> bool {
    let with = format!("{module}::");
    listed.iter().any(|l| with.starts_with(l.as_str()) || l.starts_with(&with))
}

/// Whether the file at module path `module` is below a listed prefix (its own tests run on the hosts).
fn below(module: &str, listed: &[String]) -> bool {
    let with = format!("{module}::");
    listed.iter().any(|l| with.starts_with(l.as_str()))
}

/// Whether a manifest is no member of the workspace around it: it has a `[workspace]` of its own, or sits in a folder cargo's workspace
/// excludes by convention here (`vendor`, `docs` research harnesses, `fuzz` crates). cargo cannot select such a package with `-p`
/// (Dyno's vendored `tray-icon`, Debrief's `quack-harness`), so a `hostTests` entry for one would break every scoped leg.
fn non_member(rel: &str, text: &str) -> bool {
    text.contains("[workspace]") || rel.split('/').any(|s| matches!(s, "vendor" | "docs" | "fuzz"))
}

/// `(package name, dir)` of every member package of the tree under `prefix` (a folder with a `Cargo.toml` that has a `[package]`).
fn packages(ctx: &Ctx, prefix: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in ctx
        .tree_files()
        .iter()
        .filter(|f| f.starts_with(prefix) && f.ends_with("/Cargo.toml"))
    {
        let Some(text) = ctx.read(f) else { continue };
        if !text.contains("[package]") || non_member(f, &text) {
            continue;
        }
        let after = &text[text.find("[package]").unwrap_or(0)..];
        if let Some(name) = PACKAGE_NAME.captures(after).and_then(|c| c.get(1)) {
            out.push((name.as_str().to_string(), f.trim_end_matches("Cargo.toml").to_string()));
        }
    }
    out
}

/// The package a source file belongs to: the one with the longest folder that holds it.
fn owner<'a>(file: &str, pkgs: &'a [(String, String)]) -> Option<&'a (String, String)> {
    pkgs.iter()
        .filter(|(_, dir)| file.starts_with(dir.as_str()))
        .max_by_key(|(_, dir)| dir.len())
}

fn first_site(file: &str, s: &[Site]) -> String {
    format!("{file}:{} ({})", s[0].line, s[0].kind)
}

/// A Cargo workspace whose contract declares `hostTests`: where its packages are, the contract, the declaration and the packages the
/// contract keeps off the host legs (`lastPackages`).
#[derive(Clone, Copy)]
struct Workspace<'a> {
    root: &'a str,
    contract: &'a str,
    host: &'a HostTests,
    last: &'a [String],
}

/// The check.
pub struct C155;

impl C155 {
    /// The findings of one Cargo-package workspace (`crates/` or an app) whose contract is `contract`.
    fn packages_findings(&self, ctx: &Ctx, w: &Workspace, out: &mut Vec<Finding>, probes: &mut usize) {
        let Workspace {
            root,
            contract,
            host,
            last,
        } = *w;
        let pkgs = packages(ctx, root);
        let names: BTreeSet<&str> = pkgs.iter().map(|(n, _)| n.as_str()).collect();
        for stale in host
            .packages
            .iter()
            .chain(host.only.values().flatten().filter(|e| !e.ends_with("::")))
        {
            if !names.contains(stale.as_str()) {
                out.push(Finding::warn(
                    "C155",
                    RULE,
                    contract,
                    1,
                    format!("[stale-entry] `hostTests` lists the package `{stale}`, which is no member package under {root} (a vendored, fuzz or research crate cannot be selected with `-p`)"),
                    "Delete the entry or fix its spelling: a typo here silently drops that package's host tests.",
                ));
            }
        }
        let mut by_pkg: BTreeMap<&str, Vec<(String, Vec<Site>)>> = BTreeMap::new();
        for f in ctx.tree_files().iter().filter(|f| f.starts_with(root) && f.ends_with(".rs")) {
            let Some((name, _)) = owner(f, &pkgs) else { continue };
            let Some(text) = ctx.read(f) else { continue };
            *probes += 1;
            let s = sites(&text, is_test_file(f));
            if !s.is_empty() {
                by_pkg.entry(name.as_str()).or_default().push((f.clone(), s));
            }
        }
        for (name, files) in &by_pkg {
            let count: usize = files.iter().map(|(_, s)| s.len()).sum();
            if last.iter().any(|l| l == name) {
                // `lastPackages`: kept off the host legs by the contract (a Tauri shell whose build overwrites a binary the other tests
                // start), a decision older than hostTests; it is not a package that was forgotten.
                continue;
            }
            if !host.lists_package(name) {
                let (file, s) = &files[0];
                out.push(Finding::warn(
                    "C155",
                    RULE,
                    file,
                    s[0].line,
                    format!(
                        "[os-code] `{name}` is not in `hostTests` but has {count} OS-specific site(s) in {} file(s); first: {}",
                        files.len(),
                        first_site(file, s)
                    ),
                    format!(
                        "Add \"{name}\" to `hostTests.packages` in {contract} (its tests behave differently per OS), or remove the OS code."
                    ),
                ));
            } else if let Some(skips) = host.skip.get(*name) {
                let dir = owner(&files[0].0, &pkgs).map(|(_, d)| d.clone()).unwrap_or_default();
                for (file, s) in files {
                    let rel = file.strip_prefix(&format!("{dir}src/")).unwrap_or("");
                    let module = rel.trim_end_matches(".rs").trim_end_matches("/mod").replace('/', "::");
                    if skips.iter().any(|p| format!("{module}::").starts_with(p.as_str())) {
                        out.push(Finding::warn(
                            "C155",
                            RULE,
                            file,
                            s[0].line,
                            format!("[skipped-os-code] `{name}` keeps `{module}::` on Linux (`hostTests.skip`) but it has an OS-specific site: {}", first_site(file, s)),
                            format!("Remove `{module}::` from `hostTests.skip.{name}` in {contract}."),
                        ));
                    }
                }
            }
        }
    }

    /// The findings of paddock-dev: its modules and the import edges between them.
    fn module_findings(&self, ctx: &Ctx, host: &HostTests, out: &mut Vec<Finding>, probes: &mut usize) {
        let mut listed: Vec<String> = host.modules.clone();
        listed.extend(host.only.values().flatten().filter(|e| e.ends_with("::")).cloned());
        listed.sort();
        listed.dedup();
        let files: Vec<String> = ctx
            .tree_files()
            .iter()
            .filter(|f| f.starts_with(PD_DIR) && f.ends_with(".rs"))
            .cloned()
            .collect();
        let src_modules: BTreeSet<String> = files.iter().filter_map(|f| src_module(f)).collect();
        let it_modules: BTreeSet<String> = files.iter().filter_map(|f| it_module(f)).collect();
        for l in &listed {
            let m = l.trim_end_matches("::");
            let known = src_modules.iter().chain(it_modules.iter()).any(|x| format!("{x}::").starts_with(l.as_str()))
                // a Windows-only module is not built on the machine that reads it, but its file is there
                || src_modules.contains(m);
            if !known {
                out.push(Finding::warn(
                    "C155",
                    RULE,
                    PADDOCK_DEV,
                    1,
                    format!("[stale-entry] `hostTests` lists the module `{l}`, which names no file of tools/paddock-dev/src or tests/it"),
                    "Delete the entry or fix its spelling: a typo here silently drops those tests from the host legs.",
                ));
            }
        }
        // OS-specific code in a module that no listed prefix covers: one finding per module (a file's module path).
        let mut unlisted: BTreeMap<String, Vec<(String, Vec<Site>)>> = BTreeMap::new();
        for f in &files {
            let module = src_module(f).or_else(|| it_module(f));
            let Some(module) = module else { continue };
            let Some(text) = ctx.read(f) else { continue };
            *probes += 1;
            if below(&module, &listed) {
                continue;
            }
            let s = sites(&text, is_test_file(f));
            if !s.is_empty() {
                unlisted.entry(module).or_default().push((f.clone(), s));
            }
        }
        for (module, hits) in &unlisted {
            let count: usize = hits.iter().map(|(_, s)| s.len()).sum();
            let (file, s) = &hits[0];
            out.push(Finding::warn(
                "C155",
                RULE,
                file,
                s[0].line,
                format!(
                    "[os-code] `{module}::` is not in `hostTests` but has {count} OS-specific site(s) in {} file(s); first: {}",
                    hits.len(),
                    first_site(file, s)
                ),
                format!("Add \"{module}::\" to `hostTests.modules` (or `only.macos-latest` for a dev-Mac tool) in {PADDOCK_DEV}."),
            ));
        }
        // Imports from the code of a module a runner tests into a module that runner does not test: per runner, so a module that only
        // macOS tests cannot be what a Windows-tested module calls.
        let mut seen = BTreeSet::new();
        for os in HOST_OSES {
            let on = host.module_prefixes(os);
            for f in &files {
                let Some(module) = src_module(f) else { continue };
                if !below(&module, &on) {
                    continue;
                }
                let Some(text) = ctx.read(f) else { continue };
                for (line, path) in crate_paths(&text) {
                    let segs: Vec<&str> = path.split("::").collect();
                    let target = (1..=segs.len())
                        .rev()
                        .map(|n| segs[..n].join("::"))
                        .find(|p| src_modules.contains(p));
                    let Some(target) = target else { continue };
                    if covered(&target, &on) || !seen.insert((f.clone(), target.clone())) {
                        continue;
                    }
                    out.push(Finding::warn(
                        "C155",
                        RULE,
                        f,
                        line,
                        format!("[import-edge] `{module}` is host code on {os} and calls `crate::{target}`, which that runner does not test: its tests would stop running there"),
                        format!("Add \"{target}::\" to `hostTests.modules` in {PADDOCK_DEV} (or keep `{module}` off the host legs)."),
                    ));
                }
            }
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
        "OS-specific code is in a package or module that hostTests runs on Windows and macOS"
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
                    r#"{"hostTests": {"packages": ["paddock-a"], "skip": {"paddock-a": ["model::"]}}}"#,
                ),
                ("crates/paddock-a/Cargo.toml", "[package]\nname = \"paddock-a\"\n"),
                ("crates/paddock-a/src/lib.rs", "#[cfg(windows)]\nfn win() {}\n"),
                ("crates/paddock-a/src/model.rs", "fn pure() {}\n"),
                ("crates/paddock-b/Cargo.toml", "[package]\nname = \"paddock-b\"\n"),
                ("crates/paddock-b/src/lib.rs", "fn f() {\n    let _ = libc::getpid();\n}\n"),
                ("crates/paddock-c/Cargo.toml", "[package]\nname = \"paddock-c\"\n"),
                (
                    "crates/paddock-c/src/lib.rs",
                    "// cfg(windows) in a comment\nfn f() { let _ = \"std::process::Command\"; }\n",
                ),
                (
                    "tools/paddock-dev/paddock.ci.json",
                    r#"{"hostTests": {"modules": ["ci_setup_rust::"]}}"#,
                ),
                (
                    "tools/paddock-dev/src/ci_setup_rust.rs",
                    "use crate::local_sccache::server_env;\nuse crate::plain::x;\nfn f() {}\n",
                ),
                ("tools/paddock-dev/src/local_sccache.rs", "pub fn server_env() {}\n"),
                ("tools/paddock-dev/src/plain.rs", "pub fn x() {}\n"),
                ("tools/paddock-dev/src/spawner.rs", "use std::process::Command;\n"),
            ],
            // (file, line) of each finding: the libc call, the two imports of ci_setup_rust (local_sccache, plain) and the spawner.
            expect: vec![
                ("crates/paddock-b/src/lib.rs", 2),
                ("tools/paddock-dev/src/ci_setup_rust.rs", 1),
                ("tools/paddock-dev/src/ci_setup_rust.rs", 2),
                ("tools/paddock-dev/src/spawner.rs", 1),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        let mut contracts: Vec<(String, String, bool)> =
            vec![(CRATES.into(), "crates/".into(), false), (PADDOCK_DEV.into(), PD_DIR.into(), true)];
        for f in ctx.tree_files().iter() {
            if let Some(app) = f.strip_prefix("apps/").and_then(|r| r.strip_suffix("/paddock.ci.json"))
                && !app.contains('/')
            {
                contracts.push((f.clone(), format!("apps/{app}/"), false));
            }
        }
        let mut declared = 0;
        for (contract, root, modules) in &contracts {
            let Some(text) = ctx.read(contract) else { continue };
            let value: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    findings.push(Finding::warn(
                        "C155",
                        RULE,
                        contract,
                        1,
                        format!("[bad-contract] not valid JSON: {e}"),
                        "Fix the file.",
                    ));
                    continue;
                }
            };
            let host = match HostTests::parse(&value, contract) {
                Ok(Some(h)) => h,
                Ok(None) => continue,
                Err(e) => {
                    findings.push(Finding::warn(
                        "C155",
                        RULE,
                        contract,
                        1,
                        format!("[bad-contract] {e}"),
                        "Fix `hostTests` (docs/research/2026-11-22-os-scoped-tests.md).",
                    ));
                    continue;
                }
            };
            declared += 1;
            if *modules {
                self.module_findings(ctx, &host, &mut findings, &mut probes);
            } else {
                let last: Vec<String> = value
                    .get("lastPackages")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                self.packages_findings(
                    ctx,
                    &Workspace {
                        root,
                        contract,
                        host: &host,
                        last: &last,
                    },
                    &mut findings,
                    &mut probes,
                );
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

    fn kinds(text: &str) -> Vec<&'static str> {
        sites(text, false).into_iter().map(|s| s.kind).collect()
    }

    #[test]
    fn the_cfg_forms_the_os_api_crates_and_the_portable_layers_entry_points_are_sites() {
        for text in [
            "#[cfg(windows)]\nfn a() {}",
            "#[cfg(not(unix))]\nfn a() {}",
            "#[cfg(target_os = \"macos\")]\nfn a() {}",
            "if cfg!(windows) { }",
            "#[cfg_attr(unix, allow(dead_code))]\nfn a() {}",
            "let p = libc::getpid();",
            "use windows_sys::Win32::Foundation::HANDLE;",
            "use objc2::rc::Retained;",
            "let c = std::process::Command::new(\"x\");",
            "use paddock_proc::command::run;",
            "paddock_env::winpath::normalise(p);",
            "paddock_env::atomic::write(p, b);",
            "f.try_lock().unwrap();",
        ] {
            assert_eq!(sites(text, false).len(), 1, "{text}");
        }
    }

    #[test]
    fn a_word_in_a_comment_or_a_string_and_a_plain_function_are_no_site() {
        for text in [
            "// cfg(windows) and std::process::Command\nfn a() {}",
            "/* libc::getpid() */ fn a() {}",
            "let s = \"cfg(unix) windows_sys::x\";",
            "#[cfg(feature = \"tauri\")]\nfn a() {}",
            "#[cfg(test)]\nmod tests {}",
            "fn unix_time() {}",
        ] {
            assert!(sites(text, false).is_empty(), "{text}");
        }
        assert_eq!(kinds("#[cfg(windows)]\nlet p = libc::x();"), vec!["cfg", "os-api"]);
    }

    #[test]
    fn a_process_or_a_lock_in_test_code_is_no_site_but_an_os_cfg_or_api_is() {
        let in_tests = "fn p() {}\n#[cfg(test)]\nmod tests {\n    fn t() { std::process::Command::new(\"node\"); }\n}\n";
        assert!(sites(in_tests, false).is_empty(), "a #[cfg(test)] item");
        assert!(
            sites("fn t() { std::process::Command::new(\"node\"); }", true).is_empty(),
            "a test file"
        );
        assert_eq!(
            sites("#[cfg(windows)]\nfn t() { let _ = libc::getpid(); }", true).len(),
            2,
            "cfg and an OS API still count in a test file"
        );
        assert!(
            is_test_file("crates/x/tests/it.rs")
                && is_test_file("tools/paddock-dev/src/ci_run/tests.rs")
                && is_test_file("crates/paddock-service/src/resume_tests.rs")
        );
        assert!(!is_test_file("crates/x/src/lib.rs") && !is_test_file("crates/x/src/contests.rs"));
    }

    #[test]
    fn crate_paths_read_plain_inline_and_grouped_uses_but_not_test_code() {
        let text = "use crate::a::b;\nuse crate::{c, d::e, f::{g, h as i}};\nfn x() { crate::j::k(); }\n#[cfg(test)]\nmod tests { use crate::t::u; }\n";
        let got: Vec<String> = crate_paths(text).into_iter().map(|(_, p)| p).collect();
        for want in ["a::b", "c", "d::e", "f::g", "f::h", "j::k"] {
            assert!(got.iter().any(|p| p == want), "{want} in {got:?}");
        }
        assert!(!got.iter().any(|p| p.starts_with('t')), "{got:?}");
    }

    #[test]
    fn coverage_of_a_module_by_the_listed_prefixes() {
        let listed = vec!["ci_host::".to_string(), "pre_push::exec::".to_string()];
        assert!(covered("ci_host::kept_clone", &listed));
        assert!(covered("pre_push::exec", &listed));
        assert!(
            covered("pre_push", &listed),
            "an item of the folder's mod.rs: something of it is listed"
        );
        assert!(!covered("pre_push::gates", &listed));
        assert!(!covered("local_sccache", &listed));
        assert!(below("ci_host::x", &listed) && !below("pre_push", &listed));
    }

    #[test]
    fn a_vendored_fuzz_research_or_standalone_manifest_is_no_member() {
        assert!(non_member("apps/d/vendor/tray-icon/Cargo.toml", "[package]"));
        assert!(non_member("apps/d/docs/research/h/Cargo.toml", "[package]"));
        assert!(non_member("crates/x/fuzz/Cargo.toml", "[package]"));
        assert!(non_member("apps/d/h/Cargo.toml", "[package]\n[workspace]"));
        assert!(!non_member("apps/d/app/src-tauri/Cargo.toml", "[package]"));
    }

    #[test]
    fn module_paths_of_source_and_test_files() {
        assert_eq!(
            src_module("tools/paddock-dev/src/ci_host/kept_clone.rs").as_deref(),
            Some("ci_host::kept_clone")
        );
        assert_eq!(src_module("tools/paddock-dev/src/pre_push/mod.rs").as_deref(), Some("pre_push"));
        assert_eq!(src_module("tools/paddock-dev/src/lib.rs"), None);
        assert_eq!(src_module("tools/paddock-dev/src/ci_run/tests.rs"), None);
        assert_eq!(
            it_module("tools/paddock-dev/tests/it/golden/planner.rs").as_deref(),
            Some("golden::planner")
        );
        assert_eq!(it_module("tools/paddock-dev/tests/it/main.rs"), None);
    }
}
