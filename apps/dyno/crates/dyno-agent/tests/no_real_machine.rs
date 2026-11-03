#![allow(clippy::unwrap_used, clippy::expect_used)]

//! ER-14 for the disk map: no test may build the real volume lister or the real access probe, because they read the machine's
//! mount table, run `diskutil` and probe shares. Only the real agent binaries (`Running::start_os`) do.
//!
//! Every Rust file of the agent and of the app's Tauri side is read. A file on the allow-list (exact path from the `dyno` folder)
//! may name them in its production code, but its `#[cfg(test)]` part may not; any other file may not name them at all.

use std::path::{Path, PathBuf};

const NEEDLES: [&str; 11] = [
    "OsVolumes",
    "DiskPorts::os",
    "start_os",
    "OsAccess",
    "OsSockets",
    "OsReverseDns",
    "OsTraffic",
    "OsCounters",
    "PowerPorts::os",
    "system_power",
    "system_events",
];

/// Where the real ones are defined or chosen: the production code of exactly these files.
const ALLOWED: [&str; 5] = [
    "crates/dyno-agent/src/disk/volumes.rs",
    "crates/dyno-agent/src/disk/mod.rs",
    "crates/dyno-agent/src/disk/centre.rs",
    "crates/dyno-agent/src/cli.rs",
    "crates/dyno-agent/src/lib.rs",
];

/// This file names the needles to look for them.
const SELF_PATH: &str = "crates/dyno-agent/tests/no_real_machine.rs";

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The part of a source file that is test code: from the `#[cfg(test)] mod tests` on (that module closes a file). A one-line
/// `#[cfg(test)] mod testing;` declaration earlier in a file does not start it.
fn test_part(text: &str) -> &str {
    text.find("#[cfg(test)]\nmod tests").map_or("", |at| &text[at..])
}

/// The files that break the rule, given each file's path (from the `dyno` folder) and text.
fn offenders(files: &[(String, String)]) -> Vec<String> {
    files
        .iter()
        .filter(|(path, _)| path != SELF_PATH)
        .filter(|(path, text)| {
            let checked = if ALLOWED.contains(&path.as_str()) {
                test_part(text)
            } else {
                text.as_str()
            };
            NEEDLES.iter().any(|n| checked.contains(n))
        })
        .map(|(path, _)| path.clone())
        .collect()
}

#[test]
fn only_the_real_agent_builds_the_real_volume_lister() {
    let dyno = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let mut paths = Vec::new();
    rust_files(&dyno.join("crates/dyno-agent"), &mut paths);
    rust_files(&dyno.join("app/src-tauri/src"), &mut paths);
    let files: Vec<(String, String)> = paths
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(&dyno).unwrap().to_string_lossy().replace('\\', "/");
            (rel, std::fs::read_to_string(p).unwrap())
        })
        .collect();
    assert!(
        files.iter().any(|(p, _)| ALLOWED.contains(&p.as_str())),
        "the allow-list names files that exist"
    );
    assert!(
        files.iter().any(|(p, _)| p.starts_with("app/src-tauri/src/")),
        "the Tauri side is read too"
    );
    let bad = offenders(&files);
    assert!(bad.is_empty(), "these build the real machine's volume list in test code: {bad:?}");
}

#[test]
fn the_check_catches_what_it_is_meant_to() {
    let f = |p: &str, t: &str| (p.to_string(), t.to_string());
    // A test file, or a file outside the allow-list, may not name them anywhere.
    assert_eq!(
        offenders(&[f("crates/dyno-agent/tests/x.rs", "let v = OsVolumes::default();")]).len(),
        1
    );
    assert_eq!(offenders(&[f("app/src-tauri/src/link.rs", "Running::start_os(a, b)")]).len(), 1);
    // An allowed file may use them in production code ...
    let prod = "fn real() { OsVolumes::default(); }\n";
    assert!(offenders(&[f(ALLOWED[0], prod)]).is_empty());
    // ... but not in its test module.
    let declared = "#[cfg(test)]\npub(crate) mod testing;\nfn real() { OsVolumes::default(); }\n";
    assert!(
        offenders(&[f(ALLOWED[1], declared)]).is_empty(),
        "a test module declared early does not hide the code after it"
    );
    let with_test = format!("{prod}#[cfg(test)]\nmod tests {{ fn t() {{ OsAccess; }} }}\n");
    assert_eq!(offenders(&[f(ALLOWED[0], &with_test)]).len(), 1);
    // A similarly named file elsewhere is not on the list.
    assert_eq!(offenders(&[f("crates/dyno-agent/src/other/volumes.rs", prod)]).len(), 1);
}
