//! A host that ends without running destructors leaves its endpoint folder behind (it cannot remove it). Tests that do
//! this on purpose confine the folder (`debrief_ipc::testing::confined_tmp`): the folder must land there and nowhere
//! else (not in the shared temp folder, not in `/tmp`).

#![cfg(unix)]

use debrief_ipc::endpoint::{fits, prefix};
use debrief_ipc::testing::confined_tmp;
use std::path::Path;
use std::process::Command;

/// The prefixes of the processes tests confine (the store worker's endpoint is made by the app process, not confined).
const CONFINED_PREFIXES: [&str; 4] = [prefix::DEFAULT, prefix::PLUGIN, prefix::PLUGIN_TEST, prefix::STORE];

fn names(dir: &Path, prefix: &str) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(prefix))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

#[test]
fn a_confined_folder_fits_every_endpoint_prefix() {
    let dir = confined_tmp();
    for p in CONFINED_PREFIXES {
        assert!(fits(dir.path(), p), "{p} does not fit in {:?}", dir.path());
    }
}

#[test]
fn an_endpoint_a_host_leaves_on_exit_lands_only_in_the_confined_folder_and_goes_with_it() {
    // Letters only, so the leak check's endpoint pattern (`debrief-<letters>-<12 hex>`) would catch a leak of it.
    let name = "debrief-exit-test";
    let dir = confined_tmp();
    #[expect(
        clippy::disallowed_methods,
        reason = "this test checks that nothing is left in the shared temp folder"
    )]
    let shared = std::env::temp_dir().canonicalize().unwrap();
    #[allow(clippy::disallowed_methods)] // a test-only child that ends at once
    let out = Command::new(env!("CARGO_BIN_EXE_debrief-ipc-fake-child"))
        .arg(format!("--leak-endpoint={name}"))
        .env_clear()
        .envs(dir.env())
        .output()
        .unwrap();
    assert!(out.status.success());
    let address = String::from_utf8(out.stdout).unwrap();
    assert!(
        Path::new(address.trim()).starts_with(dir.path()),
        "the endpoint is outside the confined folder: {address}"
    );
    assert_eq!(
        names(dir.path(), name).len(),
        1,
        "the confined folder holds the endpoint the host left"
    );
    assert!(names(&shared, name).is_empty(), "nothing in the shared temp folder");
    assert!(names(Path::new("/tmp"), name).is_empty(), "nothing directly in /tmp");
    let path = dir.remove();
    assert!(!path.exists(), "the endpoint goes with the confined folder");
}
