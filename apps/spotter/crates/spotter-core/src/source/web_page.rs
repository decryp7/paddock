//! Web pages: a static (or rendered) page whose version is read with a CSS selector and/or a regular expression,
//! or whose text is watched for changes. HTML is parsed, never executed; noise (whitespace, timestamps, counters)
//! does not count as a change. Also: version hints in JSON-LD and meta tags, and feed auto-discovery.
use super::SourceError;
use crate::checksum::Sha256Digest;
use crate::limits::MAX_PATTERN_LEN;
use crate::release::Candidate;
use crate::schema::Config;
use crate::time::Timestamp;
use crate::user_regex;
use regex::Regex;
use scraper::{Html, Node, Selector};
use std::sync::OnceLock;

/// Longest version text taken from a page.
pub const MAX_VERSION_TEXT: usize = 200;
/// Text lines kept for a change check.
pub const MAX_LINES: usize = 5000;
/// Feeds reported by discovery.
pub const MAX_FEEDS: usize = 20;
/// Deepest JSON-LD nesting looked at.
const MAX_JSON_DEPTH: usize = 16;

/// What a web-page watch does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMode {
    /// Read a version from the page.
    Version,
    /// Report when the (noise-filtered) text changes.
    Change,
}

/// A web-page source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPageSource {
    /// The page.
    pub url: String,
    /// CSS selector of the element holding the version (or of the region to watch); the whole page when absent.
    pub selector: Option<String>,
    /// Read this attribute of the element instead of its text.
    pub attribute: Option<String>,
    /// Regular expression applied to the text; its first group (else the whole match) is the version.
    pub pattern: Option<String>,
    /// Version or change watching.
    pub mode: PageMode,
    /// The page needs JavaScript: render it in the browser first.
    pub rendered: bool,
    /// Element to wait for before reading a rendered page.
    pub wait_selector: Option<String>,
}

/// What was read from a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extraction {
    /// The text the version is read from, or the page's text in change mode, on one line.
    pub text: String,
    /// The text lines with noise removed (change mode compares these).
    pub lines: Vec<String>,
    /// SHA-256 of the noise-filtered lines.
    pub hash: Sha256Digest,
}

fn selector(css: &str) -> Result<Selector, String> {
    Selector::parse(css).map_err(|_| format!("\"{css}\" is not a valid CSS selector."))
}

impl WebPageSource {
    /// From form values (`url`, `selector`, `attribute`, `pattern`, `mode`, `rendered`, `wait`).
    pub fn from_config(c: &Config) -> Result<WebPageSource, String> {
        let url = c.require("url")?.to_string();
        let opt = |k: &str| c.get(k).map(str::to_string);
        let sel = opt("selector");
        if let Some(s) = &sel {
            selector(s)?;
        }
        let wait = opt("wait");
        if let Some(w) = &wait {
            selector(w)?;
        }
        let pattern = opt("pattern");
        if let Some(p) = &pattern {
            user_regex::compile(p, true)?;
        }
        let mode = match c.get("mode") {
            None | Some("version") => PageMode::Version,
            Some("change") => PageMode::Change,
            Some(other) => return Err(format!("\"{other}\" is not a page mode (version or change).")),
        };
        Ok(WebPageSource {
            url,
            selector: sel,
            attribute: opt("attribute"),
            pattern,
            mode,
            rendered: c.flag("rendered"),
            wait_selector: wait,
        })
    }

