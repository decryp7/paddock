use crate::platform::Platform;
use crate::{Result, RuntimeError};
use paddock_download::Sha256Digest;
use serde::Deserialize;
use std::collections::HashSet;

/// The catalogue format this build reads.
pub const FORMAT: u32 = 1;
/// The largest catalogue text accepted.
pub const MAX_CATALOGUE_BYTES: usize = 64 * 1024;
/// The most platform entries in one catalogue.
pub const MAX_PLATFORMS: usize = 32;
/// The largest archive a catalogue may pin.
pub const MAX_ARCHIVE_BYTES: u64 = 4 << 30;

/// How a pinned archive is packed. Only zip today (Chrome for Testing, FFmpeg and llama.cpp builds for Windows and macOS);
/// a catalogue that pins another kind is refused, so the store never meets one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalogue {
    format: u32,
    name: String,
    version: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    platform: Vec<RawEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    key: String,
    url: String,
    size: u64,
    sha256: String,
}

/// One runtime's pinned builds, parsed and checked once.
///
/// ```toml
/// format = 1
/// name = "chromium"                   # the runtime: its store folder and its messages
/// version = "156.0.8078.47"           # the pinned version: the release folder
/// source = "Chrome for Testing"       # where the builds come from (shown in About and diagnostics)
/// license = "BSD-3-Clause"
///
/// [[platform]]
/// key = "macos-aarch64"               # <windows|macos|linux>-<x86_64|aarch64>
/// url = "https://storage.googleapis.com/chrome-for-testing-public/156.0.8078.47/mac-arm64/chrome-mac-arm64.zip"
/// size = 162345678                    # bytes of the archive
/// sha256 = "ad7f2b49..."              # of the archive, lowercase hex
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    pub name: String,
    pub version: String,
    pub source: String,
    pub license: String,
    entries: Vec<Pinned>,
}

/// One platform's build: everything an install needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    /// The runtime ("chromium").
    pub name: String,
    pub version: String,
    pub platform: Platform,
    /// `https://`, or `http://` to the loopback (local test servers).
    pub url: String,
    /// Bytes of the archive.
    pub size: u64,
    pub sha256: Sha256Digest,
    pub archive: ArchiveKind,
    /// The archive's file name (the last part of the URL), used for the download.
    pub file_name: String,
}

/// A name usable as a folder on every OS: lowercase ASCII letters, digits, `.`, `-` and `_`, 1 to 64 characters, not
/// starting with `.` and without `..`.
pub fn is_folder_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('.')
        && !s.contains("..")
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        })
}

fn bad(why: impl Into<String>) -> RuntimeError {
    RuntimeError::Catalogue(why.into())
}

/// `https://<host>/...`, or `http://` to 127.0.0.1, localhost or `[::1]`.
fn url_ok(u: &str) -> bool {
    let rest = if let Some(r) = u.strip_prefix("https://") {
        r
    } else if let Some(r) = u.strip_prefix("http://") {
        if !["127.0.0.1:", "localhost:", "[::1]:"]
            .iter()
            .any(|h| r.starts_with(h))
        {
            return false;
        }
        r
    } else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    !host.is_empty() && rest.len() > host.len() && !u.contains(['?', '#', ' ', '\\'])
}

/// The archive name at the end of `url` and its kind.
fn archive_of(url: &str) -> Option<(String, ArchiveKind)> {
    let name = url.rsplit('/').next()?;
    let plain = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    (plain && name.to_ascii_lowercase().ends_with(".zip"))
        .then(|| (name.to_string(), ArchiveKind::Zip))
}

