//! The build script's staging of the runtime library (build/runtime_lib.rs).

#[path = "../../build/runtime_lib.rs"]
mod build_runtime_lib;

/// A fresh folder of its own per test, removed when the guard is dropped (also after a failed assert).
fn scratch(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("debrief-store-duck-runtime-lib-{name}-"))
        .tempdir()
        .unwrap()
}

#[test]
fn names_the_runtime_library_per_target() {
    assert_eq!(build_runtime_lib::runtime_library("windows"), "duckdb.dll");
    assert_eq!(build_runtime_lib::runtime_library("macos"), "libduckdb.dylib");
    assert_eq!(build_runtime_lib::runtime_library("linux"), "libduckdb.so");
}

#[test]
fn stages_the_library_into_the_build_folder_and_replaces_a_stale_one() {
    let tmp = scratch("stage");
    let lib = tmp.path().join("lib");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("duckdb.dll"), b"v1").unwrap();
    let dir = build_runtime_lib::stage(&lib, "duckdb.dll", &out).unwrap().unwrap();
    assert_eq!(dir, out.join("libduckdb"));
    assert_eq!(std::fs::read(dir.join("duckdb.dll")).unwrap(), b"v1");
    // A new library replaces the staged one (a hard link would otherwise keep the old file's contents).
    std::fs::remove_file(lib.join("duckdb.dll")).unwrap();
    std::fs::write(lib.join("duckdb.dll"), b"v2").unwrap();
    build_runtime_lib::stage(&lib, "duckdb.dll", &out).unwrap();
    assert_eq!(std::fs::read(dir.join("duckdb.dll")).unwrap(), b"v2");
}

#[test]
fn a_missing_library_stages_nothing() {
    let tmp = scratch("missing");
    assert_eq!(
        build_runtime_lib::stage(tmp.path(), "duckdb.dll", &tmp.path().join("out")).unwrap(),
        None
    );
    assert!(!tmp.path().join("out").exists());
}
