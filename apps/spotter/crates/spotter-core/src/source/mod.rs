//! Sources: where releases come from.
//!
//! A watch stores its source as a kind id and the values of that kind's form ([`SourceConfig`]); loading parses it
//! into a typed [`Source`] once. Checking a source is split the ports-and-adapters way: [`Source::plan`] says what
//! to read (a folder, an HTTP request, a plugin call) and how to parse the answer; an adapter performs the I/O
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

use crate::release::Candidate;
use serde::{Deserialize, Serialize};
use spotter_plugin_api::schema::check_all;
use spotter_plugin_api::{Config, ErrorKind, FieldKind, PluginError, SourceKindSpec};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

pub use container::ContainerSource;
pub use folder::{EntryKind, FolderEntry, FolderSource};
pub use forge::{Forge, ForgeSource};
pub use http::HttpRequest;
pub use json_api::JsonApiSource;
pub use listing::WebDirectorySource;

/// A source as stored: its kind and the values of the kind's form. Secret fields are never stored here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConfig {
    /// A built-in kind id (`folder`, `github`, …) or `<plugin>/<kind>` for a plugin's kind.
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

    /// The values as a plugin [`Config`].
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
    /// The plugin failed (crashed, timed out, broke the protocol, or is quarantined).
    Plugin(String),
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
            | SourceError::Plugin(m)
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

impl From<PluginError> for SourceError {
    fn from(e: PluginError) -> SourceError {
        match e.kind {
            ErrorKind::Unreachable => SourceError::Unreachable(e.message),
            ErrorKind::Unauthorized => SourceError::Unauthorized(e.message),
            ErrorKind::NotFound => SourceError::NotFound(e.message),
            ErrorKind::InvalidConfig | ErrorKind::Unsupported => SourceError::InvalidConfig(e.message),
            ErrorKind::Failed => SourceError::Failed(e.message),
        }
    }
}

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
    /// A plugin's source kind.
    Plugin {
        /// The plugin's id.
        plugin: String,
        /// The kind's id within the plugin.
        kind: String,
        /// The stored values.
        config: Config,
        /// Keys of its secret fields.
        secret_keys: Vec<String>,
    },
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
    /// Ask a plugin.
    Plugin {
        /// The plugin's id.
        plugin: String,
        /// The kind's id.
        kind: String,
        /// The values, secrets included.
        config: Config,
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
        }
    }
}

/// Finds a plugin's kind by its full id (`<plugin>/<kind>`).
pub trait PluginKinds {
    /// The kind's description, `None` when no enabled plugin offers it.
    fn kind(&self, id: &str) -> Option<SourceKindSpec>;
}

impl PluginKinds for () {
    fn kind(&self, _: &str) -> Option<SourceKindSpec> {
        None
    }
}

impl<F: Fn(&str) -> Option<SourceKindSpec>> PluginKinds for F {
    fn kind(&self, id: &str) -> Option<SourceKindSpec> {
        self(id)
    }
}

impl Source {
    /// Parses a stored source. Checks the values against the kind's form first; the error lists every problem.
    pub fn parse(cfg: &SourceConfig, plugins: &dyn PluginKinds) -> Result<Source, Vec<String>> {
        let config = cfg.config();
        if !config.within_limits() {
            return Err(vec!["The source settings are too large.".into()]);
        }
        if let Some((plugin, kind)) = cfg.kind.split_once('/') {
            let spec = plugins.kind(&cfg.kind).ok_or_else(|| {
                vec![format!(
                    "No installed plugin offers the source kind \"{}\". Install or enable the plugin \"{plugin}\".",
                    cfg.kind
                )]
            })?;
            check_all(&spec.fields, &config)?;
            let secret_keys = spec
                .fields
                .iter()
                .filter(|f| f.kind == FieldKind::Secret)
                .map(|f| f.key.clone())
                .collect();
            return Ok(Source::Plugin {
                plugin: plugin.into(),
                kind: kind.into(),
                config,
                secret_keys,
            });
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
            _ => one(JsonApiSource::from_config(&config).map(Source::JsonApi)),
        }
    }

