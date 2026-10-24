//! The "Notifications" settings section (`pd.notifications`, plan X8): which messages become OS notifications, quiet hours
//! and the rate limits, over the [`SharedOsSettings`] the app's [`OsNotifier`](paddock_health::notify::os::OsNotifier)
//! reads. The section lives here because it needs the settings service, which sits above `paddock-health`. A change
//! applies to the next notification; the stored values are put into effect when the section is registered.

use paddock_health::notify::os::{
    BURST, BURST_WINDOW_SECS, COOLDOWN_SECS, ClockTime, OsLevel, OsSettings, QuietHours,
    SharedOsSettings,
};
use paddock_settings::{
    ApplyError, Configurable, EnumOption, FieldKind, SectionOwner, SettingField, SettingsSection,
    ValidValues, section,
};
use std::sync::Arc;

/// The id of the section (and of its settings page in the UI).
pub const NOTIFICATIONS: &str = "pd.notifications";

/// `HH:MM`, 24-hour.
const TIME_PATTERN: &str = r"^([01]?[0-9]|2[0-3]):[0-5][0-9]$";

fn int(range: &std::ops::RangeInclusive<u32>, unit: &str) -> FieldKind {
    FieldKind::Int {
        min: i64::from(*range.start()),
        max: i64::from(*range.end()),
        step: 1,
        unit: Some(unit.into()),
    }
}

fn time() -> FieldKind {
    FieldKind::Text {
        max_len: 5,
        pattern: Some(TIME_PATTERN.into()),
    }
}

/// The section's schema; its defaults are [`OsSettings::default`] (Debrief's: alerts, 30 seconds, 5 in 5 minutes).
pub fn schema() -> SettingsSection {
    let d = OsSettings::default();
    let level = |l: &str, label: &str| EnumOption::new(l, label);
    SettingsSection::new(NOTIFICATIONS, "Notifications", SectionOwner::Shell)
        .describe("When the app shows a system notification. Every message is still listed in the notification centre.")
        .field(
            SettingField::new(
                "level",
                "Show notifications for",
                FieldKind::Enum {
                    options: vec![
                        level("off", "Nothing"),
                        level("alerts", "Problems and things that need me"),
                        level("all", "Everything, including finished work"),
                    ],
                },
                "alerts",
            )
            .describe("A message you can already see in the window in front never notifies."),
        )
        .field(SettingField::new("quietHoursOn", "Quiet hours", FieldKind::Bool, false).describe("Hold system notifications at night."))
        .field(SettingField::new("quietStart", "Quiet from", time(), "22:00").describe("Local time, 24-hour (HH:MM)."))
        .field(SettingField::new("quietEnd", "Quiet until", time(), "07:00").describe("Local time; before the start means it ends the next morning."))
        .field(
            SettingField::new("criticalInQuietHours", "Critical failures break the quiet", FieldKind::Bool, d.critical_in_quiet_hours)
                .describe("A failure that stops the work still notifies during quiet hours."),
        )
        .field(
            SettingField::new("cooldownSecs", "Same problem again after", int(&COOLDOWN_SECS, "seconds"), i64::from(d.cooldown_secs))
                .describe("A problem that already notified stays quiet for this long, however often it repeats."),
        )
        .field(SettingField::new("burst", "At most", int(&BURST, "notifications"), i64::from(d.burst)).describe("...in the time below, whatever they are about."))
        .field(SettingField::new("burstWindowSecs", "Within", int(&BURST_WINDOW_SECS, "seconds"), i64::from(d.burst_window_secs)))
}

