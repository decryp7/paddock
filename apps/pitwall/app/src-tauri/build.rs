//! Tauri's build step: the Windows application manifest in every binary, the `pitwall-server` sidecar placeholder, and no
//! `externalBin` copy over the real program while it is a placeholder (paddock-build).

fn main() {
    paddock_build::Build::new().sidecars(["pitwall-server"]).run();
}