    /// Reads the page: the selected text (or attribute), or the whole page's visible text.
    pub fn extract(&self, html: &str) -> Result<Extraction, SourceError> {
        let doc = Html::parse_document(html);
        let mut lines: Vec<String> = Vec::new();
        let mut attribute_value = None;
        match &self.selector {
            Some(css) => {
                let sel = selector(css).map_err(SourceError::InvalidConfig)?;
                let Some(el) = doc.select(&sel).next() else {
                    return Err(SourceError::InvalidResponse(format!(
                        "The selector \"{css}\" matched nothing on the page. The page may have changed, or it needs to be rendered."
                    )));
                };
                match &self.attribute {
                    Some(a) => match el.value().attr(a) {
                        Some(v) => attribute_value = Some(collapse(v)),
                        None => {
                            return Err(SourceError::InvalidResponse(format!(
                                "The element \"{css}\" has no attribute \"{a}\"."
                            )))
                        }
                    },
                    None => visible_lines(el.descendants().map(|n| (n, true)), &mut lines),
                }
            }
            None => {
                if self.pattern.is_none() && self.mode == PageMode::Version {
                    if let Some(hint) = detect_versions(html).into_iter().next() {
                        return Ok(Extraction::of(vec![hint.value]));
                    }
                }
                visible_lines(doc.tree.root().descendants().map(|n| (n, true)), &mut lines);
            }
        }
        if let Some(v) = attribute_value {
            lines = vec![v];
        }
        lines.truncate(MAX_LINES);
        let mut text = lines.join(" ");
        if let Some(p) = &self.pattern {
            let re = user_regex::compile(p, true).map_err(SourceError::InvalidConfig)?;
            let found = re
                .captures(&text)
                .and_then(|c| c.get(1).or_else(|| c.get(0)))
                .map(|m| m.as_str().to_string());
            match found {
                Some(v) => {
                    lines = vec![v.clone()];
                    text = v;
                }
                None => {
                    return Err(SourceError::InvalidResponse(
                        "The pattern found no version in the page's text. The page may have changed.".into(),
                    ))
                }
            }
        }
        let mut ex = Extraction::of(lines);
        ex.text = text;
        Ok(ex)
    }

    /// The candidate for an extraction. In change mode the version is the date the current content was first seen
    /// (`since`), so it stays the same until the page changes.
    pub fn candidate(&self, ex: &Extraction, since: Timestamp) -> Candidate {
        match self.mode {
            PageMode::Version => {
                let v: String = ex.text.chars().take(MAX_VERSION_TEXT).collect();
                Candidate {
                    version_text: v.clone(),
                    name: v,
                    location: self.url.clone(),
                    published_at: None,
                    size: None,
                    sha256: None,
                    prerelease: false,
                }
            }
            PageMode::Change => Candidate {
                version_text: change_version(since),
                name: format!("Page changed {}", &change_version(since)[..10]),
                location: self.url.clone(),
                published_at: Some(since),
                size: None,
                sha256: None,
                prerelease: false,
            },
        }
    }

    /// Reads a body into candidates (change mode dates the content as of the epoch; the engine knows the real time).
    pub fn parse(&self, body: &[u8]) -> Result<Vec<Candidate>, SourceError> {
        let ex = self.extract(&String::from_utf8_lossy(body))?;
        Ok(vec![self.candidate(&ex, Timestamp(0))])
    }

    /// Short description for lists.
    pub fn describe(&self) -> String {
        self.url.split("://").nth(1).unwrap_or(&self.url).to_string()
    }
}

impl Extraction {
    fn of(lines: Vec<String>) -> Extraction {
        let text = lines.join(" ");
        let filtered: Vec<String> = lines.iter().map(|l| strip_noise(l)).filter(|l| !l.is_empty()).collect();
        Extraction {
            text,
            hash: Sha256Digest::of(filtered.join("\n").as_bytes()),
            lines: filtered,
        }
    }
}

