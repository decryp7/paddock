//! Releases of a repository on GitHub, Gitea/Forgejo or GitLab: the request each API needs and how to read its answer.

use super::http::{encode_segment, HttpRequest};
use super::SourceError;
use crate::checksum::Sha256Digest;
use crate::pattern::{NamePattern, PatternSyntax};
use crate::release::Candidate;
use crate::time::Timestamp;
use serde::Deserialize;
use crate::schema::Config;

/// Which forge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    /// GitHub or GitHub Enterprise.
    GitHub,
    /// Gitea, Forgejo, Codeberg.
    Gitea,
    /// GitLab.
    GitLab,
}

/// A forge release source.
#[derive(Debug, Clone, PartialEq)]
pub struct ForgeSource {
    /// Which forge.
    pub forge: Forge,
    /// The API base (GitHub) or server address (Gitea, GitLab), without a trailing slash.
    pub server: String,
    /// `owner/name` (GitLab: `group/subgroup/project`).
    pub repo: String,
    /// Asset names that a release must have; the first match becomes its location.
    pub assets: NamePattern,
}

const GITHUB_API: &str = "https://api.github.com";

impl ForgeSource {
    /// From form values (`repo` or `project`, `server`, `assets`).
    pub fn from_config(forge: Forge, c: &Config) -> Result<ForgeSource, String> {
        let repo = match forge {
            Forge::GitLab => c.require("project")?,
            _ => c.require("repo")?,
        }
        .trim_matches('/')
        .to_string();
        let parts: Vec<&str> = repo.split('/').collect();
        let valid = parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|ch| ch.is_alphanumeric() || "-_.".contains(ch)));
        let shape_ok = match forge {
            Forge::GitLab => parts.len() >= 2,
            _ => parts.len() == 2,
        };
        if !valid || !shape_ok {
            return Err(match forge {
                Forge::GitLab => format!("\"{repo}\" is not a project path (group/project)."),
                _ => format!("\"{repo}\" is not a repository (owner/name)."),
            });
        }
        let default = match forge {
            Forge::GitHub => GITHUB_API,
            Forge::GitLab => "https://gitlab.com",
            Forge::Gitea => "",
        };
        let server = c.get("server").unwrap_or(default).trim_end_matches('/').to_string();
        if !server.starts_with("https://") && !server.starts_with("http://") {
            return Err("The server address must start with https://.".into());
        }
        Ok(ForgeSource {
            forge,
            server,
            repo,
            assets: NamePattern::new(PatternSyntax::Glob, c.get("assets").unwrap_or(""))?,
        })
    }

    /// The API request, with the token in the header each forge expects.
    pub fn request(&self, token: Option<String>) -> HttpRequest {
        match self.forge {
            Forge::GitHub => HttpRequest::get(format!("{}/repos/{}/releases?per_page=100", self.server, self.repo))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header_if("Authorization", token.map(|t| format!("Bearer {t}"))),
            Forge::Gitea => HttpRequest::get(format!("{}/api/v1/repos/{}/releases?limit=50", self.server, self.repo))
                .header("Accept", "application/json")
                .header_if("Authorization", token.map(|t| format!("token {t}"))),
            Forge::GitLab => HttpRequest::get(format!(
                "{}/api/v4/projects/{}/releases?per_page=100",
                self.server,
                encode_segment(&self.repo)
            ))
            .header("Accept", "application/json")
            .header_if("PRIVATE-TOKEN", token),
        }
    }

    /// `github.com/owner/name`, `codeberg.org/owner/name`.
    pub fn describe(&self) -> String {
        let host = if self.server == GITHUB_API {
            "github.com".to_string()
        } else {
            self.server.split("://").nth(1).unwrap_or(&self.server).to_string()
        };
        format!("{host}/{}", self.repo)
    }

    /// Reads the API's answer.
    pub fn parse(&self, body: &[u8]) -> Result<Vec<Candidate>, SourceError> {
        let bad = |e: serde_json::Error| SourceError::InvalidResponse(format!("The releases could not be read ({e})."));
        match self.forge {
            Forge::GitHub | Forge::Gitea => {
                let releases: Vec<HubRelease> = serde_json::from_slice(body).map_err(bad)?;
                Ok(releases.into_iter().filter(|r| !r.draft).filter_map(|r| self.hub(r)).collect())
            }
            Forge::GitLab => {
                let releases: Vec<LabRelease> = serde_json::from_slice(body).map_err(bad)?;
                Ok(releases.into_iter().filter_map(|r| self.lab(r)).collect())
            }
        }
    }

    fn hub(&self, r: HubRelease) -> Option<Candidate> {
        let name = r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone());
        let mut c = Candidate {
            version_text: r.tag_name,
            name,
            location: r.html_url,
            published_at: r.published_at.as_deref().and_then(Timestamp::parse),
            size: None,
            sha256: None,
            prerelease: r.prerelease,
        };
        if !self.assets.is_any() {
            let asset = r.assets.into_iter().find(|a| self.assets.matches(&a.name))?;
            c.location = asset.browser_download_url;
            c.size = asset.size;
            c.sha256 = asset.digest.as_deref().and_then(Sha256Digest::parse);
        }
        Some(c)
    }

    fn lab(&self, r: LabRelease) -> Option<Candidate> {
        let name = r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone());
        let mut c = Candidate {
            version_text: r.tag_name,
            name,
            location: r.links.and_then(|l| l.self_url).unwrap_or_default(),
            published_at: r.released_at.as_deref().and_then(Timestamp::parse),
            size: None,
            sha256: None,
            prerelease: r.upcoming_release,
        };
        if !self.assets.is_any() {
            let link = r.assets.links.into_iter().find(|l| self.assets.matches(&l.name))?;
            c.location = link.direct_asset_url.unwrap_or(link.url);
        }
        Some(c)
    }
}

