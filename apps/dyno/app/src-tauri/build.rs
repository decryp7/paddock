//! Tauri's build step: the Windows application manifest in every binary, the sidecar placeholders, and no `externalBin` copy
//! over the real program while one is a placeholder (paddock-build).

fn main() {
    // The shell plugin's permission files (its `links` metadata), so the tests resolve the capabilities through Tauri's ACL.
    if let Ok(list) = std::env::var("DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH") {
        println!("cargo:rustc-env=PADDOCK_SHELL_PERMISSION_FILES={list}");
    }
    paddock_build::Build::new()
        .sidecars(["dyno-agent", "dyno", "dyno-bench", "dyno-bench-gpu"])
        .run();
}
