//! The PreToolUse input of Claude Code (<https://code.claude.com/docs/en/hooks>), parsed once at the edge.

use serde_json::Value;

/// A tool call as the hook sees it. Only the fields the rules use are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// `Bash`, `Edit`, `Write`, `mcp__serena__...`.
    pub tool_name: String,
    /// `tool_input.command` of a Bash call.
    pub command: Option<String>,
    /// `tool_input.file_path` (or `notebook_path`) of an edit call.
    pub file_path: Option<String>,
    /// The caller's working directory.
    pub cwd: Option<String>,
    /// Present inside a subagent.
    pub agent_id: Option<String>,
    /// The role of that subagent.
    pub agent_type: Option<String>,
}

impl ToolCall {
    /// Parses the input text. The error names the cause and never contains the input.
    pub fn parse(text: &str) -> Result<ToolCall, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {:?} at line {}", e.classify(), e.line()))?;
        let obj = v.as_object().ok_or("not a JSON object")?;
        let tool_name = obj.get("tool_name").and_then(Value::as_str).ok_or("no tool_name")?.to_string();
        let input = obj.get("tool_input").and_then(Value::as_object).ok_or("no tool_input")?;
        let s = |m: &serde_json::Map<String, Value>, k: &str| m.get(k).and_then(Value::as_str).map(str::to_string);
        Ok(ToolCall {
            tool_name,
            command: s(input, "command"),
            file_path: s(input, "file_path").or_else(|| s(input, "notebook_path")),
            cwd: s(obj, "cwd"),
            agent_id: s(obj, "agent_id"),
            agent_type: s(obj, "agent_type"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bash_call_keeps_its_fields() {
        let c = ToolCall::parse(
            r#"{"session_id":"s","cwd":"/w","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"},"agent_id":"a1","agent_type":"scout"}"#,
        )
        .unwrap();
        assert_eq!(c.tool_name, "Bash");
        assert_eq!(c.command.as_deref(), Some("ls"));
        assert_eq!(
            (c.cwd.as_deref(), c.agent_id.as_deref(), c.agent_type.as_deref()),
            (Some("/w"), Some("a1"), Some("scout"))
        );
    }

    #[test]
    fn an_edit_call_gives_its_path_and_a_notebook_its_own() {
        let c = ToolCall::parse(r#"{"tool_name":"Edit","tool_input":{"file_path":"/a/b"}}"#).unwrap();
        assert_eq!(c.file_path.as_deref(), Some("/a/b"));
        let n = ToolCall::parse(r#"{"tool_name":"NotebookEdit","tool_input":{"notebook_path":"/n.ipynb"}}"#).unwrap();
        assert_eq!(n.file_path.as_deref(), Some("/n.ipynb"));
    }

    #[test]
    fn malformed_input_is_an_error_that_does_not_echo_it() {
        for bad in [
            "",
            "not json",
            "[]",
            "{}",
            r#"{"tool_name":"Bash"}"#,
            r#"{"tool_name":1,"tool_input":{}}"#,
            r#"{"tool_input":{}}"#,
        ] {
            let e = ToolCall::parse(bad).unwrap_err();
            assert!(!e.is_empty());
            if !bad.is_empty() {
                assert!(!e.contains(bad), "{e}");
            }
        }
    }
}
