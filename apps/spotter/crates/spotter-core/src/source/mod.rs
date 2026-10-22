//! Sources: where releases come from.
//!
//! A watch stores its source as a kind id and the values of that kind's form ([`SourceConfig`]); loading parses it
//! into a typed [`Source`] once. Checking a source is split the ports-and-adapters way: [`Source::plan`] says what
//! to read (a folder, an HTTP request) and how to parse the answer; an adapter performs the I/O
//! behind the [`ReleaseSource`] port and hands the bytes back to the core's parsers.

pub mod catalog;
pub mod container;
pub mod feed;
pub mod folder;
pub mod forge;
pub mod http;
pub mod json_api;
pub mod json_select;
pub mod listing;
pub mod web_page;

use crate::release::Candidate;
use crate::schema::{check_all, Config};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

pub use container::ContainerSource;
pub use folder::{EntryKind, FolderEntry, FolderSource};
pub use forge::{Forge, ForgeSource};
pub use http::HttpRequest;
pub use json_api::JsonApiSource;
pub use listing::WebDirectorySource;
pub use web_page::{PageMode, WebPageSource};

/// A source as stored: its kind and the values of the kind's form. Secret fields are never stored here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConfig {
    /// A built-in kind id (`folder`, `github`, …).
    pub kind: String,
    /// The entered values by field key.
    #[serde(default, deserialize_with = "scalars")]
    pub values: BTreeMap<String, String>,
}

/// Values as text, also when a hand-edited file writes numbers or booleans (`interval = 5`, `recursive = true`).
fn scalars<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Scalar {
        Text(String),
        Integer(i64),
        Float(f64),
        Toggle(bool),
    }
    let raw = BTreeMap::<String, Scalar>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .map(|(k, v)| {
            let text = match v {
                Scalar::Text(s) => s,
                Scalar::Integer(i) => i.to_string(),
                Scalar::Float(f) => f.to_string(),
                Scalar::Toggle(b) => b.to_string(),
            };
            (k, text)
        })
        .collect())
}

impl SourceConfig {
    /// A configuration.
    pub fn new(kind: &str, values: &[(&str, &str)]) -> SourceConfig {
        SourceConfig {
            kind: kind.into(),
            values: values.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    /// The shortest time between two checks of this source: a folder is local and may be scanned often, anything on
    /// the network is checked politely.
    pub fn min_interval_secs(&self) -> u32 {
        if self.kind == "folder" {
            crate::limits::MIN_INTERVAL_SECS
        } else {
            crate::schedule::MIN_SITE_INTERVAL_SECS
        }
    }

    /// The values as a [`Config`].
    pub fn config(&self) -> Config {
        Config(self.values.clone())
    }
}

/// Secret values by field key (from the credential store), for one check.
pub type Secrets = BTreeMap<String, String>;

/// Why a check of a source failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The place could not be reached (offline share, network down, server error); checks back off and retry.
    Unreachable(String),
    /// Credentials are missing or were refused.
    Unauthorized(String),
    /// The place exists, the thing asked for does not.
    NotFound(String),
    /// The configuration is not valid.
    InvalidConfig(String),
    /// The answer could not be read.
    InvalidResponse(String),
    /// A page asked for a login or a captcha; the user has to deal with it (never solved automatically).
    NeedsYou(String),
    /// Anything else.
    Failed(String),
}

impl SourceError {
    /// The message for the user.
    pub fn message(&self) -> &str {
        match self {
            SourceError::Unreachable(m)
            | SourceError::Unauthorized(m)
            | SourceError::NotFound(m)
            | SourceError::InvalidConfig(m)
            | SourceError::InvalidResponse(m)
            | SourceError::NeedsYou(m)
            | SourceError::Failed(m) => m,
        }
    }
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for SourceError {}

/// A boxed future, so the port can be used as a trait object without an async runtime in the core.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The port every source adapter implements: list the candidates the source has now.
///
/// Contract: never panics; returns at most `limit` candidates (more are ignored); reports an unreachable place as
/// [`SourceError::Unreachable`] (not as a generic failure) so the watch shows it as a status and backs off.
pub trait ReleaseSource: Send + Sync {
    /// The candidates, in the source's own order.
    fn list(&self, limit: usize) -> BoxFuture<'_, Result<Vec<Candidate>, SourceError>>;
}

/// A typed source.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A folder: local, mounted network drive or UNC share.
    Folder(FolderSource),
    /// An HTTP(S) directory listing.
    WebDirectory(WebDirectorySource),
    /// An RSS or Atom feed.
    Feed {
        /// The feed's address.
        url: String,
    },
    /// GitHub, Gitea/Forgejo or GitLab releases.
    Forge(ForgeSource),
    /// Tags of a container image (OCI registry).
    Container(ContainerSource),
    /// A JSON API read with selectors (also the package registries).
    JsonApi(JsonApiSource),
    /// A web page: a version read with a selector or pattern, or the page's text watched for changes.
    WebPage(WebPageSource),
}

/// What to read for a check.
#[derive(Debug, Clone, PartialEq)]
// One value per watch, never in a hot loop: the size difference does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Plan {
    /// List a folder, then [`FolderSource::candidates`].
    Folder(FolderSource),
    /// Perform a request, then [`Parser::parse`] the body.
    Http {
        /// The request (secrets already in its headers).
        request: HttpRequest,
        /// How to read the answer.
        parser: Parser,
    },
    /// Render the page in a browser (it needs JavaScript), then [`Parser::parse`] the DOM.
    Render {
        /// The page.
        url: String,
        /// Element to wait for.
        wait_selector: Option<String>,
        /// How to read the DOM.
        parser: Parser,
    },
}

