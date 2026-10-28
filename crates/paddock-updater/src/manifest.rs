//! `latest.json`, the release information a release publishes (the format of Tauri's updater):
//!
//! ```json
//! { "version": "1.2.0", "notes": "...", "pub_date": "2026-10-17T12:00:00Z",
//!   "platforms": { "windows-x86_64-nsis": { "url": "https://...", "signature": "...", "size": 41234567 } } }
//! ```
//!
//! Parsed once into typed, bounded values; anything malformed is refused before a download starts. `size` is Paddock's
//! addition (Tauri ignores it): with it the download is size-pinned and the free space is checked before connecting.
//! Parsing never panics, whatever the input.

use base64::Engine;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A `latest.json` larger than this is not release information.
pub const MAX_MANIFEST_BYTES: usize = 1_000_000;
/// Release notes beyond this many characters are cut (with an ellipsis).
pub const MAX_NOTES_CHARS: usize = 20_000;
const MAX_PLATFORMS: usize = 64;
const MAX_URL_LEN: usize = 2048;
const MAX_SIGNATURE_LEN: usize = 8192;

/// Validated release information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: semver::Version,
    /// Plain text, control characters removed (except new lines and tabs), bounded.
    pub notes: String,
    /// RFC 3339, as published.
    pub pub_date: Option<String>,
    pub platforms: BTreeMap<String, Platform>,
    /// The document as received, for the installer (Tauri's updater restores its update from it and verifies again).
    pub raw: Value,
}

/// One platform's package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub url: String,
    /// Base64 of a minisign signature file.
    pub signature: String,
    pub size: Option<u64>,
}

/// The system an update is for: Tauri's target keys `{os}-{arch}` and `{os}-{arch}-{installer}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `linux`, `darwin` or `windows`.
    pub os: String,
    /// `x86_64`, `aarch64`, `i686` or `armv7`.
    pub arch: String,
    /// The installer kind of this build (`nsis`, `msi`, `appimage`, `deb`, `rpm`, `app`), when known.
    pub installer: Option<String>,
}

impl Target {
    /// This build's system (`installer` is what the app was bundled as, when it knows).
    pub fn current(installer: Option<&str>) -> Target {
        let rename =
            |name: &'static str, from: &str, to: &'static str| if name == from { to } else { name };
        let os = rename(std::env::consts::OS, "macos", "darwin");
        let arch = rename(
            rename(std::env::consts::ARCH, "x86", "i686"),
            "arm",
            "armv7",
        );
        Target {
            os: os.into(),
            arch: arch.into(),
            installer: installer.map(str::to_ascii_lowercase),
        }
    }

    /// The keys looked up, most specific first.
    pub fn keys(&self) -> Vec<String> {
        let base = format!("{}-{}", self.os, self.arch);
        match &self.installer {
            Some(i) => vec![format!("{base}-{i}"), base],
            None => vec![base],
        }
    }
}

impl Manifest {
    /// The package for `target`, with the key that matched; `Err` names the keys looked for.
    pub fn select(&self, target: &Target) -> Result<(&str, &Platform), String> {
        let keys = target.keys();
        keys.iter()
            .find_map(|k| self.platforms.get_key_value(k.as_str()))
            .map(|(k, p)| (k.as_str(), p))
            .ok_or_else(|| format!("the release has no package for {}", keys.join(" or ")))
    }
}

/// Parses `latest.json` text.
pub fn parse(text: &str) -> Result<Manifest, String> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "the release information is larger than {}",
            paddock_units::format_whole(MAX_MANIFEST_BYTES as u64, paddock_units::Kind::Storage)
        ));
    }
    let value: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    from_value(value)
}

