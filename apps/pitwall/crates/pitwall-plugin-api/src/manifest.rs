//! The plugin manifest, `pitwall-plugin.toml` next to the plugin's executable: who the plugin is, what kinds of
//! contribution it makes, what it asks for. Parsed once, at the edge, into a [`Manifest`] whose fields are valid
//! (parse, don't validate); the host reads it before it starts anything, and the plugin embeds the same file
//! (`include_str!`) so both sides agree.
//!
//! ```toml
//! id = "redact-filter"          # a-z 0-9 . - (also the plugin's folder name)
//! name = "Redact filter"
//! version = "0.1.0"
//! contract = "1.0"              # the plugin contract it was built for (CONTRACT_VERSION)
//! kinds = ["filter"]            # engine | endpoint | tool | filter
//! executable = "pitwall-redact-filter"   # in the plugin's folder; ".exe" is added on Windows
//! memory_mb = 256               # optional: the memory cap it asks for
//!
//! [permissions]
//! network = false               # optional, default false; granted only if the user enables it too
//! ```

use crate::ContractVersion;
use serde::{Deserialize, Serialize};

/// The manifest's file name in a plugin's folder.
pub const MANIFEST_FILE: &str = "pitwall-plugin.toml";
/// Largest manifest read.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
/// Longest plugin id.
pub const MAX_ID_LEN: usize = 48;
/// Most memory a plugin may ask for (64 GiB: an engine holding a large model).
pub const MAX_MEMORY_MB: u64 = 64 * 1024;
/// Most arguments a manifest may pass to its executable.
pub const MAX_ARGS: usize = 16;

/// What a plugin contributes (it may make several kinds of contribution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Runs models: load, unload, generate (a stream of tokens).
    Engine,
    /// Serves extra HTTP endpoints under `/plugins/<id>/...`.
    Endpoint,
    /// Offers tools a model or the user can invoke.
    Tool,
    /// Sees (and may change or reject) requests before they reach the model, and responses after.
    Filter,
}

impl Kind {
    /// The name used in manifests.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Engine => "engine",
            Kind::Endpoint => "endpoint",
            Kind::Tool => "tool",
            Kind::Filter => "filter",
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a plugin asks for beyond its sandbox.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Permissions {
    /// Open network connections. Off by default; the host grants it only when the user enabled it as well.
    pub network: bool,
}

/// A valid manifest (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// The plugin's id: 1–48 characters from `a–z 0–9 . -`, starting with a letter or digit.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The plugin's own version.
    pub version: String,
    /// The plugin contract it was built for.
    pub contract: ContractVersion,
    /// What it contributes (at least one, no repeats, sorted).
    pub kinds: Vec<Kind>,
    /// Its executable's file name in its folder (no path; `.exe` is added on Windows when missing).
    pub executable: String,
    /// Arguments for the executable.
    pub args: Vec<String>,
    /// What it asks for.
    pub permissions: Permissions,
    /// The memory cap it asks for, in MiB (`None`: the host's default).
    pub memory_mb: Option<u64>,
    /// One line about what it does.
    pub description: String,
}

/// The manifest as written (every key optional, so a missing one is a clear error instead of a parse failure).
#[derive(Debug, Deserialize)]
struct RawManifest {
    id: Option<String>,
    name: Option<String>,
    version: Option<String>,
    contract: Option<String>,
    kinds: Option<Vec<String>>,
    executable: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    permissions: Permissions,
    memory_mb: Option<u64>,
    #[serde(default)]
    description: String,
}

/// Why a manifest is not valid; the message says what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{MANIFEST_FILE}: {}", self.0)
    }
}

impl std::error::Error for ManifestError {}

fn bad(m: impl Into<String>) -> ManifestError {
    ManifestError(m.into())
}

/// Whether `id` is a valid plugin id.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && !id.contains("..")
}

/// Whether `name` is a plain file name (no folders, no `..`, nothing hidden).
fn plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn text(field: &str, v: Option<String>, max: usize) -> Result<String, ManifestError> {
    let v = v.map(|s| s.trim().to_string()).unwrap_or_default();
    if v.is_empty() {
        return Err(bad(format!("`{field}` is missing")));
    }
    if v.chars().count() > max || v.chars().any(char::is_control) {
        return Err(bad(format!("`{field}` must be one line of at most {max} characters")));
    }
    Ok(v)
}