/// The date-based version of content first seen at `since`: `2026.10.22.1405` (UTC).
pub fn change_version(since: Timestamp) -> String {
    let d = time::OffsetDateTime::from_unix_timestamp(since.0.div_euclid(1000)).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}.{:02}.{:02}.{:02}{:02}",
        d.year(),
        u8::from(d.month()),
        d.day(),
        d.hour(),
        d.minute()
    )
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Visible text nodes (not script, style or template content), one line each.
fn visible_lines<'a>(nodes: impl Iterator<Item = (ego_tree::NodeRef<'a, Node>, bool)>, out: &mut Vec<String>) {
    for (node, _) in nodes {
        let Node::Text(t) = node.value() else { continue };
        let hidden = node
            .ancestors()
            .any(|a| matches!(a.value(), Node::Element(e) if matches!(e.name(), "script" | "style" | "noscript" | "template" | "head")));
        if hidden {
            continue;
        }
        let line = collapse(t);
        if !line.is_empty() {
            out.push(line);
        }
        if out.len() >= MAX_LINES {
            break;
        }
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    // The patterns are constants exercised by the tests below.
    #[allow(clippy::expect_used)]
    cell.get_or_init(|| Regex::new(pattern).expect("built-in pattern"))
}

/// Removes what changes without the page really changing: timestamps and dates, relative times ("5 minutes ago"),
/// counters ("1,234 views") and extra whitespace. Case is kept.
pub fn strip_noise(line: &str) -> String {
    static ISO: OnceLock<Regex> = OnceLock::new();
    static TIME: OnceLock<Regex> = OnceLock::new();
    static WORDY: OnceLock<Regex> = OnceLock::new();
    static AGO: OnceLock<Regex> = OnceLock::new();
    static COUNT_AFTER: OnceLock<Regex> = OnceLock::new();
    static COUNT_BEFORE: OnceLock<Regex> = OnceLock::new();
    static COPYRIGHT: OnceLock<Regex> = OnceLock::new();
    let months = "(?:jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)";
    let iso = re(
        &ISO,
        r"\b\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?)?",
    );
    let time = re(
        &TIME,
        r"(?i)\b\d{1,2}:\d{2}(?::\d{2})?(?:\s?[ap]\.?m\.?)?(?:\s?(?:utc|gmt|[a-z]{3,4}t))?\b",
    );
    let wordy = re(
        &WORDY,
        &format!(r"(?i)\b(?:\d{{1,2}}(?:st|nd|rd|th)?\s+{months}\.?,?\s+\d{{4}}|{months}\.?\s+\d{{1,2}}(?:st|nd|rd|th)?,?\s+\d{{4}})\b"),
    );
    let ago = re(
        &AGO,
        r"(?i)\b(?:\d+|an?|just)\s*(?:seconds?|minutes?|mins?|hours?|days?|weeks?|months?|years?)?\s*ago\b|\bjust now\b",
    );
    let units = "views?|visitors?|visits?|downloads?|hits|comments?|likes?|stars?|users?|online|reads?|members?";
    let after = re(&COUNT_AFTER, &format!(r"(?i)\b[\d][\d,.\s]*\s*(?:{units})\b"));
    let before = re(&COUNT_BEFORE, &format!(r"(?i)\b(?:{units})\s*[:=]?\s*[\d][\d,.]*"));
    let copyright = re(&COPYRIGHT, r"(?i)(?:©|\(c\)|copyright)\s*(?:\d{4}\s*[-–]\s*)?\d{4}");
    let s = iso.replace_all(line, "");
    let s = wordy.replace_all(&s, "");
    let s = time.replace_all(&s, "");
    let s = ago.replace_all(&s, "");
    let s = after.replace_all(&s, "");
    let s = before.replace_all(&s, "");
    let s = copyright.replace_all(&s, "");
    collapse(&s)
}

/// A summary of what changed between two noise-filtered line lists: lines gone and lines new (at most 3 of each).
pub fn change_summary(old: &[String], new: &[String]) -> String {
    let added: Vec<&String> = new.iter().filter(|l| !old.contains(l)).collect();
    let removed: Vec<&String> = old.iter().filter(|l| !new.contains(l)).collect();
    if added.is_empty() && removed.is_empty() {
        return "The page's text changed.".into();
    }
    let show = |label: &str, v: &[&String]| {
        let shown: Vec<String> = v
            .iter()
            .take(3)
            .map(|l| format!("\"{}\"", l.chars().take(80).collect::<String>()))
            .collect();
        format!("{label} {} line(s): {}", v.len(), shown.join(", "))
    };
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(show("added", &added));
    }
    if !removed.is_empty() {
        parts.push(show("removed", &removed));
    }
    parts.join("; ")
}

/// A version the page states about itself (JSON-LD `softwareVersion`, meta tags, microdata).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionHint {
    /// Where it was found (`json-ld`, `meta:version`, `itemprop`).
    pub origin: String,
    /// The version text.
    pub value: String,
}

fn sel(css: &str) -> Option<Selector> {
    Selector::parse(css).ok()
}

