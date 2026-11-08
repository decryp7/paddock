//! The check after the installer's copy of `paddock-setup.exe` (ADR 0053 amendment 1, section 2/4.3): the installer's own file
//! copy silently fails to overwrite a program that is still running, and `apply-update` is such a program when it hands over.
//! The program has its own version (`crate::PROGRAM_VERSION`, in its `FileVersion`; never the app's, so its bytes do not move with an app
//! release), so the installer (after its copy, from the embedded file's version) and the app
//! (at its next start) compare it with the version they expect. A mismatch is reported, never fatal: in the installer's details,
//! in `setup.log` and as a Health line in the app.

pub use semver::Version;

/// What the comparison found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCheck {
    /// The installed program has the expected version.
    Match,
    /// It has another version: the copy was skipped (or the program was replaced).
    Mismatch { found: Version, expected: Version },
    /// The version could not be read (no resource, no file).
    Unreadable,
}

/// Compares what was read (`None`: unreadable) with the version expected.
pub fn compare(found: Option<&Version>, expected: &Version) -> VersionCheck {
    match found {
        None => VersionCheck::Unreadable,
        Some(f)
            if f.major == expected.major
                && f.minor == expected.minor
                && f.patch == expected.patch =>
        {
            VersionCheck::Match
        }
        Some(f) => VersionCheck::Mismatch {
            found: f.clone(),
            expected: expected.clone(),
        },
    }
}

impl VersionCheck {
    /// Whether to tell the user: anything but a match.
    pub fn is_problem(&self) -> bool {
        !matches!(self, VersionCheck::Match)
    }

    /// The line for the details, `setup.log` and the Health line, with the next step.
    pub fn sentence(&self) -> String {
        match self {
            VersionCheck::Match => "paddock-setup.exe has the expected version.".into(),
            VersionCheck::Mismatch { found, expected } => format!(
                "paddock-setup.exe is version {found}, not {expected}: the installer could not replace it while it was running. \
                 Run the installer again to fix it; updates still work."
            ),
            VersionCheck::Unreadable => {
                "The version of paddock-setup.exe could not be read. Run the installer again if updates from the app fail.".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn the_same_numbers_match_whatever_the_prerelease() {
        assert_eq!(compare(Some(&v("1.1.0")), &v("1.1.0")), VersionCheck::Match);
        assert_eq!(
            compare(Some(&v("1.1.0")), &v("1.1.0-beta.1")),
            VersionCheck::Match
        );
        assert!(!compare(Some(&v("1.1.0")), &v("1.1.0")).is_problem());
    }

    #[test]
    fn another_version_is_a_mismatch_that_names_both_and_the_next_step() {
        let c = compare(Some(&v("1.0.0")), &v("1.1.0"));
        assert_eq!(
            c,
            VersionCheck::Mismatch {
                found: v("1.0.0"),
                expected: v("1.1.0")
            }
        );
        let s = c.sentence();
        assert!(
            s.contains("1.0.0") && s.contains("1.1.0") && s.contains("Run the installer again"),
            "{s}"
        );
    }

    #[test]
    fn an_unreadable_version_is_reported_not_ignored() {
        let c = compare(None, &v("1.1.0"));
        assert_eq!(c, VersionCheck::Unreadable);
        assert!(c.is_problem() && c.sentence().contains("could not be read"));
    }
}
