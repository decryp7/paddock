//! The configuration file `spotter.toml`: versioned, tolerant of damage, and migrated from the old Release Monitor.
//!
//! - A newer schema than this build knows is refused (the caller keeps the file and starts with defaults).
//! - Damage is contained: bad settings fall back to the defaults, one bad watch is skipped with a warning, the rest
//!   load. Watches that parse as specs but not as watches (e.g. an unknown source kind) are kept: the app shows
//!   them with the problem instead of dropping them on the next save.
//! - The old app's `app.toml` (figment: `path`, `file_regex`, `interval_seconds`, `naggy`) becomes one folder watch
//!   with the old build-number scheme; its acknowledged version (`version.txt`) is read by
//!   [`legacy_acknowledged`].

use crate::limits::{MAX_CONFIG_BYTES, MAX_WATCHES};
use crate::notify::{NotificationPolicy, NotifyMode};
use crate::source::SourceConfig;
use crate::version::{Version, VersionScheme};
use crate::watch::{Settings, WatchSpec};
use serde::Serialize;

/// The schema this build writes.
pub const SCHEMA_VERSION: u32 = 1;

/// The configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigDoc {
    /// App settings.
    pub settings: Settings,
    /// The watches, in the user's order.
    pub watches: Vec<WatchSpec>,
}

/// A loaded configuration and what was wrong with the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// The configuration.
    pub doc: ConfigDoc,
    /// Problems that were worked around, for the log and the settings page.
    pub warnings: Vec<String>,
    /// Whether it came from the old app's format (the caller writes it back in the new one).
    pub migrated: bool,
}

#[derive(Serialize)]
struct FileOut<'a> {
    schema: u32,
    settings: &'a Settings,
    watches: &'a [WatchSpec],
}

/// Parses `spotter.toml`. `Err` only when nothing can be used (not TOML, too large, a newer schema); the caller then
/// tries the backup, then the defaults.
pub fn parse(text: &str) -> Result<Loaded, String> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err(format!("The configuration is larger than {} MiB.", MAX_CONFIG_BYTES >> 20));
    }
    let table: toml::Table = toml::from_str(text).map_err(|e| format!("The configuration is not valid TOML: {}", e.message()))?;
    let mut warnings = Vec::new();
    let schema = match table.get("schema") {
        None => {
            if table.contains_key("file_regex") || table.contains_key("path") {
                return Ok(migrate_legacy(&table));
            }
            if !table.is_empty() {
                warnings.push("The configuration has no schema version; it was read as version 1.".to_string());
            }
            SCHEMA_VERSION as i64
        }
        Some(v) => v.as_integer().ok_or("The configuration's schema version is not a number.")?,
    };
    if schema > SCHEMA_VERSION as i64 || schema < 1 {
        return Err(format!(
            "The configuration was written by another version of Spotter (schema {schema}; this one reads {SCHEMA_VERSION}). Update Spotter."
        ));
    }
    let settings = match table.get("settings") {
        None => Settings::default(),
        Some(v) => v.clone().try_into().unwrap_or_else(|e: toml::de::Error| {
            warnings.push(format!("The settings could not be read ({}); the defaults are used.", e.message()));
            Settings::default()
        }),
    };
    let mut watches: Vec<WatchSpec> = Vec::new();
    let raw = match table.get("watches") {
        None => Vec::new(),
        Some(toml::Value::Array(a)) => a.clone(),
        Some(_) => {
            warnings.push("\"watches\" is not a list of watches; it was ignored.".into());
            Vec::new()
        }
    };
    for (i, w) in raw.into_iter().enumerate() {
        match w.try_into::<WatchSpec>() {
            Ok(spec) if watches.iter().any(|x| x.id == spec.id) => warnings.push(format!(
                "Watch {} has the same id as another (\"{}\"); it was skipped.",
                i + 1,
                spec.id
            )),
            Ok(_) if watches.len() >= MAX_WATCHES => {
                warnings.push(format!("Only the first {MAX_WATCHES} watches are loaded."));
                break;
            }
            Ok(spec) => watches.push(spec),
            Err(e) => warnings.push(format!("Watch {} could not be read ({}); it was skipped.", i + 1, e.message())),
        }
    }
    Ok(Loaded {
        doc: ConfigDoc { settings, watches },
        warnings,
        migrated: false,
    })
}

/// Writes the configuration as TOML.
pub fn render(doc: &ConfigDoc) -> String {
    let out = FileOut {
        schema: SCHEMA_VERSION,
        settings: &doc.settings,
        watches: &doc.watches,
    };
    let body = toml::to_string_pretty(&out).unwrap_or_default();
    format!("# Spotter configuration (schema {SCHEMA_VERSION}). Spotter rewrites this file; comments are not kept.\n{body}")
}

/// The id of the watch made from the old app's configuration.
pub const LEGACY_WATCH_ID: &str = "builds";

