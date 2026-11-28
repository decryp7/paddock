//! What `build.rs` writes into `OUT_DIR` for the Rust checks (user decision, 2026-11-28, ER-40): the registry source from the file names
//! `src/check/c<n>.rs`, and the test module lines from `tests/it/check_c<n>.rs`. `build.rs` includes this file (`#[path]`) and the
//! library compiles it too, so the functions are tested on temporary folders (`tests/it/check_lists.rs`) and the build script only calls
//! them. File names are read, never file text (ER-42). No dependency: it must build in `build.rs`.

use std::path::Path;

/// The numbers of the files `<prefix><n>.rs` in `dir`, ascending; an unreadable folder is an error that names it.
pub fn numbered(dir: &Path, prefix: &str) -> Result<Vec<u32>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut out: Vec<u32> = rd
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|f| {
            let n = f.strip_prefix(prefix)?.strip_suffix(".rs")?;
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            n.parse().ok()
        })
        .collect();
    out.sort_unstable();
    Ok(out)
}

/// A Rust string literal for a path (escaped, so a Windows path is one too).
fn lit(p: &Path) -> String {
    format!("{:?}", p.to_string_lossy())
}

/// The source of the registry: `pub mod c<n>;` (by path) for every `c<n>.rs` in `dir`, `registry()` in id order and `sources()`.
/// Each check is the unit struct `C<n>` of its module.
pub fn registry_source(dir: &Path) -> Result<String, String> {
    let ids = numbered(dir, "c")?;
    let mut s = String::from("// Written by tools/paddock-dev/build.rs from the file names src/check/c<n>.rs; never edited.\n");
    for n in &ids {
        s.push_str(&format!("#[path = {}]\npub mod c{n};\n", lit(&dir.join(format!("c{n}.rs")))));
    }
    s.push_str("\n/// The checks the host has, in id order: every `src/check/c<n>.rs`, whose unit struct `C<n>` declares the check.\n");
    s.push_str("pub fn registry() -> Vec<Box<dyn Check>> {\n    vec![");
    s.push_str(&ids.iter().map(|n| format!("Box::new(c{n}::C{n})")).collect::<Vec<_>>().join(", "));
    s.push_str("]\n}\n\n/// `(check id, source file)` of every registered check, for `list --json`.\n");
    s.push_str("pub fn sources() -> Vec<(&'static str, &'static str)> {\n    vec![");
    s.push_str(
        &ids.iter()
            .map(|n| format!("(c{n}::C{n}.id(), \"tools/paddock-dev/src/check/c{n}.rs\")"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    s.push_str("]\n}\n");
    Ok(s)
}

/// The source of the test module lines: `mod check_c<n>;` (by path) for every `check_c<n>.rs` in `dir`.
pub fn test_mods_source(dir: &Path) -> Result<String, String> {
    let mut s = String::from("// Written by tools/paddock-dev/build.rs from the file names tests/it/check_c<n>.rs; never edited.\n");
    for n in numbered(dir, "check_c")? {
        s.push_str(&format!(
            "#[path = {}]\nmod check_c{n};\n",
            lit(&dir.join(format!("check_c{n}.rs")))
        ));
    }
    Ok(s)
}