/// How an HTTP answer is read.
#[derive(Debug, Clone, PartialEq)]
pub enum Parser {
    /// An HTML directory listing.
    Listing(WebDirectorySource),
    /// RSS or Atom.
    Feed,
    /// A forge's releases.
    Forge(ForgeSource),
    /// An OCI tag list.
    Tags(ContainerSource),
    /// JSON with selectors.
    Json(JsonApiSource),
    /// A web page.
    Page(WebPageSource),
}

impl Parser {
    /// Reads a response body into candidates.
    pub fn parse(&self, body: &[u8]) -> Result<Vec<Candidate>, SourceError> {
        match self {
            Parser::Listing(s) => s.candidates(&String::from_utf8_lossy(body)),
            Parser::Feed => feed::parse(body),
            Parser::Forge(s) => s.parse(body),
            Parser::Tags(s) => s.parse(body),
            Parser::Json(s) => s.parse(body),
            Parser::Page(s) => s.parse(body),
        }
    }
}

impl Source {
    /// Parses a stored source. Checks the values against the kind's form first; the error lists every problem.
    pub fn parse(cfg: &SourceConfig) -> Result<Source, Vec<String>> {
        let config = cfg.config();
        if !config.within_limits() {
            return Err(vec!["The source settings are too large.".into()]);
        }
        let spec = catalog::builtin(&cfg.kind).ok_or_else(|| vec![format!("\"{}\" is not a kind of source.", cfg.kind)])?;
        check_all(&spec.fields, &config)?;
        let one = |r: Result<Source, String>| r.map_err(|e| vec![e]);
        match cfg.kind.as_str() {
            catalog::FOLDER => one(FolderSource::from_config(&config).map(Source::Folder)),
            catalog::WEB_DIRECTORY => one(WebDirectorySource::from_config(&config).map(Source::WebDirectory)),
            catalog::FEED => one(config.require("url").map(|url| Source::Feed { url: url.to_string() })),
            catalog::GITHUB => one(ForgeSource::from_config(Forge::GitHub, &config).map(Source::Forge)),
            catalog::GITEA => one(ForgeSource::from_config(Forge::Gitea, &config).map(Source::Forge)),
            catalog::GITLAB => one(ForgeSource::from_config(Forge::GitLab, &config).map(Source::Forge)),
            catalog::CONTAINER => one(ContainerSource::from_config(&config).map(Source::Container)),
            catalog::PACKAGE => one(json_api::package(&config).map(Source::JsonApi)),
            catalog::WEB_PAGE => one(WebPageSource::from_config(&config).map(Source::WebPage)),
            _ => one(JsonApiSource::from_config(&config).map(Source::JsonApi)),
        }
    }

    /// Keys of the secret fields whose values the check needs from the credential store.
    pub fn secret_keys(&self) -> Vec<String> {
        match self {
            Source::Forge(_) | Source::JsonApi(_) => vec![catalog::TOKEN.into()],
            Source::Container(_) => vec![catalog::TOKEN.into()],
            Source::Folder(_) | Source::WebDirectory(_) | Source::Feed { .. } | Source::WebPage(_) => Vec::new(),
        }
    }

