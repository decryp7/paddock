//! Whether the desktop shows Dyno's tray icons (pure; the D-Bus reads are the app's adapter).
//!
//! On Linux a tray icon is a StatusNotifier item: it shows only when a host (the shell's panel or an extension) has registered
//! with the `org.kde.StatusNotifierWatcher` service. Stock GNOME has none until the AppIndicator extension is on, and the icons
//! are then invisible without any error. The adapter reads the watcher ([`HostProbe`]); this module decides what that means
//! ([`availability`]), tells a change from a repeat ([`HostTracker`]) and holds the words the person is shown, so the UI only
//! renders what the engine decided.

use serde::Serialize;

/// The first sentence of the note: what is wrong.
pub const NO_HOST_SUMMARY: &str = "Your desktop isn't showing tray icons.";
/// The second sentence: what to do about it.
pub const NO_HOST_ADVICE: &str =
    "On GNOME, turn on the AppIndicator extension (gnome-shell-extension-appindicator), then Dyno's icons appear.";

/// What the adapter read from the session bus about the StatusNotifier host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostProbe {
    /// The watcher is on the bus and a host has registered with it.
    Present,
    /// Nothing owns `org.kde.StatusNotifierWatcher`: no panel or extension offers tray icons.
    WatcherAbsent,
    /// The watcher is there but `IsStatusNotifierHostRegistered` is false: nothing displays the icons yet.
    NoHostRegistered,
    /// The bus could not be read; why (for the log).
    CantTell(String),
}

/// Whether the icons are shown, as the UI is told (camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrayAvailability {
    /// The desktop shows the icons (or this system has no such host to wait for).
    Shown,
    /// It does not. `hint` is the whole note for the person: what is wrong and what to do.
    NoHost {
        /// The note: [`NO_HOST_SUMMARY`] then [`NO_HOST_ADVICE`].
        hint: String,
    },
}

impl TrayAvailability {
    /// The no-host state with the standard note.
    pub fn no_host() -> Self {
        TrayAvailability::NoHost {
            hint: format!("{NO_HOST_SUMMARY} {NO_HOST_ADVICE}"),
        }
    }

    /// Whether the icons are shown.
    pub fn is_shown(&self) -> bool {
        matches!(self, TrayAvailability::Shown)
    }
}

/// What a probe means. A bus that cannot be read is not a missing host: the icons may well show, so it counts as shown (the
/// adapter logs why it could not tell) rather than raising a false alarm.
pub fn availability(probe: &HostProbe) -> TrayAvailability {
    match probe {
        HostProbe::Present | HostProbe::CantTell(_) => TrayAvailability::Shown,
        HostProbe::WatcherAbsent | HostProbe::NoHostRegistered => TrayAvailability::no_host(),
    }
}

/// What a probe says in the log (no user content: only the state and, for an unreadable bus, the error text).
pub fn describe(probe: &HostProbe) -> String {
    match probe {
        HostProbe::Present => "a StatusNotifier host is registered".to_string(),
        HostProbe::WatcherAbsent => "nothing owns org.kde.StatusNotifierWatcher (no panel or extension offers tray icons)".to_string(),
        HostProbe::NoHostRegistered => "the StatusNotifierWatcher is there but no host has registered with it".to_string(),
        HostProbe::CantTell(why) => format!("the session bus could not be read ({why}); assuming the icons are shown"),
    }
}

/// What changed since the last look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostChange {
    /// The first look.
    First(TrayAvailability),
    /// The icons were not shown and now are: the icons are made again.
    Appeared,
    /// The icons were shown and now are not.
    Vanished,
    /// Nothing new.
    Same,
}

/// Remembers the last [`TrayAvailability`] and names each change once.
#[derive(Debug, Default)]
pub struct HostTracker {
    last: Option<TrayAvailability>,
}

impl HostTracker {
    /// A tracker that has not looked yet.
    pub fn new() -> Self {
        HostTracker::default()
    }

    /// Records `now` and says what changed.
    pub fn observe(&mut self, now: TrayAvailability) -> HostChange {
        let change = match &self.last {
            None => HostChange::First(now.clone()),
            Some(before) if *before == now => HostChange::Same,
            Some(before) if !before.is_shown() && now.is_shown() => HostChange::Appeared,
            Some(_) if !now.is_shown() => HostChange::Vanished,
            Some(_) => HostChange::Same,
        };
        self.last = Some(now);
        change
    }

    /// The last availability seen.
    pub fn last(&self) -> Option<&TrayAvailability> {
        self.last.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registered_host_shows_the_icons() {
        assert_eq!(availability(&HostProbe::Present), TrayAvailability::Shown);
    }

    #[test]
    fn a_missing_watcher_or_host_means_no_icons_with_the_note() {
        for probe in [HostProbe::WatcherAbsent, HostProbe::NoHostRegistered] {
            match availability(&probe) {
                TrayAvailability::NoHost { hint } => {
                    assert_eq!(
                        hint,
                        "Your desktop isn't showing tray icons. On GNOME, turn on the AppIndicator extension \
                         (gnome-shell-extension-appindicator), then Dyno's icons appear."
                    );
                }
                other => panic!("{probe:?} gave {other:?}"),
            }
        }
    }

    #[test]
    fn an_unreadable_bus_is_not_a_false_alarm() {
        assert_eq!(availability(&HostProbe::CantTell("no bus".into())), TrayAvailability::Shown);
        assert!(describe(&HostProbe::CantTell("no bus".into())).contains("no bus"));
    }

    #[test]
    fn the_tracker_names_each_change_once() {
        let mut t = HostTracker::new();
        assert_eq!(
            t.observe(TrayAvailability::no_host()),
            HostChange::First(TrayAvailability::no_host())
        );
        assert_eq!(t.observe(TrayAvailability::no_host()), HostChange::Same);
        assert_eq!(t.observe(TrayAvailability::Shown), HostChange::Appeared);
        assert_eq!(t.observe(TrayAvailability::Shown), HostChange::Same);
        assert_eq!(t.observe(TrayAvailability::no_host()), HostChange::Vanished);
        assert_eq!(t.last(), Some(&TrayAvailability::no_host()));
    }

    #[test]
    fn a_host_present_from_the_start_is_a_first_look_not_an_appearance() {
        let mut t = HostTracker::new();
        assert_eq!(t.observe(TrayAvailability::Shown), HostChange::First(TrayAvailability::Shown));
    }

    #[test]
    fn the_wire_form_is_camel_case() {
        assert_eq!(serde_json::to_value(TrayAvailability::Shown).unwrap(), serde_json::json!("shown"));
        let v = serde_json::to_value(TrayAvailability::no_host()).unwrap();
        assert!(v["noHost"]["hint"].as_str().unwrap().starts_with(NO_HOST_SUMMARY));
    }
}
