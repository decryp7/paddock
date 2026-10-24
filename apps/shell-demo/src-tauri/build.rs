//! Tauri's build step: the context (tauri.conf.json, the capabilities with the shell plugin's permission sets) and the
//! bundle's resources. It also hands the tests the shell plugin's permission files (the plugin's `links` metadata), so
//! they resolve the capabilities through Tauri's ACL wherever the plugin comes from (a path or the registry).
fn main() {
    if let Ok(list) = std::env::var("DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH") {
        println!("cargo:rustc-env=PADDOCK_SHELL_PERMISSION_FILES={list}");
    }
    tauri_build::build();
}
