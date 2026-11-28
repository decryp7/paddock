//! The repository root a command works in: the folder it was given, else the top of the git checkout the current folder is in.
//! One definition for every `--root` option (ER-22); it lives in the core layer because host, repo, ci and gates commands all use it.

use crate::input_hash;
use paddock_cli::CliError;
use std::path::{Path, PathBuf};

/// The repository root: the given one, or git's top level from the current folder.
pub fn repo_root(given: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(r) = given {
        return Ok(r.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let out = input_hash::Exec::run(&input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
    Ok(PathBuf::from(out.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_given_folder_is_returned_as_it_is() {
        assert_eq!(repo_root(Some(Path::new("some/where"))).unwrap(), PathBuf::from("some/where"));
    }
}