impl Catalogue {
    /// Parses and checks a catalogue. Any problem (an unknown key, a version or name that is not a folder name, a URL that
    /// is not https, a missing or malformed checksum, a size of 0 or over [`MAX_ARCHIVE_BYTES`], an archive kind the store
    /// cannot unpack, a platform listed twice or unknown) refuses the whole catalogue with a reason.
    pub fn parse(text: &str) -> Result<Catalogue> {
        if text.len() > MAX_CATALOGUE_BYTES {
            return Err(bad(format!(
                "it is larger than {MAX_CATALOGUE_BYTES} bytes"
            )));
        }
        let raw: RawCatalogue = toml::from_str(text).map_err(|e| bad(e.message().to_string()))?;
        if raw.format != FORMAT {
            return Err(bad(format!(
                "format {} is not supported (this build reads {FORMAT})",
                raw.format
            )));
        }
        if !is_folder_name(&raw.name) {
            return Err(bad(format!(
                "the name '{}' is not a valid folder name",
                raw.name
            )));
        }
        if !is_folder_name(&raw.version) {
            return Err(bad(format!(
                "the version '{}' is not a valid folder name",
                raw.version
            )));
        }
        if raw.platform.len() > MAX_PLATFORMS {
            return Err(bad(format!("it lists more than {MAX_PLATFORMS} platforms")));
        }
        let mut seen = HashSet::new();
        let mut entries = Vec::with_capacity(raw.platform.len());
        for e in raw.platform {
            let platform = Platform::parse(&e.key)
                .ok_or_else(|| bad(format!("'{}' is not a platform key", e.key)))?;
            if !seen.insert(platform) {
                return Err(bad(format!("{} is listed twice", e.key)));
            }
            if !url_ok(&e.url) {
                return Err(bad(format!(
                    "the URL for {} is not an https address",
                    e.key
                )));
            }
            let (file_name, archive) = archive_of(&e.url)
                .ok_or_else(|| bad(format!("the archive for {} is not a .zip file", e.key)))?;
            if e.size == 0 || e.size > MAX_ARCHIVE_BYTES {
                return Err(bad(format!("the size for {} is out of range", e.key)));
            }
            let sha256 = (e.sha256.len() == 64)
                .then(|| Sha256Digest::from_hex(&e.sha256).ok())
                .flatten()
                .ok_or_else(|| bad(format!("the SHA-256 for {} is not 64 hex digits", e.key)))?;
            entries.push(Pinned {
                name: raw.name.clone(),
                version: raw.version.clone(),
                platform,
                url: e.url,
                size: e.size,
                sha256,
                archive,
                file_name,
            });
        }
        Ok(Catalogue {
            name: raw.name,
            version: raw.version,
            source: raw.source,
            license: raw.license,
            entries,
        })
    }

    /// Every pinned build, in catalogue order.
    pub fn entries(&self) -> &[Pinned] {
        &self.entries
    }

    /// The build for `platform`.
    pub fn pinned(&self, platform: Platform) -> Result<&Pinned> {
        self.entries
            .iter()
            .find(|p| p.platform == platform)
            .ok_or_else(|| RuntimeError::NotPinned {
                name: self.name.clone(),
                platform: platform.key(),
            })
    }

    /// The build for the platform this app runs on.
    pub fn for_this_platform(&self) -> Result<&Pinned> {
        self.for_platform(Platform::current())
    }

