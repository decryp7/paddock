//! No tool hands a build folder to a program as `CARGO_TARGET_DIR` (sccache 0.19 hashes every `CARGO_*` variable, so the variable
//! defeats the compile cache across worktrees; docs/lessons.md). The folder goes through `paddock_dev::cargo_target`. This audit
//! finds the variable's name as a literal followed by a comma in the tool's sources (the forms `.env("X", ..)`,
//! `.insert("X".into(), ..)`, `("X".to_string(), ..)`, and a list of names), outside the files of `cargo_target_allow.txt`. That list only
//! shrinks (ER-46): it is compared with the merge base, and a new entry fails.
#![allow(clippy::unwrap_used)]
use std::collections::BTreeSet;
use std::path::Path;

const ALLOW: &str = "tools/paddock-dev/tests/it/cargo_target_allow.txt";

/// The paths of an allow file's entries (comments and blank lines skipped; the text after `|` is the reason).
fn entries(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('|').next().unwrap().trim().to_string())
        .collect()
}

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

/// Whether a source line names either variable as a literal followed by a comma, possibly through `.to_string()` or `.into()`.
pub fn sets_the_variable(line: &str) -> bool {
    let l = line.trim_start();
    if l.starts_with("//") {
        return false;
    }
    ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"].iter().any(|name| {
        let lit = format!("\"{name}\"");
        l.match_indices(&lit).any(|(at, _)| {
            let rest = &l[at + lit.len()..];
            let rest = rest
                .strip_prefix(".to_string()")
                .or_else(|| rest.strip_prefix(".into()"))
                .unwrap_or(rest);
            rest.starts_with(',')
        })
    })
}

#[test]
fn the_audit_sees_every_form_a_tool_could_set_the_variable_in() {
    for yes in [
        r#"cmd.env("CARGO_TARGET_DIR", &target);"#,
        r#"        ("CARGO_TARGET_DIR".to_string(), path_of(Kind::Target)?),"#,
        r#"env.insert("CARGO_TARGET_DIR".into(), x);"#,
        r#"    "CARGO_BUILD_TARGET_DIR","#,
    ] {
        assert!(sets_the_variable(yes), "{yes}");
    }
    for no in [
        r#"env.get("CARGO_TARGET_DIR")"#,
        r#"step.var("CARGO_TARGET_DIR").map(PathBuf::from)"#,
        r#""CARGO_TARGET_DIR" | "CARGO_BUILD_JOBS" => {}"#,
        r#"// .env("CARGO_TARGET_DIR", x)"#,
    ] {
        assert!(!sets_the_variable(no), "{no}");
    }
}

#[test]
fn only_the_listed_files_set_the_build_folder_as_a_variable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    let mut found = BTreeSet::new();
    for f in files {
        if std::fs::read_to_string(&f).unwrap().lines().any(sets_the_variable) {
            found.insert(
                f.strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
    let allowed = entries(&std::fs::read_to_string(root.join("tests/it/cargo_target_allow.txt")).unwrap());
    let extra: Vec<_> = found.difference(&allowed).collect();
    assert!(
        extra.is_empty(),
        "these files set CARGO_TARGET_DIR for a child; pass the folder with paddock_dev::cargo_target::with_target_dir instead: {extra:?}"
    );
    let stale: Vec<_> = allowed.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "these entries no longer set the variable; remove them from cargo_target_allow.txt: {stale:?}"
    );
}

/// Whether `current` adds an entry that `base` does not have.
pub fn grown(base: &BTreeSet<String>, current: &BTreeSet<String>) -> Vec<String> {
    current.difference(base).cloned().collect()
}

#[test]
fn growth_is_found_by_comparing_with_a_base() {
    let base = entries("# c\na | r\nb | r\n");
    assert!(grown(&base, &entries("a | r\n")).is_empty(), "shrinking is fine");
    assert_eq!(grown(&base, &entries("a | r\nc | new\n")), ["c"]);
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = paddock_proc::command("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// ER-46: the list is compared with the merge base by its own check; growth needs the user's approval. Where the base cannot be read
/// (no `origin/main`, a shallow clone) the check is not measured and says so, it does not pass.
#[test]
fn the_allow_list_does_not_grow_against_the_merge_base() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base_ref = std::env::var("PADDOCK_BASE_REF").unwrap_or_else(|_| "origin/main".into());
    let Some(base) = git(&root, &["merge-base", "HEAD", &base_ref]) else {
        panic!(
            "NOT MEASURED: cannot find the merge base with {base_ref} (fetch it, or set PADDOCK_BASE_REF); the allow list's growth was not checked"
        );
    };
    let current = entries(&std::fs::read_to_string(root.join(ALLOW)).unwrap());
    // The commit that introduces the file has no earlier copy: every entry is its first.
    let Some(old) = git(&root, &["show", &format!("{base}:{ALLOW}")]) else {
        return;
    };
    let grown = grown(&entries(&old), &current);
    assert!(
        grown.is_empty(),
        "the allow list grew against {base_ref} ({base}); a new call passes the folder as configuration, and an entry needs the user's approval: {grown:?}"
    );
}
