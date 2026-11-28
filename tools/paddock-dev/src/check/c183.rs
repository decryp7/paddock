//! C183: a file that git sees renamed or deleted is no longer named by the path registers (rule 40, ER-21, ER-33, ER-42; user decision,
//! 2026-11-28, in chat). ER-21 class: during the paddock-dev split, registers keyed by path kept the old path after files moved, twice:
//! conformance's `mention` lines and `conformance/ci/checkout-profiles.txt`, the `ci-runs-reader:` test list and paddock.ci.json's
//! `hostTests` entries. Only a full conformance run saw it, and each time it cost a gate round.
//!
//! What is read, and how:
//! - the files git's own rename detection says were renamed or deleted between the run's merge base and the tree
//!   (`git diff --name-status -M`, [`crate::git::Git::moves`] through [`Ctx::moves`]: one read, parsed once by
//!   [`crate::git::parse_moves`]); a path that exists again in the tree (a swap, a re-created file) is not a move;
//! - the path registers, [`REGISTERS`], declared once here (ER-22): every tracked file that holds repo-relative paths a tool or a person
//!   keeps by hand and that no engine already carries over a rename. The allow lists and baselines of the conformance engine
//!   (`conformance/*-allow.json`, `tooling-files.json`, `baselines/`) follow a rename on their own ([`super::baseline::renames`]), so
//!   they are not here. A register file is read as text and every path-shaped token of a line is compared whole with an old path
//!   (a longer path, a `file.rs:12` suffix and a glob are told apart by the token's own boundaries), because the registers are
//!   TOML, JSON, plain lists and prose notes with one thing in common: a line that names a path.
//!
//! Each finding names the register file and line, and "moved to <new path>" or "deleted". Not read: a folder named by a register
//! after all its files moved (git reports files, not folders), workflows and scripts (code that fails when it runs, not a register),
//! a path git renamed with so many edits that it reports a deletion and an addition (the finding says "deleted"), and sources:
//! there is no source marker that names a repo path (`ci-runs-reader:` names a reason and travels with its file).
//!
//! A register that no tracked file matches is a finding too: the list cannot lag the tree (ER-33). A run that cannot read git's answer
//! (no merge base, no work tree) reports "not measured" and never a pass.
//!
//! Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree, recorded here and in docs/gates.toml
//! (`[[report_only]]` C183). No baseline file (ER-46): the report is the list. Rust host only: born in Rust.

use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use crate::git::Moved;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The rule field: rule 40, a recurring class is a missing design rule.
const RULE: u32 = 40;

/// This file, where a finding about the check itself (a register no file matches, a run that could not measure) is reported.
const SELF_FILE: &str = "tools/paddock-dev/src/check/c183.rs";

/// How a register names its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// One file, by its repository-relative path.
    Exact(&'static str),
    /// A file name wherever it is tracked (outside test, fixture and vendored folders).
    Name(&'static str),
    /// Every file with this extension directly in this folder (`folder` ends with `/`).
    DirExt(&'static str, &'static str),
}

/// The path registers, declared once. A register is a file a tool or a person keeps by hand with repo-relative paths in it.
pub const REGISTERS: [Match; 15] = [
    // Ownership and plan inputs.
    Match::Exact("CODEOWNERS"),
    Match::Exact("docs/repo-graph.toml"),
    Match::Exact("docs/generators.toml"),
    // Registers of copies, gates, escapes, endpoints and features.
    Match::Exact("docs/known-copies.toml"),
    Match::Exact("docs/gates.toml"),
    Match::Exact("docs/escape-register.toml"),
    Match::Exact("docs/ipc-endpoints.toml"),
    Match::Exact("docs/shared-crate-features.toml"),
    // Waivers and the conformance contract.
    Match::Exact("paddock.waivers.toml"),
    Match::Exact("paddock.conformance.json"),
    // What a job checks out: bootstrap folders, excluded paths and `mention` lines.
    Match::DirExt("conformance/ci/", ".txt"),
    // Each project's CI contract: workspaces, `hostTests`, nextest configs, check lists.
    Match::Name("paddock.ci.json"),
    // A tool's declared build inputs, one path per line.
    Match::Name("inputs.txt"),
    // Files that name where a thing is defined or owned: shared enums (`definedIn`), the temp-folder owners of the disk audit.
    Match::Exact("conformance/shared-enums.json"),
    Match::Exact("tools/paddock-dev/src/disk/temp-prefixes.toml"),
];

/// Folders a [`Match::Name`] never reads: test inputs and vendored trees hold paths of their own.
const SKIP_FOLDERS: [&str; 4] = ["/tests/", "/fixtures/", "/node_modules/", "/target/"];

impl Match {
    /// Whether `file` is a register of this kind.
    pub fn matches(&self, file: &str) -> bool {
        match *self {
            Match::Exact(p) => file == p,
            Match::Name(n) => {
                let in_folder = file.rsplit_once('/').map(|(d, _)| d);
                let name = file.rsplit('/').next().unwrap_or(file);
                let padded = format!("/{}", in_folder.map(|d| format!("{d}/")).unwrap_or_default());
                name == n && !SKIP_FOLDERS.iter().any(|s| padded.contains(s))
            }
            Match::DirExt(dir, ext) => file
                .strip_prefix(dir)
                .is_some_and(|rest| !rest.contains('/') && rest.ends_with(ext)),
        }
    }

    /// The register as the findings print it.
    pub fn describe(&self) -> String {
        match *self {
            Match::Exact(p) => p.to_string(),
            Match::Name(n) => format!("{n} (any folder)"),
            Match::DirExt(d, e) => format!("{d}*{e}"),
        }
    }
}

/// The characters a path token is made of; everything else (quotes, spaces, `:`, `,`, `=`, brackets, `*`) ends one.
fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | '@')
}

