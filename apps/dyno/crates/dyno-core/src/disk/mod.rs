//! The disk map's pure core (read-only): volumes, the size index, the scan job's types, the folder-reading port, and the
//! check that a folder path names no `.` or `..` segment.

pub mod index;
pub mod scan;
pub mod settings;
pub mod source;
pub mod volume;

/// Whether `path` has a `.` or `..` segment. `Path::components` reads `..` as a parent step only on ordinary paths: in a Windows
/// verbatim path (`\\?\C:\..`) it is a plain name, so this checks the names too and refuses both forms.
pub fn has_dot_segment(path: &std::path::Path) -> bool {
    use std::path::Component;
    path.components().any(|c| match c {
        Component::ParentDir | Component::CurDir => true,
        Component::Normal(n) => n == ".." || n == ".",
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_segments_are_found_in_any_spelling() {
        use std::path::Path;
        assert!(has_dot_segment(Path::new("/a/../b")));
        assert!(!has_dot_segment(Path::new("/a/b..c/.hidden/..x")));
    }
}
