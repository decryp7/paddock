//! Tools the model may call. An app implements [`AssistantTool`] for what the assistant can look at (read-only) or do
//! (write) and registers them with [`Tools`]. Tools stay in Rust: the UI only renders their results.
//!
//! A tool that writes ([`Access::Write`]) is never offered to the model unless the user ticked "Allow actions" for that
//! message, and every call of it waits for the user's explicit approval ([`crate::Assistant::approve`]) before it runs.

use crate::types::{Cancel, ToolSpec};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Most tools one assistant has (the prompt has to hold their descriptions).
pub const MAX_TOOLS: usize = 64;
/// Longest tool name.
pub const MAX_TOOL_NAME: usize = 64;

/// What a tool may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Looks at things only. Offered to the model by default; runs without asking.
    ReadOnly,
    /// Changes something. Offered only after a user click, and each call needs the user's approval.
    Write,
    /// Changes something that cannot be undone (erases, overwrites, sends). Treated as [`Access::Write`], and the approval
    /// is drawn as a danger choice with Decline as the default.
    Destructive,
}

impl Access {
    /// Whether the tool changes things (needs "Allow actions" and an approval per call).
    pub fn writes(self) -> bool {
        self != Access::ReadOnly
    }

    /// Whether what it changes cannot be undone.
    pub fn destructive(self) -> bool {
        self == Access::Destructive
    }
}

/// The limits of one tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolBudget {
    /// Most calls in one turn; further calls are refused with a message the model can read.
    pub max_calls: usize,
    /// Longest result text sent to the model; longer results are cut (the UI still gets `data`).
    pub max_output_chars: usize,
}

impl Default for ToolBudget {
    fn default() -> Self {
        ToolBudget {
            max_calls: 8,
            max_output_chars: 6000,
        }
    }
}

/// What a tool returns.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    /// What the model reads.
    pub text: String,
    /// One line for the tool card ("12 events found").
    pub summary: String,
    /// Structured result for the app's renderer of this tool (bounded when stored); never shown as HTML.
    pub data: Option<Value>,
    /// The call failed (the model is told; the card shows an error).
    pub error: bool,
}

impl ToolOutput {
    /// A result with the same text for the model and the card.
    pub fn ok(text: impl Into<String>) -> ToolOutput {
        let text = text.into();
        ToolOutput {
            summary: text.chars().take(120).collect(),
            text,
            data: None,
            error: false,
        }
    }

    /// A failed call; `message` tells the model (and the user) what went wrong.
    pub fn failed(message: impl Into<String>) -> ToolOutput {
        let text = message.into();
        ToolOutput {
            summary: text.chars().take(120).collect(),
            text,
            data: None,
            error: true,
        }
    }

    /// With structured data for the app's renderer.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> ToolOutput {
        self.data = Some(data);
        self
    }

    /// With a card line of its own.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> ToolOutput {
        self.summary = summary.into();
        self
    }
}

/// A tool of the app. Implementations must not panic on any input (the loop treats a panic as a failed call anyway) and
/// should honour `cancel` in long work.
pub trait AssistantTool: Send + Sync {
    /// Name, description and the arguments' JSON schema.
    fn spec(&self) -> ToolSpec;

    /// Its name for people ("Top processes"), shown on its card and in lists; `None` (the default) shows the name.
    fn title(&self) -> Option<String> {
        None
    }

    /// Whether it only looks or also changes things. Defaults to read-only.
    fn access(&self) -> Access {
        Access::ReadOnly
    }

    /// Its limits.
    fn budget(&self) -> ToolBudget {
        ToolBudget::default()
    }

    /// The call as the user reads it before approving ("Delete 3 bookmarks"); defaults to the arguments.
    fn describe(&self, input: &Value) -> String {
        input.to_string()
    }

    /// The first problem with `input` against this tool's schema, checked before the approval policy runs: a call that fails
    /// never reaches a card (ADR 0023 Decision 9). Defaults to the small check of [`argument_problem`]; a tool whose schema
    /// has more keywords overrides it with a full validator. A host whose tools write must override it with full schema
    /// validation: the default only checks that the arguments are an object with their `required` keys.
    fn argument_problem(&self, input: &Value) -> Option<String> {
        argument_problem(&self.spec(), input)
    }

