//! C179: a crate whose tests read files the static reads extractor cannot resolve declares what it reads (ADR 0116 §2, amended
//! 2026-11-27, user decision 122). Without a declaration the test cache never reuses the crate nor any crate that depends on it, so one
//! blind crate such as `paddock-testkit` keeps most of a workspace `Never`. The declaration is `[package.metadata.paddock] test-reads =
//! ["glob", ...]` in the crate's `Cargo.toml`, globs relative to the repository root (an empty list when the blind reads touch no
//! repository file).
//!
//! A blind site is what [`crate::repo_model::code::blind_of`] says of a Rust source's parse ([`rust_reads`]): the one definition the
//! model's blind list uses, so this check and `./dev ci test-cache blind` cannot disagree on what a site is. Only crates that are members
//! of a Cargo workspace of the tree count (a fixture or fuzz crate outside every `members` list is not a test target). One finding per
//! crate: the crate with sites and no declaration, or a declared glob that matches no tracked file (a stale declaration is not
//! trusted, and the cache keeps the crate `Never`).
//!
//! Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree, recorded here and in docs/gates.toml
//! (`[[report_only]]` C179). No baseline file (ER-46): the report is the list. Rust host only: born in Rust.

use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use crate::repo_model::code::{BLIND_TRIGGERS, blind_of};
use crate::repo_model::reads::rust_reads;
use crate::repo_model::test_reads::matches as glob_matches;
use std::collections::{BTreeMap, BTreeSet};

/// The rule field: rule 79 (declared data is read through its accessor, ER-42); a crate's `test-reads` is declared data, read through
/// `cargo metadata` by the model and through the manifest's own TOML parser here.
const RULE: u32 = 79;

/// What a `Cargo.toml` says that the check needs.
struct Manifest {
    /// The folder.
    dir: String,
    /// It has a `[package]`.
    package: bool,
    /// `[workspace] members` and `exclude`, when it has a `[workspace]`.
    workspace: Option<(Vec<String>, Vec<String>)>,
    /// `[package.metadata.paddock] test-reads`, `None` when absent or not a list of strings.
    declared: Option<Vec<String>>,
}

fn strings(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect()
}

fn manifest(rel: &str, text: &str) -> Option<Manifest> {
    let t: toml::Table = text.parse().ok()?;
    let dir = rel.strip_suffix("/Cargo.toml").unwrap_or("").to_string();
    let workspace = t
        .get("workspace")
        .and_then(toml::Value::as_table)
        .map(|w| (strings(w.get("members")), strings(w.get("exclude"))));
    let declared = t
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("paddock"))
        .and_then(|p| p.get("test-reads"))
        .and_then(toml::Value::as_array)
        .and_then(|a| a.iter().map(|g| g.as_str().map(str::to_string)).collect::<Option<Vec<String>>>());
    Some(Manifest {
        dir,
        package: t.contains_key("package"),
        workspace,
        declared,
    })
}

/// `dir` joined under `base` ("" is the repository root).
fn under(base: &str, rel: &str) -> String {
    let rel = rel.trim_start_matches("./");
    match (base.is_empty(), rel) {
        (_, ".") => base.to_string(),
        (true, r) => r.to_string(),
        (false, r) => format!("{base}/{r}"),
    }
}

/// The crates that are members of a workspace of the tree: a workspace root with a `[package]` is its own member.
fn members(all: &[Manifest]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for w in all {
        let Some((members, exclude)) = &w.workspace else { continue };
        if w.package {
            out.insert(w.dir.clone());
        }
        for m in all.iter().filter(|m| m.package) {
            let hit = |pats: &Vec<String>| {
                pats.iter()
                    .any(|p| glob_matches(&under(&w.dir, p), &m.dir) || under(&w.dir, p) == m.dir)
            };
            if hit(members) && !hit(exclude) {
                out.insert(m.dir.clone());
            }
        }
    }
    out
}

/// One crate with blind sites or a declaration that does not hold.
#[derive(Debug, PartialEq, Eq)]
pub struct Gap {
    /// The crate folder.
    pub dir: String,
    /// The blind sites, `file: reason`.
    pub sites: Vec<String>,
    /// The declaration, `None` when there is none.
    pub declared: Option<Vec<String>>,
    /// The declared globs that match no tracked file.
    pub empty: Vec<String>,
}

