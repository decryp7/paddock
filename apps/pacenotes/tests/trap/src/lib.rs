#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! The trap repository of audit finding PN1 (`docs/audits/pacenotes.md`) as a generated fixture.
//!
//! Pacenotes reads other people's repositories. Its rules (AGENTS.md "Safety") are: never execute repository code, never
//! leak a secret, never read outside the allow-listed workspace. [`generate`] writes a synthetic repository that tries all
//! three. It would **run** code if the repository were built, installed or opened by a tool that trusts it: a Cargo build
//! script and an npm install hook, a `.cargo/config.toml` naming a `rustc-wrapper` and a `target.*.runner`, a
//! `rust-toolchain.toml`, a `.git/config` with `core.fsmonitor` and filter and diff drivers, and a proc-macro crate (each
//! would leave the marker file). It would **leak** a made-up secret in `.env`. It would **escape**: symlinks to a folder,
//! to a file and to `/`, a symlink loop, a nested `.git` file (a gitfile) pointing outside, and
//! `.git/objects/info/alternates` pointing outside. Each trap is *live*: the wrappers are executable scripts that touch the
//! marker, the `.git` folders are valid repositories (so git would reach the `fsmonitor`, filter, diff and alternates
//! settings), and the workspace crate really depends on the proc-macro crate and calls it; tests check that, and none of them
//! ever runs a trap. An indexer is run over the repository and reports what it did as [`Observations`];
//! [`Observations::verdict`] says whether it ran nothing, leaked nothing and escaped nothing, and requires one search reply
//! per entry of [`TRAP_QUERIES`] so a missing check cannot pass.
//!
//! Everything is invented (ER-14): the "secret" is a fixed dummy string, nothing is taken from a real machine, and the
//! repository lives in a [`Scratch`] folder that the test removes.

use std::fmt;
use std::path::{Path, PathBuf};

use paddock_testkit::{Scratch, ScratchError};

/// The made-up secret written to the trap's `.env`. It must appear in no table, reply, log or chunk of an indexer.
pub const SECRET: &str = "PN1-DUMMY-SECRET-0f3a9c71-not-a-real-credential";

/// A distinctive fragment of [`SECRET`]: a leak that cuts or re-cases the secret still contains it.
pub const SECRET_FRAGMENT: &str = "0f3a9c71";

/// The made-up text of the file outside the workspace that the escaping symlinks point at.
pub const OUTSIDE_TEXT: &str = "PN1-OUTSIDE-FILE-7d21e0b4-must-never-be-read";

/// A distinctive fragment of [`OUTSIDE_TEXT`].
pub const OUTSIDE_FRAGMENT: &str = "7d21e0b4";

/// Queries an indexer's test runs through **the store's own search and read API** after indexing the trap; the replies go
/// into [`Observations::search_replies`]. None of them may find the secret or the outside file.
pub const TRAP_QUERIES: [&str; 6] = [
    "PN1-DUMMY-SECRET",
    "0f3a9c71",
    "API_TOKEN",
    "not-a-real-credential",
    "PN1-OUTSIDE",
    "7d21e0b4",
];

/// Why a trap repository could not be generated.
#[derive(Debug)]
pub enum TrapError {
    /// A scratch-folder operation failed.
    Scratch(ScratchError),
    /// Creating a symlink failed.
    Link(std::io::Error),
}

impl fmt::Display for TrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrapError::Scratch(e) => write!(f, "could not write the trap repository: {e}"),
            TrapError::Link(e) => write!(f, "could not create a symlink of the trap: {e}"),
        }
    }
}

impl std::error::Error for TrapError {}

impl From<ScratchError> for TrapError {
    fn from(e: ScratchError) -> Self {
        TrapError::Scratch(e)
    }
}

