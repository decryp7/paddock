use paddock_build::sidecars::{
    any_placeholder, copy_paths, hide_external_bin, is_placeholder, profile_dir, remove_files,
    stale_copies,
};
use paddock_build::{Build, BuildError, Env, sidecar_file};
use std::path::{Path, PathBuf};

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn a_missing_or_empty_sidecar_is_a_placeholder_and_a_real_one_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty");
    let real = dir.path().join("real");
    std::fs::write(&empty, b"").unwrap();
    std::fs::write(&real, b"program").unwrap();
    assert!(is_placeholder(&empty));
    assert!(is_placeholder(&dir.path().join("missing")));
    assert!(!is_placeholder(&real));
    assert!(any_placeholder(&[real.clone(), empty]));
    assert!(!any_placeholder(&[real]));
    assert!(!any_placeholder(&[]));
}

#[test]
fn hiding_external_bin_nulls_the_key_and_keeps_the_rest_of_the_config() {
    assert_eq!(
        json(&hide_external_bin(None)),
        json(r#"{"bundle":{"externalBin":null}}"#)
    );
    assert_eq!(
        json(&hide_external_bin(Some(
            r#"{"build":{"devUrl":"x"},"bundle":{"active":true,"externalBin":["a"]}}"#
        ))),
        json(r#"{"build":{"devUrl":"x"},"bundle":{"active":true,"externalBin":null}}"#)
    );
}

#[test]
fn a_config_that_is_not_an_object_is_replaced() {
    for bad in ["", "not json", "[1]", "3", r#"{"bundle":7}"#] {
        assert_eq!(
            json(&hide_external_bin(Some(bad))),
            json(r#"{"bundle":{"externalBin":null}}"#),
            "{bad:?}"
        );
    }
}

#[test]
fn the_profile_folder_is_above_out_dir_in_both_of_cargo_s_build_folder_layouts() {
    // Cargo before 1.100: <profile>/build/<package>-<hash>/out.
    assert_eq!(
        profile_dir(Path::new("/t/debug/build/app-1a2b/out")),
        Some(PathBuf::from("/t/debug"))
    );
    // Cargo 1.100 (build.build-dir-new-layout, always on): <profile>/build/<package>/<hash>/out.
    assert_eq!(
        profile_dir(Path::new("/t/debug/build/app/1a2b3c4d/out")),
        Some(PathBuf::from("/t/debug"))
    );
    assert_eq!(profile_dir(Path::new("")), None);
    assert_eq!(profile_dir(Path::new("a")), None);
}

#[test]
fn copies_are_named_for_the_sidecar_with_the_platform_suffix() {
    let names = ["svc".to_string(), "helper".to_string()];
    assert_eq!(
        copy_paths(Path::new("/t/debug"), &names, ".exe"),
        [
            PathBuf::from("/t/debug/svc.exe"),
            PathBuf::from("/t/debug/helper.exe")
        ]
    );
}

#[test]
fn only_empty_files_are_stale_copies_and_removing_them_leaves_real_programs() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("a");
    let real = dir.path().join("b");
    let missing = dir.path().join("c");
    let folder = dir.path().join("d");
    std::fs::write(&empty, b"").unwrap();
    std::fs::write(&real, b"program").unwrap();
    std::fs::create_dir(&folder).unwrap();
    let all = [empty.clone(), real.clone(), missing.clone(), folder];
    assert_eq!(stale_copies(&all), std::slice::from_ref(&empty));
    assert_eq!(
        remove_files(&stale_copies(&all)).unwrap(),
        std::slice::from_ref(&empty)
    );
    assert!(!empty.exists());
    assert!(real.exists());
    assert!(
        remove_files(&[missing]).unwrap().is_empty(),
        "already gone is fine"
    );
}

#[test]
fn removing_a_file_that_cannot_be_removed_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let err = remove_files(&[folder]).unwrap_err();
    assert!(matches!(err, BuildError::Io { .. }));
    assert!(err.to_string().contains("folder"), "{err}");
}

fn env_in(dir: &Path, config: Option<&str>) -> Env {
    let out = dir.join("target/debug/build/app-1/out");
    let app = dir.join("app");
    let mut vars = vec![
        ("TARGET", "aarch64-apple-darwin".to_string()),
        ("OUT_DIR", out.display().to_string()),
        ("CARGO_MANIFEST_DIR", app.display().to_string()),
    ];
    if let Some(c) = config {
        vars.push(("TAURI_CONFIG", c.to_string()));
    }
    let owned = vars;
    Env::of(owned.iter().map(|(k, v)| (*k, v.as_str())))
}

#[test]
fn while_a_sidecar_is_a_placeholder_tauri_build_sees_no_external_bin_and_old_empty_copies_go() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("target/debug");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("svc"), b"").unwrap();
    std::fs::write(profile.join("other"), b"real program").unwrap();
    let applied = Build::new()
        .sidecars(["svc", "other"])
        .apply(
            &env_in(dir.path(), Some(r#"{"build":{"x":1}}"#)),
            &mut |_| (),
        )
        .unwrap();
    let config = json(applied.tauri_config.as_deref().unwrap());
    assert_eq!(config["bundle"]["externalBin"], serde_json::Value::Null);
    assert_eq!(config["build"]["x"], 1);
    assert_eq!(applied.removed, [profile.join("svc")]);
    assert!(!profile.join("svc").exists());
    assert_eq!(
        std::fs::read(profile.join("other")).unwrap(),
        b"real program"
    );
}

#[test]
fn with_real_sidecars_the_copy_is_left_to_tauri_build() {
    let dir = tempfile::tempdir().unwrap();
    let binaries = dir.path().join("app/binaries");
    std::fs::create_dir_all(&binaries).unwrap();
    for name in ["a", "b"] {
        std::fs::write(
            binaries.join(sidecar_file(name, "aarch64-apple-darwin")),
            b"program",
        )
        .unwrap();
    }
    let applied = Build::new()
        .sidecars(["a", "b"])
        .apply(&env_in(dir.path(), None), &mut |_| ())
        .unwrap();
    assert_eq!(applied.tauri_config, None);
    assert!(applied.removed.is_empty());
}

#[test]
fn the_plan_reruns_when_a_sidecar_changes_and_names_where_tauri_build_copies_it() {
    let dir = tempfile::tempdir().unwrap();
    let plan = Build::new()
        .sidecars(["svc"])
        .plan(&env_in(dir.path(), None))
        .unwrap();
    let file = &plan.placeholders[0];
    assert!(
        plan.directives
            .contains(&format!("cargo:rerun-if-changed={}", file.display()))
    );
    assert_eq!(plan.copies, [dir.path().join("target/debug/svc")]);
    let no_out = Build::new()
        .sidecars(["svc"])
        .plan(&Env::of([("TARGET", "aarch64-apple-darwin")]))
        .unwrap();
    assert!(no_out.copies.is_empty());
    let windows = Build::new()
        .sidecars(["svc"])
        .plan(&Env::of([
            ("TARGET", "x86_64-pc-windows-msvc"),
            ("OUT_DIR", "/t/debug/build/app-1/out"),
        ]))
        .unwrap();
    assert_eq!(windows.copies, [PathBuf::from("/t/debug/svc.exe")]);
}

#[test]
fn an_app_without_sidecars_gets_no_config_patch() {
    let dir = tempfile::tempdir().unwrap();
    let applied = Build::new()
        .apply(&env_in(dir.path(), None), &mut |_| ())
        .unwrap();
    assert_eq!(applied.tauri_config, None);
}
