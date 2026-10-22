//! The pinned Chromium manifest: platform to archive URL and SHA-256, parsed once into typed values.

use crate::{Error, Result};
use serde::Deserialize;

const EMBEDDED: &str = include_str!("chromium-manifest.toml");

/// The manifest file.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The pinned Chromium version label.
    pub version: String,
    /// One entry per platform.
    #[serde(default)]
    pub platform: Vec<Entry>,
}

/// One platform's raw entry (may still be a placeholder).
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    /// `<os>-<arch>`.
    pub key: String,
    /// Archive address.
    pub url: String,
    /// Lowercase or uppercase hex SHA-256 of the archive.
    pub sha256: String,
}

/// A fully configured, validated entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    /// Version label from the manifest.
    pub version: String,
    /// Archive address (https, or http to loopback for tests).
    pub url: String,
    /// Lowercase hex SHA-256.
    pub sha256: String,
}

/// `<os>-<arch>` of this build.
pub fn platform_key() -> String {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
    format!("{os}-{arch}")
}

impl Manifest {
    /// The manifest shipped inside this crate.
    pub fn embedded() -> Manifest {
        // The file is part of the build and covered by a test; fall back to an empty manifest rather than panic.
        Manifest::parse(EMBEDDED).unwrap_or(Manifest {
            version: "UNCONFIGURED".into(),
            platform: Vec::new(),
        })
    }

    /// Parses manifest TOML.
    pub fn parse(text: &str) -> Result<Manifest> {
        toml::from_str(text).map_err(|e| Error::Install(format!("the Chromium manifest is invalid: {e}")))
    }

    /// The validated entry for `key`, or a message saying what is missing.
    pub fn pinned(&self, key: &str) -> Result<Pinned> {
        let not_configured = || {
            Error::Install(
                "the pinned Chromium build is not configured yet in this version of Spotter; install Edge, Chrome or Chromium instead"
                    .into(),
            )
        };
        let e = self.platform.iter().find(|e| e.key == key).ok_or_else(|| {
            Error::Install(format!(
                "there is no pinned Chromium build for {key}; install Edge, Chrome or Chromium instead"
            ))
        })?;
        if !valid_sha256(&e.sha256) || !valid_url(&e.url) {
            return Err(not_configured());
        }
        Ok(Pinned {
            version: self.version.clone(),
            url: e.url.clone(),
            sha256: e.sha256.to_ascii_lowercase(),
        })
    }

    /// Whether this platform can download a Chromium at all.
    pub fn is_configured(&self, key: &str) -> bool {
        self.pinned(key).is_ok()
    }
}

fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// https anywhere; plain http only to the loopback (local test servers).
pub fn valid_url(u: &str) -> bool {
    u.starts_with("https://") && u.len() > 12
        || u.strip_prefix("http://")
            .is_some_and(|r| r.starts_with("127.0.0.1:") || r.starts_with("localhost:") || r.starts_with("[::1]:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_manifest_pins_every_platform_to_its_versioned_https_url() {
        let m = Manifest::embedded();
        assert!(
            m.version.split('.').count() == 4 && m.version.split('.').all(|p| p.parse::<u32>().is_ok()),
            "{}",
            m.version
        );
        let keys: Vec<&str> = m.platform.iter().map(|e| e.key.as_str()).collect();
        for want in ["windows-x86_64", "macos-aarch64", "macos-x86_64", "linux-x86_64"] {
            assert!(keys.contains(&want), "{want} missing");
        }
        for e in &m.platform {
            let p = m.pinned(&e.key).unwrap_or_else(|err| panic!("{}: {err}", e.key));
            assert!(
                p.url.starts_with("https://storage.googleapis.com/chrome-for-testing-public/"),
                "{}",
                p.url
            );
            assert!(
                p.url.contains(&format!("/{}/", m.version)),
                "the URL names the pinned version: {}",
                p.url
            );
            assert_eq!(p.sha256.len(), 64);
        }
        let mut unique: Vec<&str> = m.platform.iter().map(|e| e.sha256.as_str()).collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), m.platform.len(), "every archive has its own hash");
        assert!(m.is_configured("macos-aarch64"));
        assert!(!m.is_configured("plan9-mips"));
    }

    #[test]
    fn configured_entries_validate() {
        let h = "AB".repeat(32);
        let t = format!("version = \"1\"\n[[platform]]\nkey=\"k\"\nurl=\"https://dl.test/a.zip\"\nsha256=\"{h}\"\n");
        let p = Manifest::parse(&t).unwrap().pinned("k").unwrap();
        assert_eq!(p.sha256, "ab".repeat(32));
        assert_eq!(p.version, "1");
        assert!(Manifest::parse(&t)
            .unwrap()
            .pinned("other")
            .unwrap_err()
            .to_string()
            .contains("no pinned"));
        let http = t.replace("https://dl.test", "http://dl.test");
        assert!(Manifest::parse(&http).unwrap().pinned("k").is_err());
        let local = t.replace("https://dl.test", "http://127.0.0.1:99");
        assert!(Manifest::parse(&local).unwrap().pinned("k").is_ok());
        assert!(Manifest::parse(&t.replace(&h, "abc")).unwrap().pinned("k").is_err());
        assert!(Manifest::parse("version = ").is_err());
    }

    #[test]
    fn platform_key_shape() {
        let k = platform_key();
        assert!(k.contains('-'));
    }
}