/// A generated trap repository.
#[derive(Debug, Clone)]
pub struct TrapRepo {
    /// The workspace root the indexer is allowed to read.
    pub workspace: PathBuf,
    /// A file next to the workspace, outside it, that the symlinks and gitfile point at.
    pub outside_file: PathBuf,
    /// The marker any trapped command would create if it ran.
    pub marker: PathBuf,
    /// The executable script (`rustc-wrapper`, runner) that touches the marker.
    pub wrapper: PathBuf,
    /// The nested repository whose `.git` is a gitfile pointing outside.
    pub nested: PathBuf,
    /// The repository folder outside the workspace that the gitfile points at.
    pub outside_gitdir: PathBuf,
    /// The symlinks inside the workspace (empty where symlinks need privileges: Windows). Reading through any of them leaves
    /// the workspace or does not resolve.
    pub links: Vec<PathBuf>,
}

fn write_all(scratch: &Scratch, files: &[(&str, String)]) -> Result<(), TrapError> {
    for (rel, content) in files {
        scratch.write(rel, content)?;
    }
    Ok(())
}

/// Writes the trap repository under `scratch`.
pub fn generate(scratch: &Scratch) -> Result<TrapRepo, TrapError> {
    let workspace = scratch.path("trap/workspace");
    let marker = scratch.path("trap/MARKER-code-ran");
    let m = marker.to_string_lossy().replace('\\', "/");
    let outside = scratch.path("trap/outside").to_string_lossy().replace('\\', "/");
    let touch = format!("node -e \"require('fs').writeFileSync('{m}','ran')\"");

    write_all(
        scratch,
        &[
            // run-traps
            (
                "trap/workspace/Cargo.toml",
                "[package]\nname = \"trap\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[dependencies]\ntrap-macros = { path = \"macros\" }\n".to_owned(),
            ),
            (
                "trap/workspace/build.rs",
                format!("fn main() {{\n    std::fs::write(\"{m}\", \"the build script ran\").unwrap();\n}}\n"),
            ),
            (
                "trap/workspace/src/lib.rs",
                "trap_macros::boom!();\n\npub fn visible() -> u32 {\n    1\n}\n".to_owned(),
            ),
            (
                "trap/workspace/package.json",
                format!(
                    "{{\n  \"name\": \"trap\",\n  \"version\": \"0.0.0\",\n  \"scripts\": {{ \"postinstall\": \"{}\" }}\n}}\n",
                    touch.replace('"', "\\\"")
                ),
            ),
            (
                "trap/workspace/.cargo/config.toml",
                format!(
                    "[build]\nrustc-wrapper = \"{m}.sh\"\n\n[target.x86_64-unknown-linux-gnu]\nrunner = \"{m}.sh\"\n\n[target.aarch64-apple-darwin]\nrunner = \"{m}.sh\"\n"
                ),
            ),
            (
                "trap/workspace/rust-toolchain.toml",
                format!("[toolchain]\npath = \"{outside}/toolchain\"\n"),
            ),
            (
                "trap/workspace/.git/config",
                format!(
                    "[core]\n\trepositoryformatversion = 0\n\tfsmonitor = {touch}\n[filter \"trap\"]\n\tclean = {touch}\n\tsmudge = {touch}\n[diff \"trap\"]\n\ttextconv = {touch}\n"
                ),
            ),
            ("trap/workspace/.git/HEAD", "ref: refs/heads/main\n".to_owned()),
            ("trap/outside/gitdir/HEAD", "ref: refs/heads/main\n".to_owned()),
            ("trap/outside/toolchain/bin/rustc", format!("#!/bin/sh\necho ran > '{m}'\n")),
            ("trap/MARKER-code-ran.sh", format!("#!/bin/sh\necho ran > '{m}'\n")),
            ("trap/workspace/.git/objects/info/alternates", format!("{outside}/objects\n")),
            ("trap/workspace/.gitattributes", "* filter=trap diff=trap\n".to_owned()),
            (
                "trap/workspace/macros/Cargo.toml",
                "[package]\nname = \"trap-macros\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[lib]\nproc-macro = true\n".to_owned(),
            ),
            (
                "trap/workspace/macros/src/lib.rs",
                format!(
                    "#[proc_macro]\npub fn boom(_: proc_macro::TokenStream) -> proc_macro::TokenStream {{\n    std::fs::write(\"{m}\", \"the proc macro ran\").unwrap();\n    Default::default()\n}}\n"
                ),
            ),
            // leak-trap
            ("trap/workspace/.env", format!("API_TOKEN={SECRET}\n")),
            // escape-traps: a nested checkout whose `.git` is a gitfile pointing outside
            ("trap/workspace/sub/.git", format!("gitdir: {outside}/gitdir\n")),
            ("trap/outside/outside-secret.txt", OUTSIDE_TEXT.to_owned()),
            ("trap/outside/objects/pack/pn1.pack", format!("{OUTSIDE_TEXT}\n")),
            ("trap/outside/gitdir/config", format!("[core]\n\tfsmonitor = {touch}\n")),
        ],
    )?;

    // valid, empty repositories: git reaches their configuration
    for dir in [
        "trap/workspace/.git/refs/heads",
        "trap/workspace/.git/objects/pack",
        "trap/outside/gitdir/refs/heads",
        "trap/outside/gitdir/objects/pack",
    ] {
        std::fs::create_dir_all(scratch.path(dir)).map_err(TrapError::Link)?;
    }
    let wrapper = scratch.path("trap/MARKER-code-ran.sh");
    for script in [wrapper.clone(), scratch.path("trap/outside/toolchain/bin/rustc")] {
        make_executable(&script)?;
    }

    let outside_file = scratch.path("trap/outside/outside-secret.txt");
    let links = make_links(&workspace, &scratch.path("trap/outside"), &outside_file)?;
    let nested = workspace.join("sub");
    let outside_gitdir = scratch.path("trap/outside/gitdir");
    Ok(TrapRepo {
        workspace,
        outside_file,
        marker,
        wrapper,
        nested,
        outside_gitdir,
        links,
    })
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), TrapError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(TrapError::Link)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), TrapError> {
    Ok(())
}

