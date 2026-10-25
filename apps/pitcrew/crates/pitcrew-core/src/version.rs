//! Runner versions: release tags such as `v5.2.0` and the output of `gitea-runner --version`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Longest version text Pitcrew parses (release tags and `--version` output come from the network or a child process).
pub const MAX_VERSION_LEN: usize = 64;

/// A runner version (semantic version, without the `v` of the tag).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RunnerVersion(semver::Version);

/// Why a version could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionError {
    /// Not a semantic version.
    #[error("{0:?} is not a version such as 5.2.0 or v5.2.0")]
    Invalid(String),
}

impl RunnerVersion {
    /// Parses `5.2.0` or `v5.2.0` (surrounding white space is ignored).
    pub fn parse(s: &str) -> Result<Self, VersionError> {
        let t = s.trim();
        if t.len() > MAX_VERSION_LEN {
            return Err(VersionError::Invalid(t.chars().take(MAX_VERSION_LEN).collect()));
        }
        let bare = t.strip_prefix('v').unwrap_or(t);
        semver::Version::parse(bare)
            .map(RunnerVersion)
            .map_err(|_| VersionError::Invalid(t.to_string()))
    }

    /// The version a runner binary reports, from its `--version` output, e.g. `gitea-runner version v5.2.0` or
    /// `act_runner version v0.2.11`: the first word that parses as a version.
    pub fn from_version_output(output: &str) -> Result<Self, VersionError> {
        output
            .split_whitespace()
            .take(16)
            .find_map(|w| RunnerVersion::parse(w).ok())
            .ok_or_else(|| VersionError::Invalid(output.trim().chars().take(MAX_VERSION_LEN).collect()))
    }

    /// Whether this is a pre-release (e.g. `5.3.0-rc.1`); Pitcrew never installs one automatically.
    pub fn is_prerelease(&self) -> bool {
        !self.0.pre.is_empty()
    }
}

impl TryFrom<String> for RunnerVersion {
    type Error = VersionError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        RunnerVersion::parse(&s)
    }
}

impl From<RunnerVersion> for String {
    fn from(v: RunnerVersion) -> Self {
        v.to_string()
    }
}

impl fmt::Display for RunnerVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> RunnerVersion {
        RunnerVersion::parse(s).unwrap()
    }

    #[test]
    fn parses_tags_with_and_without_v() {
        assert_eq!(v("v5.2.0"), v("5.2.0"));
        assert_eq!(v(" 0.2.11\n").to_string(), "0.2.11");
        assert!(v("5.3.0-rc.1").is_prerelease());
        assert!(!v("5.3.0").is_prerelease());
    }

    #[test]
    fn orders_semantically() {
        assert!(v("0.2.11") < v("5.2.0"));
        assert!(v("5.10.0") > v("5.9.9"));
        assert!(v("5.3.0-rc.1") < v("5.3.0"));
    }

    #[test]
    fn refuses_junk() {
        for bad in ["", "latest", "5.2", "v", "5.2.0.1", &"9".repeat(100)] {
            assert!(RunnerVersion::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reads_version_output() {
        assert_eq!(
            RunnerVersion::from_version_output("gitea-runner version v5.2.0\n").unwrap(),
            v("5.2.0")
        );
        assert_eq!(
            RunnerVersion::from_version_output("act_runner version v0.2.11").unwrap(),
            v("0.2.11")
        );
        assert!(RunnerVersion::from_version_output("error: unknown flag").is_err());
    }
}
