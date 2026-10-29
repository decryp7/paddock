//! Why a folder could not be read, said honestly per folder (the UI shows it as the reason of an "Unavailable" mark).

use super::source::ReadError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

/// Why one folder is unreadable. The engine decides; the UI only words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnreadableReason {
    /// macOS refused a folder under a privacy-protected location: the user can allow it in Full Disk Access.
    NeedsFullDiskAccess,
    /// The OS refused for lack of rights (Windows and Linux, and macOS outside the privacy-protected locations).
    NeedsAdmin,
    /// Any other error (a read failure, a folder that vanished, ...).
    Other,
}

/// The operating system whose rules decide the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
    /// Linux and every other Unix.
    Linux,
}

impl Os {
    /// The OS this build runs on (anything that is neither macOS nor Windows counts as Linux).
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Folders under a home folder that macOS protects with a privacy permission (TCC): reading them needs Full Disk Access.
const HOME_PROTECTED: &[&[&str]] = &[
    &["Desktop"],
    &["Documents"],
    &["Downloads"],
    &["Library", "Mail"],
    &["Library", "Messages"],
    &["Library", "Safari"],
    &["Library", "Calendars"],
    &["Library", "Cookies"],
    &["Library", "HomeKit"],
    &["Library", "Suggestions"],
    &["Library", "IdentityServices"],
    &["Library", "Containers"],
    &["Library", "Group Containers"],
    &["Library", "Application Support", "AddressBook"],
    &["Library", "Application Support", "CallHistoryDB"],
    &["Library", "Application Support", "com.apple.TCC"],
];

/// Whether `path` is a macOS location behind a privacy permission.
fn is_privacy_protected(path: &Path) -> bool {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let starts = |at: usize, prefix: &[&str]| parts.len() >= at + prefix.len() && prefix.iter().zip(&parts[at..]).all(|(a, b)| a == b);
    // .../Users/<name>/<protected>; anywhere in the path, so a volume mounted under /Volumes or /System/Volumes/Data counts too.
    let in_home = (0..parts.len()).any(|k| parts[k] == "Users" && parts.len() > k + 2 && HOME_PROTECTED.iter().any(|p| starts(k + 2, p)));
    in_home || (0..parts.len()).any(|k| starts(k, &["Library", "Application Support", "com.apple.TCC"]))
}

/// The reason a folder at `path` could not be read, from the typed error and the OS.
pub fn unreadable_reason(error: &ReadError, os: Os, path: &Path) -> UnreadableReason {
    match error {
        ReadError::PermissionDenied if os == Os::MacOs && is_privacy_protected(path) => UnreadableReason::NeedsFullDiskAccess,
        ReadError::PermissionDenied => UnreadableReason::NeedsAdmin,
        _ => UnreadableReason::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::PathBuf;

    fn denied() -> ReadError {
        ReadError::PermissionDenied
    }

    #[test]
    fn macos_denial_under_a_privacy_protected_folder_needs_full_disk_access() {
        for p in [
            "/Users/alex/Library/Mail",
            "/Users/alex/Library/Messages/chat.db",
            "/Users/alex/Library/Safari",
            "/Users/alex/Desktop",
            "/Users/alex/Documents/taxes",
            "/Users/alex/Downloads",
            "/Library/Application Support/com.apple.TCC",
            "/Volumes/Macintosh HD/Users/alex/Library/Mail",
            "/System/Volumes/Data/Users/alex/Desktop",
        ] {
            assert_eq!(
                unreadable_reason(&denied(), Os::MacOs, &PathBuf::from(p)),
                UnreadableReason::NeedsFullDiskAccess,
                "{p}"
            );
        }
    }

    #[test]
    fn macos_denial_elsewhere_is_an_ordinary_rights_problem() {
        for p in [
            "/private/var/root",
            "/Users/alex/Projects/secret",
            "/Users/alex/Library/Caches/x",
            "/Users",
        ] {
            assert_eq!(
                unreadable_reason(&denied(), Os::MacOs, &PathBuf::from(p)),
                UnreadableReason::NeedsAdmin,
                "{p}"
            );
        }
    }

    #[test]
    fn windows_and_linux_denial_needs_administrator_rights_wherever_it_is() {
        for os in [Os::Windows, Os::Linux] {
            for p in ["/Users/alex/Library/Mail", "C:\\Windows\\System32\\config", "/root"] {
                assert_eq!(
                    unreadable_reason(&denied(), os, Path::new(p)),
                    UnreadableReason::NeedsAdmin,
                    "{os:?} {p}"
                );
            }
        }
    }

    #[test]
    fn any_other_error_has_its_own_reason_on_every_os() {
        let other = ReadError::Other(io::Error::other("input/output error").to_string());
        for os in [Os::MacOs, Os::Windows, Os::Linux] {
            assert_eq!(
                unreadable_reason(&other, os, Path::new("/Users/alex/Library/Mail")),
                UnreadableReason::Other
            );
            assert_eq!(
                unreadable_reason(&ReadError::NotFound, os, Path::new("/x")),
                UnreadableReason::Other
            );
        }
    }

    #[test]
    fn serialises_in_camel_case() {
        assert_eq!(
            serde_json::to_string(&UnreadableReason::NeedsFullDiskAccess).unwrap(),
            "\"needsFullDiskAccess\""
        );
        assert_eq!(serde_json::to_string(&UnreadableReason::NeedsAdmin).unwrap(), "\"needsAdmin\"");
        assert_eq!(serde_json::to_string(&UnreadableReason::Other).unwrap(), "\"other\"");
    }
}
