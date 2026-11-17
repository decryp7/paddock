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

/// ER-46: the list is compared with the merge base by the shared comparison, `check::baseline::growth` (the one C53, C115 and the
/// allow-list readers use; rule 55: no second copy), against the target ref the shared checks use (`PADDOCK_BASE_REF`, else
/// `origin/main`). Growth needs the user's approval. Where the base cannot be seen (a host runner's kept clone has no `origin/main`; a
/// shallow clone) the comparison is not measured and says so with a `::warning::`, as every other growth guard of this tree does
/// (`Growth::Reason`: a warning, never silence); the Linux legs check out the whole history and measure. Once the shared
/// `baseline::target_ref` of feat/testkit-stand-in is on the train, the two lines that read the ref become `baseline::target_ref(None)`.
#[test]
fn the_allow_list_does_not_grow_against_the_merge_base() {
    use paddock_dev::check::baseline::{Growth, growth};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base_ref = std::env::var("PADDOCK_BASE_REF")
        .ok()
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| "origin/main".into());
    let current: Vec<String> = entries(&std::fs::read_to_string(root.join(ALLOW)).unwrap()).into_iter().collect();
    let entries_of = |text: &str| Some(entries(text).into_iter().collect::<Vec<String>>());
    match growth(&root, &base_ref, ALLOW, &current, &entries_of, &|k: &String| k.clone(), None) {
        Growth::Reason(why) => eprintln!(
            "::warning::NOT MEASURED: the allow list's shrink-only rule ({ALLOW}) was not checked against {base_ref}: {why}. Next: fetch the target branch (`git fetch origin main`, full history) or set PADDOCK_BASE_REF"
        ),
        Growth::Added { added: 0, .. } => {}
        Growth::Added { base, added, .. } => panic!(
            "the allow list grew against {base_ref} (merge base {base}): {added} entr{} not in the list there; it may only shrink. A new call passes the folder as configuration; an entry needs the user's approval",
            if added == 1 { "y is" } else { "ies are" }
        ),
    }
}

/// The comparison really sees growth: a branch that adds an entry to a list the base has is found, a branch that only shrinks it is not.
#[test]
fn the_shared_comparison_finds_an_added_entry_and_not_a_removed_one() {
    use paddock_dev::check::baseline::{Growth, growth};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let out = paddock_proc::command("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    let write = |text: &str| {
        std::fs::create_dir_all(root.join("tools/paddock-dev/tests/it")).unwrap();
        std::fs::write(root.join(ALLOW), text).unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "x"]);
    };
    git(&["init", "-q", "-b", "main"]);
    write("a | r\nb | r\n");
    git(&["checkout", "-q", "-b", "topic"]);
    let run = |text: &str| {
        let current: Vec<String> = entries(text).into_iter().collect();
        let entries_of = |t: &str| Some(entries(t).into_iter().collect::<Vec<String>>());
        growth(root, "main", ALLOW, &current, &entries_of, &|k: &String| k.clone(), None)
    };
    write("a | r\nb | r\nc | new\n");
    assert!(matches!(run("a | r\nb | r\nc | new\n"), Growth::Added { added: 1, .. }));
    write("a | r\n");
    assert!(matches!(run("a | r\n"), Growth::Added { added: 0, .. }), "shrinking is fine");
    // A ref that does not exist cannot be measured: a reason, never a pass.
    let current: Vec<String> = vec!["a".into()];
    let entries_of = |t: &str| Some(entries(t).into_iter().collect::<Vec<String>>());
    assert!(matches!(
        growth(root, "origin/nowhere", ALLOW, &current, &entries_of, &|k: &String| k.clone(), None),
        Growth::Reason(_)
    ));
}
