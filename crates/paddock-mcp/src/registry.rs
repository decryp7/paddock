//! What an app registers: tools and resources, and the policy that decides what is shown.

use crate::schema;
use paddock_jobs::CancelToken;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

/// What a tool may do. Everything is [`Access::ReadOnly`] unless the app registers an action and the user opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Looks, never changes anything. Shown when MCP is on.
    ReadOnly,
    /// Changes something. Hidden and refused until the policy names it in `actions` (ADR 0007: changes happen through the app's
    /// own UI; Sighter is the one acting server and keeps its own safety rules on top).
    Action,
}

/// What a running tool call gets besides its arguments.
#[derive(Clone)]
pub struct CallCtx {
    /// Cancelled when the time limit passes; long loops check it.
    pub cancel: CancelToken,
    /// The client that asked.
    pub client: String,
    /// The cursor of the page wanted (only for tools registered with [`ToolDef::paged`]).
    pub cursor: Option<String>,
}

/// A successful result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOutput {
    /// What the model reads. Capped at the tool's limit.
    pub text: String,
    /// The same data as JSON, when the tool has it.
    pub structured: Option<Value>,
    /// The cursor of the next page ([`crate::page`]).
    pub next_cursor: Option<String>,
}

impl ToolOutput {
    /// A text result.
    pub fn text(text: impl Into<String>) -> ToolOutput {
        ToolOutput {
            text: text.into(),
            ..ToolOutput::default()
        }
    }
}

/// A failed call: the message goes to the model, so it says what to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

impl From<&str> for ToolError {
    fn from(s: &str) -> Self {
        ToolError(s.into())
    }
}

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        ToolError(s)
    }
}

/// The function behind a tool.
pub type Handler = Arc<dyn Fn(&CallCtx, &Value) -> Result<ToolOutput, ToolError> + Send + Sync>;

/// A tool to register.
#[derive(Clone)]
pub struct ToolDef {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) description: String,
    pub(crate) schema: Value,
    pub(crate) access: Access,
    pub(crate) paged: bool,
    pub(crate) max_result_bytes: Option<usize>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) handler: Handler,
}

/// Why a registration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterError(pub String);

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RegisterError {}

fn name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl ToolDef {
    fn new(
        access: Access,
        name: &str,
        description: &str,
        schema: Value,
        handler: Handler,
    ) -> ToolDef {
        ToolDef {
            name: name.into(),
            title: None,
            description: description.into(),
            schema,
            access,
            paged: false,
            max_result_bytes: None,
            timeout: None,
            handler,
        }
    }

    /// A tool that only looks. `schema` is the JSON Schema of its arguments (`{"type":"object", ...}`).
    pub fn read_only(
        name: &str,
        description: &str,
        schema: Value,
        handler: impl Fn(&CallCtx, &Value) -> Result<ToolOutput, ToolError> + Send + Sync + 'static,
    ) -> ToolDef {
        ToolDef::new(
            Access::ReadOnly,
            name,
            description,
            schema,
            Arc::new(handler),
        )
    }

    /// A tool that changes something; it is hidden until the user opts in to it by name ([`crate::Policy::allow_action`]).
    pub fn action(
        name: &str,
        description: &str,
        schema: Value,
        handler: impl Fn(&CallCtx, &Value) -> Result<ToolOutput, ToolError> + Send + Sync + 'static,
    ) -> ToolDef {
        ToolDef::new(Access::Action, name, description, schema, Arc::new(handler))
    }

    /// A name for people.
    pub fn title(mut self, title: &str) -> ToolDef {
        self.title = Some(title.into());
        self
    }

    /// The tool returns pages: a `cursor` argument is added to its schema and handed over in [`CallCtx::cursor`].
    pub fn paged(mut self) -> ToolDef {
        self.paged = true;
        self
    }

    /// The most bytes of text this tool returns (default 64 KiB, never above the server's hard cap).
    pub fn max_result_bytes(mut self, n: usize) -> ToolDef {
        self.max_result_bytes = Some(n);
        self
    }

    /// The time limit of one call (default 30 s).
    pub fn timeout(mut self, d: Duration) -> ToolDef {
        self.timeout = Some(d);
        self
    }

    /// Checks the definition: a usable name, an object schema.
    pub(crate) fn validate(&self) -> Result<(), RegisterError> {
        if !name_ok(&self.name) {
            return Err(RegisterError(format!(
                "tool name {:?} must be 1-64 letters, digits, '_', '-' or '.'",
                self.name
            )));
        }
        if self.schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(RegisterError(format!(
                "tool {}: the input schema must have \"type\": \"object\"",
                self.name
            )));
        }
        if self.description.trim().is_empty() {
            return Err(RegisterError(format!(
                "tool {}: say what it does; the model reads the description",
                self.name
            )));
        }
        Ok(())
    }

    /// The schema as shown and validated against (a paged tool gets its `cursor`).
    pub(crate) fn effective_schema(&self) -> Value {
        let mut s = self.schema.clone();
        if self.paged
            && let Some(props) = s
                .as_object_mut()
                .map(|o| o.entry("properties").or_insert_with(|| json!({})))
            && let Some(p) = props.as_object_mut()
        {
            p.insert("cursor".into(), json!({"type": "string", "description": "Cursor from the previous result; omit for the first page."}));
        }
        s
    }

    pub(crate) fn check_args(&self, args: &Value) -> Result<(), String> {
        schema::validate(&self.effective_schema(), args)
    }
}

