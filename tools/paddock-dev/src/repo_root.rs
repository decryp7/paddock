//! The repository root a command works in: the folder it was given, else the top of the git checkout the current folder is in.
//! One definition for every `--root` option (ER-22); it lives in the core layer because host, repo, ci and gates commands all use it.

use crate::git::Git;
use paddock_cli::CliError;
use std::path::{Path, PathBuf};

/// The repository root: the given one, or git's top level from the current folder.
pub fn repo_root(given: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(r) = given {
        return Ok(r.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    repo_root_from(&cwd, None)
}

/// [`repo_root`] for a given current folder (the part a test can drive).
pub fn repo_root_from(cwd: &Path, given: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(r) = given {
        return Ok(r.to_path_buf());
    }
    let top = Git::new(cwd)
        .text(&["rev-parse", "--show-toplevel"])
        .map_err(|_| CliError::failure("This is not a git checkout.").with_hint("Run it inside the repository."))?;
    Ok(PathBuf::from(top.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_a_git_checkout_the_error_says_so_and_names_the_next_step() {
        let d = tempfile::tempdir().unwrap();
        let e = repo_root_from(d.path(), None).unwrap_err();
        assert!(e.message.contains("This is not a git checkout."), "{}", e.message);
        assert_eq!(e.hint, "Run it inside the repository.");
    }

    #[test]
    fn a_given_folder_is_returned_as_it_is() {
        assert_eq!(repo_root(Some(Path::new("some/where"))).unwrap(), PathBuf::from("some/where"));
    }
}
