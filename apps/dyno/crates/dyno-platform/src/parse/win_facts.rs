//! Windows settings that health and tuning read, as pure functions over values the OS adapter read: the OS name, restart
//! keys, power plan, visual effects, disk health and background apps. Nothing here touches the machine, so every OS tests it.

use dyno_core::facts::{DiskHealth, DiskHealthStatus, PowerPlan};
use dyno_core::metrics::Reading;

use crate::parse::powercfg::POWER_SAVER_GUID;

/// Most disks listed (a machine has a handful; a broken provider must not fill the page).
pub const MAX_DISKS: usize = 64;

/// The values of `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` that name the OS. Each is `None` when the value is missing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OsValues {
    /// `ProductName`. Still says "Windows 10" on Windows 11.
    pub product_name: Option<String>,
    /// `EditionID`.
    pub edition_id: Option<String>,
    /// `DisplayVersion` ("24H2").
    pub display_version: Option<String>,
    /// `ReleaseId` ("2009"-era name that Windows 10 21H1 and earlier keep).
    pub release_id: Option<String>,
    /// `InstallationType`: "Client", "Server", "Server Core".
    pub installation_type: Option<String>,
    /// `CurrentBuildNumber`.
    pub build: Option<u32>,
    /// `UBR`, the revision after the build number.
    pub ubr: Option<u32>,
}

/// The first build number of Windows 11.
const WINDOWS_11_BUILD: u32 = 22000;

