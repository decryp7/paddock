//! Lines from the webview and the span of an IPC command. The webview's log forwarder calls [`log_from_webview`], so its
//! lines carry the `Window` they came from; the shell wraps each command in [`ipc_span`] so everything logged while it
//! runs (including errors, with their breadcrumbs) carries `Window`, `OperationId` and `Command`.

use crate::settings::LogLevel;

/// Longest window label, operation id or command name kept (characters).
const MAX_ID: usize = 64;

fn short(text: &str) -> String {
    text.chars().take(MAX_ID).collect()
}

/// Logs a line the webview sent: `level` is a name or the log plugin's number ([`LogLevel::parse`]; unknown gives Info),
/// the event's target is `webview` and it carries `Window`. The message is bounded by the redaction rules like any other.
pub fn log_from_webview(window: &str, level: &str, message: &str) {
    let window = short(window);
    let window = window.as_str();
    match LogLevel::parse(level).unwrap_or(LogLevel::Info) {
        LogLevel::Error => tracing::error!(target: "webview", Window = window, "{}", message),
        LogLevel::Warn => tracing::warn!(target: "webview", Window = window, "{}", message),
        LogLevel::Info => tracing::info!(target: "webview", Window = window, "{}", message),
        LogLevel::Debug => tracing::debug!(target: "webview", Window = window, "{}", message),
        LogLevel::Trace => tracing::trace!(target: "webview", Window = window, "{}", message),
    }
}

/// The span to enter while an IPC command runs: its fields become properties of every event inside it.
pub fn ipc_span(window: &str, operation_id: &str, command: &str) -> tracing::Span {
    tracing::info_span!(
        "ipc",
        Window = short(window).as_str(),
        OperationId = short(operation_id).as_str(),
        Command = short(command).as_str()
    )
}
