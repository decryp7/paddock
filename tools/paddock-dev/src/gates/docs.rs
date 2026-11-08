//! ADR 0060 §5: rule-bearing docs name real checks and real developer commands. One scanner module (rule 55): the files are
//! selected by the register's `[docs]` globs, inline code and code blocks are extracted once, and each [`FactKind`] verifies
//! the facts it understands. "Doc facts from code" adds its kinds here.

use super::register::Register;
use super::tree::{Tree, any_glob, glob_regex};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// One quoted piece of a doc: an inline code span or a line of a code block.
#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
    /// 1-based line in the doc.
    pub line: usize,
    /// The text.
    pub text: String,
}

/// What the verifiers know about the repository.
pub struct Known<'a> {
    /// The files.
    pub tree: &'a dyn Tree,
    /// Check ids that exist (conformance, dev checks).
    pub check_ids: BTreeSet<String>,
    /// Ids that docs may name: `[[planned]]` and `[[retired]]`.
    pub allowed_ids: BTreeSet<String>,
    /// `paddock-conformance` subcommands (`paddock-conformance commands --json`); `None` when it could not be read.
    pub conformance_commands: Option<BTreeSet<String>>,
    /// `./dev` command paths, `gates check`, `ci slot acquire`, …
    pub dev_commands: BTreeSet<String>,
}

/// A fact in a doc that does not hold.
#[derive(Debug, Clone, PartialEq)]
pub struct DocProblem {
    /// The doc.
    pub path: String,
    /// The line.
    pub line: usize,
    /// What is wrong.
    pub message: String,
    /// The next step.
    pub fix: String,
}

/// A kind of fact docs state.
pub trait FactKind {
    /// Its name (for reports).
    fn name(&self) -> &'static str;
    /// Verifies the facts of one doc.
    fn verify(&self, path: &str, text: &str, snippets: &[Snippet], known: &Known) -> Vec<DocProblem>;
}

static INLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`\n]+)`").unwrap_or_else(|e| panic!("{e}")));
static CHECK_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bC(\d{2,3})\b").unwrap_or_else(|e| panic!("{e}")));

/// Inline code spans and the lines of fenced code blocks.
pub fn snippets(text: &str) -> Vec<Snippet> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_block = !in_block;
            continue;
        }
        if in_block {
            out.push(Snippet {
                line: i + 1,
                text: t.trim_start_matches("$ ").to_string(),
            });
        } else {
            for c in INLINE.captures_iter(line) {
                out.push(Snippet {
                    line: i + 1,
                    text: c[1].to_string(),
                });
            }
        }
    }
    out
}

/// Check ids (`C58`) named anywhere in the doc must be known, planned or retired.
pub struct CheckId;

impl FactKind for CheckId {
    fn name(&self) -> &'static str {
        "check id"
    }

    fn verify(&self, path: &str, text: &str, _snippets: &[Snippet], known: &Known) -> Vec<DocProblem> {
        let mut out = Vec::new();
        let mut in_block = false;
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_block = !in_block;
            }
            let _ = in_block;
            for m in CHECK_ID.find_iter(line) {
                let id = m.as_str();
                if !known.check_ids.contains(id) && !known.allowed_ids.contains(id) {
                    out.push(DocProblem {
                        path: path.to_string(),
                        line: i + 1,
                        message: format!("names check {id}, which does not exist"),
                        fix: format!(
                            "Correct the text, or register {id} under [[planned]] (with its ADR) or [[retired]] in docs/gates.toml."
                        ),
                    });
                }
            }
        }
        out
    }
}

/// Developer-tool commands in code: `paddock-conformance …`, `./dev …`, `node <path>`, `bash <path>`, `npm run <s> -w <ws>`.
pub struct ToolCommand;

fn path_exists(known: &Known, doc: &str, rel: &str) -> bool {
    // Relative to the repository, or to the doc's folder or any folder above it (an app's README names its own scripts).
    let mut dir = doc.rsplit_once('/').map_or("", |(d, _)| d).to_string();
    loop {
        let candidate = if dir.is_empty() { rel.to_string() } else { format!("{dir}/{rel}") };
        let candidate = super::shell::normalise(&candidate).unwrap_or(candidate);
        if known.tree.files().binary_search(&candidate).is_ok() {
            return true;
        }
        match dir.rsplit_once('/') {
            Some((d, _)) => dir = d.to_string(),
            None if dir.is_empty() => return false,
            None => dir.clear(),
        }
    }
}

