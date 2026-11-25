//! The LLM port, its test fake and tool calling for models without native tools.

#[cfg(any(test, feature = "testing"))]
pub mod fake;
pub mod prompted;
pub mod provider;
pub mod tool_protocol;
pub mod types;
pub mod utf8;

#[cfg(any(test, feature = "testing"))]
pub use fake::{Reply, ScriptedProvider};
pub use prompted::{prepare, prompted_chat, Finish, GenParams, Generated, Prepared, PreparedTurn};
pub use provider::LlmProvider;
pub use types::*;