#[derive(Deserialize)]
struct HubRelease {
    tag_name: String,
    name: Option<String>,
    #[serde(default)]
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<HubAsset>,
}

#[derive(Deserialize)]
struct HubAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct LabRelease {
    tag_name: String,
    name: Option<String>,
    released_at: Option<String>,
    #[serde(default)]
    upcoming_release: bool,
    #[serde(rename = "_links")]
    links: Option<LabLinks>,
    #[serde(default)]
    assets: LabAssets,
}

#[derive(Deserialize)]
struct LabLinks {
    #[serde(rename = "self")]
    self_url: Option<String>,
}

#[derive(Deserialize, Default)]
struct LabAssets {
    #[serde(default)]
    links: Vec<LabLink>,
}

#[derive(Deserialize)]
struct LabLink {
    name: String,
    url: String,
    direct_asset_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(forge: Forge, values: &[(&str, &str)]) -> Result<ForgeSource, String> {
        let mut c = Config::new();
        for (k, v) in values {
            c = c.with(k, v);
        }
        ForgeSource::from_config(forge, &c)
    }

    const HUB: &str = r#"[
      {"tag_name":"v2.1.0","name":"Tauri 2.1","html_url":"https://github.com/o/r/releases/tag/v2.1.0","published_at":"2026-10-20T10:00:00Z",
       "prerelease":false,"draft":false,"assets":[
         {"name":"app_2.1.0_x64-setup.exe","browser_download_url":"https://github.com/o/r/releases/download/v2.1.0/app_2.1.0_x64-setup.exe","size":123,
          "digest":"sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},
         {"name":"app_2.1.0_aarch64.dmg","browser_download_url":"https://x/dmg","size":456}]},
      {"tag_name":"v2.2.0-beta.1","name":"","html_url":"h","published_at":null,"prerelease":true,"assets":[]},
      {"tag_name":"v3.0.0","name":"draft","html_url":"h","draft":true}
    ]"#;

