//! A pinned release and its assets per platform, each with its size and SHA-256 (the FFmpeg runtime and the research
//! browser's download are described this way, ADR 0070 D5). Nothing that is not listed is ever downloaded.

use super::builds::is_build_id;
use serde::{Deserialize, Serialize};

/// One downloadable file of a build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// One build (variant) for one platform: `cpu`, `vulkan`, `cuda-13`, `metal`, ...
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBuild {
    pub id: String,
    /// The llama-server archive first, then runtimes unpacked next to it (CUDA's cudart).
    pub assets: Vec<RuntimeAsset>,
}

impl RuntimeBuild {
    pub fn download_bytes(&self) -> u64 {
        self.assets.iter().map(|a| a.size).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RuntimeCatalog {
    pub version: u32,
    /// The release name (e.g. `v0.7.0`).
    pub release: String,
    /// The build tag the assets belong to (e.g. `b11669`); also the folder name on disk.
    pub tag: String,
    #[serde(default)]
    pub commit: String,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub source: String,
    pub platforms: std::collections::BTreeMap<String, Vec<RuntimeBuild>>,
}

/// The catalog key of this platform (`windows-x64`, `linux-x64`, `macos-arm64`, ...).
pub fn current_platform() -> String {
    platform_key(std::env::consts::OS, std::env::consts::ARCH)
}

pub fn platform_key(os: &str, arch: &str) -> String {
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

fn valid_asset(a: &RuntimeAsset) -> bool {
    let safe_name = !a.name.is_empty() && a.name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    let hex = a.sha256.len() == 64 && a.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    let archive = a.name.ends_with(".zip") || a.name.ends_with(".tar.gz");
    safe_name && hex && archive && a.size > 0 && a.url.starts_with("https://github.com/ggml-org/llama.cpp/releases/download/")
}

impl RuntimeCatalog {
    /// Parses and validates a catalog; invalid builds are dropped.
    pub fn parse(json: &str) -> Result<RuntimeCatalog, String> {
        let mut c: RuntimeCatalog = serde_json::from_str(json).map_err(|e| format!("the runtime catalog is not valid: {e}"))?;
        if c.version != 1 {
            return Err(format!("unsupported runtime catalog version {}", c.version));
        }
        if !is_build_id(&c.tag) {
            return Err(format!("invalid release tag {:?}", c.tag));
        }
        for builds in c.platforms.values_mut() {
            builds.retain(|b| is_build_id(&b.id) && !b.assets.is_empty() && b.assets.iter().all(valid_asset));
        }
        Ok(c)
    }

    /// The builds for `platform` (empty for an unsupported one).
    pub fn builds(&self, platform: &str) -> &[RuntimeBuild] {
        self.platforms.get(platform).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn find(&self, platform: &str, id: &str) -> Option<&RuntimeBuild> {
        self.builds(platform).iter().find(|b| b.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_entries_are_dropped_or_refused() {
        let good = r#"{"name":"llama-b1-bin-win-cpu-x64.zip","url":"https://github.com/ggml-org/llama.cpp/releases/download/b1/llama-b1-bin-win-cpu-x64.zip","size":10,"sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;
        let bad_url = good.replace("https://github.com/ggml-org", "https://evil.example");
        let bad_hash = good.replace("\"0000", "\"zz00");
        let json = format!(
            r#"{{"version":1,"release":"v1","tag":"b1","platforms":{{"windows-x64":[{{"id":"cpu","assets":[{good}]}},{{"id":"x","assets":[{bad_url}]}},{{"id":"y","assets":[{bad_hash}]}},{{"id":"../z","assets":[{good}]}},{{"id":"none","assets":[]}}]}}}}"#
        );
        let c = RuntimeCatalog::parse(&json).unwrap();
        assert_eq!(c.builds("windows-x64").len(), 1);
        assert!(RuntimeCatalog::parse("{").is_err());
        assert!(RuntimeCatalog::parse(r#"{"version":2,"release":"","tag":"b1","platforms":{}}"#).is_err());
        assert!(RuntimeCatalog::parse(r#"{"version":1,"release":"","tag":"../x","platforms":{}}"#).is_err());
    }

    #[test]
    fn platform_keys() {
        assert_eq!(platform_key("windows", "x86_64"), "windows-x64");
        assert_eq!(platform_key("macos", "aarch64"), "macos-arm64");
        assert_eq!(platform_key("linux", "riscv64"), "linux-riscv64");
        assert!(!current_platform().is_empty());
    }
}
