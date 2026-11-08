//! The files C100 reads, behind a trait so the pure core is tested with in-memory trees.

use regex::Regex;
use std::collections::BTreeMap;

/// A set of repository files: tracked paths and their contents.
pub trait Tree {
    /// Every path (slash separated, from the repository root), sorted.
    fn files(&self) -> &[String];
    /// The text of a file; `None` when it is missing or not UTF-8.
    fn read(&self, path: &str) -> Option<String>;
}

/// An in-memory tree (tests).
#[derive(Debug, Default, Clone)]
pub struct MemTree {
    names: Vec<String>,
    data: BTreeMap<String, String>,
}

impl MemTree {
    /// Builds a tree from `(path, text)` pairs.
    pub fn new(files: &[(&str, &str)]) -> MemTree {
        let data: BTreeMap<String, String> = files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
        MemTree {
            names: data.keys().cloned().collect(),
            data,
        }
    }
}

impl Tree for MemTree {
    fn files(&self) -> &[String] {
        &self.names
    }
    fn read(&self, path: &str) -> Option<String> {
        self.data.get(path).cloned()
    }
}

/// A glob over slash paths: `*` and `?` stay in a segment, `**` crosses segments (`**/x` also matches `x`).
pub fn glob_regex(glob: &str) -> Regex {
    let mut re = String::from("^");
    let c: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '*' if c.get(i + 1) == Some(&'*') => {
                if c.get(i + 2) == Some(&'/') {
                    re.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    re.push_str(".*");
                    i += 2;
                }
            }
            '*' => {
                re.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                re.push_str("[^/]");
                i += 1;
            }
            ch => {
                re.push_str(&regex::escape(&ch.to_string()));
                i += 1;
            }
        }
    }
    re.push('$');
    // The pattern is built from escaped parts, so it always compiles.
    Regex::new(&re).unwrap_or_else(|e| panic!("a glob became an invalid regex ({re}): {e}"))
}

/// Whether `path` matches any glob of the list.
pub fn any_glob(globs: &[Regex], path: &str) -> bool {
    globs.iter().any(|g| g.is_match(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_regex("**/check-*.mjs").is_match("scripts/check-x.mjs"));
        assert!(glob_regex("**/check-*.mjs").is_match("check-x.mjs"));
        assert!(!glob_regex("scripts/*.mjs").is_match("scripts/a/b.mjs"));
        assert!(glob_regex("docs/adr/**").is_match("docs/adr/0001-x.md"));
        assert!(glob_regex("apps/*/AGENTS.md").is_match("apps/dyno/AGENTS.md"));
        assert!(!glob_regex("apps/*/AGENTS.md").is_match("apps/dyno/x/AGENTS.md"));
    }
}