    #[test]
    fn github_and_gitea_requests_and_answers() {
        let gh = src(Forge::GitHub, &[("repo", "o/r")]).unwrap();
        let req = gh.request(Some("t".into()));
        assert_eq!(req.url, "https://api.github.com/repos/o/r/releases?per_page=100");
        assert!(req.headers.contains(&("Authorization".into(), "Bearer t".into())));
        assert_eq!(gh.describe(), "github.com/o/r");
        let c = gh.parse(HUB.as_bytes()).unwrap();
        assert_eq!(c.len(), 2, "drafts are skipped");
        assert_eq!((c[0].name.as_str(), c[0].version_text.as_str()), ("Tauri 2.1", "v2.1.0"));
        assert_eq!(c[1].name, "v2.2.0-beta.1");
        assert!(c[1].prerelease && c[1].published_at.is_none());

        let with_assets = src(Forge::GitHub, &[("repo", "o/r"), ("assets", "*x64-setup.exe")]).unwrap();
        let c = with_assets.parse(HUB.as_bytes()).unwrap();
        assert_eq!(c.len(), 1, "releases without a matching asset are skipped");
        assert!(c[0].location.ends_with("x64-setup.exe") && c[0].size == Some(123) && c[0].sha256.is_some());

        let tea = src(Forge::Gitea, &[("server", "https://codeberg.org/"), ("repo", "forgejo/runner")]).unwrap();
        let req = tea.request(Some("t".into()));
        assert_eq!(req.url, "https://codeberg.org/api/v1/repos/forgejo/runner/releases?limit=50");
        assert!(req.headers.contains(&("Authorization".into(), "token t".into())));
        assert_eq!(tea.describe(), "codeberg.org/forgejo/runner");
        assert_eq!(tea.parse(HUB.as_bytes()).unwrap().len(), 2);
        assert!(tea.request(None).headers.iter().all(|(k, _)| k != "Authorization"));
    }

    #[test]
    fn gitlab_requests_and_answers() {
        let lab = src(Forge::GitLab, &[("project", "group/sub/project")]).unwrap();
        let req = lab.request(Some("t".into()));
        assert_eq!(
            req.url,
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Fproject/releases?per_page=100"
        );
        assert!(req.headers.contains(&("PRIVATE-TOKEN".into(), "t".into())));
        let body = r#"[
          {"tag_name":"v17.5.0","name":"GitLab Runner 17.5","released_at":"2026-10-17T00:00:00Z","upcoming_release":false,
           "_links":{"self":"https://gitlab.com/g/p/-/releases/v17.5.0"},
           "assets":{"links":[{"name":"runner-amd64.deb","url":"https://u/deb","direct_asset_url":"https://u/direct"},{"name":"other","url":"https://o"}]}},
          {"tag_name":"v17.6.0","name":null,"upcoming_release":true},
          {"tag_name":"v17.4.0","name":" ","assets":{"links":[{"name":"runner-amd64.deb","url":"https://u/only"}]}}
        ]"#;
        let c = lab.parse(body.as_bytes()).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].location, "https://gitlab.com/g/p/-/releases/v17.5.0");
        assert!(c[1].prerelease && c[1].location.is_empty() && c[1].name == "v17.6.0");
        let debs = src(Forge::GitLab, &[("project", "g/p"), ("assets", "*.deb")])
            .unwrap()
            .parse(body.as_bytes())
            .unwrap();
        assert_eq!(
            debs.iter().map(|c| c.location.as_str()).collect::<Vec<_>>(),
            ["https://u/direct", "https://u/only"]
        );
    }

    #[test]
    fn bad_values_and_answers_are_refused() {
        assert!(src(Forge::GitHub, &[("repo", "only-owner")]).is_err());
        assert!(src(Forge::GitHub, &[("repo", "o/r/x")]).is_err());
        assert!(src(Forge::GitHub, &[("repo", "o/r?x")]).is_err());
        assert!(src(Forge::GitLab, &[("project", "solo")]).unwrap_err().contains("project path"));
        assert!(src(Forge::Gitea, &[("repo", "o/r")]).unwrap_err().contains("https://"));
        assert!(
            src(Forge::GitHub, &[("repo", "o/r"), ("assets", "[")]).is_ok(),
            "globs have no syntax errors"
        );
        let gh = src(Forge::GitHub, &[("repo", "o/r"), ("server", "https://ghe.corp/api/v3")]).unwrap();
        assert_eq!(gh.describe(), "ghe.corp/api/v3/o/r");
        assert!(matches!(gh.parse(b"{\"message\":\"x\"}"), Err(SourceError::InvalidResponse(_))));
        let lab = src(Forge::GitLab, &[("project", "g/p")]).unwrap();
        assert!(lab.parse(b"nope").is_err());
        assert!(src(Forge::GitHub, &[]).is_err());
        assert!(src(Forge::GitLab, &[]).is_err());
    }
}
