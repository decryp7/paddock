//! Finding the real tool: the first match for the name along `PATH` that is not the shim itself (the rustup way).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// What identifies the running shim, so it is never found as the "real" tool.
#[derive(Debug, Clone)]
pub struct Own {
    /// The running program's file ([`std::env::current_exe`]).
    pub exe: PathBuf,
    /// The folder the shim was started from when its path was explicit (`<dir>/cargo` and not a bare `cargo`).
    pub invoked_dir: Option<PathBuf>,
}

/// The file names a tool may have in a folder on this OS.
fn names(tool: &str) -> Vec<String> {
    if cfg!(windows) {
        ["exe", "cmd", "bat", "com"]
            .iter()
            .map(|e| format!("{tool}.{e}"))
            .collect()
    } else {
        vec![tool.to_string()]
    }
}

#[cfg(unix)]
fn is_runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_runnable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
}

/// Whether two paths are the same file: the same device and inode on Unix (a link or a hard link to the shim), the same real path or
/// the same size and bytes on Windows (the shim there is a copy).
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(x), Ok(y)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return x.dev() == y.dev() && x.ino() == y.ino();
        }
        false
    }
    #[cfg(not(unix))]
    {
        if paddock_env::winpath::real_or_given(a) == paddock_env::winpath::real_or_given(b) {
            return true;
        }
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) if x.len() == y.len() => {
                matches!((std::fs::read(a), std::fs::read(b)), (Ok(p), Ok(q)) if p == q)
            }
            _ => false,
        }
    }
}

/// The real `tool`: the first runnable file with its name in a `path` folder that is neither the shim's own folder nor the shim itself.
/// `None` when there is none, which the caller says with the next step.
pub fn find_real(tool: &str, path: &OsStr, own: &Own) -> Option<PathBuf> {
    let skip = own
        .invoked_dir
        .as_deref()
        .map(paddock_env::winpath::real_or_given);
    for dir in std::env::split_paths(path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if skip
            .as_deref()
            .is_some_and(|s| paddock_env::winpath::real_or_given(&dir) == s)
        {
            continue;
        }
        for n in names(tool) {
            let cand = dir.join(n);
            if is_runnable(&cand) && !same_file(&cand, &own.exe) {
                return Some(cand);
            }
        }
    }
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn exe_in(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn joined(dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[test]
    fn the_real_tool_is_found_past_the_shims_folder() {
        let t = tempfile::tempdir().unwrap();
        let (shims, real) = (t.path().join("shims"), t.path().join("real"));
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        let me = exe_in(t.path(), "throttle");
        symlink(&me, shims.join("cargo")).unwrap();
        let want = exe_in(&real, "cargo");
        let own = Own {
            exe: me,
            invoked_dir: Some(shims.clone()),
        };
        assert_eq!(
            find_real("cargo", &joined(&[&shims, &real]), &own),
            Some(want)
        );
    }

    #[test]
    fn a_link_to_the_shim_is_never_the_real_tool_even_when_its_folder_is_not_known() {
        let t = tempfile::tempdir().unwrap();
        let (shims, real) = (t.path().join("shims"), t.path().join("real"));
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        let me = exe_in(t.path(), "throttle");
        symlink(&me, shims.join("cargo")).unwrap();
        let want = exe_in(&real, "cargo");
        let own = Own {
            exe: me,
            invoked_dir: None,
        };
        assert_eq!(
            find_real("cargo", &joined(&[&shims, &real]), &own),
            Some(want)
        );
    }

    #[test]
    fn nothing_but_the_shim_on_the_path_finds_nothing() {
        let t = tempfile::tempdir().unwrap();
        let shims = t.path().join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        let me = exe_in(t.path(), "throttle");
        symlink(&me, shims.join("cargo")).unwrap();
        let own = Own {
            exe: me,
            invoked_dir: None,
        };
        assert_eq!(find_real("cargo", &joined(&[&shims]), &own), None);
        // A file that is not executable is not a tool either.
        let plain = t.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("cargo"), "x").unwrap();
        assert_eq!(find_real("cargo", &joined(&[&plain]), &own), None);
    }
}
