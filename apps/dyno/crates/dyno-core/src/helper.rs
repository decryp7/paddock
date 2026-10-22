//! The privileged helper's closed set of system changes (ADR 0006). Every change has an exact inverse, is validated
//! here (the helper validates again, it trusts nobody), and is shown to the user as preview lines before it runs.

use serde::{Deserialize, Serialize};

/// The Windows "Balanced" power scheme.
pub const BALANCED_SCHEME: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
/// The Windows "Power saver" scheme.
pub const POWER_SAVER_SCHEME: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";

/// Where a startup entry the helper changes is registered (all-users locations only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MachineStartup {
    /// `HKLM\…\Run`.
    RunKey,
    /// The all-users Startup folder.
    StartupFolder,
}

/// One system change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperAction {
    /// Activates a power scheme (Windows, by GUID).
    SetPowerScheme(String),
    /// Sets `VisualFXSetting` (0 let Windows choose, 1 appearance, 2 performance, 3 custom).
    SetVisualEffects(u32),
    /// Turns background apps off (true) or on for this user.
    SetBackgroundAppsDisabled(bool),
    /// Excludes (true) or re-includes folders from content indexing (Windows `NotContentIndexed`; macOS
    /// `.metadata_never_index`).
    SetIndexingExcluded {
        /// Folders.
        paths: Vec<String>,
        /// Exclude or include.
        excluded: bool,
    },
    /// Adds (true) or removes Microsoft Defender exclusions for folders (needs administrator rights).
    SetDefenderExcluded {
        /// Folders.
        paths: Vec<String>,
        /// Add or remove.
        excluded: bool,
    },
    /// Turns macOS Low Power Mode on or off (needs administrator rights).
    SetLowPowerMode(bool),
    /// Enables or disables an all-users startup entry by name (needs administrator rights).
    SetMachineStartupEnabled {
        /// Where.
        location: MachineStartup,
        /// The value or file name.
        name: String,
        /// Enable or disable.
        enabled: bool,
    },
}

/// The most folders one action may name.
pub const MAX_PATHS: usize = 64;

fn is_guid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn check_paths(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() || paths.len() > MAX_PATHS {
        return Err(format!("Name 1–{MAX_PATHS} folders."));
    }
    for p in paths {
        let absolute = p.starts_with('/') || (p.len() > 2 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic());
        if !absolute || p.contains("..") || p.len() > 1024 || p.contains('\0') {
            return Err(format!("{p} is not an absolute folder path."));
        }
        let n = p.replace('\\', "/").trim_end_matches('/').to_lowercase();
        if n.len() <= 3 || n == "/system" || n.starts_with("c:/windows") || n == "/users" || n == "c:/users" {
            return Err(format!("{p} is too broad to change."));
        }
    }
    Ok(())
}

impl HelperAction {
    /// Checks the arguments.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            HelperAction::SetPowerScheme(g) if !is_guid(g) => Err(format!("{g} is not a power scheme id.")),
            HelperAction::SetVisualEffects(v) if *v > 3 => Err("Visual effects setting is 0–3.".into()),
            HelperAction::SetIndexingExcluded { paths, .. } | HelperAction::SetDefenderExcluded { paths, .. } => check_paths(paths),
            HelperAction::SetMachineStartupEnabled { name, .. }
                if name.trim().is_empty() || name.len() > 260 || name.contains(['\\', '/', '\0']) =>
            {
                Err("That startup entry name is not valid.".into())
            }
            _ => Ok(()),
        }
    }

    /// Whether it needs administrator rights (the helper then runs through a UAC / admin prompt).
    pub fn needs_admin(&self) -> bool {
        matches!(
            self,
            HelperAction::SetDefenderExcluded { .. } | HelperAction::SetLowPowerMode(_) | HelperAction::SetMachineStartupEnabled { .. }
        )
    }

    /// What it changes, in words, one line per change.
    pub fn preview(&self) -> Vec<String> {
        match self {
            HelperAction::SetPowerScheme(g) => vec![format!("Activate the power plan {}", scheme_name(g))],
            HelperAction::SetVisualEffects(v) => vec![format!(
                "Visual effects: {}",
                match v {
                    0 => "let Windows choose",
                    1 => "best appearance",
                    2 => "best performance (no animations, shadows or transparency)",
                    _ => "custom",
                }
            )],
            HelperAction::SetBackgroundAppsDisabled(true) => vec!["Stop apps from running in the background".into()],
            HelperAction::SetBackgroundAppsDisabled(false) => vec!["Let apps run in the background again".into()],
            HelperAction::SetIndexingExcluded { paths, excluded } => paths
                .iter()
                .map(|p| format!("{} search indexing: {p}", if *excluded { "Exclude from" } else { "Include in" }))
                .collect(),
            HelperAction::SetDefenderExcluded { paths, excluded } => paths
                .iter()
                .map(|p| format!("{} Microsoft Defender scanning: {p}", if *excluded { "Exclude from" } else { "Include in" }))
                .collect(),
            HelperAction::SetLowPowerMode(on) => vec![format!("Turn Low Power Mode {}", if *on { "on" } else { "off" })],
            HelperAction::SetMachineStartupEnabled { name, enabled, .. } => {
                vec![format!("{} {name} at login for every user", if *enabled { "Start" } else { "Don't start" })]
            }
        }
    }
}

