//! The compatibility rule shared by the platform protocol and every app contract namespace.
//!
//! This is a plain serde struct with the same rule as `paddock_ipc::frame::ProtocolVersion`, on purpose: a package must
//! be readable and verifiable without the IPC crate (and its process and endpoint dependencies). Both types have public
//! `major` and `minor` fields, so `paddock-plugin-api` converts between them in one line.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// `major.minor` of a protocol: major changes are incompatible, minor ones add a variant at the end of an enum.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ProtocolVersion {
    /// Incompatible changes.
    pub major: u16,
    /// Compatible additions.
    pub minor: u16,
}

impl ProtocolVersion {
    /// `major.minor`.
    pub const fn new(major: u16, minor: u16) -> ProtocolVersion {
        ProtocolVersion { major, minor }
    }

    /// Whether a host speaking `self` can serve a plugin built for `plugin`: the same major and a minor no newer than the
    /// host's (a newer minor may send requests the host does not know).
    pub fn accepts(self, plugin: ProtocolVersion) -> bool {
        self.major == plugin.major && plugin.minor <= self.minor
    }

    /// Why a host (`host_name`, e.g. "Debrief") speaking `self` refuses `label` built for `plugin`, telling the user who
    /// must update; `None` when [`ProtocolVersion::accepts`] holds.
    pub fn mismatch(self, plugin: ProtocolVersion, label: &str, host_name: &str) -> Option<String> {
        if self.accepts(plugin) {
            None
        } else if plugin.major < self.major {
            Some(format!(
                "{label} was built for an older contract ({plugin}) than this {host_name} speaks ({self}); update the plugin to a \
                 version built for {}.x",
                self.major
            ))
        } else if plugin.major > self.major {
            Some(format!(
                "{label} needs a newer {host_name}: it was built for {plugin} and this {host_name} speaks {self}; update \
                 {host_name} to a version that speaks {plugin} or later"
            ))
        } else {
            Some(format!(
                "{label} needs {host_name} to speak {plugin} or newer (it speaks {self}); update {host_name}"
            ))
        }
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for ProtocolVersion {
    type Err = String;

    /// Parses `major.minor` (two numbers up to 65535).
    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("\"{s}\" is not a version like 2.0 (major.minor)");
        let (major, minor) = s.trim().split_once('.').ok_or_else(bad)?;
        let number = |t: &str| -> Result<u16, String> {
            if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            t.parse().map_err(|_| bad())
        };
        Ok(ProtocolVersion::new(number(major)?, number(minor)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_is_same_major_and_known_minor() {
        let host = ProtocolVersion::new(2, 3);
        assert!(host.accepts(ProtocolVersion::new(2, 0)));
        assert!(host.accepts(ProtocolVersion::new(2, 3)));
        assert!(!host.accepts(ProtocolVersion::new(2, 4)));
        assert!(!host.accepts(ProtocolVersion::new(1, 0)));
        assert!(!host.accepts(ProtocolVersion::new(3, 0)));
    }

    #[test]
    fn a_refusal_says_who_must_update() {
        let host = ProtocolVersion::new(2, 3);
        assert_eq!(
            host.mismatch(ProtocolVersion::new(2, 1), "plugin X", "Debrief"),
            None
        );
        let old = host
            .mismatch(ProtocolVersion::new(1, 0), "plugin X", "Debrief")
            .unwrap();
        assert!(
            old.contains("update the plugin") && old.contains("2.x"),
            "{old}"
        );
        let new_major = host
            .mismatch(ProtocolVersion::new(3, 0), "plugin X", "Debrief")
            .unwrap();
        assert!(new_major.contains("update Debrief"), "{new_major}");
        let new_minor = host
            .mismatch(ProtocolVersion::new(2, 4), "plugin X", "Debrief")
            .unwrap();
        assert!(
            new_minor.contains("2.4 or newer") && new_minor.contains("update Debrief"),
            "{new_minor}"
        );
    }

    #[test]
    fn it_parses_and_displays_major_dot_minor() {
        assert_eq!(
            "2.10".parse::<ProtocolVersion>().unwrap(),
            ProtocolVersion::new(2, 10)
        );
        assert_eq!(ProtocolVersion::new(2, 10).to_string(), "2.10");
        for bad in [
            "", "2", "2.", ".1", "a.b", "1.2.3", "-1.0", "70000.0", "1.+2",
        ] {
            assert!(bad.parse::<ProtocolVersion>().is_err(), "{bad}");
        }
    }

    #[test]
    fn it_roundtrips_through_postcard_and_json() {
        let v = ProtocolVersion::new(2, 7);
        let bytes = postcard::to_stdvec(&v).unwrap();
        assert_eq!(postcard::from_bytes::<ProtocolVersion>(&bytes).unwrap(), v);
        assert_eq!(
            serde_json::from_str::<ProtocolVersion>(&serde_json::to_string(&v).unwrap()).unwrap(),
            v
        );
    }
}
