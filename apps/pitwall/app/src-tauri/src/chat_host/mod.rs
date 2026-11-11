//! The Playground chat's host side (ADR 0023 WP-9a): connected apps' MCP tools offered to the assistant, the approval policy
//! of Decision 8 over `pitwall_core::approvals`, and one `pitwall_chat` log line per tool call. It lives in the app process,
//! never in the server (Decision 1): the service never talks to a user's apps, and approvals have a window to appear in.

pub mod apps;
pub mod attach;
pub mod chats;
pub mod commands;
pub mod log_line;
pub mod models;
pub mod pictures;
pub mod policy;
pub mod saved;
pub mod tool;

pub use log_line::ChatLog;
pub use policy::PitwallPolicy;
pub use tool::{tools_of, McpTool, ToolNames};

#[cfg(test)]
mod chats_tests;
#[cfg(test)]
mod tests;
