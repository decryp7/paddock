//! A Pitwall plugin template: one tool, `hello`. Copy this folder, follow README.md, and replace the tool with
//! yours (or implement `Filter`, `Endpoints` or `Engine` instead and list the kind in `pitwall-plugin.toml`).

// Tests may unwrap: a failed unwrap is a failed test.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use pitwall_plugin_api::prelude::*;

/// The plugin's tools.
struct Hello;

impl Tools for Hello {
    fn tools(&self) -> Vec<ToolInfo> {
        vec![ToolInfo {
            name: "hello".into(),
            description: "Greets someone by name.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            })
            .into(),
        }]
    }

    fn invoke(&self, tool: &str, args: Value) -> Result<Value, PluginError> {
        match tool {
            "hello" => {
                let name = args["name"].as_str().ok_or_else(|| PluginError::invalid("\"name\" must be a string"))?;
                Ok(json!({"greeting": format!("Hello, {name}!")}))
            }
            other => Err(PluginError::not_found(format!("no tool \"{other}\""))),
        }
    }
}

/// The plugin: its manifest and what it implements.
fn plugin() -> Plugin {
    Plugin::new(include_str!("../pitwall-plugin.toml")).tools(Hello)
}

fn main() -> std::process::ExitCode {
    serve(plugin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_plugin_api::{Collect, Reply, Request};

    #[test]
    fn the_manifest_matches_and_the_tool_answers() {
        let p = plugin();
        assert!(p.check().is_ok(), "{:?}", p.check());
        let reply = p.answer(
            Request::InvokeTool {
                tool: "hello".into(),
                args: json!({"name": "Ada"}).into(),
            },
            &mut Collect::default(),
        );
        assert_eq!(reply.unwrap(), Reply::Tool(json!({"greeting": "Hello, Ada!"}).into()));
        let bad = p.answer(
            Request::InvokeTool {
                tool: "hello".into(),
                args: json!({}).into(),
            },
            &mut Collect::default(),
        );
        assert!(bad.is_err());
    }
}
