//! Path globs of the inputs file (`docs/repo-graph.toml`): `/`-separated, `**` any number of whole segments, `*` any run within
//! one segment, `?` one character within a segment. A pattern without a `/` matches the file name in any folder (as
//! `.gitattributes` does); one with a `/` (a leading one included: `/package.json`) is anchored at the repository root.

/// Whether `pattern` matches the repository path `path`.
pub fn matches(pattern: &str, path: &str) -> bool {
    let anchored = pattern.starts_with('/');
    let pattern = pattern.strip_prefix('/').unwrap_or(pattern);
    if !anchored && !pattern.contains('/') {
        let name = path.rsplit('/').next().unwrap_or(path);
        return segment(pattern.as_bytes(), name.as_bytes());
    }
    segments(Some(pattern), Some(path))
}

/// The first `/`-separated segment of `s` and the rest (`None`: it was the last segment).
fn split1(s: &str) -> (&str, Option<&str>) {
    match s.split_once('/') {
        Some((first, rest)) => (first, Some(rest)),
        None => (s, None),
    }
}

/// Whether the segments of `p` match the segments of `t` (`None`: no segments left). Walks both texts in place: no list of
/// segments is built per call, which made matching one pattern against every tracked file allocate twice per file (ER-20).
fn segments(p: Option<&str>, t: Option<&str>) -> bool {
    let Some(p) = p else {
        return t.is_none();
    };
    let (first, rest) = split1(p);
    if first == "**" {
        let mut at = t;
        loop {
            if segments(rest, at) {
                return true;
            }
            match at {
                Some(s) => at = split1(s).1,
                None => return false,
            }
        }
    }
    t.is_some_and(|t| {
        let (name, more) = split1(t);
        segment(first.as_bytes(), name.as_bytes()) && segments(rest, more)
    })
}

fn segment(p: &[u8], t: &[u8]) -> bool {
    match p.split_first() {
        None => t.is_empty(),
        Some((b'*', rest)) => (0..=t.len()).any(|i| segment(rest, &t[i..])),
        Some((b'?', rest)) => !t.is_empty() && segment(rest, &t[1..]),
        Some((c, rest)) => t.first() == Some(c) && segment(rest, &t[1..]),
    }
}

/// The paths of a sorted set that `pattern` matches, in order: the same answer as filtering the whole set with [`matches`], but
/// cheaper (ER-20): an anchored pattern visits only the paths that start with its literal prefix (a range of the set), and a path
/// that does not hold the pattern's longest literal run (`CHROMIUM_TEST_PATH=` of `*CHROMIUM_TEST_PATH=*`) is passed over
/// before it is matched. A step's `"$dir"/x.sh` became `*/x.sh`, matched against every tracked file; 137 such words in the CI
/// adapter cost about 0.33 s of its 0.53 s in a debug build (2026-11-24).
pub fn matching<'a>(pattern: &str, set: &'a std::collections::BTreeSet<String>) -> Vec<&'a String> {
    let run = longest_literal_run(pattern);
    let hit = |p: &&String| p.contains(run) && matches(pattern, p);
    match anchored_prefix(pattern) {
        Some(prefix) => set
            .range::<str, _>((
                std::ops::Bound::Included(prefix),
                std::ops::Bound::Unbounded,
            ))
            .take_while(|p| p.starts_with(prefix))
            .filter(hit)
            .collect(),
        None => set.iter().filter(hit).collect(),
    }
}

/// The longest run of literal characters inside one segment of `pattern` (no `/`, `*` or `?`; `**` segments have none): every path
/// the pattern matches holds it, because each other segment matches one whole segment of the path and its literal runs appear in
/// that segment as written.
fn longest_literal_run(pattern: &str) -> &str {
    pattern
        .split('/')
        .filter(|seg| *seg != "**")
        .flat_map(|seg| seg.split(['*', '?']))
        .max_by_key(|run| run.len())
        .unwrap_or("")
}

