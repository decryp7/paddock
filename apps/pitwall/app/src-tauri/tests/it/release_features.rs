//! The release build ships the Playground's chat (user decision, 2026-11-12: Pitwall 1.1 ships the chat on). The release
//! workflow builds with the package's default features, so `chat` must be one of them. This test runs only in a build that
//! has the feature: `cargo test --no-default-features` really builds without the chat, and the other tests cover that path.
//! If `chat` ever leaves `default`, a default build fails this test because the answer below becomes `false`.

#[cfg(feature = "chat")]
#[test]
fn a_default_build_has_the_chat() {
    let answer = tauri::async_runtime::block_on(pitwall_app_lib::chat_switch::chat_enabled()).unwrap();
    assert!(answer, "the release build offers the chat");
}
