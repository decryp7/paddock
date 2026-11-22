//! A `.gitattributes` change read attribute by attribute (ADR 0117 G2): only the attributes that change what a build or test reads
//! (`text`, `eol`, `binary`, `filter`, `diff`, `working-tree-encoding`, `crlf`) reach the paths their patterns match; `merge=`,
//! `linguist-*`, `export-ignore` and comments reach nothing but the checks that read the file.

use std::collections::{BTreeMap, BTreeSet};

/// The attribute names whose change alters checked-out or compared file content.
pub const CONTENT_ATTRIBUTES: [&str; 7] = [
    "text",
    "eol",
    "binary",
    "filter",
    "diff",
    "working-tree-encoding",
    "crlf",
];

fn name_of(attr: &str) -> &str {
    let a = attr.trim_start_matches(['-', '!']);
    a.split('=').next().unwrap_or(a)
}

/// Pattern to its content attributes (as written, sorted), from a `.gitattributes` text.
fn content_lines(text: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else {
            continue;
        };
        let attrs: BTreeSet<String> = parts
            .filter(|a| CONTENT_ATTRIBUTES.contains(&name_of(a)))
            .map(str::to_string)
            .collect();
        out.entry(pattern.to_string()).or_default().extend(attrs);
    }
    out.retain(|_, a| !a.is_empty());
    out
}

/// The patterns whose content attributes differ between `before` and `after` (`None`: the file does not exist), sorted.
pub fn changed_patterns(before: Option<&str>, after: Option<&str>) -> Vec<String> {
    let (b, a) = (
        content_lines(before.unwrap_or("")),
        content_lines(after.unwrap_or("")),
    );
    let keys: BTreeSet<&String> = b.keys().chain(a.keys()).collect();
    keys.into_iter()
        .filter(|k| b.get(*k) != a.get(*k))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_driver_or_a_comment_changes_no_content_pattern() {
        // Train 19's change (the audit's G2 example): `merge=union` to `merge=lessons` plus a comment.
        let before = "* text=auto eol=lf\n/docs/lessons.md merge=union\n";
        let after = "* text=auto eol=lf\n# the lessons driver\n/docs/lessons.md merge=lessons\n";
        assert!(changed_patterns(Some(before), Some(after)).is_empty());
    }

    #[test]
    fn a_content_attribute_change_names_its_pattern() {
        let before = "* text=auto eol=lf\n*.png binary\n";
        let after = "* text=auto eol=lf\n*.png binary\n*.ps1 eol=crlf\n";
        assert_eq!(changed_patterns(Some(before), Some(after)), ["*.ps1"]);
        assert_eq!(
            changed_patterns(Some(before), None),
            ["*", "*.png"],
            "a deleted file changes every pattern it had"
        );
        assert_eq!(
            changed_patterns(Some("*.sh -text\n"), Some("*.sh text\n")),
            ["*.sh"]
        );
    }
}
