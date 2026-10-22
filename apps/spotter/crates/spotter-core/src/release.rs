//! Releases: what a source reports ([`Candidate`]) and what a watch keeps after reading its version ([`Release`]),
//! plus [`select`], which turns a check's candidates into the watch's releases.

use crate::checksum::Sha256Digest;
use crate::limits::MAX_CANDIDATES;
use crate::pattern::NamePattern;
use crate::time::Timestamp;
use crate::version::{Version, VersionScheme};
use serde::{Deserialize, Serialize};
use spotter_plugin_api::WireRelease;

/// One thing a source found, before its version is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The text the version is read from (file name, tag, title, version string).
    pub version_text: String,
    /// Display name.
    pub name: String,
    /// Where it is: a path or URL.
    pub location: String,
    /// When it was published or last changed.
    pub published_at: Option<Timestamp>,
    /// Size in bytes.
    pub size: Option<u64>,
    /// The publisher's SHA-256 of the file at `location`.
    pub sha256: Option<Sha256Digest>,
    /// Whether the source marks it as a pre-release.
    pub prerelease: bool,
}

impl Candidate {
    /// A candidate whose version is read from its name.
    pub fn named(name: &str, location: &str) -> Candidate {
        Candidate {
            version_text: name.into(),
            name: name.into(),
            location: location.into(),
            published_at: None,
            size: None,
            sha256: None,
            prerelease: false,
        }
    }
}

impl From<WireRelease> for Candidate {
    fn from(w: WireRelease) -> Candidate {
        Candidate {
            version_text: w.version,
            name: w.name,
            location: w.location,
            published_at: w.published_at_ms.map(Timestamp),
            size: w.size,
            sha256: w.sha256.as_deref().and_then(Sha256Digest::parse),
            prerelease: w.prerelease,
        }
    }
}

/// A release a watch knows about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// Its version.
    pub version: Version,
    /// Display name.
    pub name: String,
    /// Where it is: a path or URL.
    pub location: String,
    /// When it was published or last changed.
    #[serde(default)]
    pub published_at: Option<Timestamp>,
    /// Size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// The publisher's SHA-256.
    #[serde(default)]
    pub sha256: Option<Sha256Digest>,
}

/// Which candidates a watch keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseFilter {
    /// Keep pre-releases (labelled versions, or marked so by the source).
    pub include_prereleases: bool,
    /// Keep only names matching this (applies to the candidate's name).
    pub name: NamePattern,
}

impl Default for ReleaseFilter {
    fn default() -> ReleaseFilter {
        ReleaseFilter {
            include_prereleases: false,
            name: NamePattern::any(),
        }
    }
}

/// The result of [`select`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selection {
    /// Releases, newest first, one per version, at most the limit.
    pub releases: Vec<Release>,
    /// Candidates whose name matched but whose text held no version (shown in the source test).
    pub without_version: Vec<String>,
    /// Candidates left out as pre-releases.
    pub prereleases_skipped: usize,
    /// Candidates left out by the name filter.
    pub names_skipped: usize,
}

/// Reads each candidate's version with `scheme`, applies `filter`, keeps one release per version (the first
/// candidate by name, so the choice is stable) and returns at most `limit`, newest first. Looks at no more than
/// [`MAX_CANDIDATES`] candidates.
pub fn select(candidates: Vec<Candidate>, scheme: &VersionScheme, filter: &ReleaseFilter, limit: usize) -> Selection {
    let mut out = Selection::default();
    let mut releases: Vec<Release> = Vec::new();
    for c in candidates.into_iter().take(MAX_CANDIDATES) {
        if !filter.name.matches(&c.name) {
            out.names_skipped += 1;
            continue;
        }
        let Some(version) = scheme.extract(&c.version_text) else {
            if out.without_version.len() < 20 {
                out.without_version.push(c.name);
            }
            continue;
        };
        if (version.is_prerelease() || c.prerelease) && !filter.include_prereleases {
            out.prereleases_skipped += 1;
            continue;
        }
        releases.push(Release {
            version,
            name: c.name,
            location: c.location,
            published_at: c.published_at,
            size: c.size,
            sha256: c.sha256,
        });
    }
    releases.sort_by(|a, b| b.version.cmp(&a.version).then_with(|| a.name.cmp(&b.name)));
    releases.dedup_by(|later, first| later.version == first.version);
    releases.truncate(limit);
    out.releases = releases;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::PatternSyntax;

    fn names(s: &Selection) -> Vec<&str> {
        s.releases.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn selection_orders_dedupes_filters_and_bounds() {
        let cands = [
            "b-R1.01.00T10.zip",
            "a-R1.01.00T10.zip",
            "R1.02.00T01.zip",
            "readme.txt",
            "R0.09.00T99.zip",
            "R1.00.00T01.log",
        ]
        .map(|n| Candidate::named(n, &format!("/s/{n}")))
        .to_vec();
        let filter = ReleaseFilter {
            include_prereleases: false,
            name: NamePattern::new(PatternSyntax::Glob, "*.zip").unwrap(),
        };
        let s = select(cands.clone(), &VersionScheme::BuildNumber, &filter, 10);
        assert_eq!(names(&s), ["R1.02.00T01.zip", "a-R1.01.00T10.zip", "R0.09.00T99.zip"]);
        assert_eq!(s.names_skipped, 2);
        assert!(s.without_version.is_empty());
        assert_eq!(select(cands.clone(), &VersionScheme::BuildNumber, &filter, 1).releases.len(), 1);
        let all = select(cands, &VersionScheme::BuildNumber, &ReleaseFilter::default(), 10);
        assert_eq!(all.without_version, ["readme.txt"]);
    }

    #[test]
    fn prereleases_are_skipped_unless_wanted() {
        let mut marked = Candidate::named("2.0.0", "l");
        marked.prerelease = true;
        let cands = vec![Candidate::named("1.0.0", "l"), Candidate::named("1.1.0-rc.1", "l"), marked];
        let s = select(cands.clone(), &VersionScheme::Semver, &ReleaseFilter::default(), 10);
        assert_eq!(names(&s), ["1.0.0"]);
        assert_eq!(s.prereleases_skipped, 2);
        let with = ReleaseFilter {
            include_prereleases: true,
            ..ReleaseFilter::default()
        };
        assert_eq!(
            names(&select(cands, &VersionScheme::Semver, &with, 10)),
            ["2.0.0", "1.1.0-rc.1", "1.0.0"]
        );
    }

    #[test]
    fn unversioned_names_are_reported_up_to_a_bound() {
        let cands = (0..30).map(|i| Candidate::named(&format!("file{i}"), "l")).collect();
        assert_eq!(
            select(cands, &VersionScheme::Semver, &ReleaseFilter::default(), 10)
                .without_version
                .len(),
            20
        );
        let many = (0..MAX_CANDIDATES + 10).map(|i| Candidate::named(&format!("1.{i}"), "l")).collect();
        let s = select(many, &VersionScheme::Semver, &ReleaseFilter::default(), usize::MAX);
        assert_eq!(s.releases.len(), MAX_CANDIDATES);
    }

    #[test]
    fn wire_releases_become_candidates() {
        let mut w = WireRelease::new("v1", "n", "l");
        w.published_at_ms = Some(5);
        w.sha256 = Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into());
        w.prerelease = true;
        let c = Candidate::from(w);
        assert_eq!(c.published_at, Some(Timestamp(5)));
        assert!(c.sha256.is_some() && c.prerelease && c.version_text == "v1");
    }
}