fn plain_path(t: &str) -> bool {
    !t.is_empty()
        && !t.contains(['<', '>', '$', '*', '{', '}', '[', ']', '~', '\\', '(', ')', '|', ';', '&'])
        && !t.starts_with('/')
        && !t.starts_with("http")
}

fn dev_path_exists(known: &Known, words: &[&str]) -> Option<String> {
    // Walk the command tree word by word.
    let mut path = String::new();
    for w in words {
        if w.starts_with('-') || w.starts_with('<') || w.starts_with('[') {
            break;
        }
        let next = if path.is_empty() { w.to_string() } else { format!("{path} {w}") };
        if known.dev_commands.contains(&next) {
            path = next;
        } else if known
            .dev_commands
            .iter()
            .any(|c| c.starts_with(&format!("{path} ")) && !path.is_empty() || path.is_empty() && c.starts_with(&format!("{w} ")))
        {
            // a group whose subcommand is missing
            return Some(next);
        } else if path.is_empty() {
            return Some(next);
        } else {
            // positional argument of a leaf command
            break;
        }
    }
    None
}

impl FactKind for ToolCommand {
    fn name(&self) -> &'static str {
        "tool command"
    }

    fn verify(&self, path: &str, _text: &str, snippets: &[Snippet], known: &Known) -> Vec<DocProblem> {
        let mut out = Vec::new();
        let mut push = |line: usize, message: String, fix: &str| {
            out.push(DocProblem {
                path: path.to_string(),
                line,
                message,
                fix: fix.to_string(),
            });
        };
        for s in snippets {
            let words: Vec<&str> = s.text.split_whitespace().collect();
            let Some(first) = words.first() else { continue };
            // paddock-conformance [subcommand] …
            let conf_at = if *first == "paddock-conformance" {
                Some(0)
            } else if *first == "node" && words.get(1).is_some_and(|w| w.ends_with("conformance/bin/paddock-conformance.mjs")) {
                Some(1)
            } else {
                None
            };
            if let Some(at) = conf_at {
                if let (Some(sub), Some(cmds)) = (
                    words.get(at + 1).filter(|w| !w.starts_with('-') && !w.starts_with('<')),
                    &known.conformance_commands,
                ) && !cmds.contains(*sub)
                {
                    push(
                        s.line,
                        format!("`{}` uses the subcommand `{sub}`, which paddock-conformance does not have", s.text),
                        "Correct the command, or mark the check and command as planned (ADR number) until they exist.",
                    );
                }
                continue;
            }
            if *first == "./dev" || *first == "dev" && words.get(1).is_some() && known.dev_commands.contains(words[1]) {
                if let Some(bad) = dev_path_exists(known, &words[1..]) {
                    push(
                        s.line,
                        format!("`{}` names `./dev {bad}`, which does not exist", s.text),
                        "Run `./dev --help` for the commands, and correct the text.",
                    );
                }
                continue;
            }
            if (*first == "node" || *first == "bash") && !words.get(1).is_some_and(|w| w.starts_with('-')) {
                if let Some(p) = words
                    .get(1)
                    .copied()
                    .filter(|p| plain_path(p) && p.contains('/') && p.contains('.'))
                {
                    let p = p.trim_matches(|c| c == '"' || c == '\'');
                    if !path_exists(known, path, p) {
                        push(
                            s.line,
                            format!("`{}` runs {p}, which is not in the repository", s.text),
                            "Correct the path, or remove the command.",
                        );
                    }
                }
                continue;
            }
            if words.len() >= 5 && words[..3] == ["npm", "run", words[2]] && words[3] == "-w" {
                let (script, ws) = (words[2], words[4]);
                if plain_path(script) && plain_path(ws) {
                    let pkg = known.tree.files().iter().find(|f| {
                        f.ends_with("/package.json")
                            && !f.contains("node_modules")
                            && (f.trim_end_matches("/package.json") == ws
                                || known.tree.read(f).is_some_and(|t| t.contains(&format!("\"name\": \"{ws}\""))))
                    });
                    let ok = pkg
                        .and_then(|f| known.tree.read(f))
                        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                        .is_some_and(|j| j.get("scripts").and_then(|s| s.get(script)).is_some());
                    if !ok {
                        push(
                            s.line,
                            format!("`{}`: workspace {ws} has no script {script}", s.text),
                            "Correct the script or workspace name.",
                        );
                    }
                }
            }
        }
        out
    }
}

