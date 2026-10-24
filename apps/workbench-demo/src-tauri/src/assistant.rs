//! The demo's assistant: Pitwall as the provider (`PitwallProvider` over the app's `paddock_pitwall::Link`) and one
//! read-only sample tool. Without Pitwall the pane shows its calm "needs AI" state; nothing here needs a model to start.

use paddock_assistant::{
    Assistant, AssistantConfig, AssistantTool, Cancel, ConversationStore, PitwallProvider,
    ToolBudget, ToolOutput, ToolSpec, Tools,
};
use paddock_health::{Clock, SystemClock};
use paddock_pitwall::Link;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

/// The sample repository the demo's views show (names only): what the tool lists.
pub const SAMPLE_FILES: [&str; 5] = [
    "README.md",
    "src/main.rs",
    "src/lib.rs",
    "docs/design.md",
    "Cargo.toml",
];

/// A read-only tool: lists the files of the sample repository, optionally only those containing a word.
pub struct ListFiles;

impl AssistantTool for ListFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_files".into(),
            description: "Lists the files of the sample repository, optionally only those whose path contains `contains`.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "contains": { "type": "string", "description": "Part of a path to look for." } }
            }),
        }
    }

    fn budget(&self) -> ToolBudget {
        ToolBudget {
            max_calls: 4,
            max_output_chars: 2_000,
        }
    }

    fn call(&self, input: &Value, _cancel: &Cancel) -> ToolOutput {
        let needle = input
            .get("contains")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        let found: Vec<&str> = SAMPLE_FILES
            .iter()
            .copied()
            .filter(|f| f.to_lowercase().contains(&needle))
            .collect();
        ToolOutput::ok(found.join("\n"))
            .with_summary(format!("{} files", found.len()))
            .with_data(json!({ "files": found }))
    }
}

/// The assistant over `link`, keeping its conversations in `<dir>/assistant.json`.
pub fn assistant(
    link: Arc<Link>,
    dir: &Path,
    app: &str,
) -> Result<Arc<Assistant>, paddock_assistant::ToolsError> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let store = Arc::new(ConversationStore::open(
        dir.join("assistant.json"),
        clock.clone(),
    ));
    Ok(Assistant::new(
        Arc::new(PitwallProvider::new(link)),
        Tools::new(vec![Arc::new(ListFiles)])?,
        store,
        clock,
        AssistantConfig::new(
            app,
            "You help the user find their way around a small sample repository. Use the tools to look; never guess file names.",
        ),
    ))
}
