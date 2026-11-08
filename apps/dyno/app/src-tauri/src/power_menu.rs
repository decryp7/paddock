//! The tray's Keep awake entries (ADR 0032 section 13), as pure functions: what the submenu offers for the agent's power view,
//! and what a chosen entry asks the agent to do. The tray draws the entries ([`crate::tray`]); the shell sends the request.

use dyno_core::power::awake::{AwakeScope, AwakeSpec, AwakeUntil, AwakeView, BusyCond};
use dyno_core::power::idle::IdleView;
use dyno_core::power::settings::KeepAwakeSettings;
use dyno_core::power::{Capability, PowerView};

/// Entry ids; the shell's tray hands a chosen id back to [`action_of`].
pub const PREFIX: &str = "dy.awake.";
const STOP: &str = "dy.awake.stop";

/// The durations the submenu offers, in order: id, text, minutes (`None` = until stopped).
const CHOICES: [(&str, &str, Option<u32>); 5] = [
    ("dy.awake.30", "30 minutes", Some(30)),
    ("dy.awake.60", "1 hour", Some(60)),
    ("dy.awake.120", "2 hours", Some(120)),
    ("dy.awake.240", "4 hours", Some(240)),
    ("dy.awake.indefinite", "Until I stop it", None),
];

/// One entry of the submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The id handed back when chosen.
    pub id: String,
    /// The words.
    pub text: String,
    /// Whether it can be chosen.
    pub enabled: bool,
    /// Whether it shows a tick (the hold that is active).
    pub checked: bool,
}

/// The submenu and its header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    /// A line above the submenu while a hold is active or has ended (always words).
    pub header: Option<String>,
    /// The durations and "Turn off".
    pub entries: Vec<Entry>,
    /// "Keep the display on" (ticked when the next hold keeps it on).
    pub display: Entry,
    /// "While busy" conditions.
    pub busy: Vec<Entry>,
    /// "Until a program quits": the busiest programs now.
    pub programs: Vec<Entry>,
    /// While a sleep or shut down countdown runs: "Cancel sleep" and "Snooze 30 minutes" (empty otherwise).
    pub countdown: Vec<Entry>,
    /// The countdown's line ("Will sleep in 0:48"), when one runs; the other header lines are about keeping awake.
    pub countdown_header: Option<String>,
}

/// A chosen entry's request.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Hold as asked.
    Start(AwakeSpec),
    /// Release.
    Stop,
    /// Flip "Keep the display on" for the next hold.
    ToggleDisplay,
    /// Cancel the countdown to sleep, hibernate or shut down.
    CancelIdle,
    /// Snooze the countdown for half an hour.
    SnoozeIdle,
}

const CANCEL_IDLE: &str = "dy.awake.cancel-idle";
const SNOOZE_IDLE: &str = "dy.awake.snooze-idle";

const DISPLAY: &str = "dy.awake.display";
const BUSY: [(&str, &str); 4] = [
    ("dy.awake.busy.cpu", "CPU is busy"),
    ("dy.awake.busy.network", "Network is busy"),
    ("dy.awake.busy.disk", "Disk is busy"),
    ("dy.awake.busy.any", "CPU, network or disk is busy"),
];
const PROGRAM_PREFIX: &str = "dy.awake.prog.";
/// How many programs the submenu lists.
pub const MAX_PROGRAMS: usize = 8;

/// "42 min", "1 h 5 min", "2 h", "3 d".
pub fn duration_text(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match (minutes / 1440, (minutes % 1440) / 60, minutes % 60) {
        (0, 0, 0) => "less than a minute".into(),
        (0, 0, m) => format!("{m} min"),
        (0, h, 0) => format!("{h} h"),
        (0, h, m) => format!("{h} h {m} min"),
        (d, 0, _) => format!("{d} d"),
        (d, h, _) => format!("{d} d {h} h"),
    }
}