    fn for_platform(&self, platform: Option<Platform>) -> Result<&Pinned> {
        let unknown = || RuntimeError::NotPinned {
            name: self.name.clone(),
            platform: "this platform".into(),
        };
        platform.ok_or_else(unknown).and_then(|p| self.pinned(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "ad7f2b49a4dc679af286b1b1b386670d93ca7868079f69012d21ba0ac8293df3";

    fn cat(entry: &str) -> String {
        format!(
            "format = 1\nname = \"chromium\"\nversion = \"1.2.3\"\nsource = \"s\"\nlicense = \"l\"\n{entry}"
        )
    }

    fn entry(key: &str, url: &str, size: u64, sha: &str) -> String {
        format!(
            "[[platform]]\nkey = \"{key}\"\nurl = \"{url}\"\nsize = {size}\nsha256 = \"{sha}\"\n"
        )
    }

    #[test]
    fn a_valid_catalogue_gives_typed_entries() {
        let text = cat(&(entry(
            "macos-aarch64",
            "https://dl.test/1.2.3/chrome-mac.zip",
            10,
            HASH,
        ) + &entry(
            "linux-x86_64",
            "http://127.0.0.1:9/a/b.ZIP",
            5,
            &HASH.to_uppercase(),
        )));
        let c = Catalogue::parse(&text).unwrap();
        assert_eq!(
            (
                c.name.as_str(),
                c.version.as_str(),
                c.source.as_str(),
                c.license.as_str()
            ),
            ("chromium", "1.2.3", "s", "l")
        );
        assert_eq!(c.entries().len(), 2);
        let p = c.pinned(Platform::parse("macos-aarch64").unwrap()).unwrap();
        assert_eq!(
            (p.file_name.as_str(), p.size, p.archive),
            ("chrome-mac.zip", 10, ArchiveKind::Zip)
        );
        assert_eq!(p.sha256.to_hex(), HASH);
        assert_eq!(
            c.pinned(Platform::parse("linux-x86_64").unwrap())
                .unwrap()
                .sha256
                .to_hex(),
            HASH
        );
        let e = c
            .pinned(Platform::parse("windows-x86_64").unwrap())
            .unwrap_err()
            .to_string();
        assert!(e.contains("no chromium build for windows-x86_64"), "{e}");
    }

    #[test]
    fn the_build_for_this_platform_is_found_or_named() {
        let all: String = ["windows", "macos", "linux"]
            .iter()
            .flat_map(|os| ["x86_64", "aarch64"].map(|a| format!("{os}-{a}")))
            .map(|k| entry(&k, &format!("https://dl.test/{k}.zip"), 1, HASH))
            .collect();
        let c = Catalogue::parse(&cat(&all)).unwrap();
        assert_eq!(c.for_this_platform().is_ok(), Platform::current().is_some());
        assert!(
            c.for_platform(None)
                .unwrap_err()
                .to_string()
                .contains("this platform")
        );
        let none = Catalogue::parse(&cat("")).unwrap();
        assert!(matches!(
            none.for_this_platform(),
            Err(RuntimeError::NotPinned { .. })
        ));
    }

    #[test]
    fn every_problem_refuses_the_whole_catalogue_with_a_reason() {
        let ok = |k: &str| entry(k, "https://dl.test/a.zip", 1, HASH);
        let cases: Vec<(String, &str)> = vec![
            (cat(&ok("plan9-x86_64")), "not a platform key"),
            (
                cat(&(ok("linux-x86_64") + &ok("linux-x86_64"))),
                "listed twice",
            ),
            (
                cat(&entry("linux-x86_64", "ftp://dl.test/a.zip", 1, HASH)),
                "not an https",
            ),
            (
                cat(&entry("linux-x86_64", "http://dl.test/a.zip", 1, HASH)),
                "not an https",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test", 1, HASH)),
                "not an https",
            ),
            (
                cat(&entry("linux-x86_64", "https:///a.zip", 1, HASH)),
                "not an https",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test/a.zip?x=1", 1, HASH)),
                "not an https",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test/a.tar.xz", 1, HASH)),
                "not a .zip",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test/.zip", 1, HASH)),
                "not a .zip",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test/a%20b.zip", 1, HASH)),
                "not a .zip",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test/", 1, HASH)),
                "not a .zip",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test/a.zip", 0, HASH)),
                "out of range",
            ),
            (
                cat(&entry(
                    "linux-x86_64",
                    "https://dl.test/a.zip",
                    MAX_ARCHIVE_BYTES + 1,
                    HASH,
                )),
                "out of range",
            ),
            (
                cat(&entry("linux-x86_64", "https://dl.test/a.zip", 1, "abc")),
                "64 hex",
            ),
            (
                cat(&entry(
                    "linux-x86_64",
                    "https://dl.test/a.zip",
                    1,
                    &"g".repeat(64),
                )),
                "64 hex",
            ),
            (cat("extra = 1\n"), "unknown field"),
            (
                "format = 2\nname = \"x\"\nversion = \"1\"\n".into(),
                "format 2",
            ),
            (
                "format = 1\nname = \"X\"\nversion = \"1\"\n".into(),
                "name 'X'",
            ),
            (
                "format = 1\nname = \"x\"\nversion = \"../1\"\n".into(),
                "version '../1'",
            ),
            ("format = ".into(), ""),
            (" ".repeat(MAX_CATALOGUE_BYTES + 1), "larger than"),
        ];
        for (text, want) in cases {
            let e = Catalogue::parse(&text).unwrap_err();
            assert!(matches!(e, RuntimeError::Catalogue(_)));
            assert!(e.to_string().contains(want), "{want}: {e}");
        }
        let many: String = (0..=MAX_PLATFORMS).map(|_| ok("linux-x86_64")).collect();
        assert!(
            Catalogue::parse(&cat(&many))
                .unwrap_err()
                .to_string()
                .contains("more than")
        );
    }

    #[test]
    fn folder_names() {
        for good in ["chromium", "156.0.8078.47", "autobuild-2026_10", "a"] {
            assert!(is_folder_name(good), "{good}");
        }
        for no in ["", ".hidden", "a..b", "A", "a/b", "a b", &"x".repeat(65)] {
            assert!(!is_folder_name(no), "{no}");
        }
    }
}
