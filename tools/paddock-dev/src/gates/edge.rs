//! The thin edge of C100: reads the checkout into the pure core's inputs.

use super::ci::Sources;
use std::fs;
use std::path::Path;

fn read(p: &Path) -> Result<String, String> {
    fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn entries(dir: &Path) -> Vec<std::fs::DirEntry> {
    let mut v: Vec<_> = fs::read_dir(dir).map(|d| d.flatten().collect()).unwrap_or_default();
    v.sort_by_key(|e| e.file_name());
    v
}

/// Reads `.gitea/workflows`, `.gitea/actions/*/action.yml` and `apps/*/paddock.ci.json` below `root`.
pub fn load_sources(root: &Path) -> Result<Sources, String> {
    let mut s = Sources::default();
    for e in entries(&root.join(".gitea/workflows")) {
        let name = e.file_name().to_string_lossy().to_string();
        if name.ends_with(".yml") || name.ends_with(".yaml") {
            s.workflows.insert(format!(".gitea/workflows/{name}"), read(&e.path())?);
        }
    }
    for e in entries(&root.join(".gitea/actions")) {
        let name = e.file_name().to_string_lossy().to_string();
        for f in ["action.yml", "action.yaml"] {
            let p = e.path().join(f);
            if p.is_file() {
                s.actions.insert(format!(".gitea/actions/{name}"), read(&p)?);
            }
        }
    }
    for e in entries(&root.join("apps")) {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path().join("paddock.ci.json");
        if p.is_file() {
            s.contracts.insert(name, read(&p)?);
        }
    }
    Ok(s)
}