fn clean(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// The OS name for the facts, for example "Windows 11 Pro 24H2 (build 26100.2314)". `ProductName` still says "Windows 10" on a
/// Windows 11 client, so a client at build 22000 or later is named "Windows 11"; a server keeps its product name. `api_build` is
/// the build `RtlGetVersion` reports (not shimmed by a manifest) and wins over the registry's.
pub fn win_os_name(v: &OsValues, api_build: Option<u32>) -> String {
    let build = api_build.or(v.build);
    let server = clean(&v.installation_type).is_some_and(|t| t.to_ascii_lowercase().starts_with("server"));
    let mut name = match (clean(&v.product_name), clean(&v.edition_id)) {
        (Some(p), _) => p.to_string(),
        (None, Some(e)) => format!("Windows {e}"),
        (None, None) => "Windows".to_string(),
    };
    if !server && build.is_some_and(|b| b >= WINDOWS_11_BUILD) {
        if let Some(rest) = name.strip_prefix("Windows 10") {
            if rest.is_empty() || rest.starts_with(' ') {
                name = format!("Windows 11{rest}");
            }
        }
    }
    if let Some(version) = clean(&v.display_version).or_else(|| clean(&v.release_id)) {
        name = format!("{name} {version}");
    }
    match (build, v.ubr) {
        (Some(b), Some(u)) => format!("{name} (build {b}.{u})"),
        (Some(b), None) => format!("{name} (build {b})"),
        _ => name,
    }
}

/// `restart_pending` from the two keys Windows creates while a restart is needed (`RebootRequired` under Windows Update and
/// `RebootPending` under Component Based Servicing). `PendingFileRenameOperations` is not read: many installers set it. A key
/// that exists is proof; with one key unread and the other absent, the answer is unavailable with the reason.
pub fn restart_pending(update_key: Result<bool, String>, servicing_key: Result<bool, String>) -> Reading<bool> {
    match (update_key, servicing_key) {
        (Ok(true), _) | (_, Ok(true)) => Reading::Available(true),
        (Ok(false), Ok(false)) => Reading::Available(false),
        (Err(why), _) | (_, Err(why)) => Reading::unavailable(format!("Windows did not let Dyno read the restart keys ({why}).")),
    }
}

/// `visual_effects_full` from `VisualFXSetting`; a custom setting has no single answer.
pub fn visual_effects(setting: Result<Option<u32>, String>) -> Reading<bool> {
    match setting {
        Ok(s) => match visual_effects_full(s) {
            Some(on) => Reading::Available(on),
            None => Reading::unavailable("Visual effects are set to custom, so there is no single answer."),
        },
        Err(why) => Reading::unavailable(format!("Windows did not let Dyno read the visual effects setting ({why}).")),
    }
}

/// "Best power efficiency", Windows 11's lowest power mode (`OVERLAY_SCHEME_MIN`, measured in the WP-0 run).
const OVERLAY_BEST_EFFICIENCY: &str = "961cc777-2547-4f9d-8174-7d86181b8a7a";
/// "Better performance" (`OVERLAY_SCHEME_HIGH`).
const OVERLAY_BETTER_PERFORMANCE: &str = "3af9b8d9-7c97-431d-ad78-34a8bfea439f";
/// "Best performance" (`OVERLAY_SCHEME_MAX`).
const OVERLAY_BEST_PERFORMANCE: &str = "ded574b5-45a0-4f42-8737-46345c09c238";

fn guid_lower(g: &str) -> Option<String> {
    let g = g.trim().trim_start_matches('{').trim_end_matches('}');
    let ok = g.len() == 36
        && g.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    ok.then(|| g.to_ascii_lowercase())
}

/// The effective power mode: the active scheme (`PowerGetActiveScheme`, with its friendly name) and, on Windows 11, the effective
/// overlay (`PowerGetEffectiveOverlayScheme`; all zeros means none). The mode is "power saver" for the Power saver scheme or the
/// "Best power efficiency" overlay. An overlay Dyno does not know leaves the scheme alone. `None` without a usable scheme GUID.
pub fn power_plan(scheme: Option<&str>, friendly_name: Option<&str>, overlay: Option<&str>) -> Option<PowerPlan> {
    let scheme = guid_lower(scheme?)?;
    let name = friendly_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(&scheme)
        .to_string();
    let scheme_saver = scheme == POWER_SAVER_GUID;
    let overlay = overlay.and_then(guid_lower);
    let overlay_name = match overlay.as_deref() {
        Some(OVERLAY_BEST_EFFICIENCY) => Some("Best power efficiency"),
        Some(OVERLAY_BETTER_PERFORMANCE) => Some("Better performance"),
        Some(OVERLAY_BEST_PERFORMANCE) => Some("Best performance"),
        _ => None,
    };
    Some(match (overlay, overlay_name) {
        (Some(id), Some(label)) => PowerPlan {
            power_saver: scheme_saver || id == OVERLAY_BEST_EFFICIENCY,
            name: format!("{name} ({label})"),
            id,
        },
        _ => PowerPlan {
            power_saver: scheme_saver,
            name,
            id: scheme,
        },
    })
}

/// A GUID as the lowercase text the power parsers and tables use (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`).
pub fn format_guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> String {
    format!(
        "{data1:08x}-{data2:04x}-{data3:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        data4[0], data4[1], data4[2], data4[3], data4[4], data4[5], data4[6], data4[7]
    )
}

/// One row of WMI `root\Microsoft\Windows\Storage` `MSFT_PhysicalDisk`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawDisk {
    /// `FriendlyName` (the model).
    pub friendly_name: Option<String>,
    /// `HealthStatus`: 0 healthy, 1 warning, 2 unhealthy, 5 unknown.
    pub health_status: Option<u16>,
}

/// Disk health from the storage provider's rows; a row without a name is "Disk <n>" (counted from 1).
pub fn disk_health(rows: &[RawDisk]) -> Vec<DiskHealth> {
    rows.iter()
        .take(MAX_DISKS)
        .enumerate()
        .map(|(i, d)| DiskHealth {
            name: clean(&d.friendly_name).map_or_else(|| format!("Disk {}", i + 1), str::to_string),
            status: match d.health_status {
                Some(0) => DiskHealthStatus::Healthy,
                Some(1) => DiskHealthStatus::Warning,
                Some(2) => DiskHealthStatus::Unhealthy,
                _ => DiskHealthStatus::Unknown,
            },
            wear: None,
            temperature: None,
        })
        .collect()
}

