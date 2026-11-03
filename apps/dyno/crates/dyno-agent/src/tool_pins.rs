//! The pinned downloads of tools Dyno can fetch on a click (ADR 0027 section 7): read once from `tools/tool-pins.toml`, parsed into
//! typed pins at the edge, and checked against the helper's own table by a test so the two cannot drift. The OpenPGP signature of a
//! file is checked by `scripts/verify-tool-pin.sh` when a pin is added or changed, never by the app.

use dyno_core::helper::HelperHost;
use dyno_core::helper_requests::ToolKind;
use dyno_core::helper_requests::MAX_TOOL_BYTES;
use serde::Deserialize;

/// The pack, compiled in.
const PACK: &str = include_str!("../tools/tool-pins.toml");
/// The publisher's signing key the pins were verified against (confirmed by a person against smartmontools.org, 2026-11-03).
pub const SMARTMONTOOLS_KEY: &str = "0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5";

/// How a pinned file gets onto the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallVia {
    /// The helper's `InstallTool` (it holds its own copy of the pin).
    Helper,
    /// Dyno runs it from its own folder (no helper; not built yet).
    OwnCopy,
}

/// One pinned download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPin {
    /// Which tool.
    pub tool: ToolKind,
    /// Its version.
    pub version: String,
    /// The system it is for.
    pub host: HelperHost,
    /// The file's name.
    pub file: String,
    /// Where it is fetched (https).
    pub url: String,
    /// Its exact size in bytes.
    pub size: u64,
    /// Its SHA-256, lower-case hex.
    pub sha256: String,
    /// The only hosts the download may contact.
    pub hosts: Vec<String>,
    /// The licence, shown to the user.
    pub licence: String,
    /// Where the source is.
    pub source_url: String,
    /// When the signature was verified (`yyyy-mm-dd`).
    pub verified: String,
    /// How it is installed.
    pub install: InstallVia,
}

#[derive(Deserialize)]
struct Pack {
    tool: Vec<Raw>,
}

#[derive(Deserialize)]
struct Raw {
    id: String,
    version: String,
    os: String,
    file: String,
    url: String,
    size: u64,
    sha256: String,
    hosts: Vec<String>,
    licence: String,
    source_url: String,
    key_fingerprint: String,
    verified: String,
    #[serde(default = "helper")]
    install: String,
}

fn helper() -> String {
    "helper".into()
}

/// Why a pack was not accepted (a developer error: the pack is compiled in).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("tool-pins.toml: {0}")]
pub struct PackError(pub String);

fn bad<T>(m: impl Into<String>) -> Result<T, PackError> {
    Err(PackError(m.into()))
}

fn host_of(url: &str) -> Option<&str> {
    url.strip_prefix("https://")?
        .split(['/', ':', '?', '#'])
        .next()
        .filter(|h| !h.is_empty())
}

/// Parses and checks a pack: https addresses on listed hosts, sizes within what the helper accepts, SHA-256 as 64 hex digits, a
/// verification date, and the confirmed signing key.
pub fn parse(text: &str) -> Result<Vec<ToolPin>, PackError> {
    let pack: Pack = toml::from_str(text).map_err(|e| PackError(e.to_string()))?;
    let mut out = Vec::new();
    for r in pack.tool {
        let tool = match r.id.as_str() {
            "smartmontools" => ToolKind::Smartmontools,
            other => return bad(format!("unknown tool {other:?}")),
        };
        let host = match r.os.as_str() {
            "windows" => HelperHost::Windows,
            "macos" => HelperHost::Macos,
            "linux" => HelperHost::Linux,
            other => return bad(format!("unknown system {other:?}")),
        };
        let Some(h) = host_of(&r.url) else {
            return bad(format!("{}: the address must be https", r.file));
        };
        if !r.hosts.iter().any(|x| x == h) {
            return bad(format!("{}: {h} is not in its host list", r.file));
        }
        if r.size == 0 || r.size > MAX_TOOL_BYTES as u64 {
            return bad(format!("{}: size {} is outside 1..={MAX_TOOL_BYTES}", r.file, r.size));
        }
        if r.sha256.len() != 64 || !r.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return bad(format!("{}: sha256 must be 64 lower-case hex digits", r.file));
        }
        if r.key_fingerprint.replace(' ', "") != SMARTMONTOOLS_KEY {
            return bad(format!("{}: the key fingerprint is not the confirmed one", r.file));
        }
        if r.verified.len() != 10 || r.verified.matches('-').count() != 2 {
            return bad(format!("{}: verified must be a yyyy-mm-dd date", r.file));
        }
        if !r.url.ends_with(&r.file) {
            return bad(format!("{}: the address does not end in the file name", r.file));
        }
        let install = match r.install.as_str() {
            "helper" => InstallVia::Helper,
            "own-copy" => InstallVia::OwnCopy,
            other => return bad(format!("{}: unknown install route {other:?}", r.file)),
        };
        out.push(ToolPin {
            tool,
            version: r.version,
            host,
            file: r.file,
            url: r.url,
            size: r.size,
            sha256: r.sha256,
            hosts: r.hosts,
            licence: r.licence,
            source_url: r.source_url,
            verified: r.verified,
            install,
        });
    }
    Ok(out)
}

