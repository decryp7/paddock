//! Which project a run belongs to, and whether it opted out. Pure: the file system is a closure.

use std::path::{Path, PathBuf};

/// The marker file that turns the shim off for a project (the folder it is in and everything below).
pub const OPT_OUT_FILE: &str = ".paddock-shim-off";

/// The project of a working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The folder name of the project root (the nearest folder with `.git`), else of the working folder, else `unknown`.
    pub name: String,
    /// Whether a [`OPT_OUT_FILE`] was found in the working folder or above it, up to the project root.
    pub opted_out: bool,
}

/// Finds the project of `cwd`; `exists` says whether a path exists (a file or a folder).
pub fn detect(cwd: &Path, exists: &dyn Fn(&Path) -> bool) -> Project {
    let mut opted_out = false;
    let mut root: Option<PathBuf> = None;
    for dir in cwd.ancestors() {
        if exists(&dir.join(OPT_OUT_FILE)) {
            opted_out = true;
        }
        if exists(&dir.join(".git")) {
            root = Some(dir.to_path_buf());
            break;
        }
    }
    let base = root.as_deref().unwrap_or(cwd);
    let name = base
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    Project {
        name: one_word(&name),
        opted_out,
    }
}

/// A name that is safe in a one-line `key=value` record: no spaces, quotes or control characters.
fn one_word(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() || c == '=' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| paths.iter().any(|x| Path::new(x) == p)
    }

    #[test]
    fn the_project_is_the_folder_with_git_found_from_below() {
        let p = detect(Path::new("w/paddock/apps/dyno"), &with(&["w/paddock/.git"]));
        assert_eq!(
            p,
            Project {
                name: "paddock".into(),
                opted_out: false
            }
        );
    }

    #[test]
    fn without_git_the_working_folder_names_the_project() {
        assert_eq!(detect(Path::new("w/scratch"), &with(&[])).name, "scratch");
        assert_eq!(detect(Path::new(""), &with(&[])).name, "unknown");
    }

    #[test]
    fn a_marker_in_or_above_the_folder_up_to_the_root_opts_out() {
        let e = with(&["w/p/.git", "w/p/.paddock-shim-off"]);
        assert!(detect(Path::new("w/p/ui"), &e).opted_out);
        // A marker above the project root belongs to another project.
        let e = with(&["w/p/.git", "w/.paddock-shim-off"]);
        assert!(!detect(Path::new("w/p/ui"), &e).opted_out);
    }

    #[test]
    fn a_name_with_spaces_or_equals_is_one_word() {
        assert_eq!(
            detect(Path::new("w/my proj=x"), &with(&[])).name,
            "my_proj_x"
        );
    }
}
