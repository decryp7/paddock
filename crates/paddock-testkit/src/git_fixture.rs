//! [`template`] and [`copy_tree`]: a git fixture built once per machine and copied per test (ER-20, rule 85, rule NEW-git-fixtures).
//!
//! A test that needs a git repository (an origin with a few commits, a landed train, a worktree) used to build it itself, and the
//! dozens of `git` starts of a fixture (about 45 ms each on the Windows runner) were paid by every test. nextest runs each test in
//! its own process, so a per-binary `OnceLock` shares nothing. A template is built once per machine and recipe in a folder under
//! the kit's temp folder, and each test copies it with [`copy_tree`], which starts no process.
//!
//! - **Owner:** this module; the tests of the crate that names a template are its users, and none of them writes into one.
//! - **Where:** `<temp>/paddock-git-fixture-templates/<name>-<hash of recipe>`, in the temp folder the kit captured
//!   ([`crate::ambient::Ambient::temp_dir`]), never the user's folders.
//! - **Size limit:** [`MAX_TEMPLATE_BYTES`] per template; a builder that makes more panics, so a fixture that grows into a data set
//!   is noticed.
//! - **Removal:** plain `std::fs::remove_dir_all` of the kit's own temp templates, as `private_temp` does; the `safe_remove` of paddock-dev needs an owner record and is not reachable from a crate (accepted, user).
//! - **Eviction:** building a template removes every template of the same name and another hash (the recipe changed), and every
//!   half-built `building-*` folder an interrupted builder left behind that is older than [`STALE_BUILDING`]; the OS clears the temp
//!   folder as it does for any other.

use crate::ambient::ambient;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The folder under the temp folder that holds every template.
pub const ROOT_NAME: &str = "paddock-git-fixture-templates";

/// The most bytes a template may hold; a bigger one is a data set, not a fixture.
pub const MAX_TEMPLATE_BYTES: u64 = 4 * 1024 * 1024;

/// How old a half-built `building-*` folder must be before a later builder removes it (an interrupted builder left it).
pub const STALE_BUILDING: Duration = Duration::from_secs(60 * 60);

/// The settings every fixture repository carries, so its files check out with LF on every OS whatever the machine's git says (ER-61:
/// the Windows runner's git converts to CRLF by default, and a template's `checkout` wrote `a\r\n` where the test wanted `a\n`, run
/// 9143). Appended to a repository's own config file: it outranks the global and system ones, and no process starts.
pub const LF_CONFIG: &str = "[core]\n\tautocrlf = false\n\teol = lf\n";

const READY: &str = ".ready";
const BUILDING: &str = "building-";

/// A fixture repository built once per machine and recipe, not once per test.
///
/// `recipe` is the text of the source that builds it (`include_str!` of the test file), so any change to the recipe is a new template.
/// `build` gets a scratch folder to fill and the final folder the template will have (for an absolute path it must record); the
/// scratch folder is renamed into place, and a second builder that loses the race drops its own. Callers copy the returned folder with
/// [`copy_tree`] and never write into it. See the module for the owner, the size limit and the eviction rule.
///
/// # Panics
/// When the folder cannot be made or renamed, or the template is bigger than [`MAX_TEMPLATE_BYTES`] (the test cannot run).
#[allow(clippy::expect_used)]
pub fn template(name: &str, recipe: &str, build: impl FnOnce(&Path, &Path)) -> PathBuf {
    template_in(&ambient().temp_dir.join(ROOT_NAME), name, recipe, build)
}

/// [`template`] under `root` (the kit's own tests give it a folder of their own).
///
/// # Panics
/// As [`template`].
#[allow(clippy::expect_used)]
pub fn template_in(
    root: &Path,
    name: &str,
    recipe: &str,
    build: impl FnOnce(&Path, &Path),
) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    recipe.hash(&mut h);
    let dir_name = format!("{name}-{:016x}", h.finish());
    let ready = root.join(&dir_name);
    if ready.join(READY).is_file() {
        return ready;
    }
    std::fs::create_dir_all(root).expect("cannot make the template folder");
    let scratch = tempfile::Builder::new()
        .prefix(BUILDING)
        .tempdir_in(root)
        .expect("cannot make the scratch folder of a template");
    build(scratch.path(), &ready);
    // Every repository the builder made checks out with LF in each test's copy too (a builder calls [`lf_only`] right after its
    // `git init`, before its first checkout).
    // per-item: a config file append, not a process start
    for repo in repositories(scratch.path()) {
        lf_only(&repo);
    }
    let size = tree_bytes(scratch.path());
    assert!(
        size <= MAX_TEMPLATE_BYTES,
        "the template {name} holds {size} bytes, over the {MAX_TEMPLATE_BYTES} a fixture may have: build a smaller repository"
    );
    std::fs::write(scratch.path().join(READY), "").expect("cannot mark the template ready");
    evict(root, name, &dir_name);
    let built = scratch.keep();
    if std::fs::rename(&built, &ready).is_err() {
        // Another test process won the race: use its template.
        let _ = std::fs::remove_dir_all(&built);
    }
    ready
}

