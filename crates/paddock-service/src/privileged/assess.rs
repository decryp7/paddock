//! What the app does about an installed helper (ADR 0027 §4, D5): compatibility is the protocol, not the version. The
//! same protocol is `Ready` (a newer bundled helper is offered, never forced); a different protocol, or an installed
//! version below the app's declared minimum, is `UpdateRequired` with one prompt. Pure.

/// What the app ships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledHelper {
    /// The bundled helper's version (`1.2.0`).
    pub version: String,
    /// The protocol (`HELPER_API`) the app speaks.
    pub protocol: u32,
    /// The oldest installed helper this app accepts (raised only when a helper fix must reach users).
    pub min_version: String,
}

/// What the installed helper answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledHelper {
    /// Its version.
    pub version: String,
    /// Its protocol.
    pub protocol: u32,
}

/// Why the helper must be updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateReason {
    /// The installed helper speaks another protocol.
    Protocol {
        /// The installed protocol.
        installed: u32,
        /// The app's protocol.
        bundled: u32,
    },
    /// The installed helper is older than the app's minimum.
    BelowMinimum {
        /// The installed version.
        installed: String,
        /// The minimum.
        minimum: String,
    },
    /// The installed version is not a version number, so it cannot be compared; it is replaced.
    UnreadableVersion(String),
}

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    /// Nothing answers and nothing is registered: offer to turn it on.
    NotInstalled,
    /// Compatible. `newer_available` offers the bundled helper without forcing it.
    Ready {
        /// The bundled helper is newer than the installed one.
        newer_available: bool,
    },
    /// One prompt must update it, for this reason.
    UpdateRequired(UpdateReason),
}

/// A dotted version as numbers (`1.2.3`, `1.2.3-rc.1` counts as `1.2.3`, missing parts are 0).
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = || -> Option<u64> {
        match parts.next() {
            Some(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => p.parse().ok(),
            Some(_) => None,
            None => Some(0),
        }
    };
    let (a, b, c) = (next()?, next()?, next()?);
    parts.next().is_none().then_some((a, b, c))
}

/// Decides what to do about `installed` (`None`: no helper answered and none is registered).
pub fn assess(installed: Option<&InstalledHelper>, bundled: &BundledHelper) -> Assessment {
    let Some(inst) = installed else {
        return Assessment::NotInstalled;
    };
    if inst.protocol != bundled.protocol {
        return Assessment::UpdateRequired(UpdateReason::Protocol {
            installed: inst.protocol,
            bundled: bundled.protocol,
        });
    }
    let Some(have) = parse_version(&inst.version) else {
        return Assessment::UpdateRequired(UpdateReason::UnreadableVersion(inst.version.clone()));
    };
    // The bundled values are the app's own constants; an unreadable one is a build mistake that must not nag the user.
    let minimum = parse_version(&bundled.min_version);
    if minimum.is_some_and(|m| have < m) {
        return Assessment::UpdateRequired(UpdateReason::BelowMinimum {
            installed: inst.version.clone(),
            minimum: bundled.min_version.clone(),
        });
    }
    Assessment::Ready {
        newer_available: parse_version(&bundled.version).is_some_and(|b| b > have),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled() -> BundledHelper {
        BundledHelper {
            version: "1.2.0".into(),
            protocol: 2,
            min_version: "1.1.0".into(),
        }
    }
    fn installed(version: &str, protocol: u32) -> InstalledHelper {
        InstalledHelper {
            version: version.into(),
            protocol,
        }
    }

    #[test]
    fn nothing_installed_is_offered() {
        assert_eq!(assess(None, &bundled()), Assessment::NotInstalled);
    }

    #[test]
    fn a_protocol_change_asks_for_an_update_either_way() {
        for p in [1, 3] {
            assert_eq!(
                assess(Some(&installed("1.2.0", p)), &bundled()),
                Assessment::UpdateRequired(UpdateReason::Protocol {
                    installed: p,
                    bundled: 2
                })
            );
        }
    }

    #[test]
    fn below_the_minimum_asks_and_at_or_above_does_not() {
        assert_eq!(
            assess(Some(&installed("1.0.9", 2)), &bundled()),
            Assessment::UpdateRequired(UpdateReason::BelowMinimum {
                installed: "1.0.9".into(),
                minimum: "1.1.0".into()
            })
        );
        assert_eq!(
            assess(Some(&installed("1.1.0", 2)), &bundled()),
            Assessment::Ready {
                newer_available: true
            }
        );
        assert_eq!(
            assess(Some(&installed("1.2.0", 2)), &bundled()),
            Assessment::Ready {
                newer_available: false
            }
        );
        assert_eq!(
            assess(Some(&installed("1.10.0", 2)), &bundled()),
            Assessment::Ready {
                newer_available: false
            }
        );
        assert_eq!(
            assess(Some(&installed("1.2.0-rc.1", 2)), &bundled()),
            Assessment::Ready {
                newer_available: false
            }
        );
        assert_eq!(
            assess(Some(&installed("2", 2)), &bundled()),
            Assessment::Ready {
                newer_available: false
            }
        );
    }

    #[test]
    fn an_unreadable_installed_version_is_replaced_and_an_unreadable_bundled_one_is_ignored() {
        for v in ["", "x.y", "1.2.3.4", "1..2"] {
            assert_eq!(
                assess(Some(&installed(v, 2)), &bundled()),
                Assessment::UpdateRequired(UpdateReason::UnreadableVersion(v.into())),
                "{v:?}"
            );
        }
        let broken = BundledHelper {
            version: "?".into(),
            protocol: 2,
            min_version: "?".into(),
        };
        assert_eq!(
            assess(Some(&installed("0.1.0", 2)), &broken),
            Assessment::Ready {
                newer_available: false
            }
        );
    }
}
