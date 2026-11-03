//! Why a folder could not be read, said honestly per folder (the UI shows it as the reason of an "Unavailable" mark).

use super::source::ReadError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Why one folder is unreadable. The engine decides; the UI only words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnreadableReason {
    /// macOS refused a folder under a privacy-protected location: the user can allow it in Full Disk Access.
    NeedsFullDiskAccess,
    /// The OS refused for lack of rights (Windows and Linux, and macOS "permission denied").
    NeedsAdmin,
    /// macOS protects the folder (it said "operation not permitted") and Full Disk Access is not what is missing.
    Protected,
    /// The folder did not answer in time (a cloud placeholder, a network-backed or stuck folder); the volume itself still did.
    NotAnswering,
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

/// What the reason depends on besides the error: the OS, whether macOS Full Disk Access is missing, and whose home folder it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonContext {
    /// The OS whose rules apply.
    pub os: Os,
    /// Whether Full Disk Access is known to be missing (always false off macOS).
    pub fda_missing: bool,
    /// The home folder of the user running Dyno, when known.
    pub home: Option<PathBuf>,
}

impl ReasonContext {
    /// This machine's OS and the user's home folder; `fda_missing` as asked of the OS once per scan.
    pub fn current(fda_missing: bool) -> ReasonContext {
        ReasonContext {
            os: Os::current(),
            fda_missing,
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }
}

/// Folders under a home folder that macOS protects with a privacy permission (TCC). Only a hint, used when the error does not say
/// (a plain "permission denied" on a path that is protected for the user running Dyno); the error code decides first.
const HOME_PROTECTED: &[&[&str]] = &[
    &["Desktop"],
    &["Documents"],
    &["Downloads"],
    &[".Trash"],
    &["Pictures", "Photos Library.photoslibrary"],
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
    &["Library", "Daemon Containers"],
    &["Library", "Biome"],
    &["Library", "Mobile Documents"],
    &["Library", "Metadata", "CoreSpotlight"],
    &["Library", "Application Support", "AddressBook"],
    &["Library", "Application Support", "CallHistoryDB"],
    &["Library", "Application Support", "MobileSync"],
    &["Library", "Application Support", "com.apple.TCC"],
];

fn components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Whether `path` is one of the user's own privacy-protected folders: under `home` (the same user), not another user's.
fn is_own_privacy_protected(path: &Path, home: Option<&Path>) -> bool {
    let Some(home) = home else { return false };
    let (parts, base) = (components(path), components(home));
    // A volume mounted under /Volumes or /System/Volumes/Data shows the same home folder after its prefix.
    (0..=parts.len().saturating_sub(base.len())).any(|k| {
        parts.len() >= k + base.len()
            && parts[k..k + base.len()] == base[..]
            && HOME_PROTECTED
                .iter()
                .any(|p| parts.len() >= k + base.len() + p.len() && p.iter().zip(&parts[k + base.len()..]).all(|(a, b)| a == b))
    })
}

/// The reason a folder at `path` could not be read, from the typed error and where it happened.
///
/// On macOS "operation not permitted" ([`ReadError::NotPermitted`]) is the privacy protection: Full Disk Access when that is
/// what is missing, otherwise the folder is protected by macOS in another way (Photos, iCloud, Time Machine, removable and
/// network volumes). "Permission denied" is ordinary rights, except for a protected folder of the current user when Full Disk
/// Access is missing (the hint list). Windows and Linux: either refusal is a rights problem.
///
/// While Full Disk Access is missing, every macOS EPERM is reported as [`UnreadableReason::NeedsFullDiskAccess`], including reads
/// that System Integrity Protection blocks and that granting the permission would not open: the errno cannot tell them apart.
pub fn unreadable_reason(error: &ReadError, ctx: &ReasonContext, path: &Path) -> UnreadableReason {
    match (error, ctx.os) {
        (ReadError::NotPermitted, Os::MacOs) if ctx.fda_missing => UnreadableReason::NeedsFullDiskAccess,
        (ReadError::NotPermitted, Os::MacOs) => UnreadableReason::Protected,
        (ReadError::PermissionDenied, Os::MacOs) if ctx.fda_missing && is_own_privacy_protected(path, ctx.home.as_deref()) => {
            UnreadableReason::NeedsFullDiskAccess
        }
        (ReadError::PermissionDenied | ReadError::NotPermitted, _) => UnreadableReason::NeedsAdmin,
        (ReadError::Unreachable(_), _) => UnreadableReason::NotAnswering,
        _ => UnreadableReason::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn ctx(os: Os, fda_missing: bool) -> ReasonContext {
        ReasonContext {
            os,
            fda_missing,
            home: Some(PathBuf::from("/Users/alex")),
        }
    }
    fn why(e: &ReadError, c: &ReasonContext, p: &str) -> UnreadableReason {
        unreadable_reason(e, c, Path::new(p))
    }

    #[test]
    fn macos_not_permitted_is_full_disk_access_only_while_it_is_missing() {
        let e = ReadError::NotPermitted;
        for p in [
            "/Users/alex/Library/Mail",
            "/Users/alex/.Trash",
            "/Volumes/Backups/Time Machine",
            "/Volumes/USB/x",
            "/anything",
        ] {
            assert_eq!(why(&e, &ctx(Os::MacOs, true), p), UnreadableReason::NeedsFullDiskAccess, "{p}");
            assert_eq!(
                why(&e, &ctx(Os::MacOs, false), p),
                UnreadableReason::Protected,
                "{p}: granted, so not a reason to grant it"
            );
        }
    }

    #[test]
    fn macos_permission_denied_is_an_ordinary_rights_problem() {
        let e = ReadError::PermissionDenied;
        for p in [
            "/private/var/root",
            "/Users/alex/Projects/secret",
            "/Users/bob",
            "/Users/bob/Desktop",
            "/Users/bob/Library/Mail",
        ] {
            for fda_missing in [true, false] {
                assert_eq!(why(&e, &ctx(Os::MacOs, fda_missing), p), UnreadableReason::NeedsAdmin, "{p}");
            }
        }
    }

    #[test]
    fn another_users_protected_folder_is_never_a_full_disk_access_matter() {
        // Mode 700 in someone else's home: EACCES, whatever the permission says.
        assert_eq!(
            why(&ReadError::PermissionDenied, &ctx(Os::MacOs, true), "/Users/bob/Desktop"),
            UnreadableReason::NeedsAdmin
        );
    }

    #[test]
    fn the_path_hint_covers_the_users_own_protected_folders_when_the_error_does_not_say() {
        let c = ctx(Os::MacOs, true);
        for p in [
            "/Users/alex/Library/Mail",
            "/Users/alex/Desktop",
            "/Users/alex/Documents/taxes",
            "/Users/alex/.Trash",
            "/Users/alex/Pictures/Photos Library.photoslibrary/originals",
            "/Users/alex/Library/Application Support/MobileSync/Backup",
            "/Users/alex/Library/Mobile Documents/com~apple~CloudDocs",
            "/Users/alex/Library/Metadata/CoreSpotlight",
            "/Users/alex/Library/Biome",
            "/Users/alex/Library/Daemon Containers",
            "/System/Volumes/Data/Users/alex/Desktop",
        ] {
            assert_eq!(
                why(&ReadError::PermissionDenied, &c, p),
                UnreadableReason::NeedsFullDiskAccess,
                "{p}"
            );
        }
        assert_eq!(
            why(&ReadError::PermissionDenied, &c, "/Users/alex/Library/Caches/x"),
            UnreadableReason::NeedsAdmin
        );
        assert_eq!(
            why(&ReadError::PermissionDenied, &ctx(Os::MacOs, false), "/Users/alex/Desktop"),
            UnreadableReason::NeedsAdmin
        );
        let no_home = ReasonContext { home: None, ..c };
        assert_eq!(
            why(&ReadError::PermissionDenied, &no_home, "/Users/alex/Desktop"),
            UnreadableReason::NeedsAdmin
        );
    }

    #[test]
    fn windows_and_linux_refusals_need_administrator_rights_either_way() {
        for os in [Os::Windows, Os::Linux] {
            for e in [ReadError::PermissionDenied, ReadError::NotPermitted] {
                for p in ["/Users/alex/Library/Mail", "C:\\Windows\\System32\\config", "/root"] {
                    assert_eq!(why(&e, &ctx(os, false), p), UnreadableReason::NeedsAdmin, "{os:?} {p}");
                }
            }
        }
    }

    #[test]
    fn any_other_error_has_its_own_reason_on_every_os() {
        let other = ReadError::Other(io::Error::other("input/output error").to_string());
        for os in [Os::MacOs, Os::Windows, Os::Linux] {
            assert_eq!(why(&other, &ctx(os, true), "/Users/alex/Library/Mail"), UnreadableReason::Other);
            assert_eq!(why(&ReadError::NotFound, &ctx(os, true), "/x"), UnreadableReason::Other);
        }
    }

    #[test]
    fn a_folder_that_did_not_answer_has_its_own_reason_on_every_os() {
        let slow = ReadError::Unreachable("Operation timed out (os error 60)".into());
        for os in [Os::MacOs, Os::Windows, Os::Linux] {
            assert_eq!(
                why(&slow, &ctx(os, true), "/Users/alex/Library/CloudStorage/x"),
                UnreadableReason::NotAnswering
            );
        }
    }

    #[test]
    fn serialises_in_camel_case() {
        for (r, s) in [
            (UnreadableReason::NeedsFullDiskAccess, "needsFullDiskAccess"),
            (UnreadableReason::NeedsAdmin, "needsAdmin"),
            (UnreadableReason::Protected, "protected"),
            (UnreadableReason::NotAnswering, "notAnswering"),
            (UnreadableReason::Other, "other"),
        ] {
            assert_eq!(serde_json::to_string(&r).unwrap(), format!("\"{s}\""));
        }
    }
}
