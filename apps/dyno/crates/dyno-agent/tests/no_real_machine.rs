#![allow(clippy::unwrap_used, clippy::expect_used)]

//! ER-14 for the disk map: no test may build the real volume lister or the real access probe, because they read the machine's
//! mount table, run `diskutil` and probe shares. Only the real agent binaries (`Running::start_os`) do.

use std::path::{Path, PathBuf};

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

#[test]
fn only_the_real_agent_builds_the_real_volume_lister() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("tests"), &mut files);
    // The unit tests inside the sources: none of them may name the real ones except the file that defines them.
    rust_files(&root.join("src"), &mut files);
    let needles = ["OsVolumes", "DiskPorts::os", "start_os", "OsAccess"];
    let allowed = ["volumes.rs", "mod.rs", "centre.rs", "cli.rs", "lib.rs", "no_real_machine.rs"];
    let mut offenders = Vec::new();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        if allowed.contains(&name.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        if needles.iter().any(|n| text.contains(n)) {
            offenders.push(f.display().to_string());
        }
    }
    assert!(offenders.is_empty(), "these build the real machine's volume list: {offenders:?}");
}