/// The literal text every path an anchored pattern matches starts with (up to its first wildcard); `None` for a pattern that
/// matches a file name in any folder.
fn anchored_prefix(pattern: &str) -> Option<&str> {
    let anchored = pattern.starts_with('/');
    let body = pattern.strip_prefix('/').unwrap_or(pattern);
    if !anchored && !body.contains('/') {
        return None;
    }
    Some(&body[..body.find(['*', '?']).unwrap_or(body.len())])
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

    /// The implementation before segments were walked in place (lists of segments per call), kept as the oracle.
    fn matches_by_lists(pattern: &str, path: &str) -> bool {
        fn lists(p: &[&str], t: &[&str]) -> bool {
            match p.split_first() {
                None => t.is_empty(),
                Some((&"**", rest)) => (0..=t.len()).any(|i| lists(rest, &t[i..])),
                Some((first, rest)) => t.split_first().is_some_and(|(name, more)| {
                    segment(first.as_bytes(), name.as_bytes()) && lists(rest, more)
                }),
            }
        }
        let anchored = pattern.starts_with('/');
        let pattern = pattern.strip_prefix('/').unwrap_or(pattern);
        if !anchored && !pattern.contains('/') {
            let name = path.rsplit('/').next().unwrap_or(path);
            return segment(pattern.as_bytes(), name.as_bytes());
        }
        let p: Vec<&str> = pattern.split('/').collect();
        let t: Vec<&str> = path.split('/').collect();
        lists(&p, &t)
    }

    /// Walking the segments in place answers what the lists answered, over edge shapes (empty segments, trailing and doubled
    /// slashes, `**` first, last and twice).
    #[test]
    fn walking_segments_in_place_answers_what_the_lists_answered() {
        let patterns = [
            "",
            "/",
            "**",
            "/**",
            "a/**",
            "**/b",
            "a/**/b",
            "**/**/b",
            "a/**/**",
            "a/*/b",
            "a/*",
            "*/b",
            "a?/b",
            "a//b",
            "a/",
            "/a",
            "a/b/",
            "**/",
            "*",
            "?",
            "a*b/**/c?d",
        ];
        let paths = [
            "",
            "a",
            "a/b",
            "a/b/c",
            "a//b",
            "a/",
            "/a",
            "ab/b",
            "a/x/b",
            "a/x/y/b",
            "b",
            "x/b",
            "a/b/",
            "axb/q/r/cxd",
            "a1/b",
        ];
        let mut compared = 0;
        for p in patterns {
            for t in paths {
                assert_eq!(matches(p, t), matches_by_lists(p, t), "{p:?} against {t:?}");
                compared += 1;
            }
        }
        assert_eq!(compared, patterns.len() * paths.len());
        // The control: the oracle is not a copy that answers true for everything.
        assert!(!matches_by_lists("a/*/b", "a/b") && matches_by_lists("a/**/b", "a/b"));
    }

    /// The range read answers exactly what the filter over every path answers, for anchored, unanchored, leading-slash and
    /// wildcard-first patterns; the control: a set where the prefix range is empty still finds nothing.
    #[test]
    fn matching_a_sorted_set_answers_what_the_filter_over_every_path_answers() {
        let set: std::collections::BTreeSet<String> = [
            "Cargo.toml",
            "apps/dyno/paddock.ci.json",
            "apps/dyno/x/paddock.ci.json",
            "conformance/ci/a.sh",
            "conformance/ci/b.mjs",
            "conformance/cix.sh",
            "crates/Cargo.toml",
            "crates/a/b/Cargo.toml",
            "docs/adr/0001.md",
            "package.json",
            "ui/package.json",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for pattern in [
            "conformance/ci/*.sh",
            "conformance/ci*",
            "crates/**/Cargo.toml",
            "apps/*/paddock.ci.json",
            "/package.json",
            "package.json",
            "*.md",
            "**/Cargo.toml",
            "*/dyno/*",
            "a?ps/dyno/paddock.ci.json",
            "nothing/here/*",
            "**/b.mjs",
            "**/Cargo.toml",
            "*/Cargo.toml",
            "*ackage*",
            "*/*/Cargo.toml",
            "c?ates/**",
            "**",
            "",
        ] {
            let all: Vec<&String> = set.iter().filter(|p| matches(pattern, p)).collect();
            assert_eq!(matching(pattern, &set), all, "{pattern}");
        }
        assert_eq!(
            matching("conformance/ci/*.sh", &set),
            ["conformance/ci/a.sh"]
        );
        assert!(matching("zz/*", &set).is_empty());
    }

    #[test]
    fn a_leading_slash_anchors_a_name_at_the_root() {
        // From --compare: `package.json` in the npm-root input matched ui/package.json and made it a global.
        assert!(matches("/package.json", "package.json"));
        assert!(!matches("/package.json", "ui/package.json"));
        assert!(
            matches("package.json", "ui/package.json"),
            "without the slash: any folder, as .gitattributes"
        );
    }

    #[test]
    fn a_longer_literal_prefix_is_more_specific() {
        assert!(specificity("conformance/reserved/**") > specificity("conformance/**"));
        assert_eq!(specificity("package.json"), "package.json".len());
    }
}