    /// What to read for a check, with `secrets` filled in.
    pub fn plan(&self, secrets: &Secrets) -> Plan {
        let token = secrets.get(catalog::TOKEN).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        match self {
            Source::Folder(f) => Plan::Folder(f.clone()),
            Source::WebDirectory(w) => Plan::Http {
                request: HttpRequest::get(w.url.clone()).header("Accept", "text/html"),
                parser: Parser::Listing(w.clone()),
            },
            Source::Feed { url } => Plan::Http {
                request: HttpRequest::get(url.clone()).header("Accept", "application/atom+xml, application/rss+xml, application/xml"),
                parser: Parser::Feed,
            },
            Source::Forge(f) => Plan::Http {
                request: f.request(token),
                parser: Parser::Forge(f.clone()),
            },
            Source::Container(c) => Plan::Http {
                request: c.request(),
                parser: Parser::Tags(c.clone()),
            },
            Source::JsonApi(j) => Plan::Http {
                request: j.request(token),
                parser: Parser::Json(j.clone()),
            },
            Source::WebPage(p) if p.rendered => Plan::Render {
                url: p.url.clone(),
                wait_selector: p.wait_selector.clone(),
                parser: Parser::Page(p.clone()),
            },
            Source::WebPage(p) => Plan::Http {
                request: HttpRequest::get(p.url.clone()).header("Accept", "text/html, application/xhtml+xml"),
                parser: Parser::Page(p.clone()),
            },
        }
    }

    /// A short description of where it looks, for lists (`\\server\builds`, `github.com/owner/repo`).
    pub fn describe(&self) -> String {
        match self {
            Source::Folder(f) => f.path.clone(),
            Source::WebDirectory(w) => w.url.clone(),
            Source::Feed { url } => url.clone(),
            Source::Forge(f) => f.describe(),
            Source::Container(c) => c.describe(),
            Source::JsonApi(j) => j.describe(),
            Source::WebPage(p) => p.describe(),
        }
    }

