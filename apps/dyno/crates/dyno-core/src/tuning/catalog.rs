//! The tuning catalogue: [`plans`](super::plans) (reasoned from the facts) plus the suggestions that read the system
//! themselves (sleep timers, hibernation, the Linux power profile, the battery charge limit), and the rule for what
//! may run now. Everything here is a pure function over typed values; the system is reached only through
//! [`TuningSystem`], whose changes are the closed set [`HelperAction`].

use super::{category_of, Plan, TuningSuggestion};
use crate::cleanup::Os;
use crate::facts::SystemFacts;
use crate::helper::{HelperAction, PowerProfile, SleepSource, TuningHost};
use crate::metrics::{PowerSource, Reading};
use crate::ports::TuningSystem;

/// Sleep timers longer than this are suggested down to [`SUGGESTED_SLEEP_MINUTES`] on battery.
pub const LONG_SLEEP_MINUTES: u32 = 30;
/// The suggested battery sleep timer.
pub const SUGGESTED_SLEEP_MINUTES: u32 = 15;
/// The charge limit suggested to keep a battery healthy.
pub const SUGGESTED_CHARGE_LIMIT: u8 = 80;

/// The `Os` the facts-based suggestions know for a tuning host (they have no Linux-specific ones).
fn os_of(host: TuningHost) -> Os {
    match host {
        TuningHost::Windows => Os::Windows,
        TuningHost::Macos => Os::Macos,
        TuningHost::Linux => Os::Linux,
    }
}

/// Minutes as people say them.
pub fn minutes_label(minutes: u32) -> String {
    match minutes {
        0 => "Never".into(),
        1 => "1 minute".into(),
        m => format!("{m} minutes"),
    }
}

fn plan(
    id: &str,
    title: &str,
    explanation: &str,
    (current, proposed): (String, String),
    (apply, revert): (HelperAction, HelperAction),
    host: TuningHost,
    applied: &[String],
) -> Plan {
    Plan {
        suggestion: TuningSuggestion {
            id: id.into(),
            title: title.into(),
            explanation: explanation.into(),
            current,
            proposed,
            needs_admin: apply.needs_admin_on(host),
            applied: applied.iter().any(|a| a == id),
            category: category_of(id),
            preview: apply.preview(),
            undo_preview: revert.preview(),
            blocked: None,
            changed_outside: false,
        },
        apply,
        revert,
    }
}

/// The suggestions that need the system's current values. A setting that cannot be read is simply not suggested.
pub fn system_plans(
    host: TuningHost,
    facts: &SystemFacts,
    power: Option<PowerSource>,
    applied: &[String],
    system: &mut dyn TuningSystem,
) -> Vec<Plan> {
    let mut out = Vec::new();
    let has_battery = power.is_some();

    if host == TuningHost::Linux {
        if let Reading::Available(p) = &facts.power_plan {
            if PowerProfile::from_tool_name(&p.id) == Some(PowerProfile::PowerSaver) && power != Some(PowerSource::Battery) {
                out.push(plan(
                    "powerProfile",
                    "Use the Balanced power profile while plugged in",
                    "Power saver keeps the CPU slow even on mains power. Balanced speeds up when you need it and still saves power when idle.",
                    ("Power saver".into(), "Balanced".into()),
                    (
                        HelperAction::SetPowerProfile(PowerProfile::Balanced),
                        HelperAction::SetPowerProfile(PowerProfile::PowerSaver),
                    ),
                    host,
                    applied,
                ));
            }
        }
    }

    if has_battery && matches!(host, TuningHost::Windows | TuningHost::Macos) {
        let probe = HelperAction::SetSleepMinutes {
            source: SleepSource::Battery,
            minutes: 0,
        };
        if let Some(now) = system.read(&probe).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
            if now == 0 || now > LONG_SLEEP_MINUTES {
                out.push(plan(
                    "sleepOnBattery",
                    "Sleep sooner when on battery",
                    "An idle laptop that never sleeps drains its battery in your bag. A short timer saves it; waking takes a keypress.",
                    (minutes_label(now), minutes_label(SUGGESTED_SLEEP_MINUTES)),
                    (
                        HelperAction::SetSleepMinutes {
                            source: SleepSource::Battery,
                            minutes: SUGGESTED_SLEEP_MINUTES,
                        },
                        HelperAction::SetSleepMinutes {
                            source: SleepSource::Battery,
                            minutes: now,
                        },
                    ),
                    host,
                    applied,
                ));
            }
        }
    }

    if host == TuningHost::Windows && !has_battery {
        if let Ok(state) = system.read(&HelperAction::SetHibernate(false)) {
            if state.trim() == "on" {
                out.push(plan(
                    "hibernate",
                    "Turn off hibernation to free disk space",
                    "Hibernation keeps a file about as large as part of your memory on the system drive. A desktop that never hibernates \
                     can give that space back; Sleep keeps working, Fast Startup turns off.",
                    ("On".into(), "Off".into()),
                    (HelperAction::SetHibernate(false), HelperAction::SetHibernate(true)),
                    host,
                    applied,
                ));
            }
        }
    }

    if has_battery && host == TuningHost::Linux {
        if let Some(now) = system
            .read(&HelperAction::SetChargeLimit(SUGGESTED_CHARGE_LIMIT))
            .ok()
            .and_then(|t| t.trim().parse::<u8>().ok())
        {
            if (SUGGESTED_CHARGE_LIMIT + 1..=100).contains(&now) {
                out.push(plan(
                    "chargeLimit",
                    "Stop charging at 80%",
                    "Holding a lithium battery at full charge wears it faster. Stopping at 80% keeps it healthy for longer; you get less \
                     runtime per charge.",
                    (format!("{now}%"), format!("{SUGGESTED_CHARGE_LIMIT}%")),
                    (
                        HelperAction::SetChargeLimit(SUGGESTED_CHARGE_LIMIT),
                        HelperAction::SetChargeLimit(now),
                    ),
                    host,
                    applied,
                ));
            }
        }
    }
    out
}

