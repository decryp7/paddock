//! The log line format: Serilog's compact JSON (CLEF, decision D1), one event per line, so a log opens in Debrief itself
//! and in Seq and other Serilog tools. Debrief's own log has used it since 1.0.

use chrono::{DateTime, SecondsFormat, Utc};
use log::kv::{Key, Value as KvValue, VisitSource};
use serde_json::{Map, Value};

/// Properties attached to every event, so a log can be matched to the exact build and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrichment {
    /// The app's display name (`Application`).
    pub application: String,
    /// The process inside the app (`Process`): `app`, `server`, `agent`, ...
    pub process: String,
    /// The app's version (`Version`).
    pub version: String,
    /// The operating system's name (`Os`).
    pub os: &'static str,
    /// The build commit (`Commit`).
    pub commit: String,
    /// An id of this run (`SessionId`).
    pub session_id: String,
    /// The process id (`ProcessId`).
    pub process_id: u32,
}

/// Formats one event as a CLEF line (without the trailing newline): `target` is its `SourceContext` (a property of that
/// name wins), `props` its properties.
///
/// `@t` time, `@l` level (omitted for Information, as CLEF does), `@m` the rendered message; `Severity = "Fatal"` raises
/// the level above what `log` has; `MessageTemplate` becomes `@mt`; keys starting with `@` are escaped as `@@`.
pub fn format_event(
    time: DateTime<Utc>,
    level: log::Level,
    target: &str,
    message: &str,
    props: Map<String, Value>,
    enrich: &Enrichment,
) -> String {
    let mut event = Map::new();
    event.insert(
        "@t".into(),
        Value::from(time.to_rfc3339_opts(SecondsFormat::Micros, true)),
    );
    if let Some(level) = clef_level(level) {
        event.insert("@l".into(), Value::from(level));
    }
    event.insert("@m".into(), Value::from(message));
    event.insert("SourceContext".into(), Value::from(target));
    event.insert(
        "Application".into(),
        Value::from(enrich.application.as_str()),
    );
    event.insert("Process".into(), Value::from(enrich.process.as_str()));
    event.insert("Version".into(), Value::from(enrich.version.as_str()));
    event.insert("ProcessId".into(), Value::from(enrich.process_id));
    event.insert("Commit".into(), Value::from(enrich.commit.as_str()));
    event.insert("Os".into(), Value::from(enrich.os));
    event.insert("SessionId".into(), Value::from(enrich.session_id.as_str()));

    for (k, v) in props {
        if k == "Severity" && v.as_str() == Some("Fatal") {
            event.insert("@l".into(), Value::from("Fatal"));
            continue;
        }
        if k == "MessageTemplate" {
            event.insert("@mt".into(), v);
            continue;
        }
        let k = if k.starts_with('@') {
            format!("@{k}")
        } else {
            k
        };
        event.insert(k, v);
    }
    Value::Object(event).to_string()
}

fn clef_level(level: log::Level) -> Option<&'static str> {
    match level {
        log::Level::Error => Some("Error"),
        log::Level::Warn => Some("Warning"),
        log::Level::Info => None,
        log::Level::Debug => Some("Debug"),
        log::Level::Trace => Some("Verbose"),
    }
}

/// A `log` record's key/values as JSON (booleans and numbers keep their type).
pub fn key_values(source: &dyn log::kv::Source) -> Map<String, Value> {
    let mut props = Props(Map::new());
    let _ = source.visit(&mut props);
    props.0
}

struct Props(Map<String, Value>);

impl<'kvs> VisitSource<'kvs> for Props {
    fn visit_pair(&mut self, key: Key<'kvs>, value: KvValue<'kvs>) -> Result<(), log::kv::Error> {
        let v = if let Some(b) = value.to_bool() {
            Value::from(b)
        } else if let Some(i) = value.to_i64() {
            Value::from(i)
        } else if let Some(f) = value.to_f64() {
            Value::from(f)
        } else {
            Value::from(value.to_string())
        };
        self.0.insert(key.as_str().to_string(), v);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn test_enrichment() -> Enrichment {
    Enrichment {
        application: "Demo".into(),
        process: "app".into(),
        version: "1.2.3".into(),
        os: "macos",
        commit: "abc1234".into(),
        session_id: "0123456789abcdef".into(),
        process_id: 42,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn parse(line: &str) -> Map<String, Value> {
        serde_json::from_str::<Value>(line)
            .unwrap()
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn formats_levels_and_enrichment() {
        let t = Utc.with_ymd_and_hms(2026, 10, 17, 8, 30, 0).unwrap();
        let e = parse(&format_event(
            t,
            log::Level::Info,
            "demo",
            "Starting",
            Map::new(),
            &test_enrichment(),
        ));
        assert_eq!(e["@t"], "2026-10-17T08:30:00.000000Z");
        assert!(!e.contains_key("@l"), "Information omits @l");
        assert_eq!(
            (
                e["@m"].as_str(),
                e["SourceContext"].as_str(),
                e["Process"].as_str()
            ),
            (Some("Starting"), Some("demo"), Some("app"))
        );
        assert_eq!(
            (
                e["Application"].as_str(),
                e["Version"].as_str(),
                e["ProcessId"].as_u64()
            ),
            (Some("Demo"), Some("1.2.3"), Some(42))
        );
        for (level, want) in [
            (log::Level::Error, "Error"),
            (log::Level::Warn, "Warning"),
            (log::Level::Debug, "Debug"),
            (log::Level::Trace, "Verbose"),
        ] {
            assert_eq!(
                parse(&format_event(
                    t,
                    level,
                    "t",
                    "m",
                    Map::new(),
                    &test_enrichment()
                ))["@l"],
                want
            );
        }
    }

    #[test]
    fn properties_keep_types_and_reserved_keys_are_handled() {
        let t = Utc.with_ymd_and_hms(2026, 10, 17, 8, 30, 0).unwrap();
        let mut props = Map::new();
        props.insert("Rows".into(), 3.into());
        props.insert("@secret".into(), "x".into());
        props.insert("MessageTemplate".into(), "Opened {Rows}".into());
        props.insert("Severity".into(), "Fatal".into());
        let e = parse(&format_event(
            t,
            log::Level::Error,
            "t",
            "m",
            props,
            &test_enrichment(),
        ));
        assert_eq!(
            (
                e["Rows"].as_i64(),
                e["@@secret"].as_str(),
                e["@mt"].as_str(),
                e["@l"].as_str()
            ),
            (Some(3), Some("x"), Some("Opened {Rows}"), Some("Fatal"))
        );
        assert!(!e.contains_key("Severity") && !e.contains_key("@secret"));
    }

    #[test]
    fn key_values_keep_booleans_and_numbers() {
        let kvs: &[(&str, KvValue)] = &[
            ("On", KvValue::from(true)),
            ("N", KvValue::from(7u64)),
            ("F", KvValue::from(1.5f64)),
            ("S", KvValue::from("s")),
        ];
        let m = key_values(&kvs);
        assert_eq!(
            (
                m["On"].as_bool(),
                m["N"].as_i64(),
                m["F"].as_f64(),
                m["S"].as_str()
            ),
            (Some(true), Some(7), Some(1.5), Some("s"))
        );
    }
}
