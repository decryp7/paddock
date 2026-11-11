//! Which folders have a process inside them: the "not in use" half of `clean` (a hint, never proof; ADR 0083 §1).
//!
//! Linux reads `/proc/<pid>/cwd`; macOS asks `lsof -a -d cwd`. Elsewhere the answer is "not available", and `clean`
//! then keeps every worktree rather than guess.

use std::path::{Path, PathBuf};

/// The current folders of the processes this user can see, or why they cannot be read.
pub fn process_folders() -> Result<Vec<PathBuf>, String> {
    imp::process_folders()
}

/// Some process has its current folder at or below `dir`.
pub fn any_inside(folders: &[PathBuf], dir: &Path) -> bool {
    folders.iter().any(|f| f.starts_with(dir))
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    pub fn process_folders() -> Result<Vec<PathBuf>, String> {
        let entries = std::fs::read_dir("/proc").map_err(|e| format!("cannot read /proc: {e}"))?;
        Ok(entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
            .filter_map(|e| std::fs::read_link(e.path().join("cwd")).ok())
            .collect())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use paddock_proc::{Limits, command, run_bounded};
    use std::path::PathBuf;
    use std::time::Duration;

    pub fn process_folders() -> Result<Vec<PathBuf>, String> {
        let mut cmd = command("lsof");
        cmd.args(["-w", "-a", "-d", "cwd", "-Fn"]);
        let out = run_bounded(cmd, Limits::new(Duration::from_secs(60)).max_output(1 << 26)).map_err(|e| e.to_string())?;
        // lsof exits 1 when nothing matched some filter; any output at all means it ran.
        if out.stdout.is_empty() && !out.success() {
            return Err(format!("lsof printed nothing: {}", out.stderr_text().trim()));
        }
        Ok(out
            .stdout_text()
            .lines()
            .filter_map(|l| l.strip_prefix('n'))
            .map(PathBuf::from)
            .collect())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use std::path::PathBuf;

    pub fn process_folders() -> Result<Vec<PathBuf>, String> {
        Err("reading other processes' folders is not implemented on this OS".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_in_use_when_a_process_is_at_or_below_it() {
        let folders = vec![PathBuf::from("/w/a/src"), PathBuf::from("/w/b")];
        assert!(any_inside(&folders, Path::new("/w/a")));
        assert!(any_inside(&folders, Path::new("/w/b")));
        assert!(!any_inside(&folders, Path::new("/w/ab")));
        assert!(!any_inside(&folders, Path::new("/w/c")));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn this_process_is_found_in_its_own_folder() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        let folders = process_folders().unwrap();
        assert!(any_inside(&folders, &here), "{here:?} not among {} folders", folders.len());
    }
}
