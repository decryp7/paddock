//! The "Diagnostics > Logging" settings section (plan X1/X2): the level, the day's size budget and how many older days are
//! kept, as the `pd.logging` section of the shell's Settings window. The level takes effect at once
//! ([`LogHandle::set_level`]); the two limits are read when the log is installed, so a change needs a restart. The log is
//! installed before the settings are managed, so [`stored`] reads the saved values first.

use paddock_log::{KEEP_FILES_RANGE, LogHandle, LogLevel, LogSettings, MAX_FILE_MB_RANGE};
use paddock_settings::{
    ApplyError, Configurable, EnumOption, FieldKind, SectionOwner, SettingField, Settings,
    SettingsSection, ValidValues, section,
};
use std::path::Path;
use std::sync::Arc;

/// The id of the section (and of its settings page in the UI).
pub const LOGGING: &str = "pd.logging";

/// The section's schema. Defaults are [`LogSettings::default`] (decision D2: seven days of at most 20 MB, Info).
pub fn schema() -> SettingsSection {
    let d = LogSettings::default();
    let level = |l: LogLevel, label: &str| EnumOption::new(level_name(l), label);
    SettingsSection::new(LOGGING, "Logging", SectionOwner::Shell)
        .describe("What the app writes to its log, which a problem report (Help > Report a problem) includes. File contents are never logged.")
        .field(
            SettingField::new(
                "level",
                "Detail",
                FieldKind::Enum {
                    options: vec![
                        level(LogLevel::Error, "Errors only"),
                        level(LogLevel::Warn, "Warnings and errors"),
                        level(LogLevel::Info, "Normal"),
                        level(LogLevel::Debug, "Detailed (for support cases)"),
                        level(LogLevel::Trace, "Everything"),
                    ],
                },
                level_name(d.level),
            )
            .describe("Takes effect at once. Detailed and Everything make the log grow faster."),
        )
        .field(
            SettingField::new(
                "maxFileMb",
                "Size of one day's log",
                FieldKind::Int {
                    min: i64::from(MAX_FILE_MB_RANGE.0),
                    max: i64::from(MAX_FILE_MB_RANGE.1),
                    step: 1,
                    unit: Some("MB".into()),
                },
                i64::from(d.max_file_mb),
            )
            .describe("After this much in one day only errors are written until the next day.")
            .needs_restart(),
        )
        .field(
            SettingField::new(
                "keepFiles",
                "Older days kept",
                FieldKind::Int {
                    min: i64::from(KEEP_FILES_RANGE.0),
                    max: i64::from(KEEP_FILES_RANGE.1),
                    step: 1,
                    unit: Some("days".into()),
                },
                i64::from(d.keep_files),
            )
            .describe("Log files older than this are removed.")
            .needs_restart(),
        )
}

/// The stored name of a level (what [`LogSettings`] serialises).
fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    }
}

/// The level in a section's values (a missing or unknown value is the default, Info).
pub fn level_of(values: &ValidValues) -> LogLevel {
    values
        .text("level")
        .and_then(LogLevel::parse)
        .unwrap_or_default()
}

/// The section applied to `handle`: a change of the level takes effect at once, in this and every later run.
pub fn logging_section(handle: LogHandle) -> Arc<dyn Configurable> {
    section(
        schema(),
        move |values: &ValidValues| -> Result<(), ApplyError> {
            handle.set_level(level_of(values));
            Ok(())
        },
    )
}

/// The saved logging settings in the settings file at `path`, for installing the log before the app's settings are
/// managed. A missing, damaged or newer file gives the defaults (the settings service reports that itself).
pub fn stored(path: &Path) -> LogSettings {
    let settings = Settings::open(path);
    if settings.register(Arc::new(schema())).is_err() {
        return LogSettings::default();
    }
    settings
        .decode::<LogSettings>(LOGGING)
        .map(LogSettings::clamped)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_log::{LogConfig, Logging};
    use serde_json::json;

    #[test]
    fn the_schema_is_valid_and_its_defaults_are_the_logs() {
        let s = schema();
        s.check().unwrap();
        assert_eq!(s.id, LOGGING);
        let from_defaults: LogSettings = serde_json::from_value(json!({
            "level": s.find("level").unwrap().default,
            "maxFileMb": s.find("maxFileMb").unwrap().default,
            "keepFiles": s.find("keepFiles").unwrap().default,
        }))
        .unwrap();
        assert_eq!(from_defaults, LogSettings::default());
        assert!(
            s.find("maxFileMb").unwrap().restart_required
                && s.find("keepFiles").unwrap().restart_required
        );
        assert!(!s.find("level").unwrap().restart_required);
    }

    #[test]
    fn the_ranges_are_the_logs_ranges() {
        let s = schema();
        let ok = |key: &str, v: i64| {
            paddock_settings::validate_field(s.find(key).unwrap(), &json!(v)).is_ok()
        };
        assert!(
            ok("maxFileMb", 1)
                && ok("maxFileMb", 100)
                && !ok("maxFileMb", 0)
                && !ok("maxFileMb", 101)
        );
        assert!(
            ok("keepFiles", 1)
                && ok("keepFiles", 50)
                && !ok("keepFiles", 0)
                && !ok("keepFiles", 51)
        );
        // Every level the section offers parses back to itself.
        if let FieldKind::Enum { options } = &s.find("level").unwrap().kind {
            for o in options {
                assert_eq!(
                    LogLevel::parse(&o.value).map(level_name),
                    Some(o.value.as_str())
                );
            }
            assert_eq!(options.len(), 5);
        } else {
            panic!("level is an enum");
        }
    }

    #[test]
    fn changing_the_level_reaches_the_running_log_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let app = paddock_env::AppId::new("demo", "Demo").unwrap();
        let config = LogConfig::new(&app, "app", "1.0.0")
            .with_log_dir(dir.path())
            .with_stdout(false)
            .with_panic_hook(false);
        Logging::new(config).scoped(|handle| {
            let settings = Settings::in_memory();
            settings.register(logging_section(handle.clone())).unwrap();
            assert_eq!(handle.level(), LogLevel::Info);
            let r = settings.set(LOGGING, &json!({ "level": "debug" })).unwrap();
            assert!(r.applied);
            assert_eq!(handle.level(), LogLevel::Debug);
            settings.set(LOGGING, &json!({ "level": "error" })).unwrap();
            assert_eq!(handle.level(), LogLevel::Error);
            // An unknown level is refused by validation and changes nothing.
            let r = settings.set(LOGGING, &json!({ "level": "loud" })).unwrap();
            assert!(!r.applied && !r.errors.is_empty());
            assert_eq!(handle.level(), LogLevel::Error);
            // Putting the field back to its default applies the default.
            settings.reset(LOGGING, Some("level")).unwrap();
            assert_eq!(handle.level(), LogLevel::Info);
        });
    }

    #[test]
    fn stored_settings_are_read_before_the_log_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(paddock_settings::FILE_NAME);
        assert_eq!(
            stored(&file),
            LogSettings::default(),
            "missing file: the defaults"
        );
        let s = Settings::open(&file);
        s.register(Arc::new(schema())).unwrap();
        s.set(
            LOGGING,
            &json!({ "level": "debug", "maxFileMb": 5, "keepFiles": 3 }),
        )
        .unwrap();
        drop(s);
        let got = stored(&file);
        assert_eq!(
            (got.level, got.max_file_mb, got.keep_files),
            (LogLevel::Debug, 5, 3)
        );
        std::fs::write(&file, b"{ not json").unwrap();
        assert_eq!(
            stored(&file),
            LogSettings::default(),
            "damaged file: the defaults"
        );
    }
}
