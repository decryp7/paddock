//! Where each runner kind is published: release discovery, asset names, checksum and signature locations.
//!
//! Researched 2026-10-22 (see `docs/adr/0002-runner-auto-update.md`):
//!
//! - **Gitea runner** (`gitea/runner`, formerly `gitea/act_runner`; binary `gitea-runner`, formerly `act_runner`).
//!   `https://dl.gitea.com/gitea-runner/version.json` gives the latest version (`{"latest":{"version":"5.2.0"}}`);
//!   binaries are `https://dl.gitea.com/gitea-runner/<v>/gitea-runner-<v>-<os>-<arch>[.exe]` with a `.sha256` next
//!   to each and a `checksums.txt`. The same files are attached to the gitea.com release `v<v>`. No signatures are
//!   published, so Pitcrew requires the checksum from both hosts to agree. `dl.gitea.com/act_runner/` redirects to
//!   `gitea-runner/`; versions before 1.0 used the `act_runner-<v>-…` names.
//! - **Forgejo runner** (`forgejo/runner` on code.forgejo.org): `forgejo-runner-<v>-linux-<arch>` with `.sha256` and
//!   an OpenPGP `.asc` signature; no Windows or macOS binaries are published, so on those platforms Pitcrew can only
//!   manage a binary the user provides (no automatic updates).

use crate::platform::{Arch, Os, Platform};
use crate::version::RunnerVersion;
use serde::{Deserialize, Serialize};

/// Largest release-discovery response Pitcrew parses.
pub const MAX_RELEASE_BODY: usize = 256 * 1024;

/// A runner implementation Pitcrew can manage. New kinds are added here (and to the service's adapters);
/// everything else works on the kind's descriptions below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunnerKind {
    /// Gitea's runner (`gitea-runner`, formerly `act_runner`).
    GiteaRunner,
    /// Forgejo's runner (`forgejo-runner`). Planned; Linux binaries only upstream.
    ForgejoRunner,
}

/// Where a release's files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFiles {
    /// The asset's file name.
    pub asset_name: String,
    /// Where to download the binary.
    pub binary_url: String,
    /// Checksum files to read; the first is required, the others must agree with it when they can be fetched.
    pub checksum_urls: Vec<String>,
    /// A detached OpenPGP signature, when the project publishes one.
    pub signature_url: Option<String>,
}

/// Problems with release information.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DistributionError {
    /// The project publishes no binary for this platform.
    #[error("{kind} publishes no binary for {os:?}/{arch:?}; provide one yourself and turn automatic updates off")]
    UnsupportedPlatform {
        /// The runner kind's display name.
        kind: &'static str,
        /// The OS.
        os: Os,
        /// The architecture.
        arch: Arch,
    },
    /// The discovery response could not be read.
    #[error("the release information could not be read: {0}")]
    BadResponse(String),
}

