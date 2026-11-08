//! The LLM port, its test fake and tool calling for models without native tools.

pub mod fake;
pub mod prompt_format;
pub mod prompted;
pub mod provider;
pub mod tool_protocol;
pub mod types;
pub mod utf8;

pub use fake::{Reply, ScriptedProvider};
pub use prompt_format::PromptFormat;
pub use prompted::{prepare, prompted_chat, Finish, GenParams, Generated, Prepared, PreparedTurn, PromptedProvider, TextEngine};
pub use provider::LlmProvider;
pub use types::*;
