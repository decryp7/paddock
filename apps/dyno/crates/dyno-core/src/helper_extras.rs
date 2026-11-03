//! The helper's Windows extras (ADR 0027 WP-10): `powercfg /requests` (what keeps the computer awake) and `wbadmin get versions`
//! (the backups of Backup and Restore). Both need more than a standard account; the answer is detail for the owner and
//! administrators and counts only for anyone else. The parsers are pure.

use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};

/// The most entries kept of either list.
pub const MAX_ITEMS: usize = 256;
/// The longest line kept.
pub const MAX_LINE: usize = 256;

/// What asks the computer to stay awake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Requester {
    /// A program.
    Process,
    /// A driver.
    Driver,
    /// A service.
    Service,
    /// Anything else powercfg lists.
    Other,
}

/// One request to keep the computer awake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeRequest {
    /// `DISPLAY`, `SYSTEM`, `AWAYMODE`, `EXECUTION`, `PERFBOOST`, `ACTIVELOCKSCREEN`.
    pub category: String,
    /// What kind of requester.
    pub requester: Requester,
    /// Its name or path.
    pub name: String,
    /// The reason text it gave, when it gave one.
    pub reason: Option<String>,
}

/// One backup version of Windows Backup and Restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVersion {
    /// When it was taken, as `wbadmin` prints it.
    pub time: String,
    /// Where it is kept.
    pub location: String,
    /// The version identifier to use with `wbadmin`.
    pub identifier: String,
    /// What it can recover (`Volume(s), File(s)`).
    pub recovers: String,
}

/// The answer to `Extras`: detail only where the caller may see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtrasAnswer {
    /// What keeps the computer awake: the count always, the list for the owner and administrators.
    WakeRequests {
        /// How many requests there are.
        count: u32,
        /// The requests, or none for a caller who sees counts only.
        items: Option<Vec<WakeRequest>>,
    },
    /// The backups: the count always, the list for the owner and administrators.
    BackupVersions {
        /// How many versions there are.
        count: u32,
        /// The versions, or none for a caller who sees counts only.
        items: Option<Vec<BackupVersion>>,
    },
}

/// The helper's reader of the extras.
pub trait ExtrasReader: Send + Sync {
    /// `powercfg /requests`.
    fn wake_requests(&self) -> Result<Vec<WakeRequest>, PlatformError>;
    /// `wbadmin get versions`.
    fn backup_versions(&self) -> Result<Vec<BackupVersion>, PlatformError>;
}

/// What a system without these readings says.
pub struct NoExtras;

impl ExtrasReader for NoExtras {
    fn wake_requests(&self) -> Result<Vec<WakeRequest>, PlatformError> {
        Err(PlatformError::Unsupported(
            "What keeps the computer awake is read through the helper on Windows only.".into(),
        ))
    }
    fn backup_versions(&self) -> Result<Vec<BackupVersion>, PlatformError> {
        Err(PlatformError::Unsupported(
            "Backup versions are read through the helper on Windows only.".into(),
        ))
    }
}

fn clip(s: &str) -> String {
    s.trim().chars().take(MAX_LINE).collect()
}

/// `powercfg /requests`: a heading (`SYSTEM:`), then `None.` or entries `[PROCESS] \Device\...\x.exe` each followed by an
/// optional reason line.
pub fn parse_requests(text: &str) -> Vec<WakeRequest> {
    let mut out: Vec<WakeRequest> = Vec::new();
    let mut category = String::new();
    let mut last_open = false;
    for line in text.lines().take(10_000) {
        let t = line.trim();
        if t.is_empty() {
            last_open = false;
        } else if let Some(h) = t
            .strip_suffix(':')
            .filter(|h| h.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c == ' '))
        {
            category = h.trim().to_string();
            last_open = false;
        } else if t.ends_with('.') && !t.starts_with('[') && !last_open {
            // "None." under a heading: nothing is asking.
        } else if let Some(rest) = t.strip_prefix('[') {
            let Some((kind, name)) = rest.split_once(']') else { continue };
            let requester = match kind.trim().to_ascii_uppercase().as_str() {
                "PROCESS" => Requester::Process,
                "DRIVER" => Requester::Driver,
                "SERVICE" => Requester::Service,
                _ => Requester::Other,
            };
            if out.len() < MAX_ITEMS {
                out.push(WakeRequest {
                    category: category.clone(),
                    requester,
                    name: clip(name),
                    reason: None,
                });
            }
            last_open = true;
        } else if last_open {
            if let Some(r) = out.last_mut() {
                r.reason = Some(clip(t));
            }
            last_open = false;
        }
    }
    out
}

