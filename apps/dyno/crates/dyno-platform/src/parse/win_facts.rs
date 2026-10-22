//! Windows settings that health and tuning read: visual effects and background apps.

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
    let allowed = apps.iter().filter(|a| a.disabled != Some(1) && a.disabled_by_user != Some(1)).count();
    u32::try_from(allowed).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            BackgroundApp { disabled: Some(1), ..Default::default() },
            BackgroundApp { disabled_by_user: Some(1), ..Default::default() },
            BackgroundApp { disabled: Some(0), disabled_by_user: Some(0) },
        ];
        assert_eq!(background_app_count(None, &apps), 2);
        assert_eq!(background_app_count(Some(0), &apps), 2);
        assert_eq!(background_app_count(Some(1), &apps), 0);
    }
}
