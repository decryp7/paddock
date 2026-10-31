//! The app's build script must never leave its empty sidecar placeholders (`binaries/<name>-<triple>`) in the build
//! folder: tauri-build copies every `bundle.externalBin` to `target/<profile>/<name>`, over the `dyno` and `dyno-agent`
//! that cargo links there. The dyno command's signing test then copied a 0-byte `dyno` and codesign called it
//! `dyno-da39a3ee…` (the SHA-1 of nothing), and `tauri dev` found an empty agent "next to itself".
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
fn the_build_leaves_no_empty_sidecar_in_the_build_folder() {
    // target/<profile>/deps/<this test> -> target/<profile>
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe.parent().and_then(std::path::Path::parent).unwrap();
    let ext = if cfg!(windows) { ".exe" } else { "" };
    for name in ["dyno", "dyno-agent"] {
        let path = profile_dir.join(format!("{name}{ext}"));
        // Absent is fine (this run did not build that binary); present must be a real program.
        if let Ok(meta) = std::fs::metadata(&path) {
            assert!(
                meta.len() > 0,
                "{} is empty: the app's build script let tauri-build copy the sidecar placeholder over the real binary",
                path.display()
            );
        }
    }
}