#[cfg(unix)]
fn make_links(workspace: &Path, outside_dir: &Path, outside_file: &Path) -> Result<Vec<PathBuf>, TrapError> {
    use std::os::unix::fs::symlink;
    let mut links = Vec::new();
    let mut link = |name: &str, target: &Path| -> Result<(), TrapError> {
        let at = workspace.join(name);
        symlink(target, &at).map_err(TrapError::Link)?;
        links.push(at);
        Ok(())
    };
    link("vendor-link", outside_dir)?; // a link to a folder outside
    link("link-file", outside_file)?; // a link to a file outside
    link("root-link", Path::new("/"))?; // a link to the root of the machine
    link("loop-a", Path::new("loop-b"))?; // a loop: a -> b -> a
    link("loop-b", Path::new("loop-a"))?;
    Ok(links)
}

#[cfg(not(unix))]
fn make_links(_workspace: &Path, _outside_dir: &Path, _outside_file: &Path) -> Result<Vec<PathBuf>, TrapError> {
    Ok(Vec::new())
}

/// What an indexer did while it indexed the trap repository, as its test seams recorded it.
#[derive(Debug, Clone, Default)]
pub struct Observations {
    /// Every child process it started (the process-spawn seam records the command line); none is allowed.
    pub children_started: Vec<String>,
    /// Every path it opened for reading.
    pub files_read: Vec<PathBuf>,
    /// The paths in the store's `files` table, workspace-relative.
    pub indexed_files: Vec<String>,
    /// Everything it stored or replied, as bytes: every table, every chunk, every reply, every log line.
    pub everything_stored_or_replied: Vec<u8>,
    /// The replies of the store's own search and read API to [`TRAP_QUERIES`], one text per reply.
    pub search_replies: Vec<String>,
}

