//! The plugin project a command works on: its folder and its `manifest.toml`.

use paddock_cli::CliError;
use paddock_plugin_package::Manifest;
use paddock_plugin_package::manifest::host_target;
use std::path::{Path, PathBuf};

/// The most `manifest.toml` text read (the platform's own limit, so a huge file is refused before parsing).
const MAX_MANIFEST: u64 = paddock_plugin_package::manifest::MAX_MANIFEST_BYTES;

/// A plugin project.
#[derive(Debug, Clone)]
pub struct Project {
    /// The project folder.
    pub dir: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
}

impl Project {
    /// Reads `<dir>/manifest.toml`.
    ///
    /// # Errors
    ///
    /// When there is no manifest (the hint says where to run the command) or it is not valid.
    pub fn load(dir: &Path) -> Result<Project, CliError> {
        let path = dir.join("manifest.toml");
        let not_a_project = || {
            CliError::failure(format!("{} has no manifest.toml.", dir.display()))
                .with_hint("Run the command inside a plugin project (`cargo paddock-plugin new <name>` makes one), or pass --dir <project>.")
        };
        let meta = std::fs::metadata(&path).map_err(|_| not_a_project())?;
        if meta.len() > MAX_MANIFEST {
            return Err(CliError::failure(format!(
                "{} is larger than {MAX_MANIFEST} bytes.",
                path.display()
            ))
            .with_hint(
                "A manifest only names the plugin and what it contributes; remove the rest.",
            ));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| {
            CliError::failure(format!("{} cannot be read ({e}).", path.display()))
                .with_hint("It must be a UTF-8 text file.")
        })?;
        let manifest = Manifest::parse(&text).map_err(|e| crate::errors::package(&e))?;
        Ok(Project {
            dir: dir.to_path_buf(),
            manifest,
        })
    }

    /// The folder cargo builds into (always inside the project, whatever `CARGO_TARGET_DIR` says).
    pub fn target_dir(&self) -> PathBuf {
        self.dir.join("target")
    }

    /// Where `cargo build --release [--target <target>]` puts the program for `target`.
    pub fn built(&self, target: &str, explicit_target: bool) -> PathBuf {
        let mut p = self.target_dir();
        if explicit_target {
            p.push(target);
        }
        p.push("release");
        p.push(self.manifest.binary_file(target));
        p
    }

    /// The built program for `target`, when there is one: built with `--target`, or (for this computer's target) without.
    pub fn find_built(&self, target: &str) -> Option<PathBuf> {
        let explicit = self.built(target, true);
        if explicit.is_file() {
            return Some(explicit);
        }
        let implicit = self.built(target, false);
        (target == host_target() && implicit.is_file()).then_some(implicit)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A project folder holding the fixture's manifest (program `binary`).
    pub(crate) fn project(dir: &Path, binary: &str) -> Project {
        let manifest = paddock_plugin_test::fixture::manifest_toml("2.0", binary, "");
        std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
        Project::load(dir).unwrap()
    }

    #[test]
    fn a_project_is_its_manifest_and_a_missing_or_bad_one_says_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let e = Project::load(dir.path()).unwrap_err();
        assert!(e.message.contains("no manifest.toml") && e.hint.contains("--dir"));
        std::fs::write(dir.path().join("manifest.toml"), "format = 1\n").unwrap();
        let e = Project::load(dir.path()).unwrap_err();
        assert!(
            e.message.contains("manifest.toml") && !e.hint.is_empty(),
            "{e:?}"
        );
        std::fs::write(
            dir.path().join("manifest.toml"),
            vec![b'a'; (MAX_MANIFEST + 1) as usize],
        )
        .unwrap();
        assert!(
            Project::load(dir.path())
                .unwrap_err()
                .message
                .contains("larger than")
        );
        std::fs::write(dir.path().join("manifest.toml"), [0xff, 0xfe]).unwrap();
        assert!(
            Project::load(dir.path())
                .unwrap_err()
                .message
                .contains("cannot be read")
        );
        let p = project(dir.path(), "prog");
        assert_eq!(p.manifest.binary, "prog");
        assert_eq!(p.target_dir(), dir.path().join("target"));
    }

    #[test]
    fn the_built_program_is_found_with_or_without_an_explicit_target() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        let host = host_target();
        assert_eq!(p.find_built(host), None);
        let implicit = p.built(host, false);
        std::fs::create_dir_all(implicit.parent().unwrap()).unwrap();
        std::fs::write(&implicit, b"x").unwrap();
        assert_eq!(p.find_built(host), Some(implicit));
        // Another target never uses the host build.
        assert_eq!(p.find_built("aarch64-unknown-linux-gnu"), None);
        let explicit = p.built(host, true);
        std::fs::create_dir_all(explicit.parent().unwrap()).unwrap();
        std::fs::write(&explicit, b"y").unwrap();
        assert_eq!(p.find_built(host), Some(explicit.clone()));
        assert!(
            explicit.ends_with(
                Path::new(host)
                    .join("release")
                    .join(p.manifest.binary_file(host))
            )
        );
    }
}
