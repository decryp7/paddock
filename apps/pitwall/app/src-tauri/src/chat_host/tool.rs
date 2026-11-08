//! A connected app's MCP tool as an assistant tool. The tool's output is untrusted data: only its text goes to the model, cut to
//! the budget; what it said never fills an approval card (the card is drawn from the parsed call).

use paddock_assistant::{Access, AssistantTool, Cancel, ToolBudget, ToolOutput, ToolSpec, Tools};
use paddock_mcp::client::{ClientError, Connection};
use paddock_mcp::types::{CallToolResult, Tool};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Longest result text the model reads from one call (ADR 0023 Decision 9: 16 KiB; the quarter-of-context bound is WP-9b's).
pub const MAX_RESULT_CHARS: usize = 16 * 1024;
/// Longest tool description passed to the model (it is the server's text, and goes into every prompt).
pub const MAX_DESCRIPTION_CHARS: usize = 1024;
/// How often a waiting call looks at Stop.
const STOP_SLICE: Duration = Duration::from_millis(50);

/// Which server and MCP tool each assistant tool name stands for (the policy keys its book on them).
#[derive(Clone, Default)]
pub struct ToolNames(Arc<Mutex<HashMap<String, (String, String)>>>);

impl ToolNames {
    /// The server and MCP tool of an assistant tool name.
    pub fn of(&self, assistant_name: &str) -> Option<(String, String)> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).get(assistant_name).cloned()
    }

    fn put(&self, assistant_name: &str, server: &str, tool: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(assistant_name.into(), (server.into(), tool.into()));
    }
}

/// `<server>_<tool>` in the assistant's `lower_snake_case`, at most 64 characters; `None` when nothing usable is left.
pub fn assistant_name(server: &str, tool: &str) -> Option<String> {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect()
    };
    let name: String = format!("{}_{}", clean(server), clean(tool)).chars().take(64).collect();
    (name.starts_with(|c: char| c.is_ascii_lowercase()) && !clean(tool).trim_matches('_').is_empty()).then_some(name)
}

/// The server's description as the model reads it: control characters out, at most [`MAX_DESCRIPTION_CHARS`].
pub fn clean_description(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_DESCRIPTION_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The tool's name for people: its MCP `title`, else one derived from its name (`top_processes` -> "Top processes").
pub fn title_of(def: &Tool) -> String {
    if let Some(t) = def.title.as_deref().map(clean_description).filter(|t| !t.is_empty()) {
        return t;
    }
    let words = def.name.replace(['_', '-'], " ");
    let mut c = words.trim().chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// The call as a verb phrase for its card and its approval: "Read top processes from Dyno" for a tool that only looks,
/// "Keep awake (Dyno)" for an action (its title is already a verb).
pub fn phrase(app: &str, title: &str, action: bool) -> String {
    if action {
        format!("{title} ({app})")
    } else {
        let mut c = title.chars();
        let lower: String = c.next().map_or_else(String::new, |f| f.to_lowercase().chain(c).collect());
        format!("Read {lower} from {app}")
    }
}

/// One MCP tool of one connected app.
pub struct McpTool {
    server: String,
    title: String,
    def: Tool,
    name: String,
    access: Access,
    conn: Arc<Connection>,
}

impl McpTool {
    /// The server's name (`dyno`) and the person-facing title of the app (`Dyno`).
    pub fn server(&self) -> &str {
        &self.server
    }
}

/// The tools of one connection as assistant tools. `family`: the user connected the server with a key it issued (a Paddock
/// app), so its `readOnlyHint` is trusted (Decision 8); any other server's tools are all actions. Tools whose names cannot
/// be made valid, or collide, are left out.
pub fn tools_of(
    server: &str,
    title: &str,
    defs: &[Tool],
    family: bool,
    conn: &Arc<Connection>,
    names: &ToolNames,
) -> Vec<Arc<dyn AssistantTool>> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<Arc<dyn AssistantTool>> = Vec::new();
    for def in defs {
        let Some(name) = assistant_name(server, &def.name) else { continue };
        if !seen.insert(name.clone()) || def.input_schema.get("type").and_then(Value::as_str) != Some("object") {
            continue;
        }
        let read_only = family && def.annotations.as_ref().and_then(|a| a.read_only_hint) == Some(true);
        names.put(&name, server, &def.name);
        out.push(Arc::new(McpTool {
            server: server.into(),
            title: title.into(),
            def: def.clone(),
            name,
            access: if read_only { Access::ReadOnly } else { Access::Write },
            conn: conn.clone(),
        }));
    }
    out.truncate(paddock_assistant::tool::MAX_TOOLS);
    out
}

/// The tool set of several connections; a set the assistant refuses becomes no tools (the chat still answers).
pub fn tool_set(tools: Vec<Arc<dyn AssistantTool>>) -> Tools {
    Tools::new(tools).unwrap_or_else(|_| Tools::none())
}

fn output_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .map(|paddock_mcp::types::Content::Text { text }| text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

impl AssistantTool for McpTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: clean_description(&format!("{}: {}", self.title, self.def.description)),
            input_schema: self.def.input_schema.clone(),
        }
    }

    fn access(&self) -> Access {
        self.access
    }

    fn title(&self) -> Option<String> {
        Some(title_of(&self.def))
    }

    fn budget(&self) -> ToolBudget {
        ToolBudget {
            max_calls: 8,
            max_output_chars: MAX_RESULT_CHARS,
        }
    }

    fn describe(&self, _input: &Value) -> String {
        phrase(&self.title, &title_of(&self.def), self.access.writes())
    }

    fn call(&self, input: &Value, cancel: &Cancel) -> ToolOutput {
        let started = Instant::now();
        let arg_keys: Vec<String> = input.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        let facts = |bytes: usize| {
            json!({"pitwall": {"server": self.server, "tool": self.def.name, "bytes": bytes,
                "durationMs": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX), "argKeys": arg_keys}})
        };
        let pending = match self.conn.start_call_tool(&self.def.name, input.clone()) {
            Ok(p) => p,
            Err(e) => return ToolOutput::failed(e.to_string()).with_data(facts(0)),
        };
        let canceller = pending.canceller();
        let (tx, rx) = channel();
        // The wait runs on its own thread; this one watches Stop and the reply (no UI or wall limit on a call, ER-19).
        std::thread::spawn(move || {
            let _ = tx.send(pending.wait_tool(None));
        });
        let result = loop {
            match rx.recv_timeout(STOP_SLICE) {
                Ok(r) => break r,
                Err(RecvTimeoutError::Timeout) => {
                    if cancel.is_cancelled() {
                        canceller.cancel("stopped by the user");
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    break Err(ClientError::Closed {
                        cause: "the call ended unexpectedly".into(),
                    })
                }
            }
        };
        match result {
            Ok(r) => {
                let text = output_of(&r);
                let bytes = text.len();
                let out = if r.is_error {
                    ToolOutput::failed(text)
                } else {
                    ToolOutput::ok(text)
                };
                out.with_data(facts(bytes))
            }
            Err(e) => ToolOutput::failed(e.to_string()).with_data(facts(0)),
        }
    }
}