/// The menu for `view` at `now_ms` (milliseconds since the Unix epoch).
pub fn menu(view: &PowerView, now_ms: i64, programs: &[(u32, String)], display: bool) -> Menu {
    let usable = matches!(view.caps.keep_awake, Capability::Known);
    let active_minutes = match &view.awake {
        AwakeView::Holding { spec, .. } | AwakeView::Waiting { spec, .. } => match spec.until {
            AwakeUntil::For { minutes } => Some(Some(minutes)),
            AwakeUntil::Indefinite => Some(None),
            _ => None,
        },
        _ => None,
    };
    let header = match &view.awake {
        AwakeView::Off => None,
        AwakeView::Holding { ends: Some(e), .. } => Some(format!(
            "Keeping awake, {} left",
            duration_text(e.since(dyno_core::time::Timestamp(now_ms)))
        )),
        AwakeView::Holding { .. } => Some("Keeping awake".into()),
        AwakeView::Waiting { .. } => Some("Keep awake paused while asleep".into()),
        AwakeView::Ended { why, .. } => Some(format!("Keep awake ended: {why}")),
    };
    let mut entries: Vec<Entry> = CHOICES
        .iter()
        .map(|(id, text, minutes)| Entry {
            id: (*id).into(),
            text: (*text).into(),
            enabled: usable,
            checked: active_minutes == Some(*minutes),
        })
        .collect();
    let active = !matches!(view.awake, AwakeView::Off | AwakeView::Ended { .. });
    entries.push(Entry {
        id: STOP.into(),
        text: "Turn off".into(),
        enabled: active,
        checked: false,
    });
    let busy = BUSY
        .iter()
        .map(|(id, text)| Entry {
            id: (*id).into(),
            text: (*text).into(),
            enabled: usable,
            checked: false,
        })
        .collect();
    let programs = programs
        .iter()
        .take(MAX_PROGRAMS)
        .map(|(pid, name)| Entry {
            id: format!("{PROGRAM_PREFIX}{pid}"),
            text: name.clone(),
            enabled: usable,
            checked: false,
        })
        .collect();
    let display = Entry {
        id: DISPLAY.into(),
        text: "Keep the display on".into(),
        enabled: usable,
        checked: display,
    };
    let (countdown_header, countdown) = match &view.idle {
        IdleView::Countdown { action, deadline, .. } => {
            let secs = (deadline.since(dyno_core::time::Timestamp(now_ms)) / 1000).max(0);
            let verb = action.verb();
            let text = format!("Will {verb} in {}:{:02}", secs / 60, secs % 60);
            (
                Some(text),
                vec![
                    Entry {
                        id: CANCEL_IDLE.into(),
                        text: format!("Cancel {verb}"),
                        enabled: true,
                        checked: false,
                    },
                    Entry {
                        id: SNOOZE_IDLE.into(),
                        text: "Snooze 30 minutes".into(),
                        enabled: true,
                        checked: false,
                    },
                ],
            )
        }
        IdleView::Acting { action, .. } => (Some(format!("Waiting for the system to {}", action.verb())), Vec::new()),
        _ => (None, Vec::new()),
    };
    Menu {
        header,
        entries,
        display,
        busy,
        programs,
        countdown,
        countdown_header,
    }
}

