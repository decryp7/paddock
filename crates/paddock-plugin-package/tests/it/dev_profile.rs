//! Pins the cause of a slow-test flake: installing a plugin hashes, deflates and verifies its program (a 10 MB binary
//! in the host's tests) several times, which unoptimised took seconds per install and minutes under load, so a macOS CI
//! test hit nextest's 60 s limit. The workspace's dev profile therefore optimises these dependencies (and only these).

use std::path::PathBuf;

const HOT_DEPENDENCIES: [&str; 5] = ["sha2", "ed25519-compact", "zlib-rs", "flate2", "crc32fast"];

#[test]
fn the_dev_profile_optimises_the_hashing_signature_and_deflate_dependencies() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let doc: toml::Table = text.parse().unwrap();
    let packages = doc
        .get("profile")
        .and_then(|p| p.get("dev"))
        .and_then(|d| d.get("package"))
        .and_then(toml::Value::as_table)
        .expect("[profile.dev.package.*] in crates/Cargo.toml");
    for name in HOT_DEPENDENCIES {
        let level = packages
            .get(name)
            .and_then(|p| p.get("opt-level"))
            .and_then(toml::Value::as_integer);
        assert!(
            level.is_some_and(|l| l >= 2),
            "[profile.dev.package.{name}] opt-level must be 2 or 3 (found {level:?}): installs of plugin programs are too slow without it"
        );
    }
}
