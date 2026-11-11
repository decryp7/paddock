//! One `pitwall_chat` log line per tool call (ADR 0023 Decision 8, ER-27): the conversation and call ids, the server and tool,
//! how it was approved, the outcome, the duration, the result's size, the argument names the tool's schema declares and a count
//! of the others. No argument values and no result text (no user content in logs).

use paddock_assistant::{Approval, AssistantEvent, Envelope, EventSink, ToolOutcome};
use serde_json::Value;
use std::sync::Arc;

/// Text from outside (a tool's or server's name, an argument name, a name sent over IPC) as it goes into a log line: control
/// characters escaped, so it can never end the line and start a forged one (ER-27).
fn plain(text: &str) -> String {
    text.escape_debug().to_string()
}

/// The line for "asks again" (Settings > Connected apps), whose `tool` comes from the window's IPC call.
pub fn revoke_line(id: &str, tool: &str) -> String {
    format!("connected apps: {} {} asks again", plain(id), plain(tool))
}

/// The line for a finished call.
pub fn line(conversation: &str, event: &AssistantEvent) -> Option<String> {
    let AssistantEvent::ToolFinished {
        call_id,
        name,
        ok,
        data,
        outcome,
        approval,
        ..
    } = event
    else {
        return None;
    };
    let facts = data.as_ref().and_then(|d| d.get("pitwall"));
    let get = |k: &str| facts.and_then(|f| f.get(k));
    let server = plain(get("server").and_then(Value::as_str).unwrap_or("-"));
    let tool = plain(get("tool").and_then(Value::as_str).unwrap_or(name));
    let keys: Vec<String> = get("argKeys")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(plain).collect())
        .unwrap_or_default();
    let how = match approval {
        Some(Approval::NotAsked) => "not_asked",
        Some(Approval::Once) => "once",
        Some(Approval::Always) => "always",
        Some(Approval::Remembered) => "remembered",
        None => "none",
    };
    let result = match (outcome, ok) {
        (Some(ToolOutcome::Declined), _) => "declined",
        (Some(ToolOutcome::Stopped), _) => "stopped",
        (None, true) => "ok",
        (None, false) => "failed",
    };
    Some(format!(
        "tool call conversation={} call={} server={server} tool={tool} approval={how} outcome={result} duration_ms={} result_bytes={} args=[{}] other_args={}",
        plain(conversation),
        plain(call_id),
        get("durationMs").and_then(Value::as_u64).unwrap_or(0),
        get("bytes").and_then(Value::as_u64).unwrap_or(0),
        keys.join(","),
        get("otherArgs").and_then(Value::as_u64).unwrap_or(0)
    ))
}

/// Passes every event on and logs one line per finished call.
pub struct ChatLog(pub Arc<dyn EventSink>);

impl EventSink for ChatLog {
    fn emit(&self, envelope: &Envelope) {
        if let Some(l) = line(&envelope.conversation_id, &envelope.event) {
            log::info!(target: "pitwall_chat", "{l}");
        }
        self.0.emit(envelope);
    }
}