/// Why the disk health query did not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskReadError {
    /// The caller may not read the storage provider (a standard user, measured in WP-0).
    AccessDenied,
    /// Any other failure, with the HRESULT.
    Failed(String),
}

/// Sorts a failed WMI call's HRESULT: `WBEM_E_ACCESS_DENIED` and `E_ACCESSDENIED` are "needs administrator readings".
pub fn classify_wmi_hresult(hresult: i32) -> DiskReadError {
    match hresult as u32 {
        0x8004_1003 | 0x8007_0005 => DiskReadError::AccessDenied,
        other => DiskReadError::Failed(format!("0x{other:08X}")),
    }
}

/// Whether full visual effects are on, from `HKCU\…\Explorer\VisualEffects\VisualFXSetting` (missing means 0, "Let
/// Windows choose", which is full effects on a capable machine): 0 and 1 → on, 2 ("best performance") → off,
/// 3 ("custom") → `None`.
pub fn visual_effects_full(setting: Option<u32>) -> Option<bool> {
    match setting.unwrap_or(0) {
        0 | 1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// One app under `HKCU\…\BackgroundAccessApplications`: its `Disabled` and `DisabledByUser` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackgroundApp {
    /// `Disabled` (1 = may not run in the background).
    pub disabled: Option<u32>,
    /// `DisabledByUser`.
    pub disabled_by_user: Option<u32>,
}

/// How many apps may run in the background; none when the user turned background apps off globally.
pub fn background_app_count(global_disabled: Option<u32>, apps: &[BackgroundApp]) -> u32 {
    if global_disabled == Some(1) {
        return 0;
    }
    let allowed = apps
        .iter()
        .filter(|a| a.disabled != Some(1) && a.disabled_by_user != Some(1))
        .count();
    u32::try_from(allowed).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::facts::DiskHealthStatus;

    #[test]
    fn visual_effects_settings() {
        assert_eq!(visual_effects_full(None), Some(true));
        assert_eq!(visual_effects_full(Some(1)), Some(true));
        assert_eq!(visual_effects_full(Some(2)), Some(false));
        assert_eq!(visual_effects_full(Some(3)), None);
    }

    #[test]
    fn background_apps() {
        let apps = [
            BackgroundApp::default(),
            BackgroundApp {
                disabled: Some(1),
                ..Default::default()
            },
            BackgroundApp {
                disabled_by_user: Some(1),
                ..Default::default()
            },
            BackgroundApp {
                disabled: Some(0),
                disabled_by_user: Some(0),
            },
        ];
        assert_eq!(background_app_count(None, &apps), 2);
        assert_eq!(background_app_count(Some(0), &apps), 2);
        assert_eq!(background_app_count(Some(1), &apps), 0);
    }

    fn os(product: &str, install: Option<&str>, version: Option<&str>, ubr: Option<u32>, build: Option<u32>) -> OsValues {
        OsValues {
            product_name: Some(product.into()),
            edition_id: None,
            display_version: version.map(Into::into),
            release_id: None,
            installation_type: install.map(Into::into),
            build,
            ubr,
        }
    }

    #[test]
    fn windows_10_is_named_as_it_says() {
        let v = os("Windows 10 Pro", Some("Client"), Some("22H2"), Some(5011), Some(19045));
        assert_eq!(win_os_name(&v, None), "Windows 10 Pro 22H2 (build 19045.5011)");
    }

    #[test]
    fn windows_11_that_still_says_windows_10_is_renamed_by_its_build() {
        // The registry says "Windows 10 Pro" on Windows 11 (measured on build 26300 in WP-0).
        let v = os("Windows 10 Pro", Some("Client"), Some("24H2"), Some(2314), Some(26100));
        assert_eq!(win_os_name(&v, None), "Windows 11 Pro 24H2 (build 26100.2314)");
        // The build from RtlGetVersion wins over a stale registry value.
        let v = os("Windows 10 Home", Some("Client"), Some("23H2"), None, Some(19045));
        assert_eq!(win_os_name(&v, Some(22631)), "Windows 11 Home 23H2 (build 22631)");
        // Build 21999 is still Windows 10 (Insider builds before the release).
        assert_eq!(
            win_os_name(&os("Windows 10 Pro", None, None, None, Some(21999)), None),
            "Windows 10 Pro (build 21999)"
        );
    }

    #[test]
    fn a_server_keeps_its_product_name() {
        let v = os("Windows Server 2025 Standard", Some("Server"), Some("24H2"), Some(1), Some(26100));
        assert_eq!(win_os_name(&v, None), "Windows Server 2025 Standard 24H2 (build 26100.1)");
        // Even a server whose product name starts with "Windows 10" is not renamed.
        let v = os("Windows 10 Enterprise LTSC", Some("Server Core"), None, None, Some(26100));
        assert_eq!(win_os_name(&v, None), "Windows 10 Enterprise LTSC (build 26100)");
    }

    #[test]
    fn the_version_falls_back_to_release_id_and_gaps_are_left_out() {
        let mut v = os("Windows 10 Pro", Some("Client"), None, None, Some(19044));
        v.release_id = Some("21H2".into());
        assert_eq!(win_os_name(&v, None), "Windows 10 Pro 21H2 (build 19044)");
        // No DisplayVersion, no ReleaseId, no UBR, no build.
        assert_eq!(win_os_name(&os("Windows 10 Pro", None, None, None, None), None), "Windows 10 Pro");
        // No ProductName: the edition, then plain "Windows".
        let v = OsValues {
            edition_id: Some("Professional".into()),
            ..Default::default()
        };
        assert_eq!(win_os_name(&v, Some(26100)), "Windows Professional (build 26100)");
        assert_eq!(win_os_name(&OsValues::default(), None), "Windows");
        // A product name that only starts with the same letters is not "Windows 10".
        assert_eq!(
            win_os_name(&os("Windows 100", Some("Client"), None, None, Some(30000)), None),
            "Windows 100 (build 30000)"
        );
    }

    const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
    const NIL: &str = "00000000-0000-0000-0000-000000000000";

    #[test]
    fn the_power_plan_is_the_effective_mode() {
        // No overlay (Windows 10, or Balanced): the scheme itself.
        let p = power_plan(Some(BALANCED), Some("Balanced"), None).unwrap();
        assert_eq!((p.id.as_str(), p.name.as_str(), p.power_saver), (BALANCED, "Balanced", false));
        let p = power_plan(Some(BALANCED), Some("Balanced"), Some(NIL)).unwrap();
        assert_eq!((p.id.as_str(), p.power_saver), (BALANCED, false));
        // The Power saver scheme.
        let p = power_plan(Some("A1841308-3541-4FAB-BC81-F71556F20B4A"), Some("Power saver"), Some(NIL)).unwrap();
        assert!(p.power_saver);
        // The overlay GUIDs measured in WP-0 item 8.
        let p = power_plan(Some(BALANCED), Some("Balanced"), Some("961cc777-2547-4f9d-8174-7d86181b8a7a")).unwrap();
        assert!(p.power_saver, "Best power efficiency is power saver");
        assert_eq!(p.id, "961cc777-2547-4f9d-8174-7d86181b8a7a");
        assert_eq!(p.name, "Balanced (Best power efficiency)");
        let p = power_plan(Some(BALANCED), Some("Balanced"), Some("3AF9B8D9-7C97-431D-AD78-34A8BFEA439F")).unwrap();
        assert_eq!((p.power_saver, p.name.as_str()), (false, "Balanced (Better performance)"));
        let p = power_plan(Some(BALANCED), Some("Balanced"), Some("ded574b5-45a0-4f42-8737-46345c09c238")).unwrap();
        assert_eq!((p.power_saver, p.name.as_str()), (false, "Balanced (Best performance)"));
        // An overlay Dyno does not know leaves the scheme alone; a scheme without a name is named by its GUID.
        let p = power_plan(Some(BALANCED), None, Some("12345678-1234-1234-1234-123456789abc")).unwrap();
        assert_eq!((p.id.as_str(), p.name.as_str()), (BALANCED, BALANCED));
        // Garbage and nothing.
        assert!(power_plan(None, Some("x"), None).is_none());
        assert!(power_plan(Some("not a guid"), None, None).is_none());
    }

    #[test]
    fn disk_health_maps_the_storage_status_values() {
        let disks = [
            RawDisk {
                friendly_name: Some("Model A".into()),
                health_status: Some(0),
            },
            RawDisk {
                friendly_name: Some("Model B".into()),
                health_status: Some(1),
            },
            RawDisk {
                friendly_name: None,
                health_status: Some(2),
            },
            RawDisk {
                friendly_name: Some("  ".into()),
                health_status: Some(5),
            },
            RawDisk {
                friendly_name: Some("Model E".into()),
                health_status: None,
            },
            RawDisk {
                friendly_name: Some("Model F".into()),
                health_status: Some(99),
            },
        ];
        let h = disk_health(&disks);
        let got: Vec<_> = h.iter().map(|d| (d.name.as_str(), d.status)).collect();
        assert_eq!(
            got,
            [
                ("Model A", DiskHealthStatus::Healthy),
                ("Model B", DiskHealthStatus::Warning),
                ("Disk 3", DiskHealthStatus::Unhealthy),
                ("Disk 4", DiskHealthStatus::Unknown),
                ("Model E", DiskHealthStatus::Unknown),
                ("Model F", DiskHealthStatus::Unknown),
            ]
        );
        assert!(h.iter().all(|d| d.wear.is_none() && d.temperature.is_none()));
        assert_eq!(disk_health(&vec![RawDisk::default(); 500]).len(), MAX_DISKS);
    }

    #[test]
    fn wmi_failures_are_told_apart() {
        assert_eq!(classify_wmi_hresult(0x8004_1003_u32 as i32), DiskReadError::AccessDenied);
        assert_eq!(classify_wmi_hresult(0x8007_0005_u32 as i32), DiskReadError::AccessDenied);
        assert!(matches!(classify_wmi_hresult(0x8004_1010_u32 as i32), DiskReadError::Failed(m) if m.contains("0x80041010")));
    }

    #[test]
    fn guids_are_written_lowercase_with_zero_padding() {
        assert_eq!(
            format_guid(0x381b_4222, 0xf694, 0x41f0, [0x96, 0x85, 0xff, 0x5b, 0xb2, 0x60, 0xdf, 0x2e]),
            BALANCED
        );
        assert_eq!(format_guid(0, 0, 0, [0; 8]), NIL);
        assert_eq!(
            format_guid(1, 2, 3, [0, 0, 0, 0, 0, 0, 0, 4]),
            "00000001-0002-0003-0000-000000000004"
        );
    }

    #[test]
    fn restart_pending_needs_one_key_or_two_answers() {
        assert_eq!(restart_pending(Ok(false), Ok(false)), Reading::Available(false));
        assert_eq!(restart_pending(Ok(true), Ok(false)), Reading::Available(true));
        assert_eq!(
            restart_pending(Err("denied".into()), Ok(true)),
            Reading::Available(true),
            "one key is proof"
        );
        assert_eq!(
            restart_pending(Ok(false), Err("denied".into())),
            Reading::Unavailable("Windows did not let Dyno read the restart keys (denied).".into())
        );
        assert!(restart_pending(Err("a".into()), Err("b".into())).value().is_none());
    }

    #[test]
    fn visual_effects_reading() {
        assert_eq!(visual_effects(Ok(Some(2))), Reading::Available(false));
        assert_eq!(visual_effects(Ok(None)), Reading::Available(true));
        assert_eq!(
            visual_effects(Ok(Some(3))),
            Reading::Unavailable("Visual effects are set to custom, so there is no single answer.".into())
        );
        assert!(visual_effects(Err("denied".into())).value().is_none());
    }
}
