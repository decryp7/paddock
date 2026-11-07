//! The demo's AI access (plan X22, ADR 0007): an MCP server with two read-only tools and one action, the settings in the config
//! folder, the client keys (hashes only) in an owner-only file beside them and the audit log the Settings page shows.

use paddock_env::AppDirs;
use paddock_jobs::time::SystemClock;
use paddock_mcp::{
    AuditLog, Emit, FileKeys, FileSettings, KeyStore, McpControl, Server, ToolDef, ToolOutput,
};
use serde_json::json;
use std::sync::Arc;

/// Builds the control around the event sender the shell hands over.
pub fn control(dirs: &AppDirs, app_name: &str, version: &str, emit: Emit) -> Arc<McpControl> {
    let log = Arc::new(AuditLog::new(500));
    let sink = McpControl::announcing_sink(log.clone(), emit.clone());
    let server = Arc::new(
        Server::new(app_name, version, Arc::new(SystemClock), sink, Arc::new(paddock_log::Redactor::new()))
            .with_instructions("Ask about the demo's runners. Everything here is read-only unless the user allowed an action."),
    );
    let tools = [
        ToolDef::read_only("runner_status", "The state of the demo runners.", json!({"type": "object"}), |_, _| {
            Ok(ToolOutput::text("runner-1: idle\nrunner-2: running a job\nrunner-3: offline"))
        })
        .title("Runner status"),
        ToolDef::read_only(
            "job_log",
            "The last lines of a demo job's log.",
            json!({"type": "object", "required": ["job"], "properties": {"job": {"type": "string", "maxLength": 64}}, "additionalProperties": false}),
            |_, args| Ok(ToolOutput::text(format!("{}: started\n{0}: step 1 done", args["job"].as_str().unwrap_or("job")))),
        )
        .title("Job log"),
        ToolDef::action("export_report", "Writes the demo report to the Documents folder (changes files).", json!({"type": "object"}), |_, _| {
            Ok(ToolOutput::text("The demo does not write a report; this tool exists to show the permission."))
        })
        .title("Export a report"),
    ];
    for t in tools {
        // The definitions are fixed above; a refusal would be a programming error, shown in the log rather than panicking.
        if let Err(e) = server.add_tool(t) {
            log::error!("the demo's MCP tool was refused: {e}");
        }
    }
    Arc::new(McpControl::new(
        server,
        log,
        Arc::new(KeyStore::new()),
        Arc::new(FileSettings::new(dirs.config().join("mcp.json"))),
        Arc::new(FileKeys::new(dirs.config().join("mcp-keys.json"))),
        emit,
        Arc::new(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        }),
    ))
}