fn migrate_legacy(table: &toml::Table) -> Loaded {
    let text = |k: &str| table.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let path = text("path").unwrap_or_else(|| "/Volumes/Data/Test".into());
    let pattern = text("file_regex").unwrap_or_else(|| ".*.txt".into());
    let interval = table.get("interval_seconds").and_then(|v| v.as_integer()).unwrap_or(60);
    let naggy = table.get("naggy").and_then(|v| v.as_bool()).unwrap_or(false);
    let watch = WatchSpec {
        id: LEGACY_WATCH_ID.into(),
        name: "Builds".into(),
        source: SourceConfig::new(
            "folder",
            &[("path", &path), ("pattern", &pattern), ("syntax", "regex"), ("entries", "files")],
        ),
        versions: "build-number".into(),
        version_pattern: None,
        include_prereleases: false,
        name_filter: String::new(),
        interval_secs: u32::try_from(interval.max(1)).unwrap_or(u32::MAX),
        notify: NotificationPolicy {
            mode: NotifyMode::EveryNew,
            nag: naggy,
            quiet_hours: None,
        },
        actions: Vec::new(),
        paused: false,
    };
    Loaded {
        doc: ConfigDoc {
            settings: Settings::default(),
            watches: vec![watch],
        },
        warnings: vec![format!(
            "Migrated the Release Monitor configuration: watching {path} as \"Builds\"."
        )],
        migrated: true,
    }
}

/// The version the old app stored as acknowledged (`version.txt`, e.g. `R1.01.00T10`); `None` for none or the old
/// app's "nothing" value `R0.00.00T00`.
pub fn legacy_acknowledged(text: &str) -> Option<Version> {
    VersionScheme::BuildNumber
        .extract(text.get(..text.len().min(256)).unwrap_or(""))
        .filter(|v| v.parts.iter().any(|&p| p != 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::tests::spec;

    #[test]
    fn configs_round_trip() {
        let doc = ConfigDoc {
            settings: Settings::default(),
            watches: vec![spec("a"), spec("b")],
        };
        let text = render(&doc);
        assert!(text.starts_with("# Spotter configuration"));
        let back = parse(&text).unwrap();
        assert_eq!(back.doc, doc);
        assert!(back.warnings.is_empty() && !back.migrated);
        assert_eq!(parse("").unwrap().doc, ConfigDoc::default());
    }

    #[test]
    fn the_old_config_is_migrated() {
        let old = "path = '\\\\server\\builds'\nfile_regex = '.*.txt'\ninterval_seconds = 30\nnaggy = true\n";
        let l = parse(old).unwrap();
        assert!(l.migrated);
        let w = &l.doc.watches[0];
        assert_eq!(
            (w.id.as_str(), w.versions.as_str(), w.interval_secs, w.notify.nag),
            ("builds", "build-number", 30, true)
        );
        assert_eq!(w.source.values["path"], "\\\\server\\builds");
        assert_eq!(w.source.values["syntax"], "regex");
        assert!(crate::watch::Watch::parse(w).is_ok());
        let defaults = parse("file_regex = 'x'\ninterval_seconds = -5").unwrap();
        assert_eq!(defaults.doc.watches[0].source.values["path"], "/Volumes/Data/Test");
        assert_eq!(defaults.doc.watches[0].interval_secs, 1);
        assert!(!defaults.doc.watches[0].notify.nag);
    }

    #[test]
    fn damage_is_contained() {
        let mut good = render(&ConfigDoc {
            settings: Settings::default(),
            watches: vec![spec("a"), spec("a"), spec("c")],
        });
        good = good.replacen("schema = 1", "schema = 1\n", 1);
        let l = parse(&good).unwrap();
        assert_eq!(l.doc.watches.len(), 2);
        assert!(l.warnings[0].contains("same id"));

        let bad_settings = "schema = 1\nsettings = 5\n[[watches]]\nid = 'x'\n";
        let l = parse(bad_settings).unwrap();
        assert_eq!(l.doc.settings, Settings::default());
        assert_eq!(l.warnings.len(), 2, "{:?}", l.warnings);
        assert!(l.doc.watches.is_empty());

        let not_list = parse("schema = 1\nwatches = 3").unwrap();
        assert!(not_list.warnings[0].contains("not a list"));
        let no_schema = parse("[settings]\ntheme = 'dark'").unwrap();
        assert!(no_schema.warnings[0].contains("no schema"));

        assert!(parse("schema = 2").unwrap_err().contains("Update Spotter"));
        assert!(parse("schema = 0").is_err());
        assert!(parse("schema = 'x'").unwrap_err().contains("not a number"));
        assert!(parse("not toml [").unwrap_err().contains("not valid TOML"));
        assert!(parse(&"#".repeat(MAX_CONFIG_BYTES + 1)).unwrap_err().contains("larger"));
    }

    #[test]
    fn watch_count_is_bounded() {
        let watches: Vec<WatchSpec> = (0..MAX_WATCHES + 2).map(|i| spec(&format!("w{i}"))).collect();
        let l = parse(&render(&ConfigDoc {
            settings: Settings::default(),
            watches,
        }))
        .unwrap();
        assert_eq!(l.doc.watches.len(), MAX_WATCHES);
        assert!(l.warnings[0].contains("first"));
    }

    #[test]
    fn the_old_acknowledged_version_is_read() {
        assert_eq!(legacy_acknowledged("R1.01.00T10").unwrap().text, "R1.01.00T10");
        assert_eq!(legacy_acknowledged("R0.00.00T00"), None);
        assert_eq!(legacy_acknowledged("garbage"), None);
        assert_eq!(legacy_acknowledged(&"é".repeat(200)), None);
    }
}
