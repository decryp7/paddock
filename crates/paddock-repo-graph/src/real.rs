//! A repository root in the one spelling cargo reports its paths in.

use std::path::{Path, PathBuf};

/// `path` with symbolic links, junctions, 8.3 short names (`RUNNER~1`) and letter case resolved, and without Windows' `\\?\`
/// prefix; `path` itself when it cannot be resolved (it does not exist). `cargo metadata` prints the paths of the manifests
/// it reads, and the graph subtracts the repository root from them as text, so both must be spelled alike.
pub fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).map_or_else(|_| path.to_path_buf(), |p| strip_verbatim(&p))
}

/// Windows' extended-length prefix removed: `\\?\C:\a` is `C:\a` and `\\?\UNC\host\share` is `\\host\share`.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extended_length_prefix_is_removed_and_other_paths_are_kept() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\Users\x")),
            Path::new(r"C:\Users\x")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\host\share\x")),
            Path::new(r"\\host\share\x")
        );
        assert_eq!(strip_verbatim(Path::new("/a/b")), Path::new("/a/b"));
    }

    #[test]
    fn a_path_through_a_dot_dot_is_resolved_and_a_missing_one_is_kept() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("a")).unwrap();
        let dotted = d.path().join("a").join("..").join("a");
        assert_eq!(real_path(&dotted), real_path(&d.path().join("a")));
        assert!(!real_path(&dotted).to_string_lossy().contains(".."));
        let missing = d.path().join("nope");
        assert_eq!(real_path(&missing), missing);
    }
}
