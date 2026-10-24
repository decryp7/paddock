//! `manifest.json` of a report: what kind of zip it is, the format version, which app and build made it and when, what
//! redaction removed and every other file with its kind and size. The same list is what the preview shows.

use crate::personal::RedactionCounts;
use serde::{Deserialize, Serialize};

/// Name of the manifest inside the zip.
pub const MANIFEST_NAME: &str = "manifest.json";
/// `kind` of every report.
pub const MANIFEST_KIND: &str = "paddock-diagnostics";
/// The format version written.
pub const SCHEMA_VERSION: u32 = 1;

/// What a file of the report is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum FileKind {
    /// `manifest.json`.
    Manifest,
    /// The user's description.
    Description,
    /// The app, its build and the machine.
    SystemInfo,
    /// The app's settings, secrets masked.
    Settings,
    /// A piece of the app's log (CLEF JSON lines).
    Log,
    /// The marker a crashed run left.
    Crash,
    /// A file an app section contributed.
    App,
    /// A kind a later version wrote (read as this one).
    #[serde(other)]
    Other,
}

/// The app and build that made the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// The app's id (`spotter`).
    pub id: String,
    /// Its display name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// The build commit, or empty.
    pub commit: String,
}

impl AppInfo {
    /// The app `name` (`id`) at `version`, built from `commit` (may be empty).
    pub fn new(id: &str, name: &str, version: &str, commit: &str) -> Self {
        AppInfo {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            commit: commit.into(),
        }
    }
}

/// One file listed in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    /// Path inside the zip.
    pub name: String,
    /// What it is.
    pub kind: FileKind,
    /// Size in bytes (uncompressed).
    pub bytes: u64,
}

/// The manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Always [`MANIFEST_KIND`].
    pub kind: String,
    /// The format version.
    pub schema_version: u32,
    /// The app and build.
    pub app: AppInfo,
    /// RFC 3339, UTC.
    pub created_at: String,
    /// What redaction removed, by kind.
    pub redactions: RedactionCounts,
    /// What could not be collected, in words (a missing folder, an unreadable file).
    pub notes: Vec<String>,
    /// Every file but the manifest itself, in zip order.
    pub files: Vec<ManifestFile>,
}

/// Why a manifest was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Not JSON, or not a manifest.
    Malformed(String),
    /// A manifest of something else.
    OtherKind,
    /// Made by a newer format.
    Newer(u32),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Malformed(why) => write!(f, "its manifest is not valid ({why})"),
            ManifestError::OtherKind => f.write_str("it is not a Paddock diagnostics report"),
            ManifestError::Newer(v) => {
                write!(
                    f,
                    "it was made by a newer format (version {v}; this reads version {SCHEMA_VERSION}); update the app to read it"
                )
            }
        }
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    /// Parses a manifest once: a typed value or the reason it is refused (the way a reader such as Debrief opens a report).
    pub fn parse(json: &[u8]) -> Result<Manifest, ManifestError> {
        let manifest: Manifest =
            serde_json::from_slice(json).map_err(|e| ManifestError::Malformed(e.to_string()))?;
        if manifest.kind != MANIFEST_KIND {
            return Err(ManifestError::OtherKind);
        }
        if manifest.schema_version > SCHEMA_VERSION {
            return Err(ManifestError::Newer(manifest.schema_version));
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            kind: MANIFEST_KIND.into(),
            schema_version: SCHEMA_VERSION,
            app: AppInfo::new("spotter", "Spotter", "1.0.0", ""),
            created_at: "2026-10-24T10:00:00.000Z".into(),
            redactions: RedactionCounts::default(),
            notes: vec![],
            files: vec![ManifestFile {
                name: "a.txt".into(),
                kind: FileKind::App,
                bytes: 3,
            }],
        }
    }

    #[test]
    fn a_manifest_round_trips_and_refuses_what_it_should() {
        let json = serde_json::to_vec(&sample()).unwrap();
        assert_eq!(Manifest::parse(&json).unwrap(), sample());
        assert!(matches!(
            Manifest::parse(b"[]"),
            Err(ManifestError::Malformed(_))
        ));
        let mut other = sample();
        other.kind = "debrief-bundle".into();
        assert_eq!(
            Manifest::parse(&serde_json::to_vec(&other).unwrap()),
            Err(ManifestError::OtherKind)
        );
        let mut newer = sample();
        newer.schema_version = 9;
        let e = Manifest::parse(&serde_json::to_vec(&newer).unwrap()).unwrap_err();
        assert_eq!(e, ManifestError::Newer(9));
        assert!(e.to_string().contains("newer"));
        assert!(
            ManifestError::OtherKind
                .to_string()
                .contains("not a Paddock")
        );
        assert!(
            ManifestError::Malformed("x".into())
                .to_string()
                .contains("not valid")
        );
    }

    #[test]
    fn an_unknown_file_kind_is_tolerated() {
        let json = serde_json::to_string(&sample())
            .unwrap()
            .replace("\"app\",\"bytes\"", "\"hologram\",\"bytes\"");
        let json = if json.contains("hologram") {
            json
        } else {
            json.replace("\"kind\":\"app\"", "\"kind\":\"hologram\"")
        };
        assert_eq!(
            Manifest::parse(json.as_bytes()).unwrap().files[0].kind,
            FileKind::Other
        );
    }
}
