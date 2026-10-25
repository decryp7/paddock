//! The validated names a manifest uses. Each is a newtype whose only constructor checks the value, and whose
//! `Deserialize` goes through that constructor, so a value of the type is valid wherever it came from (a manifest file, a
//! JSON description or a postcard frame) and nothing downstream re-checks it. [`GrantRef`] is `paddock_view`'s: the view
//! model and the manifest share the one type.

pub use paddock_view::{GrantRef, MAX_NAME, valid_ident};

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Why a name was refused (says what to fix).
fn refuse(what: &str, value: &str, rule: &str) -> String {
    let shown: String = value.chars().take(40).collect();
    format!("{what} \"{shown}\" is not valid: {rule}")
}

/// Longest host pattern (a DNS name).
pub const MAX_HOST: usize = 253;

macro_rules! name_type {
    ($(#[$doc:meta])* $name:ident, $what:expr, $rule:expr, $check:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Checks and wraps `value`.
            pub fn new(value: impl Into<String>) -> Result<$name, String> {
                let value = value.into();
                let check: fn(&str) -> bool = $check;
                if check(&value) { Ok($name(value)) } else { Err(refuse($what, &value, $rule)) }
            }

            /// The value.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, String> {
                $name::new(value)
            }
        }

        impl From<$name> for String {
            fn from(n: $name) -> String {
                n.0
            }
        }

        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, String> {
                $name::new(s)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

name_type!(
    /// A plugin's reverse-DNS id, unique among plugins (`com.example.kv-lines`): 3–128 characters from `A–Z a–z 0–9 . - _`,
    /// with a dot, not starting or ending with one.
    PluginId,
    "plugin id",
    "use 3-128 characters from A-Z, a-z, 0-9, '.', '-', '_' in reverse-DNS style (com.example.name)",
    |s| (3..=128).contains(&s.len())
        && s.contains('.')
        && !s.starts_with('.')
        && !s.ends_with('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
);

name_type!(
    /// The name of a program a plugin may run (`relog`), never a path: the app maps the name to a program it vetted.
    ProgramRef,
    "program name",
    "use 1-64 characters from A-Z, a-z, 0-9, '.', '-', '_'; it names a program, it is not a path",
    valid_ident
);

name_type!(
    /// A host a plugin may connect to: a lowercase DNS name (`example.com`) or a subdomain wildcard (`*.example.com`);
    /// no scheme, path or port.
    HostPattern,
    "host pattern",
    "use a lowercase host name such as example.com, or *.example.com for its subdomains (no scheme, path or port)",
    valid_host
);

fn valid_host(s: &str) -> bool {
    let name = s.strip_prefix("*.").unwrap_or(s);
    !name.is_empty()
        && s.len() <= MAX_HOST
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

impl HostPattern {
    /// Whether this pattern allows connecting to `host` (`*.example.com` matches subdomains, not `example.com` itself).
    pub fn matches(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        match self.0.strip_prefix("*.") {
            Some(base) => {
                host.len() > base.len() + 1
                    && host.ends_with(base)
                    && host[..host.len() - base.len()].ends_with('.')
            }
            None => host == self.0,
        }
    }
}

/// An app contract the plugin speaks, as `ns/minor` (`debrief.import/3`): the platform routes `Extension { ns, minor, body }`
/// calls by `ns` and never decodes `body`. The namespace name carries the contract's major version
/// (`debrief.import2` is a different contract than `debrief.import`); `minor` follows the same rule as
/// [`crate::ProtocolVersion::accepts`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "RawNamespace")]
pub struct Namespace {
    ns: String,
    /// The contract's minor version the plugin was built for.
    pub minor: u16,
}

#[derive(Deserialize)]
struct RawNamespace {
    ns: String,
    minor: u16,
}

impl TryFrom<RawNamespace> for Namespace {
    type Error = String;
    fn try_from(r: RawNamespace) -> Result<Self, String> {
        Namespace::new(r.ns, r.minor)
    }
}

impl Namespace {
    /// Checks the name (1–64 characters from `a–z 0–9 . - _`, starting with a letter) and wraps it.
    pub fn new(ns: impl Into<String>, minor: u16) -> Result<Namespace, String> {
        let ns = ns.into();
        let ok = (1..=MAX_NAME).contains(&ns.len())
            && ns.starts_with(|c: char| c.is_ascii_lowercase())
            && ns
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_".contains(c));
        if ok {
            Ok(Namespace { ns, minor })
        } else {
            Err(refuse(
                "contract namespace",
                &ns,
                "use 1-64 lowercase letters, digits, '.', '-', '_', starting with a letter (debrief.import)",
            ))
        }
    }

    /// The namespace name (`debrief.import`).
    pub fn ns(&self) -> &str {
        &self.ns
    }
}

impl FromStr for Namespace {
    type Err = String;

    /// Parses `ns/minor`.
    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("\"{s}\" is not a contract like debrief.import/3 (namespace/minor)");
        let (ns, minor) = s.split_once('/').ok_or_else(bad)?;
        if minor.is_empty() || !minor.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        Namespace::new(ns, minor.parse().map_err(|_| bad())?)
    }
}

impl std::fmt::Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.ns, self.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_ids_are_reverse_dns() {
        for ok in ["com.example.a_b-c", "a.b", "net.decryptology.registry"] {
            assert!(PluginId::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "a",
            "nodots",
            ".a.b",
            "a.b.",
            "a b.c",
            "../evil.x",
            "é.x",
            &format!("a.{}", "b".repeat(130)),
        ] {
            assert!(PluginId::new(bad).is_err(), "{bad}");
        }
        let id: PluginId = "com.example.x".parse().unwrap();
        assert_eq!(
            (id.as_str(), id.to_string(), id.as_ref()),
            ("com.example.x", "com.example.x".into(), "com.example.x")
        );
        assert_eq!(String::from(id), "com.example.x");
    }

    #[test]
    fn grants_and_programs_are_names_not_paths() {
        for ok in ["symbols", "Symbol-Folder_2", "relog.exe"] {
            assert!(
                GrantRef::new(ok).is_ok() && ProgramRef::new(ok).is_ok(),
                "{ok}"
            );
        }
        for bad in ["", "a/b", "..", "-x", "C:\\x", "a b", &"x".repeat(65)] {
            assert!(
                GrantRef::new(bad).is_err() && ProgramRef::new(bad).is_err(),
                "{bad}"
            );
        }
        let e = GrantRef::new("/etc/passwd").unwrap_err();
        assert!(e.contains("not a path"), "{e}");
    }

    #[test]
    fn host_patterns_match_hosts_and_subdomains() {
        for ok in ["example.com", "*.example.com", "localhost", "a-b.c1.io"] {
            assert!(HostPattern::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "*",
            "*.",
            "http://x.com",
            "x.com/path",
            "x.com:80",
            "Example.com",
            "-a.com",
            "a..com",
            "a_b.com",
            "*.*.com",
            &"a".repeat(64),
        ] {
            assert!(HostPattern::new(bad).is_err(), "{bad}");
        }
        assert!(
            HostPattern::new(format!("{}.com", "a.".repeat(130))).is_err(),
            "too long"
        );
        let exact = HostPattern::new("example.com").unwrap();
        assert!(
            exact.matches("example.com")
                && exact.matches("EXAMPLE.com")
                && !exact.matches("a.example.com")
        );
        let wild = HostPattern::new("*.example.com").unwrap();
        assert!(wild.matches("a.example.com") && wild.matches("a.b.example.com"));
        assert!(
            !wild.matches("example.com")
                && !wild.matches("badexample.com")
                && !wild.matches(".example.com")
        );
    }

    #[test]
    fn namespaces_parse_display_and_validate() {
        let ns: Namespace = "debrief.import/3".parse().unwrap();
        assert_eq!(
            (ns.ns(), ns.minor, ns.to_string()),
            ("debrief.import", 3, "debrief.import/3".into())
        );
        for bad in [
            "debrief.import",
            "debrief.import/",
            "debrief.import/x",
            "/3",
            "Debrief/3",
            "1a/3",
            "a/-1",
            "a/70000",
            "a b/1",
        ] {
            assert!(bad.parse::<Namespace>().is_err(), "{bad}");
        }
        assert!(Namespace::new("x".repeat(65), 0).is_err());
    }

    #[test]
    fn deserializing_revalidates_every_name() {
        assert!(serde_json::from_str::<PluginId>("\"nodots\"").is_err());
        assert!(serde_json::from_str::<GrantRef>("\"../x\"").is_err());
        assert!(serde_json::from_str::<HostPattern>("\"http://x\"").is_err());
        assert!(serde_json::from_str::<Namespace>(r#"{"ns":"Bad","minor":1}"#).is_err());
        // The same through postcard: a frame cannot smuggle an invalid name in.
        let bad = postcard::to_stdvec(&"../x").unwrap();
        assert!(postcard::from_bytes::<GrantRef>(&bad).is_err());
    }

    #[test]
    fn names_roundtrip_through_postcard_and_json() {
        let id = PluginId::new("com.example.x").unwrap();
        let grant = GrantRef::new("symbols").unwrap();
        let ns = Namespace::new("debrief.import", 3).unwrap();
        let host = HostPattern::new("*.example.com").unwrap();
        let program = ProgramRef::new("relog").unwrap();
        let all = (id, grant, ns, host, program);
        let bytes = postcard::to_stdvec(&all).unwrap();
        assert_eq!(
            postcard::from_bytes::<(PluginId, GrantRef, Namespace, HostPattern, ProgramRef)>(
                &bytes
            )
            .unwrap(),
            all
        );
        let json = serde_json::to_string(&all).unwrap();
        assert_eq!(
            serde_json::from_str::<(PluginId, GrantRef, Namespace, HostPattern, ProgramRef)>(&json)
                .unwrap(),
            all
        );
    }
}
