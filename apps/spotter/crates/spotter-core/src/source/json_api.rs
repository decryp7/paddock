//! JSON APIs read with selectors ([`super::json_select`]), and the package registries (crates.io, npm, NuGet, PyPI)
//! as presets of the same source.

use super::http::{encode_segment, HttpRequest};
use super::json_select::{Item, Selector};
use super::SourceError;
use crate::checksum::Sha256Digest;
use crate::release::Candidate;
use crate::schema::Config;
use crate::time::Timestamp;

/// Where a release's location comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// A selector.
    Select(Selector),
    /// An address with `{version}` (and `{package}` for registries) filled in.
    Template(String),
}

/// A JSON API source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonApiSource {
    /// The address.
    pub url: String,
    /// Where the releases are.
    pub items: Selector,
    /// The version of one release.
    pub version: Selector,
    /// Its name (the version when absent).
    pub name: Option<Selector>,
    /// Its location (the API address when absent).
    pub location: Option<Location>,
    /// When it was published.
    pub published: Option<Selector>,
    /// Its size.
    pub size: Option<Selector>,
    /// Its SHA-256.
    pub sha256: Option<Selector>,
    /// Releases for which this is true-ish are skipped (yanked versions).
    pub skip: Option<Selector>,
    /// The package name, for `{package}` in templates and for display.
    pub package: Option<String>,
    /// How it is shown in lists.
    pub label: String,
}

fn optional(c: &Config, key: &str) -> Result<Option<Selector>, String> {
    c.get(key).map(Selector::parse).transpose()
}

impl JsonApiSource {
    /// From form values (`url`, `items`, `version`, `name`, `location`, `published`, `size`, `sha256`).
    pub fn from_config(c: &Config) -> Result<JsonApiSource, String> {
        let url = c.require("url")?.to_string();
        let location = match c.get("location") {
            None => None,
            Some(l) if l.starts_with('$') || l.starts_with('@') => Some(Location::Select(Selector::parse(l)?)),
            Some(l) => Some(Location::Template(l.to_string())),
        };
        Ok(JsonApiSource {
            label: url.split("://").nth(1).unwrap_or(&url).to_string(),
            items: Selector::parse(c.get("items").unwrap_or("$[*]"))?,
            version: Selector::parse(c.get("version").unwrap_or("$.version"))?,
            name: optional(c, "name")?,
            location,
            published: optional(c, "published")?,
            size: optional(c, "size")?,
            sha256: optional(c, "sha256")?,
            skip: None,
            package: None,
            url,
        })
    }

    /// The request, with an optional Bearer token.
    pub fn request(&self, token: Option<String>) -> HttpRequest {
        HttpRequest::get(self.url.clone())
            .header("Accept", "application/json")
            .header_if("Authorization", token.map(|t| format!("Bearer {t}")))
    }

    /// How it is shown.
    pub fn describe(&self) -> String {
        self.label.clone()
    }

    /// Reads the answer.
    pub fn parse(&self, body: &[u8]) -> Result<Vec<Candidate>, SourceError> {
        let doc: serde_json::Value =
            serde_json::from_slice(body).map_err(|e| SourceError::InvalidResponse(format!("The answer is not JSON ({e}).")))?;
        let items = self.items.items(Item { key: None, value: &doc });
        Ok(items.into_iter().filter_map(|item| self.candidate(item)).collect())
    }

    fn candidate(&self, item: Item<'_>) -> Option<Candidate> {
        if self.skip.as_ref().is_some_and(|s| s.truthy(item)) {
            return None;
        }
        let version = self.version.text(item)?;
        let text = |s: &Option<Selector>| s.as_ref().and_then(|s| s.text(item));
        let location = match &self.location {
            None => self.url.clone(),
            Some(Location::Select(s)) => s.text(item).unwrap_or_else(|| self.url.clone()),
            Some(Location::Template(t)) => t
                .replace("{version}", &encode_segment(&version))
                .replace("{package}", self.package.as_deref().unwrap_or("")),
        };
        Some(Candidate {
            name: text(&self.name).unwrap_or_else(|| version.clone()),
            location,
            published_at: text(&self.published).as_deref().and_then(Timestamp::parse),
            size: text(&self.size).and_then(|s| s.parse().ok()),
            sha256: text(&self.sha256).as_deref().and_then(Sha256Digest::parse),
            prerelease: false,
            version_text: version,
        })
    }
}