/// What a chosen id asks for; `display` and the busy limits are the Settings. `None` for an id that is not ours.
pub fn action_of(id: &str, display: bool, keep: &KeepAwakeSettings) -> Option<Action> {
    let scope = if display {
        AwakeScope::SystemAndDisplay
    } else {
        AwakeScope::System
    };
    let start = |until| Some(Action::Start(AwakeSpec { scope, until }));
    if id == STOP {
        return Some(Action::Stop);
    }
    if id == CANCEL_IDLE {
        return Some(Action::CancelIdle);
    }
    if id == SNOOZE_IDLE {
        return Some(Action::SnoozeIdle);
    }
    if id == DISPLAY {
        return Some(Action::ToggleDisplay);
    }
    if let Some(kind) = id.strip_prefix("dy.awake.busy.") {
        let cpu = BusyCond::Cpu { above_pct: keep.cpu_pct };
        let net = BusyCond::Network {
            above_bytes_per_s: keep.net_bytes_per_s,
        };
        let disk = BusyCond::Disk {
            above_bytes_per_s: keep.disk_bytes_per_s,
        };
        let conds = match kind {
            "cpu" => vec![cpu],
            "network" => vec![net],
            "disk" => vec![disk],
            "any" => vec![cpu, net, disk],
            _ => return None,
        };
        return start(AwakeUntil::WhileBusy {
            conds,
            grace_secs: keep.grace_secs,
        });
    }
    if let Some(pid) = id.strip_prefix(PROGRAM_PREFIX) {
        // The agent adds the start time from its process table (a pid alone never identifies a process, rule 53).
        let pid: u32 = pid.parse().ok()?;
        return start(AwakeUntil::WhileProcess {
            process: dyno_core::process::ProcessRef { pid, started: None },
        });
    }
    let (_, _, minutes) = CHOICES.iter().find(|(c, _, _)| *c == id)?;
    start(minutes.map_or(AwakeUntil::Indefinite, |minutes| AwakeUntil::For { minutes }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::power::PowerCaps;
    use dyno_core::time::Timestamp;

    fn view(awake: AwakeView, caps: Capability) -> PowerView {
        PowerView {
            awake,
            caps: PowerCaps {
                keep_awake: caps,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn holding(until: AwakeUntil, ends: Option<Timestamp>) -> AwakeView {
        AwakeView::Holding {
            spec: AwakeSpec {
                scope: AwakeScope::System,
                until,
            },
            since: Timestamp(0),
            ends,
            os_ref: "r".into(),
            busy: None,
        }
    }

    #[test]
    fn off_offers_the_durations_and_a_disabled_turn_off() {
        let m = menu(&view(AwakeView::Off, Capability::Known), 0, &[], false);
        assert_eq!(m.header, None);
        let texts: Vec<&str> = m.entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["30 minutes", "1 hour", "2 hours", "4 hours", "Until I stop it", "Turn off"]);
        assert!(m.entries[..5].iter().all(|e| e.enabled && !e.checked));
        assert!(!m.entries[5].enabled);
    }

    #[test]
    fn a_hold_shows_the_time_left_ticks_its_entry_and_can_be_turned_off() {
        let v = view(
            holding(AwakeUntil::For { minutes: 60 }, Some(Timestamp(42 * 60_000 + 5_000))),
            Capability::Known,
        );
        let m = menu(&v, 0, &[], false);
        assert_eq!(m.header.as_deref(), Some("Keeping awake, 42 min left"));
        assert_eq!(
            m.entries.iter().filter(|e| e.checked).map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["1 hour"]
        );
        assert!(m.entries.last().unwrap().enabled);
        let forever = menu(&view(holding(AwakeUntil::Indefinite, None), Capability::Known), 0, &[], false);
        assert_eq!(forever.header.as_deref(), Some("Keeping awake"));
        assert!(forever.entries.iter().any(|e| e.checked && e.text == "Until I stop it"));
        // A hold the tray does not offer (a time, a program) ticks nothing.
        let at = menu(
            &view(
                holding(AwakeUntil::At { minute_of_day: 600 }, Some(Timestamp(1))),
                Capability::Known,
            ),
            0,
            &[(1, "a".into())],
            false,
        );
        assert!(at.entries.iter().all(|e| !e.checked));
    }

    #[test]
    fn waiting_and_ended_say_so_in_words() {
        let spec = AwakeSpec {
            scope: AwakeScope::System,
            until: AwakeUntil::Indefinite,
        };
        let w = menu(
            &view(
                AwakeView::Waiting {
                    spec,
                    why: "asleep".into(),
                    since: Timestamp(0),
                    ends: None,
                },
                Capability::Known,
            ),
            0,
            &[(1, "a".into())],
            false,
        );
        assert_eq!(w.header.as_deref(), Some("Keep awake paused while asleep"));
        assert!(w.entries.last().unwrap().enabled);
        let e = menu(
            &view(
                AwakeView::Ended {
                    why: "Dyno restarted".into(),
                    at: Timestamp(0),
                },
                Capability::Known,
            ),
            0,
            &[(1, "a".into())],
            false,
        );
        assert_eq!(e.header.as_deref(), Some("Keep awake ended: Dyno restarted"));
        assert!(!e.entries.last().unwrap().enabled, "nothing to turn off");
    }

    #[test]
    fn when_the_system_cannot_the_durations_are_disabled() {
        let m = menu(
            &view(
                AwakeView::Off,
                Capability::CantTell {
                    reason: "no logind".into(),
                },
            ),
            0,
            &[(1, "a".into())],
            false,
        );
        assert!(m
            .entries
            .iter()
            .chain(&m.busy)
            .chain(&m.programs)
            .chain([&m.display])
            .all(|e| !e.enabled));
    }

    #[test]
    fn a_chosen_entry_becomes_the_request() {
        let keep = KeepAwakeSettings::default();
        let start = |minutes| {
            Some(Action::Start(AwakeSpec {
                scope: AwakeScope::System,
                until: AwakeUntil::For { minutes },
            }))
        };
        assert_eq!(action_of("dy.awake.30", false, &keep), start(30));
        assert_eq!(action_of("dy.awake.240", false, &keep), start(240));
        assert_eq!(
            action_of("dy.awake.indefinite", true, &keep),
            Some(Action::Start(AwakeSpec {
                scope: AwakeScope::SystemAndDisplay,
                until: AwakeUntil::Indefinite
            }))
        );
        assert_eq!(action_of("dy.awake.stop", false, &keep), Some(Action::Stop));
        assert_eq!(action_of("dy.awake.display", false, &keep), Some(Action::ToggleDisplay));
        assert_eq!(action_of("dy.awake.999", false, &keep), None);
        assert_eq!(action_of("other", false, &keep), None);
        assert!(CHOICES.iter().all(|(id, _, _)| id.starts_with(PREFIX)));
    }

    #[test]
    fn busy_and_program_entries_use_the_settings_and_the_agent_adds_the_start_time() {
        let keep = KeepAwakeSettings {
            cpu_pct: 70.0,
            grace_secs: 90,
            ..Default::default()
        };
        let busy = |id: &str| match action_of(id, false, &keep) {
            Some(Action::Start(AwakeSpec {
                until: AwakeUntil::WhileBusy { conds, grace_secs },
                ..
            })) => Some((conds, grace_secs)),
            _ => None,
        };
        assert_eq!(busy("dy.awake.busy.cpu"), Some((vec![BusyCond::Cpu { above_pct: 70.0 }], 90)));
        assert_eq!(
            busy("dy.awake.busy.network").unwrap().0,
            [BusyCond::Network {
                above_bytes_per_s: keep.net_bytes_per_s
            }]
        );
        assert_eq!(
            busy("dy.awake.busy.disk").unwrap().0,
            [BusyCond::Disk {
                above_bytes_per_s: keep.disk_bytes_per_s
            }]
        );
        assert_eq!(busy("dy.awake.busy.any").unwrap().0.len(), 3);
        assert_eq!(busy("dy.awake.busy.gpu"), None);
        assert!(matches!(
            action_of("dy.awake.prog.4242", false, &keep),
            Some(Action::Start(AwakeSpec { until: AwakeUntil::WhileProcess { process }, .. })) if process.pid == 4242 && process.started.is_none()
        ));
        assert_eq!(action_of("dy.awake.prog.x", false, &keep), None);
    }

    #[test]
    fn the_submenu_lists_the_busiest_programs_up_to_the_cap_and_ticks_the_display_choice() {
        let programs: Vec<(u32, String)> = (0..12).map(|i| (i, format!("p{i}"))).collect();
        let m = menu(&view(AwakeView::Off, Capability::Known), 0, &programs, true);
        assert_eq!(m.programs.len(), MAX_PROGRAMS);
        assert_eq!(m.programs[1].id, "dy.awake.prog.1");
        assert!(m.display.checked && m.display.enabled);
        assert_eq!(m.busy.len(), 4);
    }

    #[test]
    fn a_countdown_shows_the_time_left_with_cancel_and_snooze_and_a_request_in_flight_says_so() {
        use dyno_core::power::idle::{Cause, IdleAction};
        let mut v = view(AwakeView::Off, Capability::Known);
        v.idle = IdleView::Countdown {
            action: IdleAction::ShutDown,
            deadline: Timestamp(48_500),
            cause: Cause::Idle,
        };
        let m = menu(&v, 0, &[], false);
        assert_eq!(m.countdown_header.as_deref(), Some("Will shut down in 0:48"));
        assert_eq!(
            m.countdown.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["Cancel shut down", "Snooze 30 minutes"]
        );
        v.idle = IdleView::Acting {
            action: IdleAction::Sleep,
            since: Timestamp(0),
        };
        let m = menu(&v, 0, &[], false);
        assert_eq!(m.countdown_header.as_deref(), Some("Waiting for the system to sleep"));
        assert!(m.countdown.is_empty());
        let quiet = menu(&view(AwakeView::Off, Capability::Known), 0, &[], false);
        assert!(quiet.countdown_header.is_none() && quiet.countdown.is_empty());
        let keep = KeepAwakeSettings::default();
        assert_eq!(action_of("dy.awake.cancel-idle", false, &keep), Some(Action::CancelIdle));
        assert_eq!(action_of("dy.awake.snooze-idle", false, &keep), Some(Action::SnoozeIdle));
    }

    #[test]
    fn durations_read_as_words() {
        for (ms, text) in [
            (0, "less than a minute"),
            (59_999, "less than a minute"),
            (60_000, "1 min"),
            (3_600_000, "1 h"),
            (3_900_000, "1 h 5 min"),
            (86_400_000, "1 d"),
            (90_000_000, "1 d 1 h"),
            (-5, "less than a minute"),
        ] {
            assert_eq!(duration_text(ms), text);
        }
    }
}
