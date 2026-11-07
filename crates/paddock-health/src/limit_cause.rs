//! A [`Cause`] built from the limit that stopped a service (ADR 0019 decisions 1 and 3): the message is generated from the
//! type, the stable code is `limit.<id>`, and a limit that is a memory Setting offers "Raise the cap...".

use crate::model::Cause;
use paddock_limits::{LimitExceeded, Quantity, Registry, SettingLabels};

/// The largest cap suggested, in MiB (a `u32` of them).
const MAX_SUGGESTED_MB: u64 = u32::MAX as u64;

impl Cause {
    /// The cause for a service that `limit` stopped.
    ///
    /// - `message`: for a person (the schema's words through `labels` when given), without the limit's internal id;
    /// - `detail`: the logging form of the same message, with the id and the observed value;
    /// - `code`: `limit.<id>`, the stable key of the wording;
    /// - `settings`: the section of the Setting that raises the limit (the part of its path before the first `.`);
    /// - `raise_cap`: for a limit in bytes that a Setting raises, the plugin or process (`target`) and double the limit, in MiB.
    pub fn from_limit(
        limit: &LimitExceeded,
        registry: &Registry,
        labels: Option<&dyn SettingLabels>,
        target: &str,
    ) -> Cause {
        let words = match labels {
            Some(labels) => limit.message_with(registry, labels),
            None => limit.message(registry),
        };
        let mut cause = Cause::new(words)
            .detail(limit.message(registry))
            .code(format!("limit.{}", limit.id));
        if let Some(path) = limit.setting_path(registry) {
            cause.settings = path.as_str().split('.').next().map(Box::from);
            if let Quantity::Bytes(bytes) = limit.limit {
                let mb = (bytes / (1 << 20))
                    .saturating_mul(2)
                    .clamp(1, MAX_SUGGESTED_MB);
                cause = cause.raise_cap(target, u32::try_from(mb).unwrap_or(u32::MAX));
            }
        }
        cause
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_limits::{LimitId, LimitRow, Remedy, RowKind, RowValue, Scope, SettingPath, Unit};

    static ROWS: [LimitRow; 2] = [
        LimitRow {
            id: LimitId::new("plugin.memory"),
            unit: Unit::Bytes,
            default: Quantity::Bytes(1 << 30),
            min: Quantity::Bytes(64 << 20),
            max: Quantity::Bytes(4 << 30),
            value: RowValue::Setting {
                path: SettingPath::new("performance.pluginMemoryMb"),
            },
            kind: RowKind::Stop,
            selections: &[],
            remedy: Remedy::RaiseSetting {
                path: SettingPath::new("performance.pluginMemoryMb"),
            },
        },
        LimitRow {
            id: LimitId::new("ipc.frame"),
            unit: Unit::Bytes,
            default: Quantity::Bytes(16 << 20),
            min: Quantity::Bytes(16 << 20),
            max: Quantity::Bytes(16 << 20),
            value: RowValue::Hard,
            kind: RowKind::Stop,
            selections: &[],
            remedy: Remedy::Narrow,
        },
    ];

    struct Labels;
    impl SettingLabels for Labels {
        fn label(&self, _: SettingPath) -> Option<(&str, &str)> {
            Some(("Performance", "Plugin memory"))
        }
    }

    #[test]
    fn a_memory_limit_offers_the_cap_and_names_the_section() {
        let reg = Registry::new(&ROWS);
        let limit = LimitExceeded::new(
            LimitId::new("plugin.memory"),
            Quantity::Bytes(1 << 30),
            Scope::Plugin,
        )
        .reaching(Quantity::Bytes(1210 << 20))
        .unwrap();
        let cause = Cause::from_limit(&limit, &reg, Some(&Labels), "plugin:net.example.p");
        assert_eq!(cause.code.as_deref(), Some("limit.plugin.memory"));
        assert_eq!(cause.settings.as_deref(), Some("performance"));
        let cap = cause
            .raise_cap
            .as_deref()
            .expect("a memory limit offers the cap");
        assert_eq!(
            (cap.target.as_str(), cap.suggested_mb),
            ("plugin:net.example.p", 2048)
        );
        assert!(
            cause.message.contains("Performance \u{2192} Plugin memory")
                && !cause.message.contains("plugin.memory")
        );
        assert!(
            cause
                .detail
                .as_deref()
                .is_some_and(|d| d.contains("(plugin.memory)") && d.contains("1.2 GiB"))
        );
        // Without labels the message carries the dotted path.
        let plain = Cause::from_limit(&limit, &reg, None, "plugin:p");
        assert!(plain.message.contains("performance.pluginMemoryMb"));
    }

    #[test]
    fn a_hard_limit_has_no_cap_and_no_settings_section() {
        let reg = Registry::new(&ROWS);
        let limit = LimitExceeded::new(
            LimitId::new("ipc.frame"),
            Quantity::Bytes(16 << 20),
            Scope::Process,
        );
        let cause = Cause::from_limit(&limit, &reg, None, "plugin:p");
        assert!(cause.raise_cap.is_none() && cause.settings.is_none());
        assert_eq!(cause.code.as_deref(), Some("limit.ipc.frame"));
        assert!(
            cause.message.ends_with("Narrow the query or the input."),
            "{}",
            cause.message
        );
    }

    #[test]
    fn a_count_limit_with_a_setting_names_the_section_but_offers_no_memory_cap() {
        static COUNT: [LimitRow; 1] = [LimitRow {
            id: LimitId::new("query.hits"),
            unit: Unit::Count,
            default: Quantity::Count(10_000),
            min: Quantity::Count(100),
            max: Quantity::Count(1_000_000),
            value: RowValue::Setting {
                path: SettingPath::new("performance.maxHits"),
            },
            kind: RowKind::Stop,
            selections: &[],
            remedy: Remedy::RaiseSetting {
                path: SettingPath::new("performance.maxHits"),
            },
        }];
        let limit = LimitExceeded::new(
            LimitId::new("query.hits"),
            Quantity::Count(10_000),
            Scope::Query,
        );
        let cause = Cause::from_limit(&limit, &Registry::new(&COUNT), None, "query");
        assert_eq!(cause.settings.as_deref(), Some("performance"));
        assert!(cause.raise_cap.is_none());
    }

    #[test]
    fn a_huge_limit_saturates_the_suggestion() {
        let reg = Registry::new(&ROWS);
        let limit = LimitExceeded::new(
            LimitId::new("plugin.memory"),
            Quantity::Bytes(u64::MAX),
            Scope::Plugin,
        );
        let cap = Cause::from_limit(&limit, &reg, None, "p")
            .raise_cap
            .expect("cap");
        assert_eq!(cap.suggested_mb, u32::MAX);
    }
}
