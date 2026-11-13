//! `inputs.txt` lists exactly what decides the binary: the tool's folder, the folders of its path dependencies (normal and
//! build dependencies only; dev-dependencies change the hash only through Cargo.lock) and the files that configure the build.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Files outside the dependency folders that cargo reads for this build.
const EXTRAS: [&str; 3] = ["tools/paddock-dev", "crates/Cargo.toml", ".cargo/config.toml"];
/// The same for `tools/heavy`, which `./dev check --pre-push` builds on demand under its own hash.
const HEAVY_EXTRAS: [&str; 3] = ["tools/heavy", "crates/Cargo.toml", ".cargo/config.toml"];

/// The triple this machine builds for. Metadata is read for it alone: the path dependencies are the same on every platform, and
/// resolving the others would need registry crates (Windows, Android) that an offline machine never downloaded.
fn host_triple() -> String {
    let out = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("-vV")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .expect("rustc -vV prints the host")
        .to_string()
}

fn metadata(tool: &str) -> Value {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(tool);
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--filter-platform",
            &host_triple(),
        ])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The folders (relative to the repository root, `/`-separated) of the path dependencies reachable through normal and build edges.
fn path_dependency_folders(meta: &Value) -> BTreeSet<String> {
    let nodes = meta["resolve"]["nodes"].as_array().unwrap();
    let node = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();
    let mut seen = BTreeSet::new();
    let mut todo = vec![meta["resolve"]["root"].as_str().unwrap().to_string()];
    while let Some(id) = todo.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for dep in node(&id)["deps"].as_array().unwrap() {
            let wanted = dep["dep_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k["kind"].is_null() || k["kind"] == "build");
            if wanted {
                todo.push(dep["pkg"].as_str().unwrap().to_string());
            }
        }
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["source"].is_null() && seen.contains(p["id"].as_str().unwrap()))
        .map(|p| {
            let manifest = p["manifest_path"].as_str().unwrap().replace('\\', "/");
            let dir = manifest.trim_end_matches("/Cargo.toml");
            dir.strip_prefix(&format!("{repo}/")).unwrap().to_string()
        })
        .collect()
}

#[test]
fn inputs_txt_matches_cargo_metadata() {
    let listed =
        paddock_dev::input_hash::parse_inputs(&std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("inputs.txt")).unwrap())
            .unwrap();
    let mut expected = path_dependency_folders(&metadata("paddock-dev"));
    expected.extend(EXTRAS.map(String::from));
    let listed_set: BTreeSet<String> = listed.iter().cloned().collect();
    assert_eq!(listed.len(), listed_set.len(), "inputs.txt lists a path twice");
    assert_eq!(
        listed_set, expected,
        "inputs.txt and cargo metadata disagree: update tools/paddock-dev/inputs.txt"
    );
}

#[test]
fn dev_dependencies_are_not_inputs() {
    let listed = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("inputs.txt")).unwrap();
    assert!(!listed.contains("paddock-testkit"));
}

#[test]
fn heavy_inputs_txt_matches_cargo_metadata() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../heavy/inputs.txt")).unwrap();
    let listed = paddock_dev::input_hash::parse_inputs(&text).unwrap();
    let mut expected = path_dependency_folders(&metadata("heavy"));
    expected.remove("tools/heavy");
    expected.extend(HEAVY_EXTRAS.map(String::from));
    let listed_set: BTreeSet<String> = listed.iter().cloned().collect();
    assert_eq!(listed.len(), listed_set.len(), "tools/heavy/inputs.txt lists a path twice");
    assert_eq!(
        listed_set, expected,
        "tools/heavy/inputs.txt and cargo metadata disagree: update tools/heavy/inputs.txt"
    );
}