/// A resource to register: text the model can read by address.
#[derive(Clone)]
pub struct ResourceDef {
    pub(crate) uri: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) mime_type: Option<String>,
    pub(crate) read: Arc<dyn Fn() -> Result<String, ToolError> + Send + Sync>,
}

impl ResourceDef {
    /// A resource at `uri` (`debrief://summary`) read by `read`.
    pub fn new(
        uri: &str,
        name: &str,
        read: impl Fn() -> Result<String, ToolError> + Send + Sync + 'static,
    ) -> ResourceDef {
        ResourceDef {
            uri: uri.into(),
            name: name.into(),
            description: None,
            mime_type: None,
            read: Arc::new(read),
        }
    }

    /// What it is.
    pub fn description(mut self, d: &str) -> ResourceDef {
        self.description = Some(d.into());
        self
    }

    /// Its content type.
    pub fn mime_type(mut self, m: &str) -> ResourceDef {
        self.mime_type = Some(m.into());
        self
    }

    pub(crate) fn validate(&self) -> Result<(), RegisterError> {
        let ok = self.uri.len() <= 512
            && self
                .uri
                .split_once("://")
                .is_some_and(|(scheme, rest)| !scheme.is_empty() && !rest.is_empty());
        if !ok || self.name.trim().is_empty() {
            return Err(RegisterError(format!(
                "resource {:?} needs a name and an address like app://thing",
                self.uri
            )));
        }
        Ok(())
    }
}

/// What the user allowed (the app's MCP settings page): on or off, which tools, which actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// The master switch; off by default (ADR 0007 rule 2).
    pub enabled: bool,
    /// Which tools are exposed; `None` means every read-only tool.
    pub tools: Option<BTreeSet<String>>,
    /// Action tools the user allowed, by name. An action not named here does not exist for the client.
    pub actions: BTreeSet<String>,
    /// Whether resources are exposed.
    pub resources: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            enabled: false,
            tools: None,
            actions: BTreeSet::new(),
            resources: true,
        }
    }
}

impl Policy {
    /// Switched on, every read-only tool, no actions.
    pub fn read_only() -> Policy {
        Policy {
            enabled: true,
            ..Policy::default()
        }
    }

    /// Also allows the action tool `name`.
    pub fn allow_action(mut self, name: &str) -> Policy {
        self.actions.insert(name.into());
        self
    }