/// The path-shaped tokens of a line: a run of path characters, with a leading `./` or `/` (CODEOWNERS anchors a path at the root with
/// one) and trailing dots (a sentence's full stop) cut off.
pub fn path_tokens(line: &str) -> Vec<&str> {
    line.split(|c: char| !is_path_char(c))
        .map(|t| {
            t.strip_prefix("./")
                .or_else(|| t.strip_prefix('/'))
                .unwrap_or(t)
                .trim_end_matches('.')
        })
        .filter(|t| t.contains('.') || t.contains('/'))
        .collect()
}

/// A stale line of a register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stale {
    /// The 1-based line.
    pub line: usize,
    /// The old path the line still names.
    pub old: String,
}

/// The lines of `text` that name an old path of `moved` as a whole token, in line order, once per line and path.
pub fn stale_lines(text: &str, moved: &HashMap<&str, &Moved>) -> Vec<Stale> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for t in path_tokens(line) {
            if moved.contains_key(t) && seen.insert(t) {
                out.push(Stale {
                    line: i + 1,
                    old: t.to_string(),
                });
            }
        }
    }
    out
}

fn message(old: &str, how: &Moved) -> String {
    match how {
        Moved::Renamed { to, .. } => format!("`{old}` moved to `{to}`, but this register line still names the old path"),
        Moved::Deleted(_) => format!("`{old}` was deleted, but this register line still names it"),
    }
}

fn fix(file: &str, line: usize, how: &Moved) -> String {
    match how {
        Moved::Renamed { to, .. } => format!("Edit {file}:{line} to name `{to}` (or drop the line when the entry no longer applies)."),
        Moved::Deleted(_) => format!(
            "Remove {file}:{line} or name the file that replaces it (a file rewritten beyond git's rename detection shows as a deletion: `git log --follow -- <path>`)."
        ),
    }
}

/// What the positive control says moved: a rename, a deletion and a path that exists again in the control tree.
fn control_moves() -> Vec<Moved> {
    vec![
        Moved::Renamed {
            from: "tools/x/old/moved.rs".into(),
            to: "tools/x/new/moved.rs".into(),
        },
        Moved::Deleted("tools/x/gone.rs".into()),
        Moved::Deleted("tools/x/recreated.rs".into()),
    ]
}

/// The check.
pub struct C183;

