//! Where cargo puts a program's neighbours: the profile folder (`<target>/<profile>`) above a test program or a build
//! script's `OUT_DIR`, in both build-folder layouts.
//!
//! - Cargo before 1.100: a test program runs from `<profile>/deps/`, an `OUT_DIR` is `<profile>/build/<package>-<hash>/out`.
//! - Cargo 1.100 and later (`build.build-dir-new-layout`, always on): both are `<profile>/build/<package>/<hash>/out/`.
//!
//! The binaries and examples a test needs (`<profile>/<name>`, `<profile>/examples/<name>`) are in the profile folder in
//! both. A guess of "two folders up" is right in one layout and wrong in the other; it sent Debrief's tests looking for
//! `debrief-plugins` inside `<profile>/build/debrief-app/<hash>/out` (CI run 8028, 223 macOS tests) and Pitcrew's 16
//! supervision tests into their own `cargo build` (run 8027). These functions read both shapes and return `None` for any
//! other, so the caller fails loudly instead of looking in a wrong folder. They are pure (no file system access).
//!
//! Tests use `paddock_testkit::cargo_layout`, which adds the lookups from `current_exe()`.

use std::path::{Path, PathBuf};

/// The profile folder for `dir`, the folder a test program or build script runs from (`current_exe()`'s parent, or
/// `OUT_DIR`): `<profile>/deps`, `<profile>/build/<package>/<hash>/out` or the old `<profile>/build/<package>-<hash>/out`.
/// `None` for any other shape. A package called `build` or `deps` does not confuse it.
pub fn profile_dir_of_dir(dir: &Path) -> Option<PathBuf> {
    let name = dir.file_name()?;
    if name == "deps" {
        return Some(dir.parent()?.to_path_buf());
    }
    if name != "out" {
        return None;
    }
    // Cargo 1.100: out <- hash <- package <- build <- profile.
    let new_build = dir.parent()?.parent()?.parent()?;
    if new_build.file_name()? == "build" {
        return new_build.parent().map(Path::to_path_buf);
    }
    // Cargo before 1.100: out <- package-hash <- build <- profile.
    let old_build = dir.parent()?.parent()?;
    (old_build.file_name()? == "build").then(|| old_build.parent().map(Path::to_path_buf))?
}

/// The profile folder for a test program at `exe` (`current_exe()` of a test): [`profile_dir_of_dir`] of its folder.
pub fn profile_dir_of_exe(exe: &Path) -> Option<PathBuf> {
    profile_dir_of_dir(exe.parent()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/w/target";

    fn at(rel: &str) -> PathBuf {
        Path::new(ROOT).join(rel)
    }

    #[test]
    fn a_test_program_is_found_in_the_layout_before_cargo_1_100() {
        assert_eq!(
            profile_dir_of_exe(&at("debug/deps/t-0123abcd")),
            Some(at("debug"))
        );
    }

    #[test]
    fn a_test_program_is_found_in_the_layout_of_cargo_1_100() {
        assert_eq!(
            profile_dir_of_exe(&at(
                "debug/build/debrief-app/2cb3f58e32e5ee48/out/t-2cb3f58e32e5ee48"
            )),
            Some(at("debug"))
        );
        assert_eq!(
            profile_dir_of_dir(&at("release/build/p/0123abcd/out")),
            Some(at("release"))
        );
    }

    #[test]
    fn an_out_dir_is_found_in_the_old_layout_too() {
        assert_eq!(
            profile_dir_of_dir(&at("debug/build/app-1a2b/out")),
            Some(at("debug"))
        );
    }

    #[test]
    fn a_package_called_build_or_deps_does_not_confuse_it() {
        assert_eq!(
            profile_dir_of_dir(&at("ci/build/build/0123abcd/out")),
            Some(at("ci"))
        );
        assert_eq!(
            profile_dir_of_dir(&at("ci/build/deps/0123abcd/out")),
            Some(at("ci"))
        );
    }

    #[test]
    fn any_other_shape_is_refused() {
        for p in [
            "/usr/local/bin/t",
            "/w/target/debug/t",
            "/w/target/debug/build/x/y/z/out/t",
            "t",
        ] {
            assert_eq!(profile_dir_of_exe(Path::new(p)), None, "{p}");
        }
        assert_eq!(profile_dir_of_dir(Path::new("")), None);
        assert_eq!(profile_dir_of_dir(Path::new("/out")), None);
    }
}