/// The whole catalogue that fits the machine now.
pub fn catalogue(
    host: TuningHost,
    facts: &SystemFacts,
    power: Option<PowerSource>,
    applied: &[String],
    system: &mut dyn TuningSystem,
) -> Vec<Plan> {
    let mut plans = super::plans(facts, os_of(host), power, applied);
    // Search indexing is only changeable where Dyno has a typed way to do it.
    if host == TuningHost::Linux {
        plans.retain(|p| p.suggestion.id != "indexDevFolders");
    }
    plans.extend(system_plans(host, facts, power, applied, system));
    plans
}

/// Why `plan` cannot run now, or `None` when it can. Changes that need administrator rights go through the privileged
/// helper (ADR 0016); `helper_available` is whether it is installed or can be installed here. Where it cannot, they are
/// shown and explained but not applicable.
pub fn blocked_reason(plan: &Plan, host: TuningHost, helper_available: bool) -> Option<String> {
    if plan.apply.needs_admin_on(host) && !helper_available {
        return Some(
            "Needs administrator rights, and Dyno's privileged helper cannot be installed on this computer (Settings > Helper says why)."
                .into(),
        );
    }
    None
}

/// Marks each plan with whether it is blocked.
pub fn mark_blocked(plans: &mut [Plan], host: TuningHost, helper_available: bool) {
    for p in plans {
        p.suggestion.blocked = blocked_reason(p, host, helper_available);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::PowerPlan;
    use crate::ports::TuningSystem;
    use std::collections::HashMap;

    struct Fake {
        host: TuningHost,
        values: HashMap<&'static str, String>,
    }

    fn key(a: &HelperAction) -> &'static str {
        match a {
            HelperAction::SetSleepMinutes { .. } => "sleep",
            HelperAction::SetHibernate(_) => "hibernate",
            HelperAction::SetChargeLimit(_) => "charge",
            _ => "other",
        }
    }

    impl TuningSystem for Fake {
        fn host(&self) -> TuningHost {
            self.host
        }
        fn read(&mut self, a: &HelperAction) -> Result<String, crate::ports::PlatformError> {
            self.values
                .get(key(a))
                .cloned()
                .ok_or_else(|| crate::ports::PlatformError::Unsupported("no".into()))
        }
        fn apply(&mut self, _: &HelperAction) -> Result<(), crate::ports::PlatformError> {
            Ok(())
        }
    }

    fn fake(host: TuningHost, values: &[(&'static str, &str)]) -> Fake {
        Fake {
            host,
            values: values.iter().map(|(k, v)| (*k, v.to_string())).collect(),
        }
    }

    fn ids(p: &[Plan]) -> Vec<&str> {
        p.iter().map(|p| p.suggestion.id.as_str()).collect()
    }

    #[test]
    fn linux_power_profile_and_charge_limit() {
        let mut f = SystemFacts::unknown("Linux");
        f.power_plan = Reading::Available(PowerPlan {
            id: "power-saver".into(),
            name: "Power saver".into(),
            power_saver: true,
        });
        let mut sys = fake(TuningHost::Linux, &[("charge", "100")]);
        let p = catalogue(TuningHost::Linux, &f, Some(PowerSource::Ac), &[], &mut sys);
        assert_eq!(ids(&p), ["powerProfile", "chargeLimit"]);
        assert_eq!(p[1].revert, HelperAction::SetChargeLimit(100));
        assert!(p[1].suggestion.needs_admin && !p[0].suggestion.needs_admin);
        assert_eq!(p[0].suggestion.preview, ["Switch the power profile to Balanced"]);
        assert_eq!(p[0].suggestion.undo_preview, ["Switch the power profile to Power saver"]);
        // On battery the profile stays; at 80 or less nothing to limit; unreadable means not suggested.
        let p = catalogue(
            TuningHost::Linux,
            &f,
            Some(PowerSource::Battery),
            &[],
            &mut fake(TuningHost::Linux, &[("charge", "80")]),
        );
        assert!(p.is_empty());
        assert!(
            catalogue(
                TuningHost::Linux,
                &f,
                Some(PowerSource::Ac),
                &[],
                &mut fake(TuningHost::Linux, &[("charge", "x")])
            )
            .len()
                == 1
        );
    }

    #[test]
    fn linux_has_no_indexing_item() {
        let mut f = SystemFacts::unknown("Linux");
        f.dev_folders = vec![crate::facts::DevFolder {
            path: "/home/me/code/app".into(),
            indexed: Some(true),
            av_excluded: None,
        }];
        assert!(catalogue(TuningHost::Linux, &f, None, &[], &mut fake(TuningHost::Linux, &[])).is_empty());
        let mac = catalogue(TuningHost::Macos, &f, None, &[], &mut fake(TuningHost::Macos, &[]));
        assert_eq!(ids(&mac), ["indexDevFolders"]);
    }

    #[test]
    fn sleep_timer_on_battery_machines_only() {
        let f = SystemFacts::unknown("macOS");
        for (now, suggested) in [("0", true), ("45", true), ("10", false), ("30", false), ("junk", false)] {
            let mut sys = fake(TuningHost::Macos, &[("sleep", now)]);
            let p = catalogue(TuningHost::Macos, &f, Some(PowerSource::Battery), &[], &mut sys);
            assert_eq!(ids(&p).contains(&"sleepOnBattery"), suggested, "{now}");
        }
        let mut sys = fake(TuningHost::Windows, &[("sleep", "0")]);
        assert!(
            catalogue(TuningHost::Windows, &f, None, &[], &mut sys).is_empty(),
            "a desktop has no battery timer"
        );
        let p = catalogue(TuningHost::Windows, &f, Some(PowerSource::Ac), &["sleepOnBattery".into()], &mut sys);
        assert!(p[0].suggestion.applied);
        assert_eq!(p[0].suggestion.current, "Never");
        assert_eq!(
            p[0].revert,
            HelperAction::SetSleepMinutes {
                source: SleepSource::Battery,
                minutes: 0
            }
        );
        // Admin on macOS only.
        let mac = catalogue(
            TuningHost::Macos,
            &f,
            Some(PowerSource::Ac),
            &[],
            &mut fake(TuningHost::Macos, &[("sleep", "0")]),
        );
        assert!(mac[0].suggestion.needs_admin && !p[0].suggestion.needs_admin);
    }

    #[test]
    fn hibernation_on_desktops() {
        let f = SystemFacts::unknown("Windows 11");
        let mut sys = fake(TuningHost::Windows, &[("hibernate", "on")]);
        let p = catalogue(TuningHost::Windows, &f, None, &[], &mut sys);
        assert_eq!(ids(&p), ["hibernate"]);
        assert_eq!(p[0].apply, HelperAction::SetHibernate(false));
        assert_eq!(p[0].suggestion.category, super::super::TuningCategory::Sleep);
        let mut off = fake(TuningHost::Windows, &[("hibernate", "off")]);
        assert!(catalogue(TuningHost::Windows, &f, None, &[], &mut off).is_empty());
    }

    #[test]
    fn administrator_changes_are_blocked_until_the_helper_exists() {
        let f = SystemFacts::unknown("Linux");
        let mut sys = fake(TuningHost::Linux, &[("charge", "100")]);
        let mut p = catalogue(TuningHost::Linux, &f, Some(PowerSource::Ac), &[], &mut sys);
        mark_blocked(&mut p, TuningHost::Linux, false);
        assert!(p[0].suggestion.blocked.as_deref().unwrap().contains("administrator rights"));
        mark_blocked(&mut p, TuningHost::Linux, true);
        assert!(p[0].suggestion.blocked.is_none());
        assert_eq!(minutes_label(1), "1 minute");
        assert_eq!(minutes_label(5), "5 minutes");
    }
}