impl Check for C183 {
    fn id(&self) -> &'static str {
        "C183"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a file git sees renamed or deleted is not still named by a path register"
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
        // One register of each kind that names a moved file (line 2 and 3), a deleted file and its look-alikes, a re-created file, and a
        // file that is no register.
        Control {
            files: vec![
                (
                    "CODEOWNERS",
                    "# owners\n/tools/x/old/moved.rs @owner\n/tools/x/new/moved.rs @owner\ntools/x/old/moved.rs.bak tools/x/old/moved.rsx\n",
                ),
                (
                    "docs/known-copies.toml",
                    "[[copy]]\nfiles = [\"tools/x/gone.rs\", \"tools/x/gone.rs:12\"]\nnote = \"see tools/x/recreated.rs.\"\nglob = \"tools/x/*\"\n",
                ),
                ("apps/a/paddock.ci.json", "{\"hostTests\": {\"w\": [\"./tools/x/old/moved.rs\"]}}\n"),
                ("conformance/ci/checkout-profiles.txt", "mention tools/x/old/moved.rs the writer\n"),
                ("tools/x/recreated.rs", "fn main() {}\n"),
                ("apps/a/tests/fixtures/paddock.ci.json", "tools/x/old/moved.rs\n"),
                ("docs/notes.md", "tools/x/old/moved.rs is not a register\n"),
            ],
            expect: vec![
                ("CODEOWNERS", 2),
                ("apps/a/paddock.ci.json", 1),
                ("conformance/ci/checkout-profiles.txt", 1),
                ("docs/known-copies.toml", 2),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let tree_files = ctx.tree_files();
        let mut findings = Vec::new();
        // Every declared register must match a tracked file: the list cannot lag the tree (ER-33).
        let mut registers: BTreeMap<String, ()> = BTreeMap::new();
        let mut unmatched: Vec<String> = Vec::new();
        for m in REGISTERS {
            let hits: Vec<&String> = tree_files.iter().filter(|f| m.matches(f)).collect();
            if hits.is_empty() && !ctx.is_in_memory() {
                unmatched.push(m.describe());
            }
            registers.extend(hits.into_iter().map(|f| (f.clone(), ())));
        }
        for u in &unmatched {
            findings.push(Finding::warn(
                "C183",
                RULE,
                SELF_FILE,
                1,
                format!("the register `{u}` matches no tracked file, so the list of path registers lags the tree"),
                "Update REGISTERS in tools/paddock-dev/src/check/c183.rs: drop the entry, or name the file that replaced it.",
            ));
        }
        let moves: Vec<Moved> = if ctx.is_in_memory() {
            control_moves()
        } else {
            match ctx.moves() {
                Ok(m) => m.to_vec(),
                Err(why) => {
                    // Report-only: a warning that says what was not measured, never a pass and never a CI error (flip with the mode, ER-41).
                    findings.push(Finding::warn(
                        "C183",
                        RULE,
                        SELF_FILE,
                        1,
                        format!("not measured: the files git sees renamed or deleted could not be read ({why}), so no register line was judged"),
                        "Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.",
                    ));
                    Vec::new()
                }
            }
        };
        // A path that exists again is no move (a swap, a re-created file).
        let gone: HashMap<&str, &Moved> = moves.iter().filter(|m| !ctx.exists(m.old())).map(|m| (m.old(), m)).collect();
        let mut probes = 0;
        for file in registers.keys() {
            let Some(text) = ctx.read(file) else { continue };
            probes += 1;
            for s in stale_lines(&text, &gone) {
                let how = gone[s.old.as_str()];
                findings.push(Finding::warn(
                    "C183",
                    RULE,
                    file,
                    s.line,
                    message(&s.old, how),
                    fix(file, s.line, how),
                ));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: probes > 0,
                detail: format!(
                    "{probes} path register files read against {} moved or deleted path(s), {} of {} declared registers matched no file",
                    gone.len(),
                    unmatched.len(),
                    REGISTERS.len()
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gone(items: &[Moved]) -> HashMap<&str, &Moved> {
        items.iter().map(|m| (m.old(), m)).collect()
    }

    #[test]
    fn a_token_is_a_whole_path_and_never_a_part_of_a_longer_one() {
        assert_eq!(
            path_tokens("see a/b.rs:12, a/b.rs.bak and \"./a/b.rs\"."),
            vec!["a/b.rs", "a/b.rs.bak", "a/b.rs"]
        );
        assert_eq!(path_tokens("/a/b.rs @owner"), vec!["a/b.rs"]);
        assert_eq!(path_tokens("glob = \"a/*\" word"), vec!["a/"]);
        let mv = [Moved::Deleted("a/b.rs".into())];
        let m = gone(&mv);
        let hits: Vec<usize> = stale_lines("a/b.rs.bak\nxa/b.rs\n/a/b.rs\nx = [\"a/b.rs\"]\na/b.rs a/b.rs\n", &m)
            .iter()
            .map(|s| s.line)
            .collect();
        // A longer path and a longer name are no hit; a root-anchored `/a/b.rs` is, and a path twice on a line counts once.
        assert_eq!(hits, vec![3, 4, 5]);
    }

    #[test]
    fn the_registers_match_by_exact_path_name_and_folder() {
        assert!(Match::Exact("CODEOWNERS").matches("CODEOWNERS"));
        assert!(!Match::Exact("CODEOWNERS").matches("x/CODEOWNERS"));
        let n = Match::Name("paddock.ci.json");
        assert!(n.matches("paddock.ci.json") && n.matches("apps/a/paddock.ci.json"));
        assert!(!n.matches("tools/paddock-dev/tests/golden/fixture/apps/x/paddock.ci.json"));
        assert!(!n.matches("apps/a/node_modules/paddock.ci.json") && !n.matches("apps/a/paddock.ci.json.bak"));
        let d = Match::DirExt("conformance/ci/", ".txt");
        assert!(d.matches("conformance/ci/ports.txt") && !d.matches("conformance/ci/sub/ports.txt") && !d.matches("conformance/ci/x.sh"));
    }

    #[test]
    fn a_rename_and_a_deletion_each_get_their_own_words() {
        let r = Moved::Renamed {
            from: "a".into(),
            to: "b".into(),
        };
        assert!(message("a", &r).contains("moved to `b`"));
        assert!(fix("f", 3, &r).contains("f:3") && fix("f", 3, &r).contains("`b`"));
        assert!(message("a", &Moved::Deleted("a".into())).contains("deleted"));
    }
}