/// `wbadmin get versions`: blocks of `Backup time:`, `Backup location:`, `Version identifier:`, `Can recover:` lines. The
/// labels are localised, so a block is recognised by position: four `label: value` lines in a row. "No backup was found" is an
/// empty list.
pub fn parse_versions(text: &str) -> Vec<BackupVersion> {
    let mut out = Vec::new();
    let mut block: Vec<String> = Vec::new();
    let flush = |block: &mut Vec<String>, out: &mut Vec<BackupVersion>| {
        if block.len() >= 4 && out.len() < MAX_ITEMS {
            out.push(BackupVersion {
                time: clip(&block[0]),
                location: clip(&block[1]),
                identifier: clip(&block[2]),
                recovers: clip(&block[3]),
            });
        }
        block.clear();
    };
    for line in text.lines().take(10_000) {
        let t = line.trim();
        match t.split_once(": ") {
            Some((_, v)) if !t.is_empty() => block.push(v.to_string()),
            _ => flush(&mut block, &mut out),
        }
    }
    flush(&mut block, &mut out);
    out
}

/// The answer for a caller who sees `detail` (or counts only).
pub fn wake_answer(items: Vec<WakeRequest>, detail: bool) -> ExtrasAnswer {
    ExtrasAnswer::WakeRequests {
        count: u32::try_from(items.len()).unwrap_or(u32::MAX),
        items: detail.then_some(items),
    }
}

/// The answer for a caller who sees `detail` (or counts only).
pub fn backup_answer(items: Vec<BackupVersion>, detail: bool) -> ExtrasAnswer {
    ExtrasAnswer::BackupVersions {
        count: u32::try_from(items.len()).unwrap_or(u32::MAX),
        items: detail.then_some(items),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUESTS: &str = "DISPLAY:\nNone.\n\nSYSTEM:\n[PROCESS] \\Device\\HarddiskVolume3\\Tools\\player.exe\nPlaying video\n\n[DRIVER] Realtek Audio (HDAUDIO\\FUNC_01)\nAn audio stream is currently in use.\n\nAWAYMODE:\nNone.\n\nEXECUTION:\n[SERVICE] \\Device\\X\\svc.exe\n\nPERFBOOST:\nNone.\n";

    #[test]
    fn requests_group_by_heading_with_their_reasons() {
        let r = parse_requests(REQUESTS);
        assert_eq!(r.len(), 3);
        assert_eq!(
            (r[0].category.as_str(), r[0].requester, r[0].reason.as_deref()),
            ("SYSTEM", Requester::Process, Some("Playing video"))
        );
        assert_eq!(
            (r[1].requester, r[1].name.as_str()),
            (Requester::Driver, "Realtek Audio (HDAUDIO\\FUNC_01)")
        );
        assert_eq!((r[2].category.as_str(), r[2].reason.clone()), ("EXECUTION", None));
        assert!(parse_requests("DISPLAY:\nNone.\n").is_empty());
    }

    #[test]
    fn versions_are_read_by_position_and_none_found_is_empty() {
        let text = "wbadmin 1.0\n\nBackup time: 11/3/2026 10:00 AM\nBackup location: Network Share labeled \\\\srv\\share\nVersion identifier: 11/03/2026-10:00\nCan recover: Volume(s), File(s)\n\nBackup time: 11/2/2026 10:00 AM\nBackup location: D:\nVersion identifier: 11/02/2026-10:00\nCan recover: Volume(s)\n";
        let v = parse_versions(text);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].identifier.as_str(), v[1].location.as_str()), ("11/03/2026-10:00", "D:"));
        assert!(parse_versions("ERROR - No backup was found.\n").is_empty());
    }

    #[test]
    fn counts_only_callers_get_no_list() {
        let r = parse_requests(REQUESTS);
        assert!(matches!(
            wake_answer(r.clone(), false),
            ExtrasAnswer::WakeRequests { count: 3, items: None }
        ));
        assert!(matches!(wake_answer(r, true), ExtrasAnswer::WakeRequests { count: 3, items: Some(i) } if i.len() == 3));
        assert!(matches!(
            backup_answer(vec![], false),
            ExtrasAnswer::BackupVersions { count: 0, items: None }
        ));
    }
}
