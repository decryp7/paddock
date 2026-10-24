//! The generated TypeScript of the wire types, checked on every run (a stale file fails CI).

use crate::notify::{self, persist};
use std::path::{Path, PathBuf};

/// Where the generated TypeScript goes: the `shell/notify` entry point of `@decryp7/paddock-ui`, which uses it.
pub(crate) fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/notify/generated")
}

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-health --all-features --lib generated";

/// `health.ts`: the events, the service-health types, the notification types and the command error.
fn health_ts() -> String {
    use ts_rs::TS;
    let decls = [
        crate::ServiceState::decl(),
        crate::RepairAction::decl(),
        crate::RaiseCap::decl(),
        crate::Cause::decl(),
        crate::ServiceHealth::decl(),
        crate::Snapshot::decl(),
        notify::centre::NoticeId::decl(),
        notify::centre::NoticeSource::decl(),
        notify::centre::NoticeSeverity::decl(),
        notify::centre::NoticeScope::decl(),
        notify::centre::NoticeLink::decl(),
        notify::centre::Notice::decl(),
        notify::NoticesSnapshot::decl(),
        notify::NoticesEvent::decl(),
        notify::NoticeActionEvent::decl(),
        notify::RestoredNotices::decl(),
        persist::NoticesRecord::decl(),
        notify::window::WindowNoticeScope::decl(),
        notify::window::WindowNotice::decl(),
        crate::HealthErrorCode::decl(),
        crate::HealthError::decl(),
    ];
    let mut out = format!(
        "// Generated from the Rust wire types of paddock-health (ts-rs). Do not edit: {REGENERATE}.\n\n\
         /** Every window hears it when a service changed; the payload is a `ServiceHealthSnapshot`. */\n\
         export const SERVICE_HEALTH_EVENT = '{}';\n\n\
         /** A window hears it when its notifications changed; the payload is a `NoticesEvent`. */\n\
         export const NOTIFICATIONS_EVENT = '{}';\n\n\
         /** Open this message (its OS notification was clicked); the payload is a `NoticeId`. */\n\
         export const NOTIFICATIONS_OPEN_EVENT = '{}';\n\n\
         /** Run this action of a message (a button of its OS notification that only a window can run); the payload is a `NoticeActionEvent`. */\n\
         export const NOTIFICATIONS_ACTION_EVENT = '{}';\n\n\
         /** The notification cause of a service's outage. */\n\
         export const healthCause = (service: string): string => `health:${{service}}`;\n\n\
         /**\n * What a message's button does: a `type` the UI maps to a feature, and its arguments, flat\n \
         * (`{{ type: 'repairService', service: 'store' }}`).\n */\n\
         export type NoticeAction = {{ type: string }} & {{ [arg: string]: unknown }};\n\n",
        crate::SERVICE_HEALTH_EVENT,
        notify::NOTIFICATIONS_EVENT,
        notify::OPEN_EVENT,
        notify::ACTION_EVENT,
    );
    for d in decls {
        out.push_str(&format!("export {d}\n\n"));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// Checks (or, with `PADDOCK_WRITE_TS=1`, rewrites) a generated file of the shell's entry point.
pub(crate) fn sync(name: &str, text: &str) {
    use paddock_commands::testing::{WriteMode, sync_generated};
    let path = generated_dir().join(name);
    sync_generated(&path, text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn generated_typescript_is_current() {
    sync("health.ts", &health_ts());
}

/// Every log record of this crate (target `paddock_health`), as (level, message); tests that look at logs install it.
static LOGGED: std::sync::Mutex<Vec<(log::Level, String)>> = std::sync::Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        if record.target() == "paddock_health" {
            let mut logged = LOGGED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            logged.push((record.level(), record.args().to_string()));
        }
    }
    fn flush(&self) {}
}

/// Installs the capturing logger (once per process; tests run in parallel, so callers look for their own records).
pub(crate) fn capture_logs() {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Trace);
}

/// Whether a record at `level` containing `text` was logged.
pub(crate) fn logged(level: log::Level, text: &str) -> bool {
    let logged = LOGGED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    logged.iter().any(|(l, m)| *l == level && m.contains(text))
}

#[test]
fn the_logger_captures_this_crates_records_only() {
    capture_logs();
    log::info!(target: "paddock_health", "captured-record");
    log::info!(target: "elsewhere", "foreign-record");
    assert!(logged(log::Level::Info, "captured-record"));
    assert!(!logged(log::Level::Info, "foreign-record"));
    assert!(log::logger().enabled(&log::Metadata::builder().target("x").build()));
    log::logger().flush();
}
