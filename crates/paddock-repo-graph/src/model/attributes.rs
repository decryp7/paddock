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

/// The `[attr]` macros of a text: name to the attributes it sets, as written.
fn macros(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("[attr]") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        if let Some(name) = parts.next() {
            out.insert(name.to_string(), parts.map(str::to_string).collect());
        }
    }
    out
}

/// One attribute token with its macros expanded (git expands a set macro into its attributes, recursively; the depth is bounded
/// so a macro that names itself ends).
fn expand(
    token: &str,
    macros: &BTreeMap<String, Vec<String>>,
    depth: usize,
    out: &mut BTreeSet<String>,
) {
    match macros.get(token) {
        Some(attrs) if depth < 8 => attrs.iter().for_each(|a| expand(a, macros, depth + 1, out)),
        _ => {
            if CONTENT_ATTRIBUTES.contains(&name_of(token)) {
                out.insert(token.to_string());
            }
        }
    }
}

/// The pattern lines of a text in file order, each with its content attributes after macro expansion (sorted within the line);
/// a line that sets no content attribute is dropped. Order matters to git (a later line overrides an earlier one), so it is kept.
fn content_lines(text: &str) -> Vec<(String, BTreeSet<String>)> {
    let macros = macros(text);
    let mut out = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("[attr]"))
    {
        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else {
            continue;
        };
        let mut attrs = BTreeSet::new();
        parts.for_each(|a| expand(a, &macros, 0, &mut attrs));
        if !attrs.is_empty() {
            out.push((pattern.to_string(), attrs));
        }
    }
    out
}

/// The patterns whose content attributes differ between `before` and `after` (`None`: the file does not exist), sorted. A pattern
/// differs when its lines differ (after macro expansion), and when the lines both texts share changed their order, every pattern
/// among them does (a later line overrides an earlier one).
pub fn changed_patterns(before: Option<&str>, after: Option<&str>) -> Vec<String> {
    let (b, a) = (
        content_lines(before.unwrap_or("")),
        content_lines(after.unwrap_or("")),
    );
    let per_pattern = |lines: &[(String, BTreeSet<String>)]| {
        let mut m: BTreeMap<String, Vec<BTreeSet<String>>> = BTreeMap::new();
        for (p, attrs) in lines {
            m.entry(p.clone()).or_default().push(attrs.clone());
        }
        m
    };
    let (bm, am) = (per_pattern(&b), per_pattern(&a));
    let mut out: BTreeSet<String> = bm
        .keys()
        .chain(am.keys())
        .filter(|k| bm.get(*k) != am.get(*k))
        .cloned()
        .collect();
    let common = |from: &[(String, BTreeSet<String>)], other: &[(String, BTreeSet<String>)]| {
        from.iter()
            .filter(|l| other.contains(l))
            .cloned()
            .collect::<Vec<_>>()
    };
    let (cb, ca) = (common(&b, &a), common(&a, &b));
    if cb != ca {
        out.extend(cb.into_iter().map(|(p, _)| p));
    }
    out.into_iter().collect()
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

    #[test]
    fn a_macro_definition_change_reaches_the_patterns_that_use_it() {
        // Failing first (review P2): `[attr]` macros were not expanded, so changing what `lf` sets reached nothing.
        let before = "[attr]lf text eol=lf\n*.sh lf\n*.md merge=union\n";
        let after = "[attr]lf text eol=crlf\n*.sh lf\n*.md merge=union\n";
        assert_eq!(changed_patterns(Some(before), Some(after)), ["*.sh"]);
        // Control: a macro that sets no content attribute changes nothing.
        let (m1, m2) = ("[attr]u merge=union\n*.md u\n", "[attr]u merge=x\n*.md u\n");
        assert!(changed_patterns(Some(m1), Some(m2)).is_empty());
    }

    #[test]
    fn a_reordered_line_changes_the_patterns_it_overrides() {
        // Failing first: a later line overrides an earlier one, so swapping them changes the effective attributes.
        let before = "* text=auto\n*.png binary\n";
        let after = "*.png binary\n* text=auto\n";
        assert_eq!(changed_patterns(Some(before), Some(after)), ["*", "*.png"]);
        // Control: the same lines in the same order change nothing.
        assert!(changed_patterns(Some(before), Some(before)).is_empty());
    }
}