/// A known scheme's name.
pub fn scheme_name(guid: &str) -> String {
    match guid.to_lowercase().as_str() {
        BALANCED_SCHEME => "Balanced".into(),
        POWER_SAVER_SCHEME => "Power saver".into(),
        "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c" => "High performance".into(),
        _ => guid.to_string(),
    }
}

/// The helper's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperOutcome {
    /// Done.
    Done,
    /// Some parts failed (each with the reason); the rest was done.
    Partial(Vec<String>),
    /// Nothing was done.
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        assert!(HelperAction::SetPowerScheme(BALANCED_SCHEME.into()).validate().is_ok());
        assert!(HelperAction::SetPowerScheme("nope".into()).validate().is_err());
        assert!(HelperAction::SetPowerScheme("381b4222-f694-41f0-9685-ff5bb260dfzz".into()).validate().is_err());
        assert!(HelperAction::SetVisualEffects(2).validate().is_ok());
        assert!(HelperAction::SetVisualEffects(9).validate().is_err());
        let ok = HelperAction::SetIndexingExcluded {
            paths: vec!["C:\\Users\\me\\src\\app".into(), "/Users/me/src".into()],
            excluded: true,
        };
        assert!(ok.validate().is_ok());
        for bad in ["relative\\path", "C:\\", "C:\\Windows\\System32", "C:\\Users", "/Users", "C:\\a\\..\\b", "/"] {
            let a = HelperAction::SetDefenderExcluded {
                paths: vec![bad.into()],
                excluded: true,
            };
            assert!(a.validate().is_err(), "{bad}");
        }
        assert!(HelperAction::SetDefenderExcluded {
            paths: vec![],
            excluded: true
        }
        .validate()
        .is_err());
        let s = |name: &str| HelperAction::SetMachineStartupEnabled {
            location: MachineStartup::RunKey,
            name: name.into(),
            enabled: false,
        };
        assert!(s("Updater").validate().is_ok());
        assert!(s("a\\b").validate().is_err());
        assert!(s(" ").validate().is_err());
        assert!(HelperAction::SetLowPowerMode(true).validate().is_ok());
    }

    #[test]
    fn previews_and_rights() {
        let all = vec![
            HelperAction::SetPowerScheme(BALANCED_SCHEME.into()),
            HelperAction::SetVisualEffects(0),
            HelperAction::SetVisualEffects(1),
            HelperAction::SetVisualEffects(2),
            HelperAction::SetVisualEffects(3),
            HelperAction::SetBackgroundAppsDisabled(true),
            HelperAction::SetBackgroundAppsDisabled(false),
            HelperAction::SetIndexingExcluded {
                paths: vec!["/a/b".into()],
                excluded: false,
            },
            HelperAction::SetDefenderExcluded {
                paths: vec!["/a/b".into()],
                excluded: false,
            },
            HelperAction::SetLowPowerMode(false),
            HelperAction::SetMachineStartupEnabled {
                location: MachineStartup::StartupFolder,
                name: "x".into(),
                enabled: true,
            },
        ];
        for a in &all {
            assert!(!a.preview().is_empty());
        }
        assert_eq!(all.iter().filter(|a| a.needs_admin()).count(), 3);
        assert_eq!(all[0].preview()[0], "Activate the power plan Balanced");
        assert_eq!(scheme_name(POWER_SAVER_SCHEME), "Power saver");
        assert_eq!(scheme_name("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"), "High performance");
        assert_eq!(scheme_name("x"), "x");
    }
}