/// Finds the versions a page states in structured data, best sources first.
pub fn detect_versions(html: &str) -> Vec<VersionHint> {
    let doc = Html::parse_document(html);
    let mut out: Vec<VersionHint> = Vec::new();
    let mut push = |origin: &str, value: &str| {
        let v = collapse(value);
        if !v.is_empty() && v.len() <= MAX_VERSION_TEXT && !out.iter().any(|h| h.value == v) {
            out.push(VersionHint {
                origin: origin.to_string(),
                value: v,
            });
        }
    };
    if let Some(s) = sel(r#"script[type="application/ld+json"]"#) {
        for script in doc.select(&s).take(20) {
            let text: String = script.text().collect();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let mut found = Vec::new();
                find_key(&v, "softwareVersion", 0, &mut found);
                for f in found {
                    push("json-ld", &f);
                }
            }
        }
    }
    if let Some(s) = sel("meta[content]") {
        for m in doc.select(&s).take(200) {
            let name = ["name", "property", "itemprop"]
                .iter()
                .find_map(|a| m.value().attr(a))
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "softwareversion" | "version" | "application-version" | "software-version" | "og:version"
            ) {
                if let Some(c) = m.value().attr("content") {
                    push(&format!("meta:{name}"), c);
                }
            }
        }
    }
    if let Some(s) = sel(r#"[itemprop="softwareVersion" i]"#) {
        for e in doc.select(&s).take(10) {
            let v = e.value().attr("content").map(str::to_string).unwrap_or_else(|| e.text().collect());
            push("itemprop", &v);
        }
    }
    out
}