/// Validates release information already parsed as JSON.
pub fn from_value(value: Value) -> Result<Manifest, String> {
    let root = value.as_object().ok_or("expected a JSON object")?;
    let version = root
        .get("version")
        .and_then(Value::as_str)
        .ok_or("missing \"version\"")?;
    let version = semver::Version::parse(version.strip_prefix('v').unwrap_or(version))
        .map_err(|e| format!("\"version\" is not a semantic version: {e}"))?;
    let notes = match root.get("notes") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => sanitize_notes(s),
        Some(_) => return Err("\"notes\" must be text".into()),
    };
    let pub_date = match root.get("pub_date") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if is_rfc3339(s) => Some(s.clone()),
        Some(Value::String(_)) => return Err("\"pub_date\" is not an RFC 3339 date".into()),
        Some(_) => return Err("\"pub_date\" must be text".into()),
    };
    let platforms = root
        .get("platforms")
        .and_then(Value::as_object)
        .ok_or("missing \"platforms\"")?;
    if platforms.is_empty() {
        return Err("\"platforms\" is empty".into());
    }
    if platforms.len() > MAX_PLATFORMS {
        return Err(format!("more than {MAX_PLATFORMS} platforms"));
    }
    let platforms = platforms
        .iter()
        .map(|(key, p)| {
            platform(key, p)
                .map(|p| (key.clone(), p))
                .map_err(|e| format!("platform \"{}\": {e}", clip(key)))
        })
        .collect::<Result<_, _>>()?;
    Ok(Manifest {
        version,
        notes,
        pub_date,
        platforms,
        raw: value,
    })
}

fn platform(key: &str, value: &Value) -> Result<Platform, String> {
    let key_ok = !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !key_ok {
        return Err("invalid platform name".into());
    }
    let p: &Map<String, Value> = value.as_object().ok_or("expected an object")?;
    let url = p
        .get("url")
        .and_then(Value::as_str)
        .ok_or("missing \"url\"")?;
    if url.len() > MAX_URL_LEN {
        return Err("\"url\" is too long".into());
    }
    if !url
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        return Err("\"url\" must use https".into());
    }
    paddock_download::host_of(url).map_err(|e| format!("\"url\": {e}"))?;
    let signature = p
        .get("signature")
        .and_then(Value::as_str)
        .ok_or("missing \"signature\"")?;
    check_signature(signature)?;
    let size = match p.get("size") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .ok_or("\"size\" must be a whole number of bytes")?,
        ),
    };
    Ok(Platform {
        url: url.to_string(),
        signature: signature.trim().to_string(),
        size,
    })
}

/// A Tauri updater signature is the base64 of a minisign signature file. [`crate::signature`] verifies it; this only
/// rejects what cannot be one.
fn check_signature(signature: &str) -> Result<(), String> {
    if signature.trim().is_empty() {
        return Err("\"signature\" is empty".into());
    }
    if signature.len() > MAX_SIGNATURE_LEN {
        return Err("\"signature\" is too long".into());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .map_err(|_| "\"signature\" is not base64")?;
    let text =
        String::from_utf8(decoded).map_err(|_| "\"signature\" is not a minisign signature")?;
    if !text.starts_with("untrusted comment:") {
        return Err("\"signature\" is not a minisign signature".into());
    }
    Ok(())
}

/// Release notes as plain text: control characters other than new lines and tabs removed, at most [`MAX_NOTES_CHARS`]
/// characters.
pub fn sanitize_notes(notes: &str) -> String {
    let mut out = String::new();
    for (count, c) in notes
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .enumerate()
    {
        if count == MAX_NOTES_CHARS {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// `YYYY-MM-DDTHH:MM:SS` followed by optional fractions and `Z` or an offset: the shape of an RFC 3339 date-time.
fn is_rfc3339(s: &str) -> bool {
    let b = s.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let at = |i: usize, c: u8| b.get(i) == Some(&c);
    let head = digits(0..4)
        && at(4, b'-')
        && digits(5..7)
        && at(7, b'-')
        && digits(8..10)
        && (at(10, b'T') || at(10, b't') || at(10, b' '))
        && digits(11..13)
        && at(13, b':')
        && digits(14..16)
        && at(16, b':')
        && digits(17..19);
    if !head || s.len() > 40 {
        return false;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return false;
        }
        rest = &frac[n..];
    }
    match rest.as_bytes() {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => [h1, h2, m1, m2].iter().all(|d| d.is_ascii_digit()),
        _ => false,
    }
}

fn clip(s: &str) -> String {
    s.chars().take(40).collect()
}
