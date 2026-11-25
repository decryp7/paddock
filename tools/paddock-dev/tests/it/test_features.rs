//! C170 (rule 97): the positive control of `./dev check test-features` on a real Cargo workspace (ER-33). A fixture app ships a
//! Tauri package and a sidecar; the hooked library has a `testing` feature. When the Tauri package enables it on a normal edge,
//! the check must report it; when only the sidecar's `[dev-dependencies]` enable it, a release build has no test feature and the
//! check must be clean (cargo's resolver 2 keeps dev-dependency features out of `cargo tree -e normal`).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::test_features::{RealCargo, check};
use paddock_testkit::TestCommand;
use std::path::Path;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// The fixture app; `app_hooked` is the Tauri package's dependency line on the hooked library.
fn fixture(root: &Path, app_hooked: &str) {
    write(root, "apps/demo/paddock.ci.json", r#"{"cargo": "."}"#);
    write(
        root,
        "apps/demo/Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"app/src-tauri\", \"crates/hooked\", \"crates/side\"]\n",
    );
    write(
        root,
        "apps/demo/app/src-tauri/tauri.conf.json",
        r#"{"bundle": {"externalBin": ["binaries/demo-side"]}}"#,
    );
    write(
        root,
        "apps/demo/app/src-tauri/Cargo.toml",
        &format!("[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{app_hooked}\n"),
    );
    write(root, "apps/demo/app/src-tauri/src/main.rs", "fn main() {}\n");
    write(
        root,
        "apps/demo/crates/hooked/Cargo.toml",
        "[package]\nname = \"hooked\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\ntesting = []\n",
    );
    write(
        root,
        "apps/demo/crates/hooked/src/lib.rs",
        "#[cfg(any(test, feature = \"testing\"))]\npub fn with_clock() {}\n",
    );
    write(
        root,
        "apps/demo/crates/side/Cargo.toml",
        "[package]\nname = \"demo-side\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nhooked = { path = \"../hooked\" }\n\n\
         [dev-dependencies]\nhooked = { path = \"../hooked\", features = [\"testing\"] }\n",
    );
    write(root, "apps/demo/crates/side/src/main.rs", "fn main() {}\n");
    let out = TestCommand::new(env!("CARGO"))
        .args(["generate-lockfile", "--offline"])
        .current_dir(root.join("apps/demo"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn a_release_crate_that_enables_testing_is_reported() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path(), "hooked = { path = \"../../crates/hooked\", features = [\"testing\"] }");
    let o = check(d.path(), Some("demo"), &RealCargo).unwrap();
    let shipped: Vec<&str> = o.shipped.iter().map(|s| s.package.as_str()).collect();
    assert_eq!(shipped, ["demo-app", "demo-side"]);
    let got: Vec<(&str, &str, &str)> = o
        .findings
        .iter()
        .map(|f| (f.shipped.package.as_str(), f.krate.as_str(), f.feature.as_str()))
        .collect();
    assert_eq!(got, [("demo-app", "hooked", "testing")]);
}

#[test]
fn testing_enabled_only_from_dev_dependencies_is_not_a_release_build() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path(), "hooked = { path = \"../../crates/hooked\" }");
    let o = check(d.path(), Some("demo"), &RealCargo).unwrap();
    assert_eq!(o.shipped.len(), 2);
    assert!(o.findings.is_empty(), "{:?}", o.findings);
}