impl RunnerKind {
    /// The name shown to users.
    pub fn display_name(self) -> &'static str {
        match self {
            RunnerKind::GiteaRunner => "Gitea runner",
            RunnerKind::ForgejoRunner => "Forgejo runner",
        }
    }

    /// The file name Pitcrew installs the binary as.
    pub fn binary_file_name(self, os: Os) -> &'static str {
        match (self, os) {
            (RunnerKind::GiteaRunner, Os::Windows) => "gitea-runner.exe",
            (RunnerKind::GiteaRunner, _) => "gitea-runner",
            (RunnerKind::ForgejoRunner, Os::Windows) => "forgejo-runner.exe",
            (RunnerKind::ForgejoRunner, _) => "forgejo-runner",
        }
    }

    /// The URL that tells the latest version (parsed by [`RunnerKind::parse_latest`]).
    pub fn latest_url(self) -> &'static str {
        match self {
            RunnerKind::GiteaRunner => "https://dl.gitea.com/gitea-runner/version.json",
            RunnerKind::ForgejoRunner => "https://code.forgejo.org/api/v1/repos/forgejo/runner/releases/latest",
        }
    }

    /// The latest version from the body of [`RunnerKind::latest_url`].
    pub fn parse_latest(self, body: &str) -> Result<RunnerVersion, DistributionError> {
        if body.len() > MAX_RELEASE_BODY {
            return Err(DistributionError::BadResponse("the response is too large".into()));
        }
        let json: serde_json::Value = serde_json::from_str(body).map_err(|e| DistributionError::BadResponse(e.to_string()))?;
        let text = match self {
            RunnerKind::GiteaRunner => json.get("latest").and_then(|l| l.get("version")),
            RunnerKind::ForgejoRunner => json.get("tag_name"),
        }
        .and_then(|v| v.as_str())
        .ok_or_else(|| DistributionError::BadResponse("no version in the response".into()))?;
        RunnerVersion::parse(text).map_err(|e| DistributionError::BadResponse(e.to_string()))
    }

    /// The URL that lists recent releases, pre-releases included (parsed by [`RunnerKind::parse_releases`]).
    pub fn releases_url(self) -> &'static str {
        match self {
            RunnerKind::GiteaRunner => "https://gitea.com/api/v1/repos/gitea/runner/releases?limit=20",
            RunnerKind::ForgejoRunner => "https://code.forgejo.org/api/v1/repos/forgejo/runner/releases?limit=20",
        }
    }

    /// The versions of a Gitea/Forgejo releases list (`[{"tag_name": "v5.3.0-rc.1", "draft": false, …}]`), drafts
    /// and unparsable tags left out, newest first.
    pub fn parse_releases(self, body: &str) -> Result<Vec<RunnerVersion>, DistributionError> {
        if body.len() > MAX_RELEASE_BODY * 4 {
            return Err(DistributionError::BadResponse("the response is too large".into()));
        }
        let json: serde_json::Value = serde_json::from_str(body).map_err(|e| DistributionError::BadResponse(e.to_string()))?;
        let list = json
            .as_array()
            .ok_or_else(|| DistributionError::BadResponse("the response is not a list of releases".into()))?;
        let mut out: Vec<RunnerVersion> = list
            .iter()
            .filter(|r| !r.get("draft").and_then(serde_json::Value::as_bool).unwrap_or(false))
            .filter_map(|r| r.get("tag_name").and_then(|t| t.as_str()))
            .filter_map(|t| RunnerVersion::parse(t).ok())
            .collect();
        out.sort();
        out.reverse();
        out.dedup();
        Ok(out)
    }

    /// The files of `version` for `platform`.
    pub fn release_files(self, version: &RunnerVersion, platform: Platform) -> Result<ReleaseFiles, DistributionError> {
        let v = version.to_string();
        match self {
            RunnerKind::GiteaRunner => {
                let legacy = v.starts_with("0.");
                let (dir, stem) = if legacy {
                    ("act_runner", "act_runner")
                } else {
                    ("gitea-runner", "gitea-runner")
                };
                let os = match platform.os {
                    Os::Windows => "windows",
                    Os::Macos => "darwin",
                    Os::Linux => "linux",
                };
                let arch = match platform.arch {
                    Arch::Amd64 => "amd64",
                    Arch::Arm64 => "arm64",
                };
                if platform.os == Os::Windows && platform.arch == Arch::Arm64 {
                    return Err(self.unsupported(platform));
                }
                let ext = if platform.os == Os::Windows { ".exe" } else { "" };
                let asset_name = format!("{stem}-{v}-{os}-{arch}{ext}");
                let base = format!("https://dl.gitea.com/{dir}/{v}");
                let repo = if legacy { "act_runner" } else { "runner" };
                Ok(ReleaseFiles {
                    binary_url: format!("{base}/{asset_name}"),
                    checksum_urls: vec![
                        format!("{base}/{asset_name}.sha256"),
                        format!("https://gitea.com/gitea/{repo}/releases/download/v{v}/checksums.txt"),
                    ],
                    signature_url: None,
                    asset_name,
                })
            }
            RunnerKind::ForgejoRunner => {
                if platform.os != Os::Linux {
                    return Err(self.unsupported(platform));
                }
                let arch = match platform.arch {
                    Arch::Amd64 => "amd64",
                    Arch::Arm64 => "arm64",
                };
                let asset_name = format!("forgejo-runner-{v}-linux-{arch}");
                let base = format!("https://code.forgejo.org/forgejo/runner/releases/download/v{v}");
                Ok(ReleaseFiles {
                    binary_url: format!("{base}/{asset_name}"),
                    checksum_urls: vec![format!("{base}/{asset_name}.sha256")],
                    signature_url: Some(format!("{base}/{asset_name}.asc")),
                    asset_name,
                })
            }
        }
    }

    fn unsupported(self, p: Platform) -> DistributionError {
        DistributionError::UnsupportedPlatform {
            kind: self.display_name(),
            os: p.os,
            arch: p.arch,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn release_lists_give_versions_newest_first() {
        let k = RunnerKind::GiteaRunner;
        assert!(k.releases_url().contains("gitea/runner/releases"));
        assert!(RunnerKind::ForgejoRunner.releases_url().contains("forgejo"));
        let body = r#"[{"tag_name":"v5.2.0","draft":false},{"tag_name":"v5.3.0-rc.1","prerelease":true},{"tag_name":"v9.0.0","draft":true},{"tag_name":"nightly"},{"name":"x"}]"#;
        let v = k.parse_releases(body).unwrap();
        assert_eq!(v.iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["5.3.0-rc.1", "5.2.0"]);
        assert!(k.parse_releases("{}").is_err());
        assert!(k.parse_releases("nope").is_err());
        assert!(k.parse_releases(&" ".repeat(MAX_RELEASE_BODY * 4 + 1)).is_err());
    }

    use super::*;

    fn v(s: &str) -> RunnerVersion {
        RunnerVersion::parse(s).unwrap()
    }

    const WIN: Platform = Platform {
        os: Os::Windows,
        arch: Arch::Amd64,
    };
    const MAC: Platform = Platform {
        os: Os::Macos,
        arch: Arch::Arm64,
    };

    #[test]
    fn gitea_files_follow_the_published_layout() {
        let f = RunnerKind::GiteaRunner.release_files(&v("5.2.0"), WIN).unwrap();
        assert_eq!(f.asset_name, "gitea-runner-5.2.0-windows-amd64.exe");
        assert_eq!(
            f.binary_url,
            "https://dl.gitea.com/gitea-runner/5.2.0/gitea-runner-5.2.0-windows-amd64.exe"
        );
        assert_eq!(
            f.checksum_urls,
            vec![
                "https://dl.gitea.com/gitea-runner/5.2.0/gitea-runner-5.2.0-windows-amd64.exe.sha256".to_string(),
                "https://gitea.com/gitea/runner/releases/download/v5.2.0/checksums.txt".to_string(),
            ]
        );
        assert_eq!(f.signature_url, None);
        let mac = RunnerKind::GiteaRunner.release_files(&v("5.2.0"), MAC).unwrap();
        assert_eq!(mac.asset_name, "gitea-runner-5.2.0-darwin-arm64");
        let linux = RunnerKind::GiteaRunner
            .release_files(
                &v("5.2.0"),
                Platform {
                    os: Os::Linux,
                    arch: Arch::Amd64,
                },
            )
            .unwrap();
        assert_eq!(linux.asset_name, "gitea-runner-5.2.0-linux-amd64");
    }

    #[test]
    fn old_act_runner_versions_use_the_old_names() {
        let f = RunnerKind::GiteaRunner.release_files(&v("0.2.11"), WIN).unwrap();
        assert_eq!(
            f.binary_url,
            "https://dl.gitea.com/act_runner/0.2.11/act_runner-0.2.11-windows-amd64.exe"
        );
        assert!(f.checksum_urls[1].contains("gitea/act_runner/releases/download/v0.2.11"));
    }

    #[test]
    fn unpublished_platforms_are_refused_with_a_next_step() {
        let e = RunnerKind::GiteaRunner
            .release_files(
                &v("5.2.0"),
                Platform {
                    os: Os::Windows,
                    arch: Arch::Arm64,
                },
            )
            .unwrap_err();
        assert!(matches!(e, DistributionError::UnsupportedPlatform { .. }));
        let e = RunnerKind::ForgejoRunner.release_files(&v("13.2.0"), MAC).unwrap_err();
        assert!(e.to_string().contains("provide one yourself"), "{e}");
    }

    #[test]
    fn forgejo_linux_files_include_the_signature() {
        let amd = RunnerKind::ForgejoRunner
            .release_files(
                &v("13.2.0"),
                Platform {
                    os: Os::Linux,
                    arch: Arch::Amd64,
                },
            )
            .unwrap();
        assert_eq!(amd.asset_name, "forgejo-runner-13.2.0-linux-amd64");
        let f = RunnerKind::ForgejoRunner
            .release_files(
                &v("13.2.0"),
                Platform {
                    os: Os::Linux,
                    arch: Arch::Arm64,
                },
            )
            .unwrap();
        assert_eq!(f.asset_name, "forgejo-runner-13.2.0-linux-arm64");
        assert_eq!(
            f.signature_url.as_deref(),
            Some("https://code.forgejo.org/forgejo/runner/releases/download/v13.2.0/forgejo-runner-13.2.0-linux-arm64.asc")
        );
    }

    #[test]
    fn parses_latest_version_responses() {
        assert_eq!(
            RunnerKind::GiteaRunner.parse_latest(r#"{"latest":{"version":"5.2.0"}}"#).unwrap(),
            v("5.2.0")
        );
        assert_eq!(
            RunnerKind::ForgejoRunner
                .parse_latest(r#"{"tag_name":"v13.2.0","assets":[]}"#)
                .unwrap(),
            v("13.2.0")
        );
        for bad in ["", "{}", r#"{"latest":{"version":"newest"}}"#, r#"{"latest":{"version":5}}"#] {
            assert!(RunnerKind::GiteaRunner.parse_latest(bad).is_err(), "{bad}");
        }
        assert!(RunnerKind::GiteaRunner.parse_latest(&" ".repeat(MAX_RELEASE_BODY + 1)).is_err());
    }

    #[test]
    fn names_binaries_per_platform() {
        assert_eq!(RunnerKind::GiteaRunner.binary_file_name(Os::Windows), "gitea-runner.exe");
        assert_eq!(RunnerKind::GiteaRunner.binary_file_name(Os::Macos), "gitea-runner");
        assert_eq!(RunnerKind::ForgejoRunner.binary_file_name(Os::Windows), "forgejo-runner.exe");
        assert_eq!(RunnerKind::ForgejoRunner.binary_file_name(Os::Linux), "forgejo-runner");
        assert!(RunnerKind::ForgejoRunner.latest_url().starts_with("https://code.forgejo.org/"));
        assert!(RunnerKind::GiteaRunner.latest_url().starts_with("https://dl.gitea.com/"));
    }
}
