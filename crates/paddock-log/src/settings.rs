use serde::{Deserialize, Serialize};

/// The level the log writes at; changeable at run time without a restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Only errors.
    Error,
    /// Warnings and errors.
    Warn,
    /// The normal level.
    #[default]
    Info,
    /// Detail for support cases.
    Debug,
    /// Everything.
    Trace,
}

impl LogLevel {
    /// The `log` filter for this level.
    pub fn log_filter(self) -> log::LevelFilter {
        match self {
            LogLevel::Error => log::LevelFilter::Error,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Debug,
            LogLevel::Trace => log::LevelFilter::Trace,
        }
    }

    pub(crate) fn tracing_filter(self) -> tracing::level_filters::LevelFilter {
        use tracing::level_filters::LevelFilter as F;
        match self {
            LogLevel::Error => F::ERROR,
            LogLevel::Warn => F::WARN,
            LogLevel::Info => F::INFO,
            LogLevel::Debug => F::DEBUG,
            LogLevel::Trace => F::TRACE,
        }
    }

    /// Parses a level name (`error`, `warn`, `info`, `debug`, `trace`, any case) or the webview log plugin's number
    /// (1 trace .. 5 error).
    pub fn parse(text: &str) -> Option<LogLevel> {
        match text.trim().to_ascii_lowercase().as_str() {
            "error" | "5" => Some(LogLevel::Error),
            "warn" | "warning" | "4" => Some(LogLevel::Warn),
            "info" | "3" => Some(LogLevel::Info),
            "debug" | "2" => Some(LogLevel::Debug),
            "trace" | "verbose" | "1" => Some(LogLevel::Trace),
            _ => None,
        }
    }
}

/// Smallest and largest values of the limits (the settings section clamps to them; the engine re-validates).
pub const MAX_FILE_MB_RANGE: (u32, u32) = (1, 100);
/// Range of `keep_files`.
pub const KEEP_FILES_RANGE: (u32, u32) = (1, 50);

/// The "Diagnostics > Logging" settings: a month of daily files (decision of 2026-11-05; D2 gave seven days), each at most 20 MB, and 100 MB in all per process
/// ([`crate::TOTAL_CAP_BYTES`]; the oldest days go first, and Debug can use only half of a day's budget).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LogSettings {
    /// The level.
    pub level: LogLevel,
    /// Most megabytes written in one day's file; after that only errors are written until the next file.
    pub max_file_mb: u32,
    /// Old daily files kept besides today's.
    pub keep_files: u32,
}

impl Default for LogSettings {
    fn default() -> Self {
        LogSettings {
            level: LogLevel::Info,
            max_file_mb: 20,
            keep_files: 30,
        }
    }
}

impl LogSettings {
    /// The settings with every value brought into its allowed range.
    pub fn clamped(mut self) -> Self {
        self.max_file_mb = self
            .max_file_mb
            .clamp(MAX_FILE_MB_RANGE.0, MAX_FILE_MB_RANGE.1);
        self.keep_files = self
            .keep_files
            .clamp(KEEP_FILES_RANGE.0, KEEP_FILES_RANGE.1);
        self
    }

    pub(crate) fn max_bytes(&self) -> u64 {
        u64::from(self.max_file_mb) * 1024 * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_decision_d2_and_clamp() {
        let d = LogSettings::default();
        assert_eq!(
            (d.level, d.max_file_mb, d.keep_files),
            (LogLevel::Info, 20, 30)
        );
        let c = LogSettings {
            level: LogLevel::Debug,
            max_file_mb: 0,
            keep_files: 9999,
        }
        .clamped();
        assert_eq!((c.max_file_mb, c.keep_files), (1, 50));
        let c = LogSettings {
            max_file_mb: 5000,
            keep_files: 0,
            ..d
        }
        .clamped();
        assert_eq!((c.max_file_mb, c.keep_files), (100, 1));
    }

    #[test]
    fn settings_round_trip_and_tolerate_missing_keys() {
        let s: LogSettings = serde_json::from_str(r#"{"level":"debug"}"#).unwrap();
        assert_eq!((s.level, s.keep_files), (LogLevel::Debug, 30));
        assert_eq!(
            serde_json::to_string(&LogSettings::default()).unwrap(),
            r#"{"level":"info","maxFileMb":20,"keepFiles":30}"#
        );
    }

    #[test]
    fn parses_names_and_plugin_numbers() {
        for (text, want) in [
            ("ERROR", LogLevel::Error),
            ("Warning", LogLevel::Warn),
            ("3", LogLevel::Info),
            ("2", LogLevel::Debug),
            ("1", LogLevel::Trace),
        ] {
            assert_eq!(LogLevel::parse(text), Some(want), "{text}");
        }
        assert_eq!(LogLevel::parse("loud"), None);
        assert_eq!(LogLevel::Warn.log_filter(), log::LevelFilter::Warn);
    }
}
