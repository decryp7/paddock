//! Container images: the tags of a repository in an OCI registry (Docker Hub, GHCR, Quay, a private registry),
//! read with the distribution API (`/v2/<name>/tags/list`). Registries that want a token answer 401 with a Bearer
//! challenge; the adapter fetches an (anonymous or credentialed) token and asks again ([`super::http::BearerChallenge`]).

use super::http::HttpRequest;
use super::SourceError;
use crate::limits::MAX_CANDIDATES;
use crate::release::Candidate;
use serde::Deserialize;
use crate::schema::Config;

/// A container image source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSource {
    /// The registry host the API is on (`registry-1.docker.io`, `ghcr.io`).
    pub registry: String,
    /// The repository (`library/nginx`, `owner/app`).
    pub repository: String,
    /// How the image is written for people (`nginx`, `ghcr.io/owner/app`).
    pub display: String,
    /// User name for private images.
    pub username: Option<String>,
}

impl ContainerSource {
    /// From form values (`image`, `username`).
    pub fn from_config(c: &Config) -> Result<ContainerSource, String> {
        let image = c.require("image")?.trim();
        let image = image.strip_prefix("docker://").unwrap_or(image).trim_matches('/');
        let bad = || format!("\"{image}\" is not an image name (for example nginx, owner/app or ghcr.io/owner/app), without a tag.");
        let segments: Vec<&str> = image.split('/').collect();
        if image.contains('@') || segments.iter().any(|s| s.is_empty()) {
            return Err(bad());
        }
        let first = segments[0];
        let has_registry = segments.len() > 1 && (first.contains('.') || first.contains(':') || first == "localhost");
        let (registry, repo_segments) = if has_registry {
            (first, &segments[1..])
        } else {
            ("docker.io", &segments[..])
        };
        if repo_segments.iter().any(|s| {
            !s.chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "._-".contains(ch))
        }) {
            return Err(bad());
        }
        let mut repository = repo_segments.join("/");
        let api_host = match registry {
            "docker.io" | "index.docker.io" | "registry-1.docker.io" => {
                if !repository.contains('/') {
                    repository = format!("library/{repository}");
                }
                "registry-1.docker.io".to_string()
            }
            other => other.to_string(),
        };
        Ok(ContainerSource {
            registry: api_host,
            repository,
            display: image.to_string(),
            username: c.get("username").map(str::to_string),
        })
    }

    /// The tag list request.
    pub fn request(&self) -> HttpRequest {
        HttpRequest::get(format!("https://{}/v2/{}/tags/list?n=1000", self.registry, self.repository)).header("Accept", "application/json")
    }

    /// `nginx`, `ghcr.io/owner/app`.
    pub fn describe(&self) -> String {
        self.display.clone()
    }

    /// Reads a tag list.
    pub fn parse(&self, body: &[u8]) -> Result<Vec<Candidate>, SourceError> {
        #[derive(Deserialize)]
        struct Tags {
            tags: Option<Vec<String>>,
        }
        let tags: Tags =
            serde_json::from_slice(body).map_err(|e| SourceError::InvalidResponse(format!("The tag list could not be read ({e}).")))?;
        Ok(tags
            .tags
            .unwrap_or_default()
            .into_iter()
            .take(MAX_CANDIDATES)
            .map(|t| {
                let reference = format!("{}:{t}", self.display);
                Candidate {
                    version_text: t,
                    name: reference.clone(),
                    location: reference,
                    published_at: None,
                    size: None,
                    sha256: None,
                    prerelease: false,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(name: &str) -> Result<ContainerSource, String> {
        ContainerSource::from_config(&Config::new().with("image", name))
    }

    #[test]
    fn image_names_resolve_to_registries() {
        let hub = img("nginx").unwrap();
        assert_eq!(
            (hub.registry.as_str(), hub.repository.as_str(), hub.describe()),
            ("registry-1.docker.io", "library/nginx", "nginx".into())
        );
        assert_eq!(hub.request().url, "https://registry-1.docker.io/v2/library/nginx/tags/list?n=1000");
        assert_eq!(img("owner/app").unwrap().repository, "owner/app");
        assert_eq!(img("docker.io/library/redis").unwrap().registry, "registry-1.docker.io");
        let gh = img("docker://ghcr.io/owner/app").unwrap();
        assert_eq!((gh.registry.as_str(), gh.repository.as_str()), ("ghcr.io", "owner/app"));
        assert_eq!(img("localhost:5000/team/svc").unwrap().registry, "localhost:5000");
        let creds = ContainerSource::from_config(&Config::new().with("image", "quay.io/a/b").with("username", "me")).unwrap();
        assert_eq!(creds.username.as_deref(), Some("me"));
        for bad in ["nginx@sha256:abc", "a//b", "Owner/App", "ghcr.io/o/app:1.0"] {
            assert!(img(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tag_lists_become_candidates() {
        let s = img("ghcr.io/o/app").unwrap();
        let c = s.parse(br#"{"name":"o/app","tags":["1.0.0","latest","1.1.0"]}"#).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(
            (c[2].version_text.as_str(), c[2].location.as_str()),
            ("1.1.0", "ghcr.io/o/app:1.1.0")
        );
        assert!(s.parse(br#"{"name":"o/app","tags":null}"#).unwrap().is_empty());
        assert!(matches!(s.parse(b"<html>"), Err(SourceError::InvalidResponse(_))));
    }
}