    /// Credentials for an OCI token request (`username`, token), when the source is a container image.
    pub fn registry_credentials(&self, secrets: &Secrets) -> Option<(String, String)> {
        match self {
            Source::Container(c) => {
                let token = secrets.get(catalog::TOKEN)?.trim().to_string();
                (!token.is_empty()).then(|| (c.username.clone().unwrap_or_else(|| "token".into()), token))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_builtin_kind_parses_and_plans() {
        let cases = [
            SourceConfig::new("folder", &[("path", r"\\server\builds")]),
            SourceConfig::new("web-directory", &[("url", "https://dl.example.org/builds/")]),
            SourceConfig::new("feed", &[("url", "https://example.org/releases.atom")]),
            SourceConfig::new("github", &[("repo", "tauri-apps/tauri")]),
            SourceConfig::new("gitea", &[("server", "https://gitea.com"), ("repo", "gitea/act_runner")]),
            SourceConfig::new("gitlab", &[("project", "gitlab-org/gitlab-runner")]),
            SourceConfig::new("container", &[("image", "ghcr.io/owner/app"), ("username", "me")]),
            SourceConfig::new("package", &[("registry", "npm"), ("package", "left-pad")]),
            SourceConfig::new(
                "json-api",
                &[("url", "https://api.example.org/v"), ("items", "$[*]"), ("version", "$.v")],
            ),
            SourceConfig::new("web-page", &[("url", "https://example.org/dl"), ("selector", "b")]),
            SourceConfig::new(
                "web-page",
                &[("url", "https://example.org/app"), ("rendered", "true"), ("wait", "#v")],
            ),
        ];
        let secrets: Secrets = [("token".to_string(), " t ".to_string())].into();
        for cfg in cases {
            let s = Source::parse(&cfg).unwrap_or_else(|e| panic!("{}: {e:?}", cfg.kind));
            assert!(!s.describe().is_empty());
            let plan = s.plan(&secrets);
            match (&s, plan) {
                (Source::Folder(_), Plan::Folder(_)) => assert!(s.secret_keys().is_empty()),
                (_, Plan::Http { request, .. }) => assert!(request.url.starts_with("https://"), "{}", request.url),
                (Source::WebPage(_), Plan::Render { url, .. }) => assert!(url.starts_with("https://")),
                other => panic!("unexpected plan {other:?}"),
            }
        }
    }

    #[test]
    fn parsers_read_each_kind_of_answer() {
        let feed = br#"<rss><channel><item><title>App 1.2.0</title><link>l</link></item></channel></rss>"#;
        assert_eq!(Parser::Feed.parse(feed).unwrap().len(), 1);
        let web = WebDirectorySource::from_config(&Config::new().with("url", "https://a.b/d/")).unwrap();
        let html = br#"<a href="app-1.0.0.zip">app-1.0.0.zip</a>"#;
        assert_eq!(Parser::Listing(web).parse(html).unwrap().len(), 1);
        let json = JsonApiSource::from_config(&Config::new().with("url", "https://a.b/v").with("version", "$.v")).unwrap();
        assert_eq!(Parser::Json(json).parse(br#"[{"v":"1.0"}]"#).unwrap().len(), 1);
        let page = WebPageSource::from_config(&Config::new().with("url", "https://a.b/")).unwrap();
        assert_eq!(Parser::Page(page).parse(b"<p>1.0.0</p>").unwrap().len(), 1);
        let forge = ForgeSource::from_config(Forge::GitHub, &Config::new().with("repo", "o/r")).unwrap();
        assert!(Parser::Forge(forge).parse(b"[]").unwrap().is_empty());
        let image = ContainerSource::from_config(&Config::new().with("image", "nginx")).unwrap();
        assert!(Parser::Tags(image).parse(br#"{"tags":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn secret_keys_follow_the_kind() {
        let token = |kind: &str, values: &[(&str, &str)]| Source::parse(&SourceConfig::new(kind, values)).unwrap().secret_keys();
        assert_eq!(token("github", &[("repo", "o/r")]), ["token"]);
        assert_eq!(token("container", &[("image", "nginx")]), ["token"]);
        assert!(token("web-page", &[("url", "https://a.b/")]).is_empty());
    }

    #[test]
    fn bad_configs_list_their_problems() {
        assert!(Source::parse(&SourceConfig::new("folder", &[])).unwrap_err()[0].contains("required"));
        assert!(Source::parse(&SourceConfig::new("teleport", &[])).unwrap_err()[0].contains("not a kind"));
        let big = SourceConfig::new("folder", &[("path", &"x".repeat(5000))]);
        assert!(Source::parse(&big).unwrap_err()[0].contains("too large"));
        let bad_repo = SourceConfig::new("github", &[("repo", "no-slash")]);
        assert!(Source::parse(&bad_repo).is_err());
        assert!(Source::parse(&SourceConfig::new("feed", &[("url", "ftp://x")])).is_err());
    }

    #[test]
    fn registry_credentials_only_for_images_with_a_token() {
        let img = Source::parse(&SourceConfig::new("container", &[("image", "nginx")])).unwrap();
        assert_eq!(img.registry_credentials(&Secrets::new()), None);
        let with: Secrets = [("token".to_string(), "t".to_string())].into();
        assert_eq!(img.registry_credentials(&with), Some(("token".into(), "t".into())));
        let feed = Source::parse(&SourceConfig::new("feed", &[("url", "https://a.b/f")])).unwrap();
        assert_eq!(feed.registry_credentials(&with), None);
        assert!(feed.secret_keys().is_empty());
    }

    #[test]
    fn stored_values_accept_scalars() {
        let cfg: SourceConfig = toml::from_str(
            "kind = \"x\"
[values]
a = \"t\"
b = 5
c = 1.5
d = true",
        )
        .unwrap();
        let v: Vec<&str> = cfg.values.values().map(String::as_str).collect();
        assert_eq!(v, ["t", "5", "1.5", "true"]);
        assert!(toml::from_str::<SourceConfig>(
            "kind = \"x\"
[values]
a = [1]"
        )
        .is_err());
        assert!(toml::from_str::<SourceConfig>("kind = \"x\"").unwrap().values.is_empty());
    }

    #[test]
    fn errors_carry_their_message() {
        for e in [SourceError::InvalidResponse("r".into()), SourceError::Unreachable("u".into())] {
            assert_eq!(e.message().len(), 1);
            assert_eq!(e.to_string(), e.message());
        }
    }
}
