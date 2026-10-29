#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! The trap repository of audit finding PN1 (`docs/audits/pacenotes.md`) as a generated fixture.
//!
//! Pacenotes reads other people's repositories. Its rules (AGENTS.md "Safety") are: never execute repository code, never
//! leak a secret, never read outside the allow-listed workspace. [`generate`] writes a synthetic repository that tries all
//! three: a Cargo build script and an npm install hook that would leave a marker if they ran, a `.env` holding a made-up
//! secret, and a symlink that points out of the workspace. An indexer is run over it and reports what it did as
//! [`Observations`]; [`Observations::verdict`] says whether it ran nothing, leaked nothing and escaped nothing.
//!
//! Everything is invented (ER-14): the "secret" is a fixed dummy string, nothing is taken from a real machine, and the
//! repository lives in a [`Scratch`] folder that the test removes.

use std::fmt;
use std::path::{Path, PathBuf};

use paddock_testkit::{Scratch, ScratchError};

/// The made-up secret written to the trap's `.env`. It must appear in no table, reply, log or chunk of an indexer.
pub const SECRET: &str = "PN1-DUMMY-SECRET-0f3a9c71-not-a-real-credential";

/// The made-up text of the file outside the workspace that the escaping symlink points at.
pub const OUTSIDE_TEXT: &str = "PN1-OUTSIDE-FILE-7d21e0b4-must-never-be-read";

/// Why a trap repository could not be generated.
#[derive(Debug)]
pub enum TrapError {
    /// A scratch-folder operation failed.
    Scratch(ScratchError),
    /// Creating the symlink failed (not for lack of platform support: see [`TrapRepo::escape_link`]).
    Link(std::io::Error),
}

impl fmt::Display for TrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrapError::Scratch(e) => write!(f, "could not write the trap repository: {e}"),
            TrapError::Link(e) => write!(f, "could not create the escaping symlink: {e}"),
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
    /// A file next to the workspace, outside it, that the symlink points at.
    pub outside_file: PathBuf,
    /// The marker a build script or hook would create if it ran.
    pub marker: PathBuf,
    /// The escaping symlink inside the workspace; `None` where symlinks cannot be created without privileges (Windows).
    pub escape_link: Option<PathBuf>,
    /// The workspace-relative paths of the files that would execute if the repository were built or installed.
    pub executable_files: Vec<&'static str>,
}

/// Writes the trap repository under `scratch`.
pub fn generate(scratch: &Scratch) -> Result<TrapRepo, TrapError> {
    let workspace = scratch.path("trap/workspace");
    let marker = scratch.path("trap/MARKER-code-ran");
    let marker_text = marker.to_string_lossy().replace('\\', "/");

    scratch.write(
        "trap/workspace/Cargo.toml",
        "[package]\nname = \"trap\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
    )?;
    scratch.write(
        "trap/workspace/build.rs",
        format!("fn main() {{\n    std::fs::write(\"{marker_text}\", \"the build script ran\").unwrap();\n}}\n"),
    )?;
    scratch.write("trap/workspace/src/lib.rs", "pub fn visible() -> u32 {\n    1\n}\n")?;
    scratch.write(
        "trap/workspace/package.json",
        format!(
            "{{\n  \"name\": \"trap\",\n  \"version\": \"0.0.0\",\n  \"scripts\": {{ \"postinstall\": \"node -e \\\"require('fs').writeFileSync('{marker_text}','hook ran')\\\"\" }}\n}}\n"
        ),
    )?;
    scratch.write("trap/workspace/.env", format!("API_TOKEN={SECRET}\n"))?;
    scratch.write("trap/outside/outside-secret.txt", OUTSIDE_TEXT)?;

    let outside_file = scratch.path("trap/outside/outside-secret.txt");
    let escape_link = link_out(&workspace, &scratch.path("trap/outside"))?;

    Ok(TrapRepo {
        workspace,
        outside_file,
        marker,
        escape_link,
        executable_files: vec!["build.rs", "package.json"],
    })
}

#[cfg(unix)]
fn link_out(workspace: &Path, target: &Path) -> Result<Option<PathBuf>, TrapError> {
    let link = workspace.join("vendor-link");
    std::os::unix::fs::symlink(target, &link).map_err(TrapError::Link)?;
    Ok(Some(link))
}

#[cfg(not(unix))]
fn link_out(_workspace: &Path, _target: &Path) -> Result<Option<PathBuf>, TrapError> {
    Ok(None)
}

