//! The build step (`paddock-build`, plan X34): the Windows manifest in every binary (test binaries included), an empty
//! placeholder for the agent (`bundle.externalBin`) wherever the real one has not been built, and tauri-build. It also hands the
//! tests the shell plugin's permission files (the plugin's `links` metadata), so they resolve the capabilities through Tauri's
//! ACL wherever the plugin comes from (a path or the registry). Bundles build the real agent first (`agent/sidecar.mjs`, in
//! `beforeBuildCommand`); the app reports the placeholder as a missing program if it is ever asked to register it.
fn main() {
    if let Ok(list) = std::env::var("DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH") {
        println!("cargo:rustc-env=PADDOCK_SHELL_PERMISSION_FILES={list}");
    }
    paddock_build::Build::new()
        .sidecars(["shell-demo-agent"])
        .run();
}
