//! Content extractors (ADR 0117 §3, review P2-2): an input narrowed by reading its text before and after the change. Each is one
//! [`ContentExtractor`], registered once in [`content_extractor`]:
//!
//! - `attributes`: the `.gitattributes` patterns whose content attributes changed (G2, D4);
//! - `npm-lock`: the npm workspaces whose resolved packages differ ([`crate::lock_diff`]);
//! - `guards`: whether a test file's ignore attributes or environment guards changed (C100 reads them).
//!
//! A name the registry does not know fails closed (every project on full).

use super::attributes;

/// What a content extractor found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Narrowing {
    /// Path patterns whose matching files' owners are reached.
    Patterns(Vec<String>),
    /// npm workspace folders that are reached (their UI half).
    Packages(Vec<String>),
    /// Whether the input's own targets are reached at all.
    Changed(bool),
}

/// One content extractor.
pub trait ContentExtractor {
    /// Its name (the inputs file's `extractor = "..."`).
    fn name(&self) -> &'static str;
    /// What the change from `before` to `after` reaches (`None`: the file does not exist there). `Err` puts the input's targets
    /// and every project on full.
    fn narrow(&self, before: Option<&str>, after: Option<&str>) -> Result<Narrowing, String>;
}

/// The names the registry knows (the accessor refuses others).
pub const NAMES: [&str; 3] = ["attributes", "npm-lock", "guards"];

/// The extractor of this name, or `None` when the name is unknown (the caller fails closed).
pub fn content_extractor(name: &str) -> Option<Box<dyn ContentExtractor>> {
    match name {
        "attributes" => Some(Box::new(Attributes)),
        "npm-lock" => Some(Box::new(NpmLock)),
        "guards" => Some(Box::new(Guards)),
        _ => None,
    }
}

struct Attributes;

impl ContentExtractor for Attributes {
    fn name(&self) -> &'static str {
        "attributes"
    }

    fn narrow(&self, before: Option<&str>, after: Option<&str>) -> Result<Narrowing, String> {
        Ok(Narrowing::Patterns(attributes::changed_patterns(
            before, after,
        )))
    }
}

struct NpmLock;

impl ContentExtractor for NpmLock {
    fn name(&self) -> &'static str {
        "npm-lock"
    }

    fn narrow(&self, before: Option<&str>, after: Option<&str>) -> Result<Narrowing, String> {
        crate::lock_diff(before, after)
            .map(Narrowing::Packages)
            .map_err(|e| e.to_string())
    }
}

struct Guards;

impl ContentExtractor for Guards {
    fn name(&self) -> &'static str {
        "guards"
    }

    fn narrow(&self, before: Option<&str>, after: Option<&str>) -> Result<Narrowing, String> {
        Ok(Narrowing::Changed(guards_changed(before, after)))
    }
}

mod guard_rules {
    use regex::Regex;
    use std::sync::LazyLock;

    fn re(s: &str) -> Regex {
        // The patterns are this module's literals; the tests below compile each.
        Regex::new(s).unwrap_or_else(|e| panic!("bad built-in pattern {s}: {e}"))
    }

    pub static GUARD: LazyLock<Regex> =
        LazyLock::new(|| re(r"#\[ignore|cfg_attr\([^)\n]*ignore|env::var|process\.env"));
    pub static RUST_FN: LazyLock<Regex> = LazyLock::new(|| re(r"(?-u:\b)fn\s+((?-u:\w)+)"));
    pub static TS_TEST: LazyLock<Regex> =
        LazyLock::new(|| re(r#"(?-u:\b)(?:test|it)(?:\.(?-u:\w)+)?\(\s*['"`]([^'"`]+)"#));
}

/// The ignore attributes and environment guards of a test file, each paired with the Rust function or TypeScript test title it
/// belongs to: moving an ignore from one test to another changes the set though the count stays (the one implementation; the
/// planner's C100 rule calls it).
pub fn guard_set(text: &str) -> std::collections::BTreeSet<String> {
    use guard_rules::{GUARD, RUST_FN, TS_TEST};
    let lines: Vec<&str> = text.split('\n').collect();
    let mut set = std::collections::BTreeSet::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(m) = GUARD.find(line) else {
            continue;
        };
        let mut name = String::new();
        for l in lines.iter().take((i + 8).min(lines.len())).skip(i) {
            if let Some(c) = RUST_FN.captures(l) {
                name = c[1].to_string();
                break;
            }
        }
        if name.is_empty() {
            for k in (0..=i).rev().take_while(|k| *k + 40 > i) {
                if let Some(c) = TS_TEST.captures(lines[k]) {
                    name = c[1].to_string();
                    break;
                }
            }
        }
        set.insert(format!("{}|{name}", m.as_str()));
    }
    set
}

/// Whether a file's guards differ between two texts (a missing file has none).
pub fn guards_changed(before: Option<&str>, after: Option<&str>) -> bool {
    guard_set(before.unwrap_or_default()) != guard_set(after.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_name_is_known_and_an_unknown_one_is_none() {
        for n in NAMES {
            assert_eq!(content_extractor(n).map(|x| x.name()), Some(n));
        }
        assert!(content_extractor("magic").is_none());
    }

    #[test]
    fn attributes_narrow_to_the_changed_content_patterns() {
        let a = content_extractor("attributes").unwrap();
        assert_eq!(
            a.narrow(
                Some("*.md merge=union\n"),
                Some("*.md merge=x\n*.ps1 eol=crlf\n")
            ),
            Ok(Narrowing::Patterns(vec!["*.ps1".to_string()]))
        );
        // Control: no content attribute changed, nothing is narrowed in.
        assert_eq!(
            a.narrow(Some("*.md merge=union\n"), Some("# c\n*.md merge=x\n")),
            Ok(Narrowing::Patterns(vec![]))
        );
    }

    #[test]
    fn npm_lock_narrows_to_the_workspaces_whose_packages_changed() {
        let lock = |v: &str| {
            format!(
                r#"{{"packages":{{"":{{"workspaces":["ui"]}},"ui":{{"dependencies":{{"react":"1"}}}},"node_modules/react":{{"version":"{v}"}}}}}}"#
            )
        };
        let n = content_extractor("npm-lock").unwrap();
        let got = n.narrow(Some(&lock("1")), Some(&lock("2"))).unwrap();
        assert_eq!(got, Narrowing::Packages(vec!["ui".to_string()]));
        // Control: the same lock narrows to nothing.
        assert_eq!(
            n.narrow(Some(&lock("1")), Some(&lock("1"))),
            Ok(Narrowing::Packages(vec![]))
        );
    }

    #[test]
    fn guards_see_a_moved_ignore_and_not_a_comment() {
        let g = content_extractor("guards").unwrap();
        let a = "#[test]\n#[ignore]\nfn one() {}\n#[test]\nfn two() {}\n";
        let b = "#[test]\nfn one() {}\n#[test]\n#[ignore]\nfn two() {}\n";
        assert_eq!(g.narrow(Some(a), Some(b)), Ok(Narrowing::Changed(true)));
        assert_eq!(
            g.narrow(Some(a), Some(&format!("// note\n{a}"))),
            Ok(Narrowing::Changed(false))
        );
        assert!(guards_changed(
            None,
            Some("it('x', () => { process.env.A })")
        ));
    }
}
