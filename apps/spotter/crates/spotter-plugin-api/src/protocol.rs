//! The plugin protocol (ADR 0003): the messages Spotter and a plugin exchange, and the protocol version.
//!
//! The conversation: the plugin sends [`Message::Hello`] as soon as it starts; the host answers
//! [`Message::Welcome`] (or [`Message::Refused`] and closes); then the host sends requests one at a time and the
//! plugin answers each with a response carrying the same id. The host ends with [`Request::Shutdown`] or by
//! closing the pipe.
//!
//! The bodies are postcard, which is not self-describing: these types use no `untagged`, `flatten`, internal tags
//! or `skip_serializing_if`. Changing them is a protocol change ([`PROTOCOL`]): add variants at the end (a minor
//! bump) or bump the major version.

use crate::schema::{Config, FieldSpec};
use serde::{Deserialize, Serialize};

/// The first bytes of a [`Message::Hello`], so the host recognises a plugin (and not some other program writing to
/// its output).
pub const MAGIC: [u8; 4] = *b"SPTR";

/// The protocol this SDK speaks.
pub const PROTOCOL: ProtocolVersion = ProtocolVersion::new(1, 0);

/// Most releases one list answer may carry (the host also caps what it keeps).
pub const MAX_RELEASES: usize = 2000;

/// A protocol version: plugins built for the same major version and an older or equal minor version work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Incompatible changes.
    pub major: u16,
    /// Compatible additions (variants added at the end of an enum).
    pub minor: u16,
}

impl ProtocolVersion {
    /// `major.minor`.
    pub const fn new(major: u16, minor: u16) -> ProtocolVersion {
        ProtocolVersion { major, minor }
    }

    /// Whether a host speaking `self` can talk to a plugin built for `plugin`.
    pub fn accepts(self, plugin: ProtocolVersion) -> bool {
        self.major == plugin.major && plugin.minor <= self.minor
    }

    /// Why a host speaking `self` refuses the plugin `name` built for `plugin`, saying what to update; `None` when
    /// [`ProtocolVersion::accepts`] holds.
    pub fn mismatch(self, plugin: ProtocolVersion, name: &str) -> Option<String> {
        if self.accepts(plugin) {
            None
        } else if plugin.major < self.major {
            Some(format!(
                "The plugin \"{name}\" is too old (protocol {plugin}; this Spotter speaks {self}). Install a version of the plugin built for protocol {}.x.",
                self.major
            ))
        } else {
            Some(format!(
                "The plugin \"{name}\" needs a newer Spotter (protocol {plugin}; this one speaks {self}). Update Spotter."
            ))
        }
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// One frame's body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    /// The plugin's first message.
    Hello {
        /// Always [`MAGIC`].
        magic: [u8; 4],
        /// The frame format ([`crate::frame::FRAME_VERSION`]).
        frame: u16,
        /// The protocol the plugin was built for.
        protocol: ProtocolVersion,
    },
    /// The host accepts the plugin.
    Welcome {
        /// The protocol the host speaks.
        protocol: ProtocolVersion,
    },
    /// The host refuses the plugin, and why; the host closes the pipe after it.
    Refused {
        /// Why.
        reason: String,
    },
    /// A request from the host.
    Request {
        /// Echoed in the response.
        id: u32,
        /// What is asked.
        request: Request,
    },
    /// The plugin's answer.
    Response {
        /// The request's id.
        id: u32,
        /// The answer.
        response: Response,
    },
}

/// What the host asks a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// The plugin's name, version and the source kinds it offers.
    Describe,
    /// The releases a source of `kind` configured with `config` has now, at most `limit`, newest first if the
    /// source knows an order. Secret fields carry their value (from the credential store) only in this request.
    List {
        /// One of the plugin's [`SourceKindSpec::id`]s.
        kind: String,
        /// The entered values.
        config: Config,
        /// Most releases wanted.
        limit: u32,
    },
    /// The host is done; the plugin exits.
    Shutdown,
}

/// A plugin's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    /// Answer to [`Request::Describe`].
    Described(Manifest),
    /// Answer to [`Request::List`].
    Listed(Vec<WireRelease>),
    /// The request failed.
    Failed(PluginError),
    /// Answer to [`Request::Shutdown`].
    Bye,
}