/// A package registry source (form values `registry`, `package`) as a preset JSON API source.
pub fn package(c: &Config) -> Result<JsonApiSource, String> {
    let registry = c.require("registry")?;
    let name = c.require("package")?;
    if name.len() > 214 || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || "@/._-".contains(ch)) || name.starts_with(['.', '/']) {
        return Err(format!("\"{name}\" is not a package name."));
    }
    let sel = |s: &str| Selector::parse(s);
    let mut src = JsonApiSource {
        url: String::new(),
        items: sel("$.versions[*]")?,
        version: sel("$")?,
        name: None,
        location: None,
        published: None,
        size: None,
        sha256: None,
        skip: None,
        package: Some(name.to_string()),
        label: String::new(),
    };
    match registry {
        "crates-io" => {
            src.url = format!("https://crates.io/api/v1/crates/{}/versions", encode_segment(name));
            src.version = sel("$.num")?;
            src.published = Some(sel("$.created_at")?);
            src.size = Some(sel("$.crate_size")?);
            src.skip = Some(sel("$.yanked")?);
            src.location = Some(Location::Template(format!(
                "https://crates.io/crates/{}/{{version}}",
                encode_segment(name)
            )));
            src.label = format!("crates.io/{name}");
        }
        "npm" => {
            src.url = format!("https://registry.npmjs.org/{}", name.replace('/', "%2F"));
            src.items = sel("$.time.*")?;
            src.version = Selector::Key;
            src.published = Some(sel("$")?);
            src.location = Some(Location::Template(format!("https://www.npmjs.com/package/{name}/v/{{version}}")));
            src.label = format!("npm/{name}");
        }
        "nuget" => {
            src.url = format!(
                "https://api.nuget.org/v3-flatcontainer/{}/index.json",
                encode_segment(&name.to_ascii_lowercase())
            );
            src.location = Some(Location::Template(format!(
                "https://www.nuget.org/packages/{}/{{version}}",
                encode_segment(name)
            )));
            src.label = format!("nuget/{name}");
        }
        "pypi" => {
            src.url = format!("https://pypi.org/pypi/{}/json", encode_segment(name));
            src.items = sel("$.releases.*")?;
            src.version = Selector::Key;
            src.published = Some(sel("$[0].upload_time_iso_8601")?);
            src.skip = Some(sel("$[0].yanked")?);
            src.location = Some(Location::Template(format!(
                "https://pypi.org/project/{}/{{version}}/",
                encode_segment(name)
            )));
            src.label = format!("pypi/{name}");
        }
        other => return Err(format!("\"{other}\" is not a supported package registry.")),
    }
    Ok(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(registry: &str, name: &str) -> Result<JsonApiSource, String> {
        package(&Config::new().with("registry", registry).with("package", name))
    }

    #[test]
    fn crates_io_skips_yanked_versions() {
        let s = pkg("crates-io", "serde").unwrap();
        assert_eq!(s.url, "https://crates.io/api/v1/crates/serde/versions");
        let body = br#"{"versions":[{"num":"1.0.229","created_at":"2026-10-01T00:00:00Z","crate_size":78000,"yanked":false},
                                    {"num":"1.0.228","yanked":true}]}"#;
        let c = s.parse(body).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(
            (c[0].location.as_str(), c[0].size),
            ("https://crates.io/crates/serde/1.0.229", Some(78000))
        );
        assert!(c[0].published_at.is_some());
        assert_eq!(s.describe(), "crates.io/serde");
    }

    #[test]
    fn npm_nuget_and_pypi_presets() {
        let npm = pkg("npm", "@scope/pkg").unwrap();
        assert_eq!(npm.url, "https://registry.npmjs.org/@scope%2Fpkg");
        let c = npm
            .parse(br#"{"time":{"created":"2020-01-01T00:00:00Z","1.2.0":"2026-10-01T00:00:00Z"}}"#)
            .unwrap();
        assert_eq!(c[0].location, "https://www.npmjs.com/package/@scope/pkg/v/1.2.0");
        assert_eq!(c[0].version_text, "1.2.0");

        let nuget = pkg("nuget", "Newtonsoft.Json").unwrap();
        assert_eq!(nuget.url, "https://api.nuget.org/v3-flatcontainer/newtonsoft.json/index.json");
        let c = nuget.parse(br#"{"versions":["13.0.1","13.0.3"]}"#).unwrap();
        assert_eq!(c[1].location, "https://www.nuget.org/packages/Newtonsoft.Json/13.0.3");

        let pypi = pkg("pypi", "requests").unwrap();
        let c = pypi
            .parse(br#"{"releases":{"2.32.3":[{"upload_time_iso_8601":"2026-05-29T15:37:47.123Z","yanked":false}],"2.32.0":[{"yanked":true}],"1.0":[]}}"#)
            .unwrap();
        let versions: Vec<&str> = c.iter().map(|c| c.version_text.as_str()).collect();
        assert_eq!(versions, ["1.0", "2.32.3"]);
        assert!(c[1].published_at.is_some() && c[0].published_at.is_none());
        assert_eq!(c[1].location, "https://pypi.org/project/requests/2.32.3/");

        assert!(pkg("maven", "x").unwrap_err().contains("not a supported"));
        assert!(pkg("npm", "bad name").is_err());
        assert!(pkg("npm", "/abs").is_err());
        assert!(package(&Config::new().with("registry", "npm")).is_err());
    }

    #[test]
    fn generic_json_apis_use_their_selectors() {
        let c = Config::new()
            .with("url", "https://api.example.org/builds")
            .with("items", "$.builds[*]")
            .with("version", "$.v")
            .with("name", "$.title")
            .with("location", "$.href")
            .with("published", "$.at")
            .with("size", "$.bytes")
            .with("sha256", "$.sum");
        let s = JsonApiSource::from_config(&c).unwrap();
        assert_eq!(s.describe(), "api.example.org/builds");
        let req = s.request(Some("t".into()));
        assert!(req.headers.contains(&("Authorization".into(), "Bearer t".into())));
        let body = br#"{"builds":[
            {"v":"2.0","title":"Two","href":"https://dl/2","at":"2026-10-01","bytes":"9","sum":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},
            {"v":"1.0"},
            {"title":"no version"}]}"#;
        let r = s.parse(body).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].name.as_str(), r[0].location.as_str(), r[0].size),
            ("Two", "https://dl/2", Some(9))
        );
        assert!(r[0].sha256.is_some() && r[0].published_at.is_some());
        assert_eq!(
            (r[1].name.as_str(), r[1].location.as_str()),
            ("1.0", "https://api.example.org/builds")
        );

        let tmpl =
            JsonApiSource::from_config(&Config::new().with("url", "https://a/b").with("location", "https://dl/{version}/x")).unwrap();
        assert_eq!(tmpl.parse(br#"[{"version":"1 0"}]"#).unwrap()[0].location, "https://dl/1%200/x");
        let plain = JsonApiSource::from_config(&Config::new().with("url", "u")).unwrap();
        assert_eq!(plain.describe(), "u");
        assert!(plain.parse(b"nope").is_err());
        assert!(JsonApiSource::from_config(&Config::new().with("url", "u").with("items", "bad")).is_err());
        assert!(JsonApiSource::from_config(&Config::new().with("url", "u").with("location", "$[")).is_err());
        assert!(JsonApiSource::from_config(&Config::new()).is_err());
    }
}
