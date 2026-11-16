//! How a build folder is handed to cargo: the one accessor (ER-42).
//!
//! The folder goes in as cargo configuration (`--config build.target-dir='<dir>'`), never as `CARGO_TARGET_DIR` or
//! `CARGO_BUILD_TARGET_DIR`: sccache 0.19 puts every `CARGO_*` variable into its cache key, so a per-worktree folder in the
//! environment gives 0% hits across worktrees, while the same folder as configuration gives 82% (docs/research/sccache-basedirs-2026-11-16.md,
//! docs/lessons.md "Test and build speed"). The folder is the same one as before; only how it is passed changes (ADR 0014 §1).

use std::path::Path;

/// The value of `--config`: `build.target-dir='<dir>'` (a TOML literal string, so Windows backslashes need no escape; a folder
/// name with a quote in it becomes a basic string with the escapes TOML needs).
pub fn config_value(dir: &str) -> String {
    if dir.contains('\'') || dir.contains(['\n', '\r']) {
        let escaped: String = dir
            .chars()
            .flat_map(|c| match c {
                '\\' => vec!['\\', '\\'],
                '"' => vec!['\\', '"'],
                '\n' => vec!['\\', 'n'],
                '\r' => vec!['\\', 'r'],
                c => vec![c],
            })
            .collect();
        format!("build.target-dir=\"{escaped}\"")
    } else {
        format!("build.target-dir='{dir}'")
    }
}

/// Whether `program` is cargo itself (a path or a bare name), not a script that may start cargo.
pub fn is_cargo(program: &str) -> bool {
    let name = Path::new(program).file_name().and_then(|n| n.to_str()).unwrap_or(program);
    name == "cargo" || name == "cargo.exe"
}

/// Where the cargo subcommand's own words end: after an optional `+toolchain`, the subcommand, and for `cargo xwin` the cargo
/// command it wraps. `--config` goes right there, before any `--`.
fn subcommand_end(args: &[String]) -> usize {
    let mut i = usize::from(args.first().is_some_and(|a| a.starts_with('+')));
    if args.get(i).is_some_and(|a| !a.starts_with('-')) {
        i += 1;
        if args.get(i - 1).is_some_and(|a| a == "xwin") && args.get(i).is_some_and(|a| !a.starts_with('-')) {
            i += 1;
        }
    }
    i
}

/// `args` (of a cargo call, without the program) with the build folder passed as configuration, once: arguments that already
/// name `build.target-dir` are left as they are.
pub fn with_target_dir(args: &[String], dir: &str) -> Vec<String> {
    if names_target_dir(args) {
        return args.to_vec();
    }
    let at = subcommand_end(args);
    let mut out: Vec<String> = args[..at].to_vec();
    out.push("--config".into());
    out.push(config_value(dir));
    out.extend(args[at..].iter().cloned());
    out
}

/// Whether the arguments already pass `build.target-dir` (or `--target-dir`).
pub fn names_target_dir(args: &[String]) -> bool {
    args.iter()
        .any(|a| a.starts_with("build.target-dir") || a == "--target-dir" || a.starts_with("--target-dir="))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_folder_is_passed_as_a_config_argument_after_the_subcommand_and_never_as_a_variable() {
        let got = with_target_dir(&v(&["clippy", "--workspace", "--", "-D", "warnings"]), "/t/crates");
        assert_eq!(
            got,
            v(&[
                "clippy",
                "--config",
                "build.target-dir='/t/crates'",
                "--workspace",
                "--",
                "-D",
                "warnings"
            ])
        );
        assert!(
            got.iter()
                .all(|a| !a.contains("CARGO_TARGET_DIR") && !a.contains("CARGO_BUILD_TARGET_DIR"))
        );
    }

    #[test]
    fn cargo_xwin_and_a_toolchain_prefix_keep_their_words_before_the_config() {
        let got = with_target_dir(&v(&["xwin", "clippy", "--target", "x", "--", "-D", "warnings"]), "/t/x");
        assert_eq!(&got[..4], &v(&["xwin", "clippy", "--config", "build.target-dir='/t/x'"])[..]);
        let got = with_target_dir(&v(&["+nightly", "build"]), "/t/x");
        assert_eq!(got, v(&["+nightly", "build", "--config", "build.target-dir='/t/x'"]));
    }

    #[test]
    fn a_call_that_already_names_its_folder_is_left_alone() {
        let a = v(&["build", "--target-dir", "/mine"]);
        assert_eq!(with_target_dir(&a, "/t"), a);
    }

    #[test]
    fn a_quote_or_backslash_in_the_folder_is_escaped_not_spliced() {
        assert_eq!(config_value(r"C:\work\t"), r"build.target-dir='C:\work\t'");
        assert_eq!(config_value("/a'b"), "build.target-dir=\"/a'b\"");
        assert_eq!(config_value("/a\"b'"), "build.target-dir=\"/a\\\"b'\"");
    }

    #[test]
    fn only_cargo_itself_takes_the_config() {
        assert!(is_cargo("cargo") && is_cargo("/x/bin/cargo") && is_cargo("cargo.exe"));
        assert!(!is_cargo("sh") && !is_cargo("npm") && !is_cargo("cargo-xwin"));
    }
}