/// Makes the repository at `repo` (a working tree with `.git`, or a bare repository) check its files out with LF: appends
/// [`LF_CONFIG`] to its config file once. A builder calls it right after `git init`, before anything is checked out.
///
/// # Panics
/// When `repo` has no config file (not a repository) or it cannot be written (the fixture cannot be built).
#[allow(clippy::expect_used)]
pub fn lf_only(repo: &Path) {
    let config = [repo.join(".git").join("config"), repo.join("config")]
        .into_iter()
        .find(|c| c.is_file())
        .unwrap_or_else(|| {
            panic!(
                "{} is no git repository: no .git/config or config",
                repo.display()
            )
        });
    let text = std::fs::read_to_string(&config).expect("cannot read a fixture repository's config");
    if !text.contains(LF_CONFIG) {
        let sep = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        std::fs::write(&config, format!("{text}{sep}{LF_CONFIG}"))
            .expect("cannot write a fixture repository's config");
    }
}

/// The repositories below `dir`: working trees (a `.git` folder) and bare repositories (`HEAD`, `config` and `objects`).
fn repositories(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let bare =
        dir.join("HEAD").is_file() && dir.join("config").is_file() && dir.join("objects").is_dir();
    if dir.join(".git").is_dir() || bare {
        out.push(dir.to_path_buf());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    // per-item: a folder read, not a process start
    for e in entries.flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) && e.file_name() != ".git" && !bare {
            out.extend(repositories(&e.path()));
        }
    }
    out
}

/// Removes the templates of `name` with another hash than `keep`, and the stale half-built folders.
fn evict(root: &Path, name: &str, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let prefix = format!("{name}-");
    for old in entries.flatten() {
        let n = old.file_name().to_string_lossy().into_owned();
        let stale_build = n.starts_with(BUILDING)
            && old
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STALE_BUILDING);
        if (n.starts_with(&prefix) && n != keep) || stale_build {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
}

/// The bytes of the files below `dir`.
fn tree_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    // per-item: a file stat, not a process start
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => tree_bytes(&e.path()),
            _ => e.metadata().map_or(0, |m| m.len()),
        })
        .sum()
}