/// The pins of this build.
pub fn pins() -> Vec<ToolPin> {
    parse(PACK).unwrap_or_default()
}

/// The pin for `tool` on `host`.
pub fn pin_for(tool: ToolKind, host: HelperHost) -> Option<ToolPin> {
    pins().into_iter().find(|p| p.tool == tool && p.host == host)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[[tool]]
id = "smartmontools"
version = "7.5"
os = "windows"
file = "x.exe"
url = "https://github.com/x/x.exe"
size = 10
sha256 = "896337fcc253220614cf8cdbd5cf2321c5aa326a37a04160a672a281e6104c70"
hosts = ["github.com"]
licence = "GPL-2.0-or-later"
source_url = "https://www.smartmontools.org"
key_fingerprint = "0C95 77FD 2C4C FCB4 B9A5 9964 0A30 812E FF3A EFF5"
verified = "2026-11-03"
"#;

    #[test]
    fn the_pack_in_the_tree_parses_and_matches_the_helpers_own_table() {
        let p = pin_for(ToolKind::Smartmontools, HelperHost::Windows).expect("a Windows pin");
        let h = dyno_platform::tool_install::pin_for(ToolKind::Smartmontools, HelperHost::Windows).expect("the helper's pin");
        assert_eq!(
            (p.version.as_str(), p.file.as_str(), p.size, p.sha256.as_str()),
            (h.version, h.file, h.size, h.sha256)
        );
        assert!(p.url.ends_with(&p.file) && p.url.starts_with("https://github.com/smartmontools/"));
        assert!(
            pin_for(ToolKind::Smartmontools, HelperHost::Linux).is_none(),
            "no upstream Linux binary"
        );
    }

    #[test]
    fn a_good_pack_is_accepted() {
        assert_eq!(parse(GOOD).unwrap().len(), 1);
    }

    #[test]
    fn every_way_a_pin_can_be_wrong_is_refused() {
        let cases = [
            ("https://github.com/x/x.exe", "http://github.com/x/x.exe", "https"),
            ("https://github.com/x/x.exe", "https://evil.example/x.exe", "host list"),
            ("https://github.com/x/x.exe", "https://github.com/x/y.exe", "file name"),
            ("size = 10", "size = 0", "outside"),
            ("size = 10", "size = 99999999", "outside"),
            ("896337fcc253220614cf8cdbd5cf2321c5aa326a37a04160a672a281e6104c70", "ABC", "sha256"),
            ("0C95 77FD 2C4C FCB4 B9A5 9964 0A30 812E FF3A EFF5", "0000 0000", "confirmed"),
            ("verified = \"2026-11-03\"", "verified = \"yesterday\"", "date"),
            ("id = \"smartmontools\"", "id = \"nmap\"", "unknown tool"),
            ("os = \"windows\"", "os = \"plan9\"", "unknown system"),
            (
                "verified = \"2026-11-03\"",
                "verified = \"2026-11-03\"\ninstall = \"magic\"",
                "install route",
            ),
        ];
        for (from, to, word) in cases {
            let e = parse(&GOOD.replace(from, to)).unwrap_err();
            assert!(e.0.contains(word), "{e} should mention {word}");
        }
        assert!(parse("not toml [").is_err());
    }
}
