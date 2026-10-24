//! The `log` side: records from dependencies, from the app's own `log::info!` and from the webview's forwarded lines go
//! into `tracing`, so both reach the one CLEF stream. Key/values travel as one JSON field so properties keep their type.

use serde_json::Value;

/// Forwards `log` records to `tracing` (install with `log::set_boxed_logger`; [`crate::Logging::install`] does).
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingBridge;

macro_rules! bridged {
    ($level:expr, $record:expr, $kv:expr) => {
        tracing::event!(target: "log", $level, { log.target = $record.target(), log.kv = $kv }, "{}", $record.args())
    };
}

impl log::Log for TracingBridge {
    fn enabled(&self, _: &log::Metadata) -> bool {
        // `log::max_level` and the subscriber's level filter decide.
        true
    }

    fn log(&self, record: &log::Record) {
        thread_local! {
            static INSIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }
        // Without a subscriber there is nowhere to forward to yet; never recurse either.
        if !tracing::dispatcher::has_been_set() || INSIDE.with(|i| i.replace(true)) {
            return;
        }
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                INSIDE.with(|i| i.set(false));
            }
        }
        let _reset = Reset;
        let kv = crate::clef::key_values(record.key_values());
        if kv.is_empty() {
            let _ = tracing_log::format_trace(record);
            return;
        }
        let kv = Value::Object(kv).to_string();
        let kv = kv.as_str();
        match record.level() {
            log::Level::Error => bridged!(tracing::Level::ERROR, record, kv),
            log::Level::Warn => bridged!(tracing::Level::WARN, record, kv),
            log::Level::Info => bridged!(tracing::Level::INFO, record, kv),
            log::Level::Debug => bridged!(tracing::Level::DEBUG, record, kv),
            log::Level::Trace => bridged!(tracing::Level::TRACE, record, kv),
        }
    }

    fn flush(&self) {}
}