/// Checks every rule-bearing doc.
pub fn check_docs(tree: &dyn Tree, reg: &Register, known: &Known, kinds: &[&dyn FactKind]) -> Vec<DocProblem> {
    let include: Vec<Regex> = reg.docs_include.iter().map(|g| glob_regex(g)).collect();
    let exclude: Vec<Regex> = reg.docs_exclude.iter().map(|g| glob_regex(g)).collect();
    let mut out = Vec::new();
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();
    for p in tree.files() {
        if !p.ends_with(".md") || !any_glob(&include, p) || any_glob(&exclude, p) || seen.insert(p.clone(), ()).is_some() {
            continue;
        }
        let Some(text) = tree.read(p) else { continue };
        let sn = snippets(&text);
        for k in kinds {
            out.extend(k.verify(p, &text, &sn, known));
        }
    }
    out
}

/// The facts C100 verifies today.
pub fn default_kinds() -> [&'static dyn FactKind; 2] {
    [&CheckId, &ToolCommand]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::register;
    use crate::gates::tree::MemTree;

    fn known<'a>(tree: &'a MemTree) -> Known<'a> {
        Known {
            tree,
            check_ids: ["C13", "C100"].iter().map(|s| s.to_string()).collect(),
            allowed_ids: ["C58"].iter().map(|s| s.to_string()).collect(),
            conformance_commands: Some(["commit-lint", "sync", "list"].iter().map(|s| s.to_string()).collect()),
            dev_commands: ["gates", "gates check", "gates list", "wait", "ci", "ci slot", "ci slot acquire"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    fn reg() -> Register {
        register::parse("version = 1\n[docs]\ninclude = [\"AGENTS.md\", \"**/README.md\"]\nexclude = [\"docs/adr/**\"]\n").0
    }

    fn problems(files: &[(&str, &str)]) -> Vec<String> {
        let tree = MemTree::new(files);
        let kinds = default_kinds();
        check_docs(&tree, &reg(), &known(&tree), &kinds)
            .into_iter()
            .map(|p| format!("{}:{} {}", p.path, p.line, p.message))
            .collect()
    }

    #[test]
    fn check_ids_good_and_bad() {
        // AGENTS.md of eight apps: "register check `paddock-conformance.mjs copies --check-register` (ADR 0034, C58)"
        let p = problems(&[("AGENTS.md", "see C13 and C100, planned C58\nand C99 and C21 here\n")]);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains("AGENTS.md:2 names check C99"));
        // ADRs are history: they may name what was planned
        assert!(problems(&[("docs/adr/0034.md", "C58 C77"), ("AGENTS.md", "")]).is_empty());
    }

    #[test]
    fn conformance_and_dev_commands() {
        let md = "run `node conformance/bin/paddock-conformance.mjs copies --check-register` or `paddock-conformance list`\nand `./dev gates check --warn` or `./dev gates frobnicate` or `./dev ci slot acquire --n 2`\n";
        let p = problems(&[("AGENTS.md", md)]);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains("subcommand `copies`"));
        assert!(p[1].contains("`./dev gates frobnicate`"));
    }

    #[test]
    fn node_paths_and_npm_scripts() {
        let files = [
            (
                "apps/dyno/README.md",
                "```sh\nnode scripts/check-wording.mjs\nnode scripts/missing.mjs\nnpm run test:tokens -w ui\nnpm run nope -w ui\n```\n",
            ),
            ("apps/dyno/scripts/check-wording.mjs", ""),
            ("ui/package.json", r#"{"name":"@decryp7/paddock-ui","scripts":{"test:tokens":"x"}}"#),
        ];
        let p = problems(&files);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains("scripts/missing.mjs"));
        assert!(p[1].contains("no script nope"));
    }
}
