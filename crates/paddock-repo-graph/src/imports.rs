//! The file level of the graph for TypeScript and JavaScript: which files a file imports by a relative specifier
//! (`import ... from './x'`, `import './x'`, `export ... from './x'`, `import('./x')`, `require('./x')`), and which of a set of
//! entry files (test specs) reach a changed file through those imports. The planner uses it to run only the specs a test-only
//! change reaches (ADR 0113 amendment 2026-11-23, §7); package imports (`@scope/pkg`) are the project level ([`crate::Graph`]).
//!
//! A scanner, not a parser: it finds string literals after `from`, `import` and `require(` outside comments. A specifier it
//! cannot resolve to a listed file is ignored, so a caller that needs a proof ("no spec reaches this helper") must treat an empty
//! answer as "cannot tell", never as "nothing to run".

use crate::paths::{dirname, join_normalized};
use std::collections::{BTreeMap, BTreeSet};

/// The extensions a relative specifier without one may name, in the order a bundler tries them.
const EXTENSIONS: [&str; 6] = [".ts", ".tsx", ".mts", ".js", ".mjs", ".cjs"];

/// At most this many files are followed from one entry (bounded resources; a real spec reaches a few dozen).
pub const MAX_REACH: usize = 5_000;

/// The relative specifiers of a source text (`./x`, `../y/z`), in order, without duplicates.
pub fn relative_specifiers(text: &str) -> Vec<String> {
    let code = strip_comments(text);
    let mut out: Vec<String> = Vec::new();
    for key in ["from", "import", "require("] {
        let mut at = 0;
        while let Some(i) = code[at..].find(key) {
            let start = at + i;
            at = start + key.len();
            // A whole word: `importer` or `fromage` is not a keyword.
            let before_ok = start == 0
                || !code[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
            if !before_ok {
                continue;
            }
            let rest = code[at..].trim_start_matches([' ', '\t', '\n', '\r', '(']);
            let Some(q) = rest
                .chars()
                .next()
                .filter(|c| matches!(c, '\'' | '"' | '`'))
            else {
                continue;
            };
            let Some(end) = rest[1..].find(q) else {
                continue;
            };
            let spec = &rest[1..1 + end];
            if (spec.starts_with("./") || spec.starts_with("../")) && !out.iter().any(|s| s == spec)
            {
                out.push(spec.to_string());
            }
        }
    }
    out
}

/// The text with `//` and `/* */` comments blanked (string literals are kept, so a `//` inside a quoted URL stays).
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for n in chars.by_ref() {
                    if last == '*' && n == '/' {
                        break;
                    }
                    last = n;
                }
                out.push(' ');
            }
            ('\'' | '"' | '`', _) => {
                quote = Some(c);
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// The file a relative specifier of `from` names among `files`: the path itself, with one of [`EXTENSIONS`], or its
/// `index` file. A `.js` specifier also finds the `.ts` source (TypeScript's ESM style).
pub fn resolve(from: &str, spec: &str, files: &BTreeSet<String>) -> Option<String> {
    let base = join_normalized(&dirname(from), spec)?;
    let mut tries = vec![base.clone()];
    if let Some(stem) = base.strip_suffix(".js") {
        tries.push(format!("{stem}.ts"));
    }
    tries.extend(EXTENSIONS.iter().map(|e| format!("{base}{e}")));
    tries.extend(EXTENSIONS.iter().map(|e| format!("{base}/index{e}")));
    tries.into_iter().find(|t| files.contains(t))
}

/// The import edges of `files` (each file to the listed files it imports by a relative specifier), read with `read`. A file
/// that cannot be read has no edges.
pub fn import_edges(
    files: &BTreeSet<String>,
    read: &dyn Fn(&str) -> Option<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    files
        .iter()
        .map(|f| {
            let to = read(f)
                .map(|t| {
                    relative_specifiers(&t)
                        .iter()
                        .filter_map(|s| resolve(f, s, files))
                        .collect()
                })
                .unwrap_or_default();
            (f.clone(), to)
        })
        .collect()
}

/// The entries that reach one of `changed` through `edges` (an entry that is itself changed reaches it), sorted. Each walk stops
/// after [`MAX_REACH`] files; an entry cut off there counts as reaching (it cannot be ruled out).
pub fn entries_reaching(
    entries: &BTreeSet<String>,
    changed: &BTreeSet<String>,
    edges: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|e| {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            let mut todo: Vec<&str> = vec![e.as_str()];
            while let Some(at) = todo.pop() {
                if changed.contains(at) {
                    return true;
                }
                if !seen.insert(at) {
                    continue;
                }
                if seen.len() > MAX_REACH {
                    return true;
                }
                todo.extend(edges.get(at).into_iter().flatten().map(String::as_str));
            }
            false
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn specifiers_are_the_relative_ones_outside_comments() {
        let text = "import { a } from './a';\nimport '../setup';\nexport * from \"./b.js\";\nconst c = await import('./c');\n\
                    const d = require('./d');\nimport x from '@scope/pkg';\n// import y from './commented';\n/* from './block' */\n\
                    const url = 'http://x//y'; const importer = './not-a-keyword';";
        assert_eq!(
            relative_specifiers(text),
            ["./a", "./b.js", "../setup", "./c", "./d"]
        );
    }

    #[test]
    fn a_specifier_resolves_to_a_listed_file_an_extension_or_an_index() {
        let files = set(&["app/e2e/helper.ts", "app/e2e/lib/index.ts", "app/src/x.ts"]);
        assert_eq!(
            resolve("app/e2e/a.spec.ts", "./helper", &files).as_deref(),
            Some("app/e2e/helper.ts")
        );
        assert_eq!(
            resolve("app/e2e/a.spec.ts", "./helper.js", &files).as_deref(),
            Some("app/e2e/helper.ts")
        );
        assert_eq!(
            resolve("app/e2e/a.spec.ts", "./lib", &files).as_deref(),
            Some("app/e2e/lib/index.ts")
        );
        assert_eq!(
            resolve("app/e2e/a.spec.ts", "../src/x", &files).as_deref(),
            Some("app/src/x.ts")
        );
        assert_eq!(resolve("app/e2e/a.spec.ts", "./missing", &files), None);
    }

    #[test]
    fn the_entries_reaching_a_changed_helper_follow_imports_transitively() {
        let texts: BTreeMap<&str, &str> = BTreeMap::from([
            ("e2e/a.spec.ts", "import { h } from './fixtures';"),
            ("e2e/b.spec.ts", "import { g } from './other';"),
            ("e2e/c.spec.ts", "test('c', () => {});"),
            ("e2e/fixtures.ts", "export * from './deep/h';"),
            ("e2e/deep/h.ts", "export const h = 1;"),
            ("e2e/other.ts", "export const g = 1;"),
        ]);
        let files: BTreeSet<String> = texts.keys().map(|s| s.to_string()).collect();
        let edges = import_edges(&files, &|f| texts.get(f).map(|t| t.to_string()));
        let specs = set(&["e2e/a.spec.ts", "e2e/b.spec.ts", "e2e/c.spec.ts"]);
        assert_eq!(
            entries_reaching(&specs, &set(&["e2e/deep/h.ts"]), &edges),
            set(&["e2e/a.spec.ts"])
        );
        assert_eq!(
            entries_reaching(&specs, &set(&["e2e/c.spec.ts", "e2e/other.ts"]), &edges),
            set(&["e2e/b.spec.ts", "e2e/c.spec.ts"])
        );
        // Positive control: a helper no entry imports reaches none (the caller must not read that as "run nothing").
        assert!(entries_reaching(&specs, &set(&["e2e/unused.ts"]), &edges).is_empty());
    }

    #[test]
    fn an_import_cycle_ends() {
        let texts: BTreeMap<&str, &str> = BTreeMap::from([
            ("a.ts", "import './b';"),
            ("b.ts", "import './a';"),
            ("s.spec.ts", "import './a';"),
        ]);
        let files: BTreeSet<String> = texts.keys().map(|s| s.to_string()).collect();
        let edges = import_edges(&files, &|f| texts.get(f).map(|t| t.to_string()));
        assert!(entries_reaching(&set(&["s.spec.ts"]), &set(&["z.ts"]), &edges).is_empty());
    }
}
