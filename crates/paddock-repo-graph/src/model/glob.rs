//! Path globs of the inputs file (`docs/repo-graph.toml`): `/`-separated, `**` any number of whole segments, `*` any run within
//! one segment, `?` one character within a segment. A pattern without a `/` matches the file name in any folder (as
//! `.gitattributes` does); one with a `/` is anchored at the repository root.

/// Whether `pattern` matches the repository path `path`.
pub fn matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.strip_prefix('/').unwrap_or(pattern);
    if !pattern.contains('/') {
        let name = path.rsplit('/').next().unwrap_or(path);
        return segment(pattern.as_bytes(), name.as_bytes());
    }
    let p: Vec<&str> = pattern.split('/').collect();
    let t: Vec<&str> = path.split('/').collect();
    segments(&p, &t)
}

fn segments(p: &[&str], t: &[&str]) -> bool {
    match p.split_first() {
        None => t.is_empty(),
        Some((&"**", rest)) => (0..=t.len()).any(|i| segments(rest, &t[i..])),
        Some((first, rest)) => t.split_first().is_some_and(|(name, more)| {
            segment(first.as_bytes(), name.as_bytes()) && segments(rest, more)
        }),
    }
}

fn segment(p: &[u8], t: &[u8]) -> bool {
    match p.split_first() {
        None => t.is_empty(),
        Some((b'*', rest)) => (0..=t.len()).any(|i| segment(rest, &t[i..])),
        Some((b'?', rest)) => !t.is_empty() && segment(rest, &t[1..]),
        Some((c, rest)) => t.first() == Some(c) && segment(rest, &t[1..]),
    }
}

/// How specific a pattern is: its literal prefix length in characters (the longer, the more specific), for choosing the owner of
/// a path two patterns match.
pub fn specificity(pattern: &str) -> usize {
    pattern.find(['*', '?']).unwrap_or(pattern.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_star_spans_folders_and_single_star_stays_in_one() {
        assert!(matches("spikes/**", "spikes/dialog-hidden/Cargo.lock"));
        assert!(matches("crates/**/Cargo.toml", "crates/Cargo.toml"));
        assert!(matches("crates/**/Cargo.toml", "crates/a/b/Cargo.toml"));
        assert!(matches(".gitea/**/*.md", ".gitea/actions/README.md"));
        assert!(!matches(
            "apps/*/paddock.ci.json",
            "apps/dyno/x/paddock.ci.json"
        ));
        assert!(matches(
            "apps/*/paddock.ci.json",
            "apps/dyno/paddock.ci.json"
        ));
        assert!(matches("*.md", "docs/adr/0001.md"), "no slash: any folder");
        assert!(matches("/rust-toolchain.toml", "rust-toolchain.toml"));
        assert!(!matches("rust-toolchain.toml/x", "rust-toolchain.toml"));
        assert!(matches("a?c", "abc") && !matches("a?c", "a/c"));
    }

    #[test]
    fn a_longer_literal_prefix_is_more_specific() {
        assert!(specificity("conformance/reserved/**") > specificity("conformance/**"));
        assert_eq!(specificity("package.json"), "package.json".len());
    }
}
