//! Safe mode: a start with everything optional switched off, so an app that crashes or hangs on something the user added
//! (an extension, a plugin, the last session) can still be opened and repaired.
//!
//! Debrief's Safe Mode ("plugins and the saved profile are off"), as a contract every Paddock app reads the same way:
//! [`Mode`] is decided once at launch (the `--safe-mode` flag, or the app's environment variable), and each part that
//! could be the cause asks [`Mode::allows`] for its [`Feature`] before it loads. The app's settings sections and its
//! plugin host read the same answer, so no part can forget to stay off.

use serde::{Deserialize, Serialize};

/// The launch flag that starts the app in safe mode (also parsed by `paddock_window::launch`).
pub const SAFE_MODE_FLAG: &str = "--safe-mode";

/// How the app was started.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Everything on.
    #[default]
    Normal,
    /// Extensions, plugins and the last session are off; the app runs in its own state folder (see
    /// [`InstanceScope`](crate::lifecycle::InstanceScope)) so it cannot disturb the normal instance's session.
    Safe,
}

/// A part of the app that safe mode switches off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// Extensions the user installed or enabled (the app's own add-ons).
    Extensions,
    /// Plugins and sidecars that are not part of the app's core.
    Plugins,
    /// Reopening the previous session (open files, window layout, view state).
    LastSession,
}

impl Feature {
    /// Every feature safe mode governs.
    pub const ALL: [Feature; 3] = [Feature::Extensions, Feature::Plugins, Feature::LastSession];
}

impl Mode {
    /// The mode for a launch: safe when the flag was given or the app's environment variable is set to something other
    /// than empty, `0` or `false`.
    pub fn from_launch(flag: bool, env: Option<&str>) -> Mode {
        let env_on = env.is_some_and(|v| {
            let v = v.trim();
            !(v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false"))
        });
        if flag || env_on {
            Mode::Safe
        } else {
            Mode::Normal
        }
    }

    /// Whether this is safe mode.
    pub fn is_safe(self) -> bool {
        self == Mode::Safe
    }

    /// Whether `feature` may load in this mode. In safe mode none of them may.
    pub fn allows(self, _feature: Feature) -> bool {
        self == Mode::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_or_the_variable_starts_safe_mode() {
        assert_eq!(Mode::from_launch(false, None), Mode::Normal);
        assert_eq!(Mode::from_launch(true, None), Mode::Safe);
        assert_eq!(Mode::from_launch(false, Some("1")), Mode::Safe);
        assert_eq!(Mode::from_launch(false, Some(" yes ")), Mode::Safe);
        for off in ["", "  ", "0", "false", "FALSE"] {
            assert_eq!(Mode::from_launch(false, Some(off)), Mode::Normal, "{off:?}");
        }
        assert_eq!(
            Mode::from_launch(true, Some("0")),
            Mode::Safe,
            "the flag wins"
        );
    }

    #[test]
    fn safe_mode_allows_no_optional_part_and_normal_allows_all() {
        for f in Feature::ALL {
            assert!(Mode::Normal.allows(f));
            assert!(!Mode::Safe.allows(f), "{f:?}");
        }
        assert!(Mode::Safe.is_safe() && !Mode::Normal.is_safe());
        assert_eq!(Mode::default(), Mode::Normal);
    }
}