/// What an indexer did while it indexed the trap repository, as its test seams recorded it.
#[derive(Debug, Clone, Default)]
pub struct Observations {
    /// Every child process it started (the process-spawn seam records the command line); none is allowed.
    pub children_started: Vec<String>,
    /// Every path it opened for reading.
    pub files_read: Vec<PathBuf>,
    /// Everything it stored or replied, as bytes: every table, every chunk, every reply, every log line.
    pub everything_stored_or_replied: Vec<u8>,
}

/// Which promise of the trap the indexer broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    /// Repository code ran: a child process was started, or the marker exists.
    Ran(String),
    /// A secret or outside text reached a table or a reply.
    Leaked(&'static str),
    /// A file outside the workspace was read.
    Escaped(PathBuf),
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
        let stored = String::from_utf8_lossy(&self.everything_stored_or_replied);
        if stored.contains(SECRET) {
            breaches.push(Breach::Leaked("the .env secret"));
        }
        if stored.contains(OUTSIDE_TEXT) {
            breaches.push(Breach::Leaked("the text of the file outside the workspace"));
        }
        let workspace = repo.workspace.canonicalize().unwrap_or_else(|_| repo.workspace.clone());
        for path in &self.files_read {
            let real = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !real.starts_with(&workspace) {
                breaches.push(Breach::Escaped(path.clone()));
            }
        }
        breaches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trap_contains_a_build_script_a_secret_and_a_symlink_out() {
        let scratch = Scratch::new();
        let repo = generate(&scratch).expect("trap");
        assert!(repo.workspace.join("build.rs").is_file());
        assert!(repo.workspace.join("package.json").is_file());
        let env = std::fs::read_to_string(repo.workspace.join(".env")).expect(".env");
        assert!(env.contains(SECRET));
        assert!(!repo.marker.exists(), "generating the trap runs nothing");
        #[cfg(unix)]
        {
            let link = repo.escape_link.as_ref().expect("link on unix");
            let real = link.canonicalize().expect("resolves");
            assert!(!real.starts_with(repo.workspace.canonicalize().expect("workspace")));
        }
    }

    #[test]
    fn a_clean_indexer_passes_the_verdict() {
        let scratch = Scratch::new();
        let repo = generate(&scratch).expect("trap");
        let seen = Observations {
            files_read: vec![repo.workspace.join("src/lib.rs"), repo.workspace.join("Cargo.toml")],
            everything_stored_or_replied: b"symbol visible in src/lib.rs".to_vec(),
            ..Observations::default()
        };
        assert_eq!(seen.verdict(&repo), vec![]);
    }

    #[test]
    fn every_kind_of_breach_is_reported() {
        let scratch = Scratch::new();
        let repo = generate(&scratch).expect("trap");
        let seen = Observations {
            children_started: vec!["cargo build".to_owned()],
            files_read: vec![repo.outside_file.clone()],
            everything_stored_or_replied: format!("{SECRET} {OUTSIDE_TEXT}").into_bytes(),
        };
        let breaches = seen.verdict(&repo);
        assert!(breaches.contains(&Breach::Ran("cargo build".to_owned())));
        assert!(breaches.contains(&Breach::Leaked("the .env secret")));
        assert!(breaches.contains(&Breach::Leaked("the text of the file outside the workspace")));
        assert!(breaches.contains(&Breach::Escaped(repo.outside_file.clone())));
    }

    #[test]
    fn a_marker_on_disk_is_a_breach() {
        let scratch = Scratch::new();
        let repo = generate(&scratch).expect("trap");
        std::fs::write(&repo.marker, "x").expect("write marker");
        assert!(matches!(Observations::default().verdict(&repo).as_slice(), [Breach::Ran(_)]));
    }

    #[cfg(unix)]
    #[test]
    fn reading_through_the_symlink_counts_as_escaping() {
        let scratch = Scratch::new();
        let repo = generate(&scratch).expect("trap");
        let through = repo.escape_link.clone().expect("link").join("outside-secret.txt");
        let seen = Observations {
            files_read: vec![through.clone()],
            ..Observations::default()
        };
        assert_eq!(seen.verdict(&repo), vec![Breach::Escaped(through)]);
    }

    /// The indexer exists from A3a (`pacenotes-index`); until then this is the failing test it must make pass.
    #[test]
    #[ignore = "A3a"]
    fn indexing_the_trap_runs_nothing_leaks_nothing_and_escapes_nothing() {
        let scratch = Scratch::new();
        let repo = generate(&scratch).expect("trap");
        // A3a replaces this with: index `repo.workspace` through the process-spawn seam and the read seam, collect the
        // Observations, and assert `seen.verdict(&repo).is_empty()`.
        let _ = &repo;
        panic!("the indexer of A3a is not written yet; this test fails until it exists");
    }
}