/// Which promise of the trap the indexer broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    /// Repository code ran: a child process was started, or the marker exists.
    Ran(String),
    /// A secret or outside text (or a distinctive fragment of it, in any letter case) reached a table, a reply or a search.
    Leaked(&'static str),
    /// A file outside the workspace was read.
    Escaped(PathBuf),
    /// A path that does not resolve (a symlink loop, a dangling link) was opened for reading.
    Unresolvable(PathBuf),
    /// A file the denylist must keep out of the index is in the `files` table.
    Indexed(String),
    /// The store was not queried once per entry of [`TRAP_QUERIES`] (this many replies were given); a check that did not
    /// run cannot pass.
    Unchecked(usize),
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

impl Observations {
    /// Checks the three promises against `repo`: nothing ran, nothing leaked, nothing escaped. Returns every breach.
    #[must_use]
    pub fn verdict(&self, repo: &TrapRepo) -> Vec<Breach> {
        let mut breaches = Vec::new();
        for child in &self.children_started {
            breaches.push(Breach::Ran(child.clone()));
        }
        if repo.marker.exists() {
            breaches.push(Breach::Ran(format!("marker {} exists", repo.marker.display())));
        }
        if self.search_replies.len() != TRAP_QUERIES.len() {
            breaches.push(Breach::Unchecked(self.search_replies.len()));
        }
        let everything = String::from_utf8_lossy(&self.everything_stored_or_replied).into_owned() + "\n" + &self.search_replies.join("\n");
        if [SECRET, SECRET_FRAGMENT].iter().any(|n| contains_ci(&everything, n)) {
            breaches.push(Breach::Leaked("the .env secret"));
        }
        if [OUTSIDE_TEXT, OUTSIDE_FRAGMENT].iter().any(|n| contains_ci(&everything, n)) {
            breaches.push(Breach::Leaked("the text of the file outside the workspace"));
        }
        for file in &self.indexed_files {
            let name = file.rsplit('/').next().unwrap_or(file);
            if name == ".env" || name.starts_with(".env.") || file.starts_with(".git/") {
                breaches.push(Breach::Indexed(file.clone()));
            }
        }
        let workspace = repo.workspace.canonicalize().unwrap_or_else(|_| repo.workspace.clone());
        for path in &self.files_read {
            match path.canonicalize() {
                Ok(real) if real.starts_with(&workspace) => {}
                Ok(_) => breaches.push(Breach::Escaped(path.clone())),
                Err(_) => breaches.push(Breach::Unresolvable(path.clone())),
            }
        }
        breaches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answered() -> Vec<String> {
        vec!["no hits".to_owned(); TRAP_QUERIES.len()]
    }

    fn trap() -> (Scratch, TrapRepo) {
        let scratch = Scratch::new();
        let repo = generate(&scratch).expect("trap");
        (scratch, repo)
    }

    #[test]
    fn the_trap_holds_every_run_leak_and_escape_trap() {
        let (_s, repo) = trap();
        let w = &repo.workspace;
        for f in [
            "build.rs",
            "package.json",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            ".git/config",
            ".git/objects/info/alternates",
            ".gitattributes",
            "macros/src/lib.rs",
            "sub/.git",
            ".env",
        ] {
            assert!(w.join(f).is_file(), "{f}");
        }
        let cargo = std::fs::read_to_string(w.join(".cargo/config.toml")).expect("config");
        assert!(cargo.contains("rustc-wrapper") && cargo.contains("runner"));
        let git = std::fs::read_to_string(w.join(".git/config")).expect("git config");
        assert!(git.contains("fsmonitor") && git.contains("[filter") && git.contains("[diff"));
        assert!(
            std::fs::read_to_string(w.join("macros/Cargo.toml"))
                .expect("macros")
                .contains("proc-macro = true")
        );
        assert!(
            std::fs::read_to_string(w.join("sub/.git"))
                .expect("gitfile")
                .starts_with("gitdir: ")
        );
        assert!(std::fs::read_to_string(w.join(".env")).expect(".env").contains(SECRET));
        assert!(!repo.marker.exists(), "generating the trap runs nothing");
    }

    #[cfg(unix)]
    #[test]
    fn the_symlinks_leave_the_workspace_or_do_not_resolve() {
        let (_s, repo) = trap();
        let ws = repo.workspace.canonicalize().expect("workspace");
        assert_eq!(repo.links.len(), 5);
        let mut escaping = 0;
        let mut unresolved = 0;
        for l in &repo.links {
            match l.canonicalize() {
                Ok(real) => {
                    assert!(!real.starts_with(&ws), "{l:?}");
                    escaping += 1;
                }
                Err(_) => unresolved += 1,
            }
        }
        assert_eq!(
            (escaping, unresolved),
            (3, 2),
            "folder, file and root escape; the two loop links do not resolve"
        );
    }

    #[test]
    fn a_clean_indexer_passes_the_verdict() {
        let (_s, repo) = trap();
        let seen = Observations {
            files_read: vec![repo.workspace.join("src/lib.rs"), repo.workspace.join("Cargo.toml")],
            indexed_files: vec!["src/lib.rs".into(), "Cargo.toml".into()],
            everything_stored_or_replied: b"symbol visible in src/lib.rs".to_vec(),
            search_replies: answered(),
            ..Observations::default()
        };
        assert_eq!(seen.verdict(&repo), vec![]);
    }

    #[test]
    fn every_kind_of_breach_is_reported() {
        let (_s, repo) = trap();
        let seen = Observations {
            children_started: vec!["cargo build".to_owned()],
            files_read: vec![repo.outside_file.clone()],
            indexed_files: vec![".env".into(), "sub/.env.local".into(), ".git/config".into()],
            everything_stored_or_replied: format!("{SECRET} {OUTSIDE_TEXT}").into_bytes(),
            search_replies: answered(),
        };
        let breaches = seen.verdict(&repo);
        assert!(breaches.contains(&Breach::Ran("cargo build".to_owned())));
        assert!(breaches.contains(&Breach::Leaked("the .env secret")));
        assert!(breaches.contains(&Breach::Leaked("the text of the file outside the workspace")));
        assert!(breaches.contains(&Breach::Escaped(repo.outside_file.clone())));
        for f in [".env", "sub/.env.local", ".git/config"] {
            assert!(breaches.contains(&Breach::Indexed(f.into())), "{f}");
        }
    }

    #[test]
    fn a_fragment_in_another_letter_case_or_in_a_search_reply_is_a_leak() {
        let (_s, repo) = trap();
        for (stored, reply) in [
            ("token 0F3A9C71 here", ""),
            ("", "hit: pn1-dummy-secret-0f3a9c71"),
            ("", "...7D21E0B4..."),
        ] {
            let mut replies = answered();
            replies[1] = reply.to_owned();
            let seen = Observations {
                everything_stored_or_replied: stored.as_bytes().to_vec(),
                search_replies: replies,
                ..Observations::default()
            };
            assert!(
                matches!(seen.verdict(&repo).as_slice(), [Breach::Leaked(_)]),
                "{stored:?} {reply:?}"
            );
        }
    }

    #[test]
    fn a_marker_on_disk_is_a_breach() {
        let (_s, repo) = trap();
        std::fs::write(&repo.marker, "x").expect("write marker");
        let seen = Observations {
            search_replies: answered(),
            ..Observations::default()
        };
        assert!(matches!(seen.verdict(&repo).as_slice(), [Breach::Ran(_)]));
    }

    #[cfg(unix)]
    #[test]
    fn reading_through_a_link_or_a_loop_is_a_breach() {
        let (_s, repo) = trap();
        let through = repo.workspace.join("vendor-link").join("outside-secret.txt");
        let file_link = repo.workspace.join("link-file");
        let looped = repo.workspace.join("loop-a");
        let seen = Observations {
            files_read: vec![through.clone(), file_link.clone(), looped.clone()],
            search_replies: answered(),
            ..Observations::default()
        };
        assert_eq!(
            seen.verdict(&repo),
            vec![Breach::Escaped(through), Breach::Escaped(file_link), Breach::Unresolvable(looped)]
        );
    }

    #[test]
    fn a_check_that_did_not_run_cannot_pass() {
        let (_s, repo) = trap();
        assert_eq!(Observations::default().verdict(&repo), vec![Breach::Unchecked(0)]);
        let few = Observations {
            search_replies: vec!["no hits".into(); TRAP_QUERIES.len() - 1],
            ..Observations::default()
        };
        assert_eq!(few.verdict(&repo), vec![Breach::Unchecked(TRAP_QUERIES.len() - 1)]);
        let many = Observations {
            search_replies: vec!["no hits".into(); TRAP_QUERIES.len() + 1],
            ..Observations::default()
        };
        assert_eq!(many.verdict(&repo), vec![Breach::Unchecked(TRAP_QUERIES.len() + 1)]);
    }

    // ---- liveness: every trap could fire. Nothing here runs a trap. -------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn the_wrappers_exist_and_are_executable_scripts_that_touch_the_marker() {
        use std::os::unix::fs::PermissionsExt;
        let (_s, repo) = trap();
        let rustc = repo.outside_gitdir.parent().expect("outside").join("toolchain/bin/rustc");
        for script in [&repo.wrapper, &rustc] {
            let meta = std::fs::metadata(script).expect("script exists");
            assert!(meta.permissions().mode() & 0o111 != 0, "{script:?} is executable");
            let text = std::fs::read_to_string(script).expect("script text");
            assert!(
                text.starts_with("#!/bin/sh\n") && text.contains(&*repo.marker.to_string_lossy()),
                "{script:?}"
            );
        }
        let cargo = std::fs::read_to_string(repo.workspace.join(".cargo/config.toml")).expect("config");
        assert!(
            cargo.contains(&*repo.wrapper.to_string_lossy()),
            "rustc-wrapper and runners name the wrapper"
        );
        let toolchain = std::fs::read_to_string(repo.workspace.join("rust-toolchain.toml")).expect("toolchain");
        assert!(toolchain.contains(&*repo.outside_gitdir.parent().expect("outside").join("toolchain").to_string_lossy()));
    }

    #[test]
    fn the_workspace_crate_depends_on_the_proc_macro_crate_and_calls_it() {
        let (_s, repo) = trap();
        let manifest = std::fs::read_to_string(repo.workspace.join("Cargo.toml")).expect("manifest");
        assert!(manifest.contains("trap-macros = { path = \"macros\" }"));
        assert!(
            std::fs::read_to_string(repo.workspace.join("src/lib.rs"))
                .expect("lib")
                .contains("trap_macros::boom!();")
        );
        assert!(
            std::fs::read_to_string(repo.workspace.join("macros/src/lib.rs"))
                .expect("macro")
                .contains("pub fn boom")
        );
        assert!(repo.workspace.join("macros/Cargo.toml").is_file());
    }

    /// `git rev-parse --git-dir`: git accepts the folder as a repository, so it would go on to read its configuration.
    /// (rev-parse does not refresh the index, so it does not run `core.fsmonitor`, and no filter or diff driver runs.)
    #[allow(clippy::disallowed_methods)]
    fn rev_parse(scratch: &Scratch, args: &[&str]) -> std::process::Output {
        std::process::Command::new("git")
            .args(args)
            .arg("rev-parse")
            .arg("--git-dir")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("HOME", scratch.home())
            .output()
            .expect("git is installed (the CI image and developer machines have it)")
    }

    #[test]
    fn git_accepts_each_trap_repository() {
        let (scratch, repo) = trap();
        let ws = repo.workspace.to_string_lossy().into_owned();
        let out = repo.outside_gitdir.to_string_lossy().into_owned();
        let nested = repo.nested.to_string_lossy().into_owned();
        for (what, args) in [
            ("the workspace .git", vec!["-C", ws.as_str()]),
            ("the gitdir outside", vec!["--git-dir", out.as_str()]),
            ("the nested gitfile", vec!["-C", nested.as_str()]),
        ] {
            let o = rev_parse(&scratch, &args);
            assert!(o.status.success(), "{what}: {}", String::from_utf8_lossy(&o.stderr));
        }
        let o = rev_parse(&scratch, &["-C", &nested]);
        let resolved = String::from_utf8_lossy(&o.stdout).trim().to_owned();
        assert!(
            resolved.ends_with("outside/gitdir"),
            "the gitfile leads out of the workspace: {resolved}"
        );
        assert!(!repo.marker.exists(), "checking the traps ran none");
    }

    /// The indexer exists from A3a (`pacenotes-index`); until then this is the failing test it must make pass.
    #[test]
    #[ignore = "A3a"]
    fn indexing_the_trap_runs_nothing_leaks_nothing_and_escapes_nothing() {
        let (_s, repo) = trap();
        // A3a replaces this with: index `repo.workspace` through the process-spawn seam and the read seam, run
        // TRAP_QUERIES through the store's search and read API, collect the Observations, and assert that
        // `seen.verdict(&repo)` is empty.
        let _ = &repo;
        panic!("the indexer of A3a is not written yet; this test fails until it exists");
    }
}
