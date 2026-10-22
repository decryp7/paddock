//! Versions and version schemes.
//!
//! Every scheme reads a version out of some text (a file name, a tag, a feed title) and produces the same
//! [`Version`]: up to six numeric parts plus an optional pre-release label. Versions order numerically part by
//! part (missing parts count as zero), and a pre-release orders before its release, with semver's rules between
//! pre-release labels (`alpha < alpha.1 < beta < rc.1 < rc.2`).

use crate::user_regex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::OnceLock;

/// Most numeric parts of a version.
pub const MAX_PARTS: usize = 6;

/// A version read by a [`VersionScheme`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    /// How it is shown (`1.4.0-rc.1`, `R1.01.00T10`, `2026.10.22.3`).
    pub text: String,
    /// The numeric parts, most significant first (1 to [`MAX_PARTS`]).
    pub parts: Vec<u64>,
    /// The pre-release label (`rc.1`); empty for a release.
    #[serde(default)]
    pub pre: String,
}

impl Version {
    /// A version from parts; `text` is how it is shown.
    pub fn new(text: impl Into<String>, parts: Vec<u64>, pre: impl Into<String>) -> Version {
        Version {
            text: text.into(),
            parts,
            pre: pre.into(),
        }
    }

    /// Whether it is a pre-release.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    fn significant_parts(&self) -> &[u64] {
        let end = self.parts.iter().rposition(|&p| p != 0).map_or(0, |i| i + 1);
        &self.parts[..end]
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.parts.len().max(other.parts.len());
        for i in 0..n {
            let (a, b) = (self.parts.get(i).copied().unwrap_or(0), other.parts.get(i).copied().unwrap_or(0));
            match a.cmp(&b) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        compare_pre(&self.pre, &other.pre)
    }
}

impl std::hash::Hash for Version {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.significant_parts().hash(state);
        self.pre.to_ascii_lowercase().hash(state);
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// Semver pre-release precedence: no label is greater than any label; identifiers compare numerically when both are
/// numbers, numbers before words, words by ASCII order (case-insensitively); a longer list wins when all shared
/// identifiers are equal.
fn compare_pre(a: &str, b: &str) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    let mut xs = a.split(['.', '-']);
    let mut ys = b.split(['.', '-']);
    loop {
        match (xs.next(), ys.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(p), Ok(q)) => p.cmp(&q),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

/// How a watch reads versions.
#[derive(Debug, Clone)]
pub enum VersionScheme {
    /// Semantic versions (`1.4`, `v1.4.2`, `1.4.2-rc.1`); a version that is the whole text may carry any
    /// pre-release label, one inside a longer name only a common one (alpha, beta, rc, …).
    Semver,
    /// Dotted numbers of any length (`1.2.3.4`, `2026.1`), with the same pre-release rules as semver.
    Numeric,
    /// The build numbers the old Release Monitor watched: `R<major>.<minor>.<patch>T<build>` (`R1.01.00T10`).
    BuildNumber,
    /// Dates (`2026.10.22`, `2026-10-22`, `20261022`), optionally followed by a build number (`2026.10.22.3`).
    Date,
    /// A regular expression with named groups `major`, `minor`, `patch`, `build` (numbers, at least one) and
    /// optionally `pre` (a pre-release label).
    Custom(CustomScheme),
}

impl PartialEq for VersionScheme {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id() && self.custom_pattern() == other.custom_pattern()
    }
}

/// A compiled custom scheme.
#[derive(Debug, Clone)]
pub struct CustomScheme {
    pattern: String,
    regex: Regex,
}

const GROUPS: [&str; 4] = ["major", "minor", "patch", "build"];

impl VersionScheme {
    /// From a stored id (`semver`, `numeric`, `build-number`, `date`, `custom`) and, for `custom`, the pattern.
    pub fn parse(id: &str, pattern: Option<&str>) -> Result<VersionScheme, String> {
        match id {
            "semver" => Ok(VersionScheme::Semver),
            "numeric" => Ok(VersionScheme::Numeric),
            "build-number" => Ok(VersionScheme::BuildNumber),
            "date" => Ok(VersionScheme::Date),
            "custom" => VersionScheme::custom(pattern.unwrap_or("")),
            other => Err(format!(
                "\"{other}\" is not a version scheme (semver, numeric, build-number, date or custom)."
            )),
        }
    }

