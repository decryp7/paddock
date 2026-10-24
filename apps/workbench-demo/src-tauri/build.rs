//! The build step (`paddock-build`, plan X34): the Windows manifest in every binary (test binaries included) and
//! tauri-build. It also hands the tests the two plugins' permission files (their `links` metadata), so they resolve the
//! capabilities through Tauri's ACL wherever the plugins come from (a path or the registry).
fn main() {
    for (var, name) in [
        (
            "DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH",
            "PADDOCK_SHELL_PERMISSION_FILES",
        ),
        (
            "DEP_PADDOCK_WORKBENCH_PERMISSION_FILES_PATH",
            "PADDOCK_WORKBENCH_PERMISSION_FILES",
        ),
    ] {
        if let Ok(list) = std::env::var(var) {
            println!("cargo:rustc-env={name}={list}");
        }
    }
    paddock_build::Build::new().run();
}
