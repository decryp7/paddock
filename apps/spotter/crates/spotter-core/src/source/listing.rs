//! HTTP(S) directory listings (nginx/Apache/IIS index pages, S3-style HTML indexes): every link below the listed
//! address is an entry; its last path segment is its name.

use super::SourceError;
use crate::limits::MAX_CANDIDATES;
use crate::pattern::{NamePattern, PatternSyntax};
use crate::release::Candidate;
use regex::Regex;
use crate::schema::Config;
use std::sync::OnceLock;
use url::Url;

/// A web directory source.
#[derive(Debug, Clone, PartialEq)]
pub struct WebDirectorySource {
    /// The listing's address.
    pub url: String,
    /// Which names.
    pub pattern: NamePattern,
}

impl WebDirectorySource {
    /// From form values (`url`, `pattern`, `syntax`).
    pub fn from_config(c: &Config) -> Result<WebDirectorySource, String> {
        let url = c.require("url")?;
        Url::parse(url).map_err(|e| format!("The address is not valid: {e}."))?;
        let syntax = PatternSyntax::parse(c.get("syntax").unwrap_or("glob"))?;
        Ok(WebDirectorySource {
            url: url.to_string(),
            pattern: NamePattern::new(syntax, c.get("pattern").unwrap_or("*"))?,
        })
    }

    /// The candidates linked from a listing page.
    pub fn candidates(&self, html: &str) -> Result<Vec<Candidate>, SourceError> {
        let base = Url::parse(&self.url).map_err(|e| SourceError::InvalidConfig(format!("The address is not valid: {e}.")))?;
        let mut dir = base.clone();
        dir.set_query(None);
        dir.set_fragment(None);
        if !dir.path().ends_with('/') {
            let path = format!("{}/", dir.path());
            dir.set_path(&path);
        }
        static HREF: OnceLock<Regex> = OnceLock::new();
        #[allow(clippy::expect_used)] // a constant pattern, exercised by the tests
        let href = HREF.get_or_init(|| Regex::new(r#"(?i)<a\s[^>]*?href\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>"']+))"#).expect("pattern"));
        let mut out: Vec<Candidate> = Vec::new();
        for caps in href.captures_iter(html).take(MAX_CANDIDATES * 2) {
            let Some(raw) = caps.get(1).or_else(|| caps.get(2)).or_else(|| caps.get(3)) else {
                continue;
            };
            let raw = raw.as_str().replace("&amp;", "&");
            let Ok(link) = dir.join(&raw) else { continue };
            if link.origin() != dir.origin() || link.query().is_some() || !link.path().starts_with(dir.path()) || link.path() == dir.path()
            {
                continue;
            }
            let Some(name) = last_segment(&link) else { continue };
            if !self.pattern.matches(&name) || out.iter().any(|c| c.location == link.as_str()) {
                continue;
            }
            out.push(Candidate::named(&name, link.as_str()));
            if out.len() >= MAX_CANDIDATES {
                break;
            }
        }
        Ok(out)
    }
}

fn last_segment(url: &Url) -> Option<String> {
    let seg = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = url::form_urlencoded::parse(format!("x={}", seg.replace('+', "%2B")).as_bytes())
        .next()?
        .1
        .into_owned();
    (!decoded.is_empty()).then_some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NGINX: &str = r#"<html><head><title>Index of /builds/</title></head><body><h1>Index of /builds/</h1><hr><pre>
<a href="../">../</a>
<a href="app-1.2.0.zip">app-1.2.0.zip</a>          22-Oct-2026 08:00   10485760
<a href="app-1.3.0%2Bbuild%201.zip">app-1.3.0+build 1.zip</a>
<a href='nightly/'>nightly/</a>
<A HREF=app-1.1.0.zip>old</A>
<a href="?C=M;O=A">sort</a>
<a href="https://elsewhere.example/app-9.zip">mirror</a>
<a href="/other/app-8.zip">other folder</a>
<a href="app-1.2.0.zip">duplicate</a>
<a name="anchor">no href</a>
</pre></body></html>"#;

    fn source(url: &str, pattern: &str) -> WebDirectorySource {
        WebDirectorySource::from_config(&Config::new().with("url", url).with("pattern", pattern)).unwrap()
    }

    #[test]
    fn links_below_the_listing_are_entries() {
        let s = source("https://dl.example.org/builds", "*");
        let c = s.candidates(NGINX).unwrap();
        let names: Vec<&str> = c.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["app-1.2.0.zip", "app-1.3.0+build 1.zip", "nightly", "app-1.1.0.zip"]);
        assert_eq!(c[0].location, "https://dl.example.org/builds/app-1.2.0.zip");
        let zips = source("https://dl.example.org/builds/", "*.zip").candidates(NGINX).unwrap();
        assert_eq!(zips.len(), 3);
    }

    #[test]
    fn bad_addresses_are_refused() {
        assert!(WebDirectorySource::from_config(&Config::new().with("url", "not a url")).is_err());
        assert!(WebDirectorySource::from_config(&Config::new().with("url", "https://a.b/").with("syntax", "x")).is_err());
        let broken = WebDirectorySource {
            url: "::".into(),
            pattern: NamePattern::any(),
        };
        assert!(matches!(broken.candidates(""), Err(SourceError::InvalidConfig(_))));
        assert!(source("https://a.b/x/", "*")
            .candidates("<a href=\"http://[::1\">x</a>")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn listings_are_bounded() {
        let page: String = (0..MAX_CANDIDATES + 5).map(|i| format!("<a href=\"f{i}.zip\">x</a>\n")).collect();
        assert_eq!(source("https://a.b/x/", "*").candidates(&page).unwrap().len(), MAX_CANDIDATES);
    }
}
