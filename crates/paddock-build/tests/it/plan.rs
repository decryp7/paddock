use paddock_build::{
    Build, BuildError, Env, MANIFEST, MANIFEST_FILE, create_placeholders, sidecar_file,
    valid_sidecar_name, write_manifest,
};

fn windows(out: &str, manifest_dir: &str) -> Env {
    Env::of([
        ("CARGO_CFG_TARGET_OS", "windows"),
        ("CARGO_CFG_TARGET_ENV", "msvc"),
        ("OUT_DIR", out),
        ("CARGO_MANIFEST_DIR", manifest_dir),
        ("TARGET", "x86_64-pc-windows-msvc"),
    ])
}

#[test]
fn the_manifest_is_common_controls_v6() {
    assert!(MANIFEST.contains("Microsoft.Windows.Common-Controls"));
    assert!(MANIFEST.contains("6.0.0.0"));
    assert!(MANIFEST.trim_start().starts_with("<assembly"));
}

#[test]
fn on_windows_msvc_the_manifest_is_linked_into_every_binary_and_tauri_adds_none() {
    let plan = Build::new().plan(&windows("/out", "/app")).unwrap();
    assert!(plan.own_manifest);
    assert_eq!(
        plan.directives,
        [
            "cargo:rustc-link-arg=/MANIFEST:EMBED".to_string(),
            format!(
                "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
                std::path::Path::new("/out").join(MANIFEST_FILE).display()
            ),
            "cargo:rerun-if-env-changed=TAURI_UPDATER_PUBKEY".to_string(),
        ]
    );
}

#[test]
fn elsewhere_only_the_rerun_triggers_remain() {
    for (os, env) in [("macos", ""), ("linux", "gnu"), ("windows", "gnu")] {
        let e = Env::of([("CARGO_CFG_TARGET_OS", os), ("CARGO_CFG_TARGET_ENV", env)]);
        let plan = Build::new()
            .rerun_if_env_changed("MY_KEY")
            .plan(&e)
            .unwrap();
        assert!(!plan.own_manifest, "{os}-{env}");
        assert_eq!(
            plan.directives,
            [
                "cargo:rerun-if-env-changed=TAURI_UPDATER_PUBKEY",
                "cargo:rerun-if-env-changed=MY_KEY"
            ]
        );
    }
}

#[test]
fn an_app_can_leave_the_manifest_to_tauri() {
    let plan = Build::new()
        .without_windows_manifest()
        .plan(&windows("/out", "/app"))
        .unwrap();
    assert!(!plan.own_manifest);
    assert!(!plan.directives.iter().any(|d| d.contains("MANIFEST")));
}

#[test]
fn placeholders_are_named_for_the_target_with_the_exe_suffix_on_windows() {
    assert_eq!(
        sidecar_file("x-service", "aarch64-apple-darwin"),
        "x-service-aarch64-apple-darwin"
    );
    assert_eq!(
        sidecar_file("x-service", "x86_64-pc-windows-msvc"),
        "x-service-x86_64-pc-windows-msvc.exe"
    );
    let plan = Build::new()
        .sidecars(["dyno-agent", "dyno"])
        .plan(&windows("/out", "/app"))
        .unwrap();
    let names: Vec<_> = plan
        .placeholders
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "dyno-agent-x86_64-pc-windows-msvc.exe",
            "dyno-x86_64-pc-windows-msvc.exe"
        ]
    );
    assert!(
        plan.placeholders[0].starts_with("/app/binaries")
            || plan.placeholders[0].starts_with("/app\\binaries")
    );
    assert!(
        Build::new()
            .sidecars(["a"])
            .plan(&Env::of([]))
            .unwrap()
            .placeholders
            .is_empty(),
        "no TARGET, no placeholders"
    );
}

#[test]
fn names_that_are_not_file_names_are_refused() {
    for bad in ["", "../x", "a/b", "a b", "a.exe", &"x".repeat(65)] {
        assert!(!valid_sidecar_name(bad), "{bad:?}");
        let err = Build::new().sidecars([bad]).plan(&Env::of([])).unwrap_err();
        assert!(matches!(&err, BuildError::BadSidecarName(n) if n == bad));
        assert!(err.to_string().contains("not a sidecar name"));
    }
    assert!(valid_sidecar_name("pitcrew-service_2"));
}

#[test]
fn apply_writes_the_manifest_and_placeholders_prints_the_directives_and_never_overwrites_a_real_sidecar()
 {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let app = dir.path().join("app");
    let env = windows(out.to_str().unwrap(), app.to_str().unwrap());
    let real = app
        .join("binaries")
        .join(sidecar_file("svc", "x86_64-pc-windows-msvc"));
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, b"real binary").unwrap();
    let build = Build::new().sidecars(["svc", "helper"]);
    let mut printed = Vec::new();
    let applied = build
        .apply(&env, &mut |l| printed.push(l.to_string()))
        .unwrap();
    assert_eq!(printed, applied.plan.directives);
    assert_eq!(
        std::fs::read_to_string(out.join(MANIFEST_FILE)).unwrap(),
        MANIFEST
    );
    assert_eq!(std::fs::read(&real).unwrap(), b"real binary");
    let helper = app
        .join("binaries")
        .join(sidecar_file("helper", "x86_64-pc-windows-msvc"));
    assert_eq!(std::fs::read(&helper).unwrap(), b"");
    assert!(
        create_placeholders(&[real, helper]).unwrap().is_empty(),
        "a second run creates nothing"
    );
}

#[test]
fn a_failing_apply_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, b"x").unwrap();
    let err = write_manifest(&blocker.join("sub")).unwrap_err();
    assert!(matches!(err, BuildError::Io { .. }));
    assert!(err.to_string().contains("file"), "{err}");
    let env = Env::of([
        ("TARGET", "aarch64-apple-darwin"),
        ("CARGO_MANIFEST_DIR", blocker.to_str().unwrap()),
    ]);
    assert!(matches!(
        Build::new()
            .sidecars(["a"])
            .apply(&env, &mut |_| ())
            .unwrap_err(),
        BuildError::Io { .. }
    ));
    let env_bad_out = windows(blocker.join("o").to_str().unwrap(), "/nowhere");
    assert!(Build::new().apply(&env_bad_out, &mut |_| ()).is_err());
}

#[test]
fn the_process_environment_is_readable() {
    let env = Env::process();
    let plan = Build::new().plan(&env).unwrap();
    assert!(
        plan.directives
            .iter()
            .any(|d| d.starts_with("cargo:rerun-if-env-changed="))
    );
    assert_eq!(
        Build::default()
            .plan(&Env::of([]))
            .unwrap()
            .directives
            .len(),
        1
    );
}