/// The crates of `files` (read through `read`) that are findings: blind sites with no declaration, or a declaration with a glob that
/// matches nothing.
pub fn gaps(files: &[String], read: &dyn Fn(&str) -> Option<String>) -> (Vec<Gap>, usize) {
    let manifests: Vec<Manifest> = files
        .iter()
        .filter(|f| f.ends_with("Cargo.toml") && (f.as_str() == "Cargo.toml" || f.ends_with("/Cargo.toml")))
        .filter_map(|f| manifest(f, &read(f)?))
        .collect();
    let member = members(&manifests);
    let declared: BTreeMap<&str, &Option<Vec<String>>> = manifests
        .iter()
        .filter(|m| member.contains(&m.dir))
        .map(|m| (m.dir.as_str(), &m.declared))
        .collect();
    // The longest member folder a file is under owns it.
    let owner = |f: &str| {
        member
            .iter()
            .filter(|d| d.is_empty() || f.starts_with(&format!("{d}/")))
            .max_by_key(|d| d.len())
            .cloned()
    };
    let mut sites: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut probes = 0;
    for f in files
        .iter()
        .filter(|f| f.ends_with(".rs") && !f.split('/').any(|s| s == "target" || s == "node_modules"))
    {
        let Some(dir) = owner(f) else { continue };
        let Some(text) = read(f) else { continue };
        if !BLIND_TRIGGERS.iter().any(|t| text.contains(t)) {
            continue;
        }
        probes += 1;
        for why in blind_of(&rust_reads(f, &text, &dir), &text) {
            sites.entry(dir.clone()).or_default().push(format!("{f}: {why}"));
        }
    }
    let mut out = Vec::new();
    for dir in &member {
        let decl = declared.get(dir.as_str()).and_then(|d| (*d).clone());
        let empty: Vec<String> = decl
            .iter()
            .flatten()
            .filter(|g| !files.iter().any(|f| glob_matches(g, f)))
            .cloned()
            .collect();
        let own = sites.remove(dir).unwrap_or_default();
        if (decl.is_none() && !own.is_empty()) || !empty.is_empty() {
            out.push(Gap {
                dir: dir.clone(),
                sites: own,
                declared: decl,
                empty,
            });
        }
    }
    (out, probes)
}

/// The check.
pub struct C179;