    /// Keys of the secret fields whose values the check needs from the credential store.
    pub fn secret_keys(&self) -> Vec<String> {
        match self {
            Source::Forge(_) | Source::JsonApi(_) => vec![catalog::TOKEN.into()],
            Source::Container(_) => vec![catalog::TOKEN.into()],
            Source::Plugin { secret_keys, .. } => secret_keys.clone(),
            Source::Folder(_) | Source::WebDirectory(_) | Source::Feed { .. } => Vec::new(),
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
            Source::Plugin { plugin, kind, config, .. } => {
                let mut config = config.clone();
                for (k, v) in secrets {
                    config.0.insert(k.clone(), v.clone());
                }
                Plan::Plugin {
                    plugin: plugin.clone(),
                    kind: kind.clone(),
                    config,
                }
            }
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
            Source::Plugin { plugin, kind, .. } => format!("{plugin}/{kind}"),
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
    use spotter_plugin_api::FieldSpec;

    fn plugin_kinds(id: &str) -> Option<SourceKindSpec> {
        (id == "demo/files").then(|| {
            SourceKindSpec::new("files", "Files", "d")
                .with_field(FieldSpec::new("dir", "Dir", FieldKind::Text).required())
                .with_field(FieldSpec::new("key", "Key", FieldKind::Secret))
        })
    }

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
        ];
        let secrets: Secrets = [("token".to_string(), " t ".to_string())].into();
        for cfg in cases {
            let s = Source::parse(&cfg, &()).unwrap_or_else(|e| panic!("{}: {e:?}", cfg.kind));
            assert!(!s.describe().is_empty());
            let plan = s.plan(&secrets);
            match (&s, plan) {
                (Source::Folder(_), Plan::Folder(_)) => assert!(s.secret_keys().is_empty()),
                (_, Plan::Http { request, .. }) => assert!(request.url.starts_with("https://"), "{}", request.url),
                other => panic!("unexpected plan {other:?}"),
            }
        }
    }

    #[test]
    fn bad_configs_list_their_problems() {
        assert!(Source::parse(&SourceConfig::new("folder", &[]), &()).unwrap_err()[0].contains("required"));
        assert!(Source::parse(&SourceConfig::new("teleport", &[]), &()).unwrap_err()[0].contains("not a kind"));
        let big = SourceConfig::new("folder", &[("path", &"x".repeat(5000))]);
        assert!(Source::parse(&big, &()).unwrap_err()[0].contains("too large"));
        let bad_repo = SourceConfig::new("github", &[("repo", "no-slash")]);
        assert!(Source::parse(&bad_repo, &()).is_err());
        assert!(Source::parse(&SourceConfig::new("feed", &[("url", "ftp://x")]), &()).is_err());
    }

    #[test]
    fn plugin_kinds_parse_and_get_their_secrets() {
        let cfg = SourceConfig::new("demo/files", &[("dir", "/x")]);
        assert!(Source::parse(&cfg, &()).unwrap_err()[0].contains("Install or enable the plugin \"demo\""));
        let s = Source::parse(&cfg, &plugin_kinds).unwrap();
        assert_eq!(s.secret_keys(), ["key"]);
        assert_eq!(s.describe(), "demo/files");
        let Plan::Plugin { plugin, kind, config } = s.plan(&[("key".to_string(), "s3cret".to_string())].into()) else {
            panic!("plugin plan")
        };
        assert_eq!(
            (plugin.as_str(), kind.as_str(), config.get("key"), config.get("dir")),
            ("demo", "files", Some("s3cret"), Some("/x"))
        );
        assert!(Source::parse(&SourceConfig::new("demo/files", &[]), &plugin_kinds).is_err());
        assert_eq!(cfg.config().get("dir"), Some("/x"));
    }

    #[test]
    fn registry_credentials_only_for_images_with_a_token() {
        let img = Source::parse(&SourceConfig::new("container", &[("image", "nginx")]), &()).unwrap();
        assert_eq!(img.registry_credentials(&Secrets::new()), None);
        let with: Secrets = [("token".to_string(), "t".to_string())].into();
        assert_eq!(img.registry_credentials(&with), Some(("token".into(), "t".into())));
        let feed = Source::parse(&SourceConfig::new("feed", &[("url", "https://a.b/f")]), &()).unwrap();
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
    fn errors_carry_their_message_and_map_from_plugins() {
        let all = [
            (ErrorKind::Unreachable, "Unreachable"),
            (ErrorKind::Unauthorized, "Unauthorized"),
            (ErrorKind::NotFound, "NotFound"),
            (ErrorKind::InvalidConfig, "InvalidConfig"),
            (ErrorKind::Unsupported, "InvalidConfig"),
            (ErrorKind::Failed, "Failed"),
        ];
        for (kind, name) in all {
            let e = SourceError::from(PluginError::new(kind, "m"));
            assert!(format!("{e:?}").starts_with(name));
            assert_eq!(e.to_string(), "m");
        }
        for e in [SourceError::InvalidResponse("r".into()), SourceError::Plugin("p".into())] {
            assert_eq!(e.message().len(), 1);
        }
        assert_eq!(<() as PluginKinds>::kind(&(), "x"), None);
    }
}
