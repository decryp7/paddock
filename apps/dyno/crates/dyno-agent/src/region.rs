//! Reads the OS region setting: the country of the user's locale (macOS `AppleLocale`, Windows' Geo setting, `LC_ALL`,
//! `LC_MESSAGES` and `LANG` elsewhere). One short, fixed command at most, with a timeout in the runner; nothing is sent anywhere.

use dyno_core::region::country_from_locale;
#[cfg(windows)]
use dyno_core::region::country_from_windows_geo;
#[cfg(any(target_os = "macos", windows))]
use dyno_platform::runner::tool;
use dyno_platform::runner::ToolRunner;

/// The country of the region setting, or `None` when the OS names none.
pub fn region_country(runner: &mut dyn ToolRunner, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = runner.run(&tool("/usr/bin/defaults", &["read", "-g", "AppleLocale"])) {
            if o.ok {
                if let Some(c) = country_from_locale(o.stdout.trim()) {
                    return Some(c);
                }
            }
        }
    }
    #[cfg(windows)]
    {
        if let Ok(o) = runner.run(&tool("reg.exe", &["query", r"HKCU\Control Panel\International\Geo", "/v", "Name"])) {
            if o.ok {
                if let Some(c) = country_from_windows_geo(&o.stdout) {
                    return Some(c);
                }
            }
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = &runner;
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| env(k).as_deref().and_then(country_from_locale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_platform::runner::{Tool, ToolOutput};

    struct NoTools;
    impl ToolRunner for NoTools {
        fn run(&mut self, _: &Tool) -> Result<ToolOutput, dyno_core::ports::PlatformError> {
            Err(dyno_core::ports::PlatformError::Failed("no tools in tests".into()))
        }
    }

    #[test]
    fn the_environment_names_the_region_in_the_usual_order() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| (*v).to_string())
        };
        let got = region_country(&mut NoTools, &env(&[("LANG", "en_SG.UTF-8")]));
        // On macOS and Windows the OS setting is asked first; with no tool answering, the environment is used.
        assert_eq!(got.as_deref(), Some("SG"));
        assert_eq!(
            region_country(&mut NoTools, &env(&[("LC_ALL", "de_DE"), ("LANG", "en_SG")])).as_deref(),
            Some("DE")
        );
        assert_eq!(region_country(&mut NoTools, &env(&[("LANG", "C")])), None);
        assert_eq!(region_country(&mut NoTools, &env(&[])), None);
    }
}