fn find_key(v: &serde_json::Value, key: &str, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_JSON_DEPTH || out.len() >= 10 {
        return;
    }
    match v {
        serde_json::Value::Object(m) => {
            for (k, val) in m {
                if k == key {
                    match val {
                        serde_json::Value::String(s) => out.push(s.clone()),
                        serde_json::Value::Number(n) => out.push(n.to_string()),
                        _ => {}
                    }
                } else {
                    find_key(val, key, depth + 1, out);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| find_key(x, key, depth + 1, out)),
        _ => {}
    }
}

/// A feed a page advertises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFeed {
    /// Its absolute address.
    pub url: String,
    /// Its title (the address when the page gives none).
    pub title: String,
}

/// Finds `<link rel="alternate" type="application/rss+xml|atom+xml">` feeds, resolved against `base`.
pub fn discover_feeds(html: &str, base: &str) -> Vec<DiscoveredFeed> {
    let Ok(base) = url::Url::parse(base) else { return Vec::new() };
    let doc = Html::parse_document(html);
    let Some(s) = sel("link[href]") else { return Vec::new() };
    let mut out: Vec<DiscoveredFeed> = Vec::new();
    for l in doc.select(&s) {
        let v = l.value();
        let rel_ok = v
            .attr("rel")
            .is_some_and(|r| r.split_whitespace().any(|t| t.eq_ignore_ascii_case("alternate")));
        let ty = v.attr("type").unwrap_or("").to_ascii_lowercase();
        if !rel_ok || !(ty.starts_with("application/rss+xml") || ty.starts_with("application/atom+xml")) {
            continue;
        }
        let Some(url) = v.attr("href").and_then(|h| base.join(h.trim()).ok()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") || out.iter().any(|f| f.url == url.as_str()) {
            continue;
        }
        let title = v
            .attr("title")
            .map(collapse)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| url.to_string());
        out.push(DiscoveredFeed {
            url: url.to_string(),
            title: title.chars().take(MAX_PATTERN_LEN).collect(),
        });
        if out.len() >= MAX_FEEDS {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(values: &[(&str, &str)]) -> Config {
        let mut c = Config::new();
        for (k, v) in values {
            c = c.with(k, v);
        }
        c
    }

    const PAGE: &str = r#"<html><head><title>Downloads</title><script>var v = "9.9.9";</script></head>
        <body><h1>Foo</h1><p class="ver">Latest: <b>v2.4.1</b> released 2026-10-01 12:30</p>
        <p>1,234 downloads</p><a id="dl" href="/foo-2.4.1.zip" data-version="2.4.1">Download</a></body></html>"#;

    fn src(values: &[(&str, &str)]) -> WebPageSource {
        let mut v = vec![("url", "https://example.org/dl")];
        v.extend_from_slice(values);
        WebPageSource::from_config(&cfg(&v)).unwrap()
    }

    #[test]
    fn forms_are_validated() {
        assert!(WebPageSource::from_config(&cfg(&[])).is_err());
        assert!(WebPageSource::from_config(&cfg(&[("url", "https://a.b"), ("selector", "p[")]))
            .unwrap_err()
            .contains("CSS"));
        assert!(WebPageSource::from_config(&cfg(&[("url", "https://a.b"), ("wait", "[[")])).is_err());
        assert!(WebPageSource::from_config(&cfg(&[("url", "https://a.b"), ("pattern", "(")])).is_err());
        assert!(WebPageSource::from_config(&cfg(&[("url", "https://a.b"), ("mode", "x")]))
            .unwrap_err()
            .contains("mode"));
        let s = src(&[("mode", "change"), ("rendered", "true"), ("wait", "#app")]);
        assert!(s.rendered && s.mode == PageMode::Change && s.wait_selector.as_deref() == Some("#app"));
        assert_eq!(s.describe(), "example.org/dl");
    }

    #[test]
    fn selectors_attributes_and_patterns_read_versions() {
        let by_class = src(&[("selector", "p.ver b")]).extract(PAGE).unwrap();
        assert_eq!(by_class.text, "v2.4.1");
        let by_attr = src(&[("selector", "#dl"), ("attribute", "data-version")]).extract(PAGE).unwrap();
        assert_eq!(by_attr.text, "2.4.1");
        let by_pattern = src(&[("pattern", r"Latest:\s*v?([0-9.]+)")]).extract(PAGE).unwrap();
        assert_eq!(by_pattern.text, "2.4.1");
        let whole_match = src(&[("selector", "p.ver"), ("pattern", r"v\d+\.\d+\.\d+")]).extract(PAGE).unwrap();
        assert_eq!(whole_match.text, "v2.4.1");
        let c = by_class_candidate(&src(&[("selector", "p.ver b")]));
        assert_eq!((c.version_text.as_str(), c.location.as_str()), ("v2.4.1", "https://example.org/dl"));
    }

    fn by_class_candidate(s: &WebPageSource) -> Candidate {
        s.parse(PAGE.as_bytes()).unwrap().remove(0)
    }

    #[test]
    fn script_text_is_not_page_text_and_failures_say_what_to_fix() {
        let all = src(&[("pattern", r"([0-9]+\.[0-9]+\.[0-9]+)")]).extract(PAGE).unwrap();
        assert_eq!(all.text, "2.4.1", "the version in <script> is ignored");
        assert!(src(&[("selector", "#nothing")])
            .extract(PAGE)
            .unwrap_err()
            .message()
            .contains("matched nothing"));
        assert!(src(&[("selector", "#dl"), ("attribute", "nope")])
            .extract(PAGE)
            .unwrap_err()
            .message()
            .contains("no attribute"));
        assert!(src(&[("pattern", "zzz")])
            .extract(PAGE)
            .unwrap_err()
            .message()
            .contains("found no version"));
    }

    #[test]
    fn structured_data_is_preferred_without_a_selector() {
        let html = r#"<head><script type="application/ld+json">{"@graph":[{"@type":"SoftwareApplication","softwareVersion":"3.1.4"}]}</script>
            <meta name="version" content="3.1.4"><meta property="og:version" content="3.1.5"></head><body>Nothing 1.0 here</body>"#;
        let hints = detect_versions(html);
        assert_eq!(
            hints[0],
            VersionHint {
                origin: "json-ld".into(),
                value: "3.1.4".into()
            }
        );
        assert_eq!(hints.len(), 2, "duplicates collapse: {hints:?}");
        assert_eq!(src(&[]).extract(html).unwrap().text, "3.1.4");
        let item = r#"<span itemprop="softwareVersion">7.0</span><meta itemprop="softwareVersion" content="7.0.1">"#;
        assert_eq!(detect_versions(item).len(), 2);
        assert!(detect_versions("<p>no hints</p><script type=\"application/ld+json\">{broken</script>").is_empty());
        let numbers = r#"<script type="application/ld+json">[{"softwareVersion":12},{"softwareVersion":[1]}]</script>"#;
        assert_eq!(detect_versions(numbers)[0].value, "12");
    }

    #[test]
    fn json_ld_nesting_is_bounded() {
        let deep = format!("{}{}{}", "[".repeat(40), r#"{"softwareVersion":"1"}"#, "]".repeat(40));
        let html = format!(r#"<script type="application/ld+json">{deep}</script>"#);
        assert!(detect_versions(&html).is_empty());
    }

    #[test]
    fn noise_does_not_change_the_hash() {
        let a = "<p>Version 2.0 is out</p><p>Updated 2026-10-01 10:00:00Z</p><p>1,234 views</p><p>Visitors: 50</p><p>5 minutes ago</p>";
        let b = "<p>Version   2.0 is out</p><p>Updated 2026-10-22 23:59:59Z</p><p>9,999 views</p><p>Visitors: 77</p><p>3 hours ago</p>";
        let c = "<p>Version 2.1 is out</p>";
        let s = src(&[("mode", "change")]);
        let (ea, eb, ec) = (s.extract(a).unwrap(), s.extract(b).unwrap(), s.extract(c).unwrap());
        assert_eq!(ea.hash, eb.hash, "{:?} vs {:?}", ea.lines, eb.lines);
        assert_ne!(ea.hash, ec.hash);
        assert_eq!(ea.lines, ["Version 2.0 is out", "Updated"]);
    }

    #[test]
    fn noise_filter_cases() {
        for (input, expected) in [
            ("Last updated: October 5, 2026 at 10:15 PM UTC", "Last updated: at"),
            ("Posted 5 Oct 2026", "Posted"),
            ("  a   b ", "a b"),
            ("(c) 2020-2026 Foo", "Foo"),
            ("12 comments", ""),
            ("Downloads: 4,521", ""),
            ("2 days ago", ""),
            ("just now", ""),
            ("Release 3.4 notes", "Release 3.4 notes"),
        ] {
            assert_eq!(strip_noise(input), expected, "{input}");
        }
    }

    #[test]
    fn change_candidates_are_dated_and_stable() {
        let s = src(&[("mode", "change")]);
        let ex = s.extract("<p>hello</p>").unwrap();
        let since = Timestamp::parse("2026-10-22T14:05:09Z").unwrap();
        let c = s.candidate(&ex, since);
        assert_eq!(c.version_text, "2026.10.22.1405");
        assert_eq!(c.name, "Page changed 2026.10.22");
        assert_eq!(c.published_at, Some(since));
        assert_eq!(change_version(Timestamp(0)), "1970.01.01.0000");
        assert_eq!(s.parse(b"<p>x</p>").unwrap().len(), 1);
    }

    #[test]
    fn change_summaries_name_the_lines() {
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "c".to_string(), "d".to_string()];
        assert_eq!(
            change_summary(&old, &new),
            "added 2 line(s): \"c\", \"d\"; removed 1 line(s): \"b\""
        );
        assert_eq!(change_summary(&old, &old), "The page's text changed.");
        assert_eq!(change_summary(&old, &[]), "removed 2 line(s): \"a\", \"b\"");
    }

    #[test]
    fn feeds_are_discovered_and_resolved() {
        let html = r#"<head>
            <link rel="alternate" type="application/rss+xml" title="Posts" href="/feed.xml">
            <link rel="ALTERNATE" type="application/atom+xml" href="https://other.example.org/atom">
            <link rel="alternate" type="application/atom+xml" href="/feed.xml">
            <link rel="alternate" type="text/html" href="/de">
            <link rel="stylesheet" type="application/rss+xml" href="/no">
            <link rel="alternate" type="application/rss+xml" href="javascript:alert(1)">
            <link rel="alternate" type="application/rss+xml" href="mailto:a@b.c">
            </head>"#;
        let feeds = discover_feeds(html, "https://example.org/blog/");
        assert_eq!(
            feeds,
            [
                DiscoveredFeed {
                    url: "https://example.org/feed.xml".into(),
                    title: "Posts".into()
                },
                DiscoveredFeed {
                    url: "https://other.example.org/atom".into(),
                    title: "https://other.example.org/atom".into()
                },
            ]
        );
        assert!(discover_feeds(html, "not a url").is_empty());
        let many: String = (0..40)
            .map(|i| format!(r#"<link rel="alternate" type="application/rss+xml" href="/f{i}">"#))
            .collect();
        assert_eq!(discover_feeds(&many, "https://e.org").len(), MAX_FEEDS);
    }

    #[test]
    fn long_version_text_is_cut() {
        let s = src(&[("selector", "p")]);
        let html = format!("<p>{}</p>", "x".repeat(500));
        let c = s.parse(html.as_bytes()).unwrap().remove(0);
        assert_eq!(c.version_text.chars().count(), MAX_VERSION_TEXT);
    }
}
