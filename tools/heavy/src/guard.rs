//! The home guard (ER-14, from heavy.sh): a command whose build folder would land inside the home folder is refused (exit 2,
//! nothing run, no slot taken). The folder comes from `CARGO_TARGET_DIR`, `--target-dir <d>`, `--target-dir=<d>` and
//! `--config build.target-dir=<d>`. Relative paths resolve against the current folder; symlinks of existing folders are followed.

use std::path::{Component, Path, PathBuf};

/// The absolute, symlink-resolved form of `p`: the deepest existing ancestor is canonicalised, the rest appended as written.
pub fn abs_path(p: &str, home: Option<&Path>, cwd: &Path) -> PathBuf {
    let mut path = if p == "~" {
        home.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(p))
    } else if let (Some(rest), Some(h)) = (p.strip_prefix("~/"), home) {
        h.join(rest)
    } else {
        PathBuf::from(p)
    };
    if path.is_relative() {
        path = cwd.join(path);
    }
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut base = path.clone();
    while !base.is_dir() {
        match (base.file_name().map(ToOwned::to_owned), base.parent().map(Path::to_path_buf)) {
            (Some(n), Some(parent)) => {
                rest.push(n);
                base = parent;
            }
            _ => break,
        }
    }
    let mut out = std::fs::canonicalize(&base).unwrap_or(base);
    for n in rest.iter().rev() {
        out.push(n);
    }
    // Fold `..` and `.` that remain in the appended part, lexically.
    let mut norm = PathBuf::new();
    for c in out.components() {
        match c {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            other => norm.push(other.as_os_str()),
        }
    }
    norm
}

/// The build folders named by the environment value and the arguments.
pub fn build_dirs(cargo_target_dir: Option<&str>, args: &[String]) -> Vec<String> {
    let mut dirs = Vec::new();
    if let Some(v) = cargo_target_dir.filter(|v| !v.is_empty()) {
        dirs.push(v.to_string());
    }
    let mut prev = "";
    for a in args {
        if let Some(v) = a.strip_prefix("--target-dir=") {
            dirs.push(v.to_string());
        } else if let Some(v) = a.strip_prefix("build.target-dir=") {
            if prev == "--config" {
                dirs.push(v.to_string());
            }
        } else if prev == "--target-dir" {
            dirs.push(a.clone());
        }
        prev = a;
    }
    dirs
}

/// The first build folder inside `home`, if any.
pub fn violation(dirs: &[String], home: Option<&Path>, cwd: &Path) -> Option<String> {
    let home = home?;
    let h = abs_path(&home.to_string_lossy(), Some(home), cwd);
    dirs.iter().find_map(|v| {
        let v = v.strip_prefix(['"', '\'']).unwrap_or(v);
        let v = v.strip_suffix(['"', '\'']).unwrap_or(v);
        let d = abs_path(v, Some(home), cwd);
        d.starts_with(&h).then(|| v.to_string())
    })
}
