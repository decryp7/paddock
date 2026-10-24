//! MCP message types (2025-06-18), shared by the server and by clients such as Pitwall. Serialization is camelCase as the spec has it.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The protocol versions this crate speaks, newest first.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Name and version of a client or server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    /// The program.
    pub name: String,
    /// Its version.
    pub version: String,
    /// A name for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// What a tool declares about itself; hosts use it to decide what to confirm.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// The tool changes nothing. True for every tool unless the app opted in to actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// Only meaningful when not read-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// The tool only looks at what it is given, never at the wider world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// A tool as `tools/list` shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// The name the model calls.
    pub name: String,
    /// A name for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What it does and when to use it.
    pub description: String,
    /// JSON Schema of the arguments (an object).
    pub input_schema: Value,
    /// Hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

/// The result of `tools/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    /// This page.
    pub tools: Vec<Tool>,
    /// Pass as `cursor` for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// One piece of a tool's result. Tool output is untrusted data to the host (ADR 0007 rule 8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Content {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
}

/// The result of `tools/call`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    /// What the model reads.
    pub content: Vec<Content>,
    /// The same data as JSON, when the tool has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    /// The call failed; the text says why and what to change.
    #[serde(default)]
    pub is_error: bool,
    /// Implementation extras: `paddock/nextCursor`, `paddock/truncated`.
    #[serde(default, rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

impl CallToolResult {
    /// A failed call with a message.
    pub fn error(text: impl Into<String>) -> CallToolResult {
        CallToolResult {
            content: vec![Content::Text { text: text.into() }],
            structured_content: None,
            is_error: true,
            meta: None,
        }
    }

    /// The text of the first text content (what a client shows or feeds the model).
    pub fn text(&self) -> &str {
        self.content
            .iter()
            .map(|Content::Text { text }| text.as_str())
            .next()
            .unwrap_or("")
    }
}

/// A resource as `resources/list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    /// Its address (`debrief://summary`).
    pub uri: String,
    /// A short name.
    pub name: String,
    /// What it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The content type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// The result of `resources/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    /// This page.
    pub resources: Vec<Resource>,
    /// Pass as `cursor` for the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// The text of a resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContents {
    /// Its address.
    pub uri: String,
    /// The content type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// The text.
    pub text: String,
}

/// The result of `resources/read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// One entry for the resource.
    pub contents: Vec<ResourceContents>,
}

/// What a server can do, as `initialize` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Present when the server has tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Map<String, Value>>,
    /// Present when the server has resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<Map<String, Value>>,
}

/// The result of `initialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// The version both sides will speak.
    pub protocol_version: String,
    /// What the server offers.
    pub capabilities: ServerCapabilities,
    /// Who the server is.
    pub server_info: Implementation,
    /// How to use it, for the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// The version to speak: the client's when this crate knows it, else the newest.
pub fn negotiate(requested: Option<&str>) -> &'static str {
    PROTOCOL_VERSIONS
        .iter()
        .copied()
        .find(|v| Some(*v) == requested)
        .unwrap_or(PROTOCOL_VERSIONS[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialization_is_the_specs_camel_case() {
        let t = Tool {
            name: "search".into(),
            title: None,
            description: "d".into(),
            input_schema: json!({"type":"object"}),
            annotations: Some(ToolAnnotations {
                read_only_hint: Some(true),
                ..ToolAnnotations::default()
            }),
        };
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            json!({"name":"search","description":"d","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}})
        );
        let r = CallToolResult::error("no");
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"content":[{"type":"text","text":"no"}],"isError":true})
        );
        assert_eq!(r.text(), "no");
        assert_eq!(
            CallToolResult {
                content: vec![],
                ..r
            }
            .text(),
            ""
        );
    }

    #[test]
    fn versions_are_negotiated_down_to_one_we_know() {
        assert_eq!(negotiate(Some("2024-11-05")), "2024-11-05");
        assert_eq!(negotiate(Some("2099-01-01")), "2025-06-18");
        assert_eq!(negotiate(None), "2025-06-18");
    }
}