    /// A custom scheme from a regular expression with named groups.
    pub fn custom(pattern: &str) -> Result<VersionScheme, String> {
        if pattern.trim().is_empty() {
            return Err("A custom version scheme needs a pattern.".into());
        }
        let regex = user_regex::compile(pattern, true)?;
        let names: Vec<&str> = regex.capture_names().flatten().collect();
        if !GROUPS.iter().any(|g| names.contains(g)) {
            return Err("The pattern needs at least one of the named groups (?<major>…), (?<minor>…), (?<patch>…) or (?<build>…).".into());
        }
        Ok(VersionScheme::Custom(CustomScheme {
            pattern: pattern.to_string(),
            regex,
        }))
    }

    /// The stored id.
    pub fn id(&self) -> &'static str {
        match self {
            VersionScheme::Semver => "semver",
            VersionScheme::Numeric => "numeric",
            VersionScheme::BuildNumber => "build-number",
            VersionScheme::Date => "date",
            VersionScheme::Custom(_) => "custom",
        }
    }

    /// The pattern of a custom scheme.
    pub fn custom_pattern(&self) -> Option<&str> {
        match self {
            VersionScheme::Custom(c) => Some(&c.pattern),
            _ => None,
        }
    }

    /// Reads the version in `text`; `None` when there is none.
    pub fn extract(&self, text: &str) -> Option<Version> {
        let text = text.trim();
        if text.is_empty() || text.len() > 1024 {
            return None;
        }
        match self {
            VersionScheme::Semver => dotted(text, 2, 3),
            VersionScheme::Numeric => dotted(text, 1, MAX_PARTS),
            VersionScheme::BuildNumber => build_number(text),
            VersionScheme::Date => date(text),
            VersionScheme::Custom(c) => custom(&c.regex, text),
        }
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    // The patterns are constants tested below; a failure here is a programming error caught by the tests.
    #[allow(clippy::expect_used)]
    cell.get_or_init(|| Regex::new(pattern).expect("built-in pattern"))
}

/// Pre-release words accepted inside longer names (`app-1.2.0-rc.1-x64.zip`).
const PRE_WORDS: &str = r"(?:alpha|beta|rc|pre|preview|dev|nightly|canary|snapshot|insiders|a|b)(?:[.-]?[0-9]+)*";

fn dotted(text: &str, min: usize, max: usize) -> Option<Version> {
    static WHOLE: OnceLock<Regex> = OnceLock::new();
    static INSIDE: OnceLock<Regex> = OnceLock::new();
    let whole = re(
        &WHOLE,
        r"(?i)^v?([0-9]+(?:\.[0-9]+)*)(?:-([0-9a-z][0-9a-z.-]*))?(?:\+[0-9a-z.-]+)?$",
    );
    let inside_pattern = format!(r"(?i)(?:^|[^0-9a-z.])v?([0-9]+(?:\.[0-9]+)*)(?:[-.]?({PRE_WORDS}))?(?:[^0-9a-z]|$)");
    let inside = re(&INSIDE, &inside_pattern);
    let (nums, pre) = if let Some(c) = whole.captures(text) {
        (c.get(1)?.as_str(), c.get(2).map_or("", |m| m.as_str()))
    } else {
        let mut found = None;
        for c in inside.captures_iter(text) {
            let nums = c.get(1)?.as_str();
            if (min..=max).contains(&nums.split('.').count()) {
                found = Some((nums, c.get(2).map_or("", |m| m.as_str())));
                break;
            }
        }
        found?
    };
    let parts: Vec<u64> = nums.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    if !(min..=max).contains(&parts.len()) {
        return None;
    }
    let pre = pre.trim_start_matches(['-', '.']).to_string();
    let text = if pre.is_empty() {
        nums.to_string()
    } else {
        format!("{nums}-{pre}")
    };
    Some(Version::new(text, parts, pre))
}

fn build_number(text: &str) -> Option<Version> {
    static RE: OnceLock<Regex> = OnceLock::new();
    // The old app's pattern was `.*(R\d).(\d{2}).(\d{2})(T\d{2}).*` (greedy, so the last match wins); this keeps that
    // and also takes longer numbers.
    let r = re(&RE, r"(?i)R([0-9]{1,4})[^0-9]([0-9]{2,3})[^0-9]([0-9]{2,3})T([0-9]{2,4})");
    let c = r.captures_iter(text).last()?;
    let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u64>().ok());
    let (major, minor, patch, build) = (n(1)?, n(2)?, n(3)?, n(4)?);
    Some(Version::new(
        format!("R{major}.{minor:02}.{patch:02}T{build:02}"),
        vec![major, minor, patch, build],
        "",
    ))
}