/// Copies a folder tree: a fixture's private copy of its template, with no git start.
///
/// # Panics
/// When a folder cannot be made or a file cannot be copied (the test cannot run).
#[allow(clippy::expect_used)]
pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("cannot make the folder of the copy");
    // per-item: a file copy, not a process start
    for e in std::fs::read_dir(from).expect("cannot read the template") {
        let e = e.expect("cannot read a template entry");
        let target = to.join(e.file_name());
        if e.file_type()
            .expect("cannot stat a template entry")
            .is_dir()
        {
            copy_tree(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).expect("cannot copy a template file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// ER-61: every repository of a template, a working tree and a bare one, checks out with LF; read back through git's own parser.
    #[test]
    fn every_repository_of_a_template_has_autocrlf_off_and_eol_lf() {
        let root = crate::tempdir();
        let t = template_in(root.path(), "lf", "recipe", |s, _| {
            for (dir, bare) in [("work", false), ("origin.git", true)] {
                let mut c = crate::TestCommand::new("git");
                c.args(["init", "-q"]);
                if bare {
                    c.arg("--bare");
                }
                assert!(c.arg(s.join(dir)).status().expect("git init").success());
            }
        });
        for (dir, config) in [("work", ".git/config"), ("origin.git", "config")] {
            for (key, want) in [("core.autocrlf", "false"), ("core.eol", "lf")] {
                let out = crate::TestCommand::new("git")
                    .args(["config", "--file"])
                    .arg(t.join(dir).join(config))
                    .args(["--get", key])
                    .output()
                    .expect("git config");
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout).trim(),
                    want,
                    "{dir} {key}"
                );
            }
        }
    }

    #[test]
    fn lf_only_writes_the_settings_once() {
        let dir = crate::tempdir();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/config"), "[core]\n\tbare = false").unwrap();
        lf_only(dir.path());
        lf_only(dir.path());
        let text = std::fs::read_to_string(dir.path().join(".git/config")).unwrap();
        assert_eq!(text.matches("autocrlf = false").count(), 1, "{text}");
        assert!(text.starts_with("[core]\n\tbare = false\n[core]"), "{text}");
    }

    #[test]
    fn a_template_is_built_once_and_a_second_call_reuses_it() {
        let root = crate::tempdir();
        let built = Cell::new(0);
        let make = |scratch: &Path, _: &Path| {
            built.set(built.get() + 1);
            std::fs::write(scratch.join("f.txt"), "x").unwrap();
        };
        let a = template_in(root.path(), "t", "recipe", make);
        let b = template_in(root.path(), "t", "recipe", |_, _| panic!("built twice"));
        assert_eq!((built.get(), &a), (1, &b));
        assert!(a.join("f.txt").is_file() && a.join(READY).is_file());
    }

    #[test]
    fn a_changed_recipe_is_a_new_template_and_the_old_one_of_the_name_is_evicted() {
        let root = crate::tempdir();
        let one = template_in(root.path(), "t", "v1", |s, _| {
            std::fs::write(s.join("a"), "1").unwrap()
        });
        let other = template_in(root.path(), "other", "v1", |s, _| {
            std::fs::write(s.join("a"), "1").unwrap()
        });
        let two = template_in(root.path(), "t", "v2", |s, _| {
            std::fs::write(s.join("a"), "2").unwrap()
        });
        assert_ne!(one, two);
        assert!(!one.exists(), "the template of the old recipe stays");
        assert!(
            two.is_dir() && other.is_dir(),
            "a template of another name must be kept"
        );
    }

    #[test]
    fn the_builder_is_given_the_final_folder_and_the_scratch_folder_is_gone_after() {
        let root = crate::tempdir();
        let seen = Cell::new(false);
        let ready = template_in(root.path(), "t", "r", |scratch, ready| {
            seen.set(ready.starts_with(root.path()) && scratch != ready);
        });
        assert!(seen.get());
        let left: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            left,
            vec![ready.file_name().unwrap().to_os_string()],
            "only the template is left"
        );
    }

    #[test]
    #[should_panic(expected = "a fixture may have")]
    fn a_template_over_the_size_limit_is_refused() {
        let root = crate::tempdir();
        let _ = template_in(root.path(), "big", "r", |s, _| {
            std::fs::write(
                s.join("big"),
                vec![0u8; usize::try_from(MAX_TEMPLATE_BYTES).unwrap() + 1],
            )
            .unwrap();
        });
    }

    #[test]
    fn copy_tree_copies_files_and_folders_without_changing_the_source() {
        let src = crate::tempdir();
        std::fs::create_dir_all(src.path().join("d/e")).unwrap();
        std::fs::write(src.path().join("d/e/f.txt"), "hi").unwrap();
        std::fs::write(src.path().join("top.txt"), "t").unwrap();
        let dst = crate::tempdir();
        let to = dst.path().join("copy");
        copy_tree(src.path(), &to);
        assert_eq!(std::fs::read_to_string(to.join("d/e/f.txt")).unwrap(), "hi");
        assert_eq!(std::fs::read_to_string(to.join("top.txt")).unwrap(), "t");
        assert!(src.path().join("d/e/f.txt").is_file());
    }

    #[test]
    fn a_half_built_folder_is_evicted_only_when_it_is_older_than_the_limit() {
        let root = crate::tempdir();
        let fresh = root.path().join("building-fresh");
        let stale = root.path().join("building-stale");
        for d in [&fresh, &stale] {
            std::fs::create_dir_all(d).unwrap();
        }
        // A folder younger than the limit may belong to a running builder; the old one is set back in time, no sleep.
        #[cfg(unix)]
        std::fs::File::open(&stale)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - STALE_BUILDING * 2)
            .unwrap();
        let _ = template_in(root.path(), "t", "r", |s, _| {
            std::fs::write(s.join("a"), "1").unwrap()
        });
        assert!(fresh.is_dir(), "a young half-built folder must be kept");
        #[cfg(unix)]
        assert!(!stale.exists(), "an old half-built folder must be removed");
    }
}
