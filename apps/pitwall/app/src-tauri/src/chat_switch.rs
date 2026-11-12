//! Whether this build has the Playground's chat (ADR 0023). Temporary: Rust decides from the Cargo feature `chat` (a default
//! feature, so on in the release workflow) until ADR 0049's switches
//! (`apps/pitwall/switches.toml`) replace it. The UI shows the Chat tab only when this says so.

use pitwall_core::mgmt::CommandError;

/// The chat is built in (the default `chat` feature).
pub const CHAT_ENABLED: bool = cfg!(feature = "chat");

/// Whether the window offers the chat.
#[tauri::command]
pub async fn chat_enabled() -> Result<bool, CommandError> {
    Ok(CHAT_ENABLED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_reports_the_build_feature() {
        let answer = tauri::async_runtime::block_on(chat_enabled()).unwrap();
        assert_eq!(answer, cfg!(feature = "chat"));
        #[cfg(not(feature = "chat"))]
        assert!(!answer, "a build without the feature has no chat");
        #[cfg(feature = "chat")]
        assert!(answer, "a build with the feature has the chat");
    }
}