impl Check for C179 {
    fn id(&self) -> &'static str {
        "C179"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a crate whose tests read files the graph cannot resolve declares them in [package.metadata.paddock] test-reads"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn origin(&self) -> super::Origin {
        super::Origin::BornInRust
    }

    fn control(&self) -> Control {
        let manifest =
            |extra: &str| -> &'static str { Box::leak(format!("[package]\nname = \"x\"\nversion = \"0.1.0\"\n{extra}").into_boxed_str()) };
        Control {
            files: vec![
                (
                    "crates/Cargo.toml",
                    "[workspace]\nmembers = [\"blind\", \"declared\", \"stale\", \"clean\", \"empty\"]\nexclude = [\"outside\"]\n",
                ),
                // Red: a blind site and no declaration (line 1: the finding stands on the manifest's first line).
                ("crates/blind/Cargo.toml", manifest("")),
                ("crates/blind/src/lib.rs", "pub fn f() { let _ = std::env::current_dir(); }\n"),
                // Red: a declared glob that matches no tracked file.
                (
                    "crates/stale/Cargo.toml",
                    manifest("[package.metadata.paddock]\ntest-reads = [\"gone/**\"]\n"),
                ),
                ("crates/stale/src/lib.rs", "pub fn f() {}\n"),
                // Green: the same blind site with a declaration that holds, an empty declaration, a crate with no blind site, a crate
                // that is no workspace member (a fixture).
                (
                    "crates/declared/Cargo.toml",
                    manifest("[package.metadata.paddock]\ntest-reads = [\"data/**\"]\n"),
                ),
                ("crates/declared/src/lib.rs", "pub fn f() { let _ = std::env::current_dir(); }\n"),
                ("crates/empty/Cargo.toml", manifest("[package.metadata.paddock]\ntest-reads = []\n")),
                ("crates/empty/src/lib.rs", "pub fn f() { let _ = std::env::current_dir(); }\n"),
                ("crates/clean/Cargo.toml", manifest("")),
                ("crates/clean/src/lib.rs", "pub fn f() {}\n"),
                ("crates/outside/Cargo.toml", manifest("")),
                ("crates/outside/src/lib.rs", "pub fn f() { let _ = std::env::current_dir(); }\n"),
                ("data/table.txt", "x\n"),
            ],
            expect: vec![("crates/blind/Cargo.toml", 1), ("crates/stale/Cargo.toml", 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let tree = ctx.tree_files();
        let scoped: BTreeSet<String> = ctx.files().iter().cloned().collect();
        let (found, probes) = gaps(&tree, &|f| ctx.read(f).map(|t| t.to_string()));
        let mut findings = Vec::new();
        for g in found {
            let manifest = under(&g.dir, "Cargo.toml");
            // A narrowed run (`--files`) reports the crates it touches.
            if !scoped.contains(&manifest) && !g.sites.iter().any(|s| s.split(':').next().is_some_and(|f| scoped.contains(f))) {
                continue;
            }
            let (message, next) = if !g.empty.is_empty() {
                (
                    format!(
                        "`test-reads` of {} names {} that match no tracked file: the declaration is stale, so the test cache keeps the crate and its dependents `Never`",
                        g.dir,
                        g.empty.iter().map(|e| format!("`{e}`")).collect::<Vec<_>>().join(", ")
                    ),
                    "Make each glob match the files the tests read, or remove it.",
                )
            } else {
                let shown: Vec<&str> = g.sites.iter().take(2).map(String::as_str).collect();
                (
                    format!(
                        "{} has {} blind read(s) its tests may depend on ({}{}) and no `[package.metadata.paddock] test-reads`: the test cache never reuses it, nor any crate that depends on it",
                        g.dir,
                        g.sites.len(),
                        shown.join("; "),
                        if g.sites.len() > 2 { "; ..." } else { "" }
                    ),
                    "Read each site and list the repository files it reads as globs relative to the root in the crate's `[package.metadata.paddock] test-reads` (an empty list when it touches none), or rewrite the site as a literal chain from CARGO_MANIFEST_DIR. Never write a glob only to silence this: a glob the tests do not read, or miss, makes the key wrong. `./dev ci test-cache blind` lists every site.",
                )
            };
            findings.push(Finding::warn("C179", RULE, &manifest, 1, message, next));
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: probes > 0 || ctx.is_in_memory(),
                detail: format!("{probes} Rust files with a read the graph may not resolve, read"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(files: &[(&str, &str)]) -> Vec<Gap> {
        let list: Vec<String> = files.iter().map(|(f, _)| f.to_string()).collect();
        let text = |f: &str| files.iter().find(|(p, _)| *p == f).map(|(_, t)| t.to_string());
        gaps(&list, &text).0
    }

    const WS: (&str, &str) = ("Cargo.toml", "[workspace]\nmembers = [\"a\", \"nested/*\"]\n");

    fn pkg(extra: &str) -> String {
        format!("[package]\nname = \"a\"\nversion = \"0.1.0\"\n{extra}")
    }

    #[test]
    fn a_blind_site_without_a_declaration_is_a_gap_and_with_one_it_is_not() {
        let site = "fn f() { let _ = std::env::current_dir(); }\n";
        let bare = pkg("");
        let got = run(&[WS, ("a/Cargo.toml", &bare), ("a/src/lib.rs", site)]);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].dir, "a");
        assert!(got[0].sites[0].starts_with("a/src/lib.rs:"), "{:?}", got[0].sites);
        let with = pkg("[package.metadata.paddock]\ntest-reads = [\"data/*\"]\n");
        assert!(run(&[WS, ("a/Cargo.toml", &with), ("a/src/lib.rs", site), ("data/x.txt", "")]).is_empty());
    }

    #[test]
    fn a_glob_that_matches_nothing_is_a_gap_even_without_a_blind_site() {
        let with = pkg("[package.metadata.paddock]\ntest-reads = [\"gone/*\"]\n");
        let got = run(&[WS, ("a/Cargo.toml", &with), ("a/src/lib.rs", "fn f() {}\n")]);
        assert_eq!(got[0].empty, vec!["gone/*".to_string()]);
    }

    #[test]
    fn only_workspace_members_count_and_a_nested_site_belongs_to_its_own_crate() {
        let site = "fn f() { let _ = std::env::current_dir(); }\n";
        let bare = pkg("");
        // `fixture` is no member; `nested/b` is a member through a glob and owns its own file.
        let got = run(&[
            WS,
            ("a/Cargo.toml", &bare),
            ("a/src/lib.rs", "fn f() {}\n"),
            ("fixture/Cargo.toml", &bare),
            ("fixture/src/lib.rs", site),
            ("nested/b/Cargo.toml", &bare),
            ("nested/b/src/lib.rs", site),
        ]);
        assert_eq!(got.iter().map(|g| g.dir.as_str()).collect::<Vec<_>>(), ["nested/b"]);
    }
}