/// The settings a section's values stand for. A missing or unknown value is the default; quiet hours need both times.
pub fn from_values(values: &ValidValues) -> OsSettings {
    let d = OsSettings::default();
    let level = match values.text("level") {
        Some("off") => OsLevel::Off,
        Some("all") => OsLevel::All,
        _ => OsLevel::Alerts,
    };
    let time = |key: &str| {
        values
            .text(key)
            .and_then(|t| ClockTime::try_from(t.to_string()).ok())
    };
    let quiet_hours = match (
        values.bool("quietHoursOn"),
        time("quietStart"),
        time("quietEnd"),
    ) {
        (Some(true), Some(start), Some(end)) => Some(QuietHours { start, end }),
        _ => None,
    };
    let number = |key: &str, default: u32| {
        values
            .int(key)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(default)
    };
    OsSettings {
        level,
        quiet_hours,
        critical_in_quiet_hours: values
            .bool("criticalInQuietHours")
            .unwrap_or(d.critical_in_quiet_hours),
        cooldown_secs: number("cooldownSecs", d.cooldown_secs),
        burst: number("burst", d.burst),
        burst_window_secs: number("burstWindowSecs", d.burst_window_secs),
    }
    .clamped()
}

/// The section applied to `shared`: every change, and the stored values at registration, reach the notifier at once.
pub fn notifications_section(shared: SharedOsSettings) -> Arc<dyn Configurable> {
    section(
        schema(),
        move |values: &ValidValues| -> Result<(), ApplyError> {
            shared.set(from_values(values));
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_settings::Settings;
    use serde_json::json;

    #[test]
    fn the_schema_is_valid_and_its_defaults_are_the_notifiers() {
        let s = schema();
        s.check().unwrap();
        assert_eq!(s.id, NOTIFICATIONS);
        let shared = SharedOsSettings::new(OsSettings {
            burst: 9,
            ..OsSettings::default()
        });
        let settings = Settings::in_memory();
        settings
            .register(notifications_section(shared.clone()))
            .unwrap();
        assert_eq!(
            shared.get(),
            OsSettings::default(),
            "registering applies the section's defaults"
        );
    }

    #[test]
    fn a_change_reaches_the_notifier_and_bad_values_are_refused() {
        let shared = SharedOsSettings::default();
        let settings = Settings::in_memory();
        settings
            .register(notifications_section(shared.clone()))
            .unwrap();
        let r = settings
            .set(
                NOTIFICATIONS,
                &json!({ "level": "all", "quietHoursOn": true, "quietStart": "23:30", "quietEnd": "6:00", "cooldownSecs": 120, "burst": 3 }),
            )
            .unwrap();
        assert!(r.applied, "{r:?}");
        let now = shared.get();
        assert_eq!(
            (now.level, now.cooldown_secs, now.burst),
            (OsLevel::All, 120, 3)
        );
        let q = now.quiet_hours.unwrap();
        assert_eq!((q.start.minutes(), q.end.minutes()), (23 * 60 + 30, 6 * 60));
        for bad in [
            json!({ "quietStart": "25:00" }),
            json!({ "quietStart": "7pm" }),
            json!({ "level": "loud" }),
            json!({ "burst": 0 }),
            json!({ "cooldownSecs": 99999 }),
        ] {
            let r = settings.set(NOTIFICATIONS, &bad).unwrap();
            assert!(!r.applied && !r.errors.is_empty(), "{bad}");
        }
        assert_eq!(shared.get(), now, "a refused change changes nothing");
        settings
            .set(NOTIFICATIONS, &json!({ "quietHoursOn": false }))
            .unwrap();
        assert!(
            shared.get().quiet_hours.is_none(),
            "off means no quiet hours even with times set"
        );
    }

    #[test]
    fn values_that_cannot_be_read_fall_back_to_the_defaults() {
        assert_eq!(from_values(&ValidValues::default()), OsSettings::default());
    }

    #[test]
    fn stored_values_are_in_effect_when_the_section_registers() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let first = Settings::open(&file);
        first
            .register(notifications_section(SharedOsSettings::default()))
            .unwrap();
        first
            .set(NOTIFICATIONS, &json!({ "level": "off" }))
            .unwrap();
        let shared = SharedOsSettings::default();
        let second = Settings::open(&file);
        second
            .register(notifications_section(shared.clone()))
            .unwrap();
        assert_eq!(shared.get().level, OsLevel::Off);
    }
}