fn date(text: &str) -> Option<Version> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let r = re(
        &RE,
        r"(?:^|[^0-9])((?:19|20|21)[0-9]{2})([-._]?)([01][0-9])([-._]?)([0-3][0-9])(?:[-._]([0-9]{1,6}))?(?:[^0-9]|$)",
    );
    for c in r.captures_iter(text) {
        let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u64>().ok());
        let (y, m, d) = (n(1)?, n(3)?, n(5)?);
        // Both separators or neither: `2026-1022` is not a date.
        if c.get(2).map(|s| s.as_str().len()) != c.get(4).map(|s| s.as_str().len()) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            continue;
        }
        let mut parts = vec![y, m, d];
        let mut text = format!("{y}.{m:02}.{d:02}");
        if let Some(b) = n(6) {
            parts.push(b);
            text.push_str(&format!(".{b}"));
        }
        return Some(Version::new(text, parts, ""));
    }
    None
}

fn custom(regex: &Regex, text: &str) -> Option<Version> {
    let c = regex.captures(text)?;
    let mut parts = Vec::new();
    for g in GROUPS {
        if let Some(m) = c.name(g) {
            parts.push(m.as_str().parse::<u64>().ok()?);
        } else if regex.capture_names().flatten().any(|n| n == g) {
            // A declared group that did not take part counts as zero, so versions stay comparable.
            parts.push(0);
        }
    }
    let pre = c.name("pre").map_or("", |m| m.as_str()).to_string();
    Some(Version::new(c.get(0).map_or("", |m| m.as_str()).to_string(), parts, pre))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(scheme: &VersionScheme, text: &str) -> Version {
        scheme.extract(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    #[test]
    fn semver_reads_tags_names_and_prereleases() {
        let s = VersionScheme::Semver;
        assert_eq!(v(&s, "v1.4.2").text, "1.4.2");
        assert_eq!(v(&s, "1.4").parts, vec![1, 4]);
        assert_eq!(v(&s, "release-2.0.0-rc.1+build.5").text, "2.0.0-rc.1");
        assert_eq!(v(&s, "tool@3.1.0").text, "3.1.0");
        assert_eq!(v(&s, "app-1.2.0-x64.zip").text, "1.2.0");
        assert_eq!(v(&s, "app-1.2.0-rc.2-x64.zip").text, "1.2.0-rc.2");
        assert_eq!(v(&s, "app_v1.2.0.beta3.tar.gz").pre, "beta3");
        assert_eq!(v(&s, "Release 1.10 (stable)").text, "1.10");
        assert!(v(&s, "1.0.0-alpha").is_prerelease());
        assert_eq!(s.extract("no version here"), None);
        assert_eq!(s.extract("build 7"), None, "one number is not a semver");
        assert_eq!(s.extract("1.2.3.4"), None, "four parts are numeric, not semver");
        assert_eq!(s.extract(""), None);
        assert_eq!(s.extract(&"1".repeat(2000)), None);
        assert_eq!(s.extract("v99999999999999999999.1"), None, "numbers that overflow are not versions");
    }

    #[test]
    fn numeric_reads_any_dotted_numbers() {
        let s = VersionScheme::Numeric;
        assert_eq!(v(&s, "1.2.3.4").parts, vec![1, 2, 3, 4]);
        assert_eq!(v(&s, "build-17").parts, vec![17]);
        assert_eq!(v(&s, "setup 10.0.19045.3803.exe").text, "10.0.19045.3803");
        assert_eq!(s.extract("1.2.3.4.5.6.7"), None, "at most six parts");
    }

    #[test]
    fn build_numbers_read_like_the_old_app() {
        let s = VersionScheme::BuildNumber;
        let x = v(&s, "Product_R1.01.00T10.txt");
        assert_eq!((x.text.as_str(), x.parts.clone()), ("R1.01.00T10", vec![1, 1, 0, 10]));
        assert_eq!(v(&s, "r2_03_04t05.zip").text, "R2.03.04T05");
        assert_eq!(
            v(&s, "R1.01.00T10 then R1.02.00T01").text,
            "R1.02.00T01",
            "the last one wins, as before"
        );
        assert_eq!(v(&s, "R12.100.07T1234").text, "R12.100.07T1234");
        assert_eq!(s.extract("R1.1.0T10"), None);
        assert!(v(&s, "R1.02.00T01") > v(&s, "R1.01.09T99"));
    }

    #[test]
    fn dates_read_with_optional_build_numbers() {
        let s = VersionScheme::Date;
        assert_eq!(v(&s, "nightly-2026-10-22.zip").text, "2026.10.22");
        assert_eq!(v(&s, "20261022").parts, vec![2026, 10, 22]);
        assert_eq!(v(&s, "build_2026.10.22.3").text, "2026.10.22.3");
        assert_eq!(v(&s, "2026_10_22-12").parts, vec![2026, 10, 22, 12]);
        assert_eq!(s.extract("2026-1022"), None);
        assert_eq!(s.extract("2026-13-01"), None);
        assert_eq!(s.extract("2026-12-00"), None);
        assert_eq!(v(&s, "2026-13-01 or 2026-01-02").text, "2026.01.02", "the first valid date wins");
        assert!(v(&s, "2026.10.22.3") > v(&s, "2026.10.22"));
    }

    #[test]
    fn custom_schemes_use_named_groups() {
        let s = VersionScheme::custom(r"build-(?<major>\d+)_(?<build>\d+)(?:-(?<pre>\w+))?").unwrap();
        let x = v(&s, "my-build-7_123-beta.zip");
        assert_eq!(
            (x.parts.clone(), x.pre.as_str(), x.text.as_str()),
            (vec![7, 123], "beta", "build-7_123-beta")
        );
        let opt = VersionScheme::custom(r"r(?<major>\d+)(?:\.(?<minor>\d+))?").unwrap();
        assert_eq!(v(&opt, "r5").parts, vec![5, 0]);
        let big = VersionScheme::custom(r"(?<major>\d+)").unwrap();
        assert_eq!(big.extract("99999999999999999999999"), None);
        assert_eq!(s.extract("nothing"), None);
        assert!(VersionScheme::custom("").is_err());
        assert!(VersionScheme::custom(r"(\d+)").unwrap_err().contains("named groups"));
        assert!(VersionScheme::custom("(").is_err());
        assert_eq!(s.custom_pattern(), Some(r"build-(?<major>\d+)_(?<build>\d+)(?:-(?<pre>\w+))?"));
    }

    #[test]
    fn schemes_parse_from_their_ids() {
        for id in ["semver", "numeric", "build-number", "date"] {
            let s = VersionScheme::parse(id, None).unwrap();
            assert_eq!(s.id(), id);
            assert_eq!(s.custom_pattern(), None);
            assert_eq!(s, VersionScheme::parse(id, Some("ignored")).unwrap());
        }
        let c = VersionScheme::parse("custom", Some(r"(?<major>\d+)")).unwrap();
        assert_eq!(c.id(), "custom");
        assert_ne!(c, VersionScheme::custom(r"(?<minor>\d+)").unwrap());
        assert!(VersionScheme::parse("custom", None).is_err());
        assert!(VersionScheme::parse("roman", None).unwrap_err().contains("not a version scheme"));
    }

    #[test]
    fn ordering_follows_semver_precedence() {
        let s = VersionScheme::Semver;
        let order = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.2",
            "1.10.0",
            "2.0.0",
        ];
        for w in order.windows(2) {
            assert!(v(&s, w[0]) < v(&s, w[1]), "{} < {}", w[0], w[1]);
        }
        assert_eq!(v(&s, "1.2"), v(&s, "1.2.0"));
        assert_eq!(v(&s, "1.0.0-RC.1"), v(&s, "1.0.0-rc.1"));
        assert!(Version::new("1-1", vec![1], "1") < Version::new("1-a", vec![1], "a"));
        assert!(Version::new("1-a.1", vec![1], "a.1") > Version::new("1-a", vec![1], "a"));
        assert_eq!(v(&s, "2.0").partial_cmp(&v(&s, "1.0")), Some(Ordering::Greater));
        assert_eq!(v(&s, "2.0").to_string(), "2.0");
        use std::collections::HashSet;
        let set: HashSet<Version> = [v(&s, "1.2"), v(&s, "1.2.0"), v(&s, "1.2.1")].into_iter().collect();
        assert_eq!(set.len(), 2);
        assert_eq!(Version::new("0", vec![0], "").significant_parts(), &[] as &[u64]);
    }
}
