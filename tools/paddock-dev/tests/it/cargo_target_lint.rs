//! No tool hands a build folder to a program as `CARGO_TARGET_DIR` (sccache 0.19 hashes every `CARGO_*` variable, so the variable
//! defeats the compile cache across worktrees; docs/lessons.md). The folder goes through `paddock_dev::cargo_target`. This audit
//! finds `.env("CARGO_TARGET_DIR", ..)` and `.env("CARGO_BUILD_TARGET_DIR", ..)` in the tool's sources outside the list below. The
//! list only shrinks (ER-46): an entry leaves with its call.
#![allow(clippy::unwrap_used)]
use std::path::Path;

/// File and why it still sets the variable.
const ALLOWED: [(&str, &str); 1] = [(
    "src/generators/run.rs",
    "a generator that is not a cargo call (a script, npm, an external subcommand) starts cargo itself: no argument to put the folder in, and no wrapper",
)];

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn only_the_listed_calls_set_the_build_folder_as_a_variable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    let mut found = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let hit = text.lines().any(|l| {
            let l = l.trim_start();
            !l.starts_with("//") && (l.contains(".env(\"CARGO_TARGET_DIR\"") || l.contains(".env(\"CARGO_BUILD_TARGET_DIR\""))
        });
        if hit {
            found.push(
                f.strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
    found.sort();
    let allowed: Vec<&str> = ALLOWED.iter().map(|(f, _)| *f).collect();
    let extra: Vec<&String> = found.iter().filter(|f| !allowed.contains(&f.as_str())).collect();
    assert!(
        extra.is_empty(),
        "these files set CARGO_TARGET_DIR for a child; pass the folder with paddock_dev::cargo_target::with_target_dir instead: {extra:?}"
    );
    let stale: Vec<&&str> = allowed.iter().filter(|a| !found.iter().any(|f| f == **a)).collect();
    assert!(
        stale.is_empty(),
        "these entries no longer set the variable; remove them from ALLOWED: {stale:?}"
    );
}