    /// Runs the call.
    fn call(&self, input: &Value, cancel: &Cancel) -> ToolOutput;
}

/// Why a set of tools was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolsError {
    /// More than [`MAX_TOOLS`].
    TooMany,
    /// A name that is empty, longer than [`MAX_TOOL_NAME`] or not `lower_snake_case`.
    BadName(String),
    /// Two tools with this name.
    Duplicate(String),
    /// A schema that is not an object schema.
    BadSchema(String),
}

impl std::fmt::Display for ToolsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolsError::TooMany => write!(f, "more than {MAX_TOOLS} tools"),
            ToolsError::BadName(n) => write!(
                f,
                "the tool name {n:?} is not lower_snake_case of at most {MAX_TOOL_NAME} characters"
            ),
            ToolsError::Duplicate(n) => write!(f, "two tools are called {n}"),
            ToolsError::BadSchema(n) => write!(f, "the schema of {n} is not a JSON object schema"),
        }
    }
}

impl std::error::Error for ToolsError {}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_TOOL_NAME
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The validated set of an app's tools.
#[derive(Default, Clone)]
pub struct Tools {
    by_name: BTreeMap<String, Arc<dyn AssistantTool>>,
}

impl Tools {
    /// No tools: the assistant only chats.
    pub fn none() -> Tools {
        Tools::default()
    }

    /// Validates and registers `tools` (unique `lower_snake_case` names, object schemas).
    pub fn new(tools: Vec<Arc<dyn AssistantTool>>) -> Result<Tools, ToolsError> {
        if tools.len() > MAX_TOOLS {
            return Err(ToolsError::TooMany);
        }
        let mut by_name = BTreeMap::new();
        for tool in tools {
            let spec = tool.spec();
            if !valid_name(&spec.name) {
                return Err(ToolsError::BadName(spec.name));
            }
            if spec.input_schema.get("type").and_then(Value::as_str) != Some("object") {
                return Err(ToolsError::BadSchema(spec.name));
            }
            if by_name.insert(spec.name.clone(), tool).is_some() {
                return Err(ToolsError::Duplicate(spec.name));
            }
        }
        Ok(Tools { by_name })
    }

    /// The tool called `name`.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn AssistantTool>> {
        self.by_name.get(name)
    }

    /// Whether there is any tool.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Whether any tool writes.
    pub fn has_write(&self) -> bool {
        self.by_name.values().any(|t| t.access().writes())
    }

    /// The specs to offer the model: read-only tools, plus the writing ones when the user allowed actions.
    pub fn offered(&self, allow_write: bool) -> Vec<ToolSpec> {
        self.by_name
            .values()
            .filter(|t| allow_write || t.access() == Access::ReadOnly)
            .map(|t| t.spec())
            .collect()
    }

    /// Every tool as the UI lists it.
    pub fn infos(&self) -> Vec<ToolInfo> {
        self.by_name
            .values()
            .map(|t| {
                let spec = t.spec();
                ToolInfo {
                    name: spec.name,
                    description: spec.description,
                    write: t.access().writes(),
                    destructive: t.access().destructive(),
                    title: t.title(),
                }
            })
            .collect()
    }
}

/// A tool as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    /// The name.
    pub name: String,
    /// What it does.
    pub description: String,
    /// It changes things (needs the user's approval per call).
    pub write: bool,
    /// What it changes cannot be undone (implies `write`): its approval is a danger choice.
    pub destructive: bool,
    /// Its name for people, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
}

/// The first problem with `input` against the tool's schema: not an object, or a `required` argument missing. The check is
/// deliberately small (the tool validates values itself); it keeps obviously empty calls from running.
pub fn argument_problem(spec: &ToolSpec, input: &Value) -> Option<String> {
    let Some(object) = input.as_object() else {
        return Some("the arguments must be a JSON object".into());
    };
    let missing: Vec<&str> = spec
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|k| !object.contains_key(*k))
        .collect();
    (!missing.is_empty()).then(|| {
        format!(
            "missing argument{}: {}",
            if missing.len() == 1 { "" } else { "s" },
            missing.join(", ")
        )
    })
}
