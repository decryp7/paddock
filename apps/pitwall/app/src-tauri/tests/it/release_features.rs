//! The release build ships the Playground's chat (user decision 2026-11-12, D-3). The release workflow builds with the
//! package's default features, so `chat` must be one of them; a build that drops it is a build without the chat.

#[test]
fn the_default_features_include_the_chat() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let default_line = manifest
        .lines()
        .find(|line| line.trim_start().starts_with("default"))
        .expect("the package declares its default features");
    assert!(
        default_line.contains("\"chat\""),
        "the release build uses the default features, which must include chat: {default_line}"
    );
}

#[test]
fn a_default_build_has_the_chat() {
    let answer = tauri::async_runtime::block_on(pitwall_app_lib::chat_switch::chat_enabled()).unwrap();
    assert!(answer, "the release build offers the chat");
}
