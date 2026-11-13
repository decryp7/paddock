//! The app's build script must never leave its empty sidecar placeholder (`binaries/<name>-<triple>`) in the build folder:
//! tauri-build copies every `bundle.externalBin` to `target/<profile>/<name>`, over the `pitwall-server` that cargo links
//! there (Dyno's signing test then copied a 0-byte program). See docs/lessons.md, "tauri-build copies every sidecar".

#[test]
fn the_build_leaves_no_empty_sidecar_in_the_build_folder() {
    // target/<profile>, whichever build-folder layout this cargo uses (before 1.100: deps/<test>; after: build/<package>/<hash>/out/<test>)
    let profile_dir = paddock_testkit::cargo_layout::profile_dir();
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let path = profile_dir.join(format!("pitwall-server{ext}"));
    // Absent is fine (this run did not build that binary); present must be a real program.
    if let Ok(meta) = std::fs::metadata(&path) {
        assert!(
            meta.len() > 0,
            "{} is empty: the app's build script let tauri-build copy the sidecar placeholder over the real binary",
            path.display()
        );
    }
}