impl Manifest {
    /// Parses and validates a manifest's text.
    pub fn parse(source: &str) -> Result<Manifest, ManifestError> {
        if source.len() > MAX_MANIFEST_BYTES {
            return Err(bad(format!("larger than {MAX_MANIFEST_BYTES} bytes")));
        }
        let raw: RawManifest = toml::from_str(source).map_err(|e| bad(e.message().to_string()))?;
        let id = text("id", raw.id, MAX_ID_LEN)?;
        if !valid_id(&id) {
            return Err(bad(format!(
                "`id` \"{id}\" may only use a-z, 0-9, '.' and '-' (at most {MAX_ID_LEN}, starting with a letter or digit)"
            )));
        }
        let name = text("name", raw.name, 80)?;
        let version = text("version", raw.version, 32)?;
        let contract: ContractVersion = text("contract", raw.contract, 16)?.parse().map_err(bad)?;
        let mut kinds = Vec::new();
        for k in raw.kinds.unwrap_or_default() {
            let kind = match k.as_str() {
                "engine" => Kind::Engine,
                "endpoint" => Kind::Endpoint,
                "tool" => Kind::Tool,
                "filter" => Kind::Filter,
                other => return Err(bad(format!("unknown kind \"{other}\" (engine, endpoint, tool or filter)"))),
            };
            if kinds.contains(&kind) {
                return Err(bad(format!("kind \"{kind}\" is listed twice")));
            }
            kinds.push(kind);
        }
        if kinds.is_empty() {
            return Err(bad("`kinds` must name at least one of engine, endpoint, tool, filter"));
        }
        kinds.sort();
        let executable = text("executable", raw.executable, 128)?;
        if !plain_file_name(&executable) {
            return Err(bad(format!(
                "`executable` \"{executable}\" must be a file name in the plugin's folder (letters, digits, '-', '_', '.')"
            )));
        }
        if raw.args.len() > MAX_ARGS || raw.args.iter().any(|a| a.len() > 256 || a.contains('\0')) {
            return Err(bad(format!("`args` may hold at most {MAX_ARGS} arguments of at most 256 bytes")));
        }
        if let Some(mb) = raw.memory_mb {
            if mb == 0 || mb > MAX_MEMORY_MB {
                return Err(bad(format!("`memory_mb` must be between 1 and {MAX_MEMORY_MB}")));
            }
        }
        let description = raw.description.trim().to_string();
        if description.chars().count() > 300 {
            return Err(bad("`description` must be at most 300 characters"));
        }
        Ok(Manifest {
            id,
            name,
            version,
            contract,
            kinds,
            executable,
            args: raw.args,
            permissions: raw.permissions,
            memory_mb: raw.memory_mb,
            description,
        })
    }

    /// Whether the plugin makes contributions of `kind`.
    pub fn has(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }

    /// The executable's file name on this platform (`.exe` added on Windows when missing).
    pub fn executable_file(&self) -> String {
        let suffix = std::env::consts::EXE_SUFFIX;
        if suffix.is_empty() || self.executable.ends_with(suffix) {
            self.executable.clone()
        } else {
            format!("{}{suffix}", self.executable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = r#"
id = "redact-filter"
name = "Redact filter"
version = "0.1.0"
contract = "1.0"
kinds = ["filter", "tool"]
executable = "pitwall-redact-filter"
memory_mb = 128
description = "Removes secrets"
unknown_future_key = 1

[permissions]
network = false
"#;

    #[test]
    fn a_valid_manifest_parses_into_typed_values() {
        let m = Manifest::parse(OK).unwrap();
        assert_eq!(m.id, "redact-filter");
        assert_eq!(m.contract, ContractVersion::new(1, 0));
        assert_eq!(m.kinds, vec![Kind::Tool, Kind::Filter], "sorted");
        assert!(m.has(Kind::Filter) && !m.has(Kind::Engine));
        assert!(!m.permissions.network);
        assert_eq!(m.memory_mb, Some(128));
        if cfg!(windows) {
            assert_eq!(m.executable_file(), "pitwall-redact-filter.exe");
        } else {
            assert_eq!(m.executable_file(), "pitwall-redact-filter");
        }
        // Defaults: no network, no memory request.
        let minimal = "id='a'\nname='A'\nversion='1'\ncontract='1.0'\nkinds=['engine']\nexecutable='a.exe'";
        let m = Manifest::parse(minimal).unwrap();
        assert!(!m.permissions.network && m.memory_mb.is_none() && m.args.is_empty());
        assert_eq!(m.executable_file(), "a.exe");
        assert_eq!(Kind::Endpoint.to_string(), "endpoint");
    }

    #[test]
    fn invalid_manifests_say_what_to_fix() {
        let with = |k: &str, v: &str| {
            let mut lines: Vec<String> = OK
                .lines()
                .filter(|l| !l.starts_with(&format!("{k} ")))
                .map(String::from)
                .collect();
            if !v.is_empty() {
                lines.insert(1, format!("{k} = {v}"));
            }
            Manifest::parse(&lines.join("\n")).unwrap_err().to_string()
        };
        assert!(with("id", "").contains("`id` is missing"));
        assert!(with("id", "\"Bad Id\"").contains("may only use"));
        assert!(with("id", "\"../x\"").contains("may only use"));
        assert!(with("id", "\"a..b\"").contains("may only use"));
        assert!(with("id", &format!("\"{}\"", "a".repeat(49))).contains("at most"));
        assert!(with("contract", "\"one\"").contains("contract version"));
        assert!(with("kinds", "[\"magic\"]").contains("unknown kind"));
        assert!(with("kinds", "[\"tool\", \"tool\"]").contains("twice"));
        assert!(with("kinds", "[]").contains("at least one"));
        assert!(with("executable", "\"../evil\"").contains("file name"));
        assert!(with("executable", "\"C:\\\\x.exe\"").contains("file name"));
        assert!(with("executable", "\".hidden\"").contains("file name"));
        assert!(with("memory_mb", "0").contains("memory_mb"));
        assert!(with("memory_mb", "999999999").contains("memory_mb"));
        assert!(with("name", "\"a\\nb\"").contains("one line"));
        assert!(with("description", &format!("\"{}\"", "d".repeat(301))).contains("description"));
        assert!(with("args", &format!("[{}]", vec!["\"a\""; 17].join(","))).contains("args"));
        assert!(Manifest::parse("not toml = = =").is_err());
        assert!(Manifest::parse(&"#".repeat(MAX_MANIFEST_BYTES + 1)).unwrap_err().0.contains("larger"));
        let e = ManifestError("x".into());
        assert_eq!(e.to_string(), "pitwall-plugin.toml: x");
    }

    #[test]
    fn ids() {
        for ok in ["a", "redact-filter", "net.decryptology.x", "9lives"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "-a", ".a", "A", "a b", "a/b", "a\\b", "é", "a..b"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }
}