/// Who a plugin is and what it offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Stable id (`[a-z0-9-]+`), also the prefix of its source kinds in watches.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The plugin's own version.
    pub version: String,
    /// The source kinds it offers.
    pub kinds: Vec<SourceKindSpec>,
}

impl Manifest {
    /// A manifest without kinds.
    pub fn new(id: &str, name: &str, version: &str) -> Manifest {
        Manifest {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            kinds: Vec::new(),
        }
    }

    /// The same manifest with one more kind.
    pub fn with_kind(mut self, kind: SourceKindSpec) -> Manifest {
        self.kinds.push(kind);
        self
    }
}

/// A source kind: an id, how it is shown and its configuration form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceKindSpec {
    /// Stable id (`[a-z0-9-]+`) unique within the plugin.
    pub id: String,
    /// Display name.
    pub label: String,
    /// One sentence about it.
    pub description: String,
    /// The form.
    pub fields: Vec<FieldSpec>,
}

impl SourceKindSpec {
    /// A kind without fields.
    pub fn new(id: &str, label: &str, description: &str) -> SourceKindSpec {
        SourceKindSpec {
            id: id.into(),
            label: label.into(),
            description: description.into(),
            fields: Vec::new(),
        }
    }

    /// The same kind with one more field.
    pub fn with_field(mut self, field: FieldSpec) -> SourceKindSpec {
        self.fields.push(field);
        self
    }
}

/// One release as a plugin reports it. The host parses `version` with the watch's version scheme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRelease {
    /// The text the version is read from (a tag, a file name, a version string).
    pub version: String,
    /// The release's display name.
    pub name: String,
    /// Where it is: a URL or a path.
    pub location: String,
    /// When it was published, in milliseconds since the Unix epoch.
    pub published_at_ms: Option<i64>,
    /// Size in bytes.
    pub size: Option<u64>,
    /// SHA-256 of the file at `location`, as hex.
    pub sha256: Option<String>,
    /// Whether the source marks it as a pre-release.
    pub prerelease: bool,
}

impl WireRelease {
    /// A release with only version text, name and location.
    pub fn new(version: &str, name: &str, location: &str) -> WireRelease {
        WireRelease {
            version: version.into(),
            name: name.into(),
            location: location.into(),
            published_at_ms: None,
            size: None,
            sha256: None,
            prerelease: false,
        }
    }
}

/// Why a request failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginError {
    /// The class of failure, which decides how the app shows it.
    pub kind: ErrorKind,
    /// What happened, for the user.
    pub message: String,
}

impl PluginError {
    /// An error.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> PluginError {
        PluginError {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PluginError {}

/// Classes of failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    /// The place could not be reached (network down, share offline); checks back off and retry.
    Unreachable,
    /// The credentials were refused or are missing.
    Unauthorized,
    /// The place exists but the thing asked for does not.
    NotFound,
    /// The configuration is not valid.
    InvalidConfig,
    /// The request is not supported (e.g. an unknown kind).
    Unsupported,
    /// Anything else.
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_versions_say_which_side_to_update() {
        let host = ProtocolVersion::new(2, 3);
        assert!(host.accepts(ProtocolVersion::new(2, 0)) && host.accepts(host));
        assert!(!host.accepts(ProtocolVersion::new(2, 4)) && !host.accepts(ProtocolVersion::new(1, 9)));
        assert_eq!(host.mismatch(ProtocolVersion::new(2, 1), "p"), None);
        let old = host.mismatch(ProtocolVersion::new(1, 7), "x").unwrap();
        assert!(old.contains("too old") && old.contains("1.7") && old.contains("2.x"), "{old}");
        let new = host.mismatch(ProtocolVersion::new(3, 0), "x").unwrap();
        assert!(new.contains("newer Spotter"), "{new}");
        assert_eq!(PROTOCOL.to_string(), "1.0");
    }

    #[test]
    fn builders_and_errors() {
        let m = Manifest::new("p", "P", "1").with_kind(SourceKindSpec::new("k", "K", "d").with_field(crate::FieldSpec::new(
            "a",
            "A",
            crate::FieldKind::Text,
        )));
        assert_eq!(m.kinds[0].fields[0].key, "a");
        let e = PluginError::new(ErrorKind::NotFound, "gone");
        assert_eq!(e.to_string(), "gone");
        let r = WireRelease::new("1.0", "n", "l");
        assert!(!r.prerelease && r.size.is_none());
    }
}