    /// Whether the client may see and call `tool`.
    pub fn exposes(&self, tool: &ToolDef) -> bool {
        match tool.access {
            Access::ReadOnly => self.tools.as_ref().is_none_or(|t| t.contains(&tool.name)),
            Access::Action => {
                self.actions.contains(&tool.name)
                    && self.tools.as_ref().is_none_or(|t| t.contains(&tool.name))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(access: Access, name: &str) -> ToolDef {
        let h = |_: &CallCtx, _: &Value| Ok(ToolOutput::text("ok"));
        match access {
            Access::ReadOnly => {
                ToolDef::read_only(name, "does a thing", json!({"type": "object"}), h)
            }
            Access::Action => ToolDef::action(name, "does a thing", json!({"type": "object"}), h),
        }
    }

    #[test]
    fn the_default_policy_is_off_and_actions_need_their_name() {
        assert!(!Policy::default().enabled);
        let read = tool(Access::ReadOnly, "look");
        let act = tool(Access::Action, "change");
        let p = Policy::read_only();
        assert!(p.exposes(&read) && !p.exposes(&act));
        let p = p.allow_action("change");
        assert!(p.exposes(&act));
        let only: BTreeSet<String> = ["look".to_string()].into();
        let narrowed = Policy {
            tools: Some(only),
            ..p.clone()
        };
        assert!(
            narrowed.exposes(&read) && !narrowed.exposes(&act),
            "the tool list narrows actions too"
        );
        let none = Policy {
            tools: Some(BTreeSet::new()),
            ..Policy::read_only()
        };
        assert!(!none.exposes(&read));
    }

    #[test]
    fn definitions_are_checked_when_registered() {
        let ok = tool(Access::ReadOnly, "search.v2-x_y");
        assert!(ok.validate().is_ok());
        for bad in ["", "has space", "ü", &"a".repeat(65)] {
            assert!(
                tool(Access::ReadOnly, bad)
                    .validate()
                    .unwrap_err()
                    .0
                    .contains("1-64"),
                "{bad:?}"
            );
        }
        let no_obj = ToolDef::read_only("t", "d", json!({"type": "string"}), |_, _| {
            Ok(ToolOutput::default())
        });
        assert!(
            no_obj
                .validate()
                .unwrap_err()
                .to_string()
                .contains("object")
        );
        let no_desc = ToolDef::read_only("t", " ", json!({"type": "object"}), |_, _| {
            Ok(ToolOutput::default())
        });
        assert!(no_desc.validate().unwrap_err().0.contains("description"));
    }

    #[test]
    fn a_paged_tool_gets_a_cursor_property() {
        let t = tool(Access::ReadOnly, "list")
            .paged()
            .title("List")
            .max_result_bytes(10)
            .timeout(Duration::from_secs(1));
        assert!(t.effective_schema()["properties"]["cursor"]["type"] == "string");
        assert!(t.check_args(&json!({"cursor": "p1"})).is_ok());
        assert!(
            tool(Access::ReadOnly, "plain")
                .effective_schema()
                .get("properties")
                .is_none()
        );
        assert_eq!(
            (t.title.as_deref(), t.max_result_bytes, t.timeout),
            (Some("List"), Some(10), Some(Duration::from_secs(1)))
        );
    }

    #[test]
    fn resources_need_an_address() {
        assert!(
            ResourceDef::new("app://thing", "Thing", || Ok(String::new()))
                .description("d")
                .mime_type("text/plain")
                .validate()
                .is_ok()
        );
        for bad in ["thing", "://x", "app://", ""] {
            assert!(
                ResourceDef::new(bad, "n", || Ok(String::new()))
                    .validate()
                    .is_err(),
                "{bad:?}"
            );
        }
        assert!(
            ResourceDef::new("app://x", " ", || Ok(String::new()))
                .validate()
                .is_err()
        );
        assert_eq!(ToolError::from("a"), ToolError::from("a".to_string()));
    }
}
