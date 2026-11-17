//! `dev golden confirm <test filter>`: a golden file captured on one machine passes only if the tests also pass on the other kind
//! (ER-21 class "a golden captured on one machine fails on another", train 14 WP-2; conformance C131 looks for the literals, this runs
//! the tests). It runs the filtered tests with `cargo nextest run <filter>` twice, in the same workspace: on this machine, then on
//! Linux as root in the CI image (`tools/linux-container.sh --ci-image`, the container runs as root unless `PADDOCK_LINUX_USER` is
//! set). It passes only when both pass; a run that selects no test fails (nextest exits 4), so a typo cannot pass. Both runs always
//! happen, so one report shows which side failed. The filter is parsed once: letters, digits and `_ : . -` only, so it is safe to
//! put into the container's `sh -c`.

use crate::ci_tool::{Output, ToolCall, error_line, quote};
use crate::wait::parse_duration;
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The longest each of the two runs may take unless `--max` says otherwise.
const DEFAULT_MAX: &str = "40m";

/// The variables the runs are given besides the base environment: the golden mode, the build caps and what `linux-container.sh` reads.
const PASSED_ENV: [&str; 12] = [
    "PADDOCK_UPDATE_GOLDEN",
    "PADDOCK_LINUX_CPUS",
    "PADDOCK_LINUX_JOBS",
    "PADDOCK_LINUX_MEMORY",
    "PADDOCK_LINUX_USER",
    "PADDOCK_LINUX_CI_REGISTRY",
    "PADDOCK_LINUX_HOST_ARCH",
    "PADDOCK_LINUX_BASE_IMAGE",
    "PADDOCK_LINUX_RUN_LABEL",
    "CARGO_BUILD_JOBS",
    "CARGO_TARGET_DIR",
    "DOCKER",
];

/// `dev golden`.
#[derive(Subcommand, Debug)]
pub enum GoldenCommand {
    /// Run the filtered tests on this machine and in the CI image as root; pass only if both pass.
    Confirm(ConfirmArgs),
}

/// Arguments of `dev golden confirm`.
#[derive(Args, Debug)]
pub struct ConfirmArgs {
    /// The test filter, as `cargo nextest run` takes it (a test name or part of one; letters, digits and `_ : . -`).
    pub filter: String,
    /// Run only this package's tests (`cargo nextest run -p <package>`).
    #[arg(short = 'p', long)]
    pub package: Option<String>,
    /// The folder of the workspace the tests are in (default: the current folder).
    #[arg(long)]
    pub manifest_dir: Option<PathBuf>,
    /// The longest each of the two runs may take (for example 40m).
    #[arg(long, default_value = DEFAULT_MAX)]
    pub max: String,
}

/// One run of the filtered tests.
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    /// What it is called in the report: `this machine` or `the CI image as root`.
    pub name: &'static str,
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// The folder it runs in.
    pub cwd: PathBuf,
}

/// A filter of letters, digits and `_ : . -` that does not start with `-` (nextest would read it as an option: `--no-run` runs nothing
/// and passes, `-E` takes an expression), never empty; or why it is not one.
pub fn parse_filter(filter: &str) -> Result<&str, String> {
    if !filter.is_empty()
        && !filter.starts_with('-')
        && filter
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-'))
    {
        Ok(filter)
    } else {
        Err(format!(
            "`{filter}` is not a test filter: write a test name or part of one (letters, digits and _ : . -, not starting with -)"
        ))
    }
}

/// A package name of letters, digits, `_` and `-`, not starting with `-`.
pub fn parse_package(package: &str) -> Result<&str, String> {
    if !package.is_empty() && !package.starts_with('-') && package.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
        Ok(package)
    } else {
        Err(format!(
            "`{package}` is not a package name (letters, digits, _ and -, not starting with -)"
        ))
    }
}

/// The repository root above `dir`: the folder with `ci-image/VERSION` (what `linux-container.sh --ci-image` reads).
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join("ci-image/VERSION").is_file())
        .map(Path::to_path_buf)
}

/// The two runs for `filter` in the workspace `manifest_dir` of the repository at `root`.
pub fn plan(root: &Path, manifest_dir: &Path, package: Option<&str>, filter: &str) -> Result<[Step; 2], String> {
    let filter = parse_filter(filter)?;
    let package = package.map(parse_package).transpose()?;
    let rel = manifest_dir
        .strip_prefix(root)
        .map_err(|_| format!("{} is not inside the repository {}", manifest_dir.display(), root.display()))?;
    let rel = if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().replace('\\', "/")
    };
    let nextest = |filter: &str| {
        let mut v = vec!["nextest".to_string(), "run".to_string()];
        if let Some(p) = package {
            v.extend(["-p".to_string(), p.to_string()]);
        }
        v.push(filter.to_string());
        v
    };
    let in_container = format!(
        "cd {} && {}",
        quote(&rel),
        ["cargo".to_string()]
            .into_iter()
            .chain(nextest(filter))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok([
        Step {
            name: "this machine",
            program: "cargo".into(),
            args: nextest(filter),
            cwd: manifest_dir.to_path_buf(),
        },
        Step {
            name: "the CI image as root",
            program: "bash".into(),
            args: [
                "tools/linux-container.sh",
                "--id",
                "paddock",
                "--ci-image",
                "--root",
                &root.to_string_lossy(),
                "--",
                "sh",
                "-c",
                &in_container,
            ]
            .map(String::from)
            .to_vec(),
            cwd: root.to_path_buf(),
        },
    ])
}

/// What the two runs say together: `Ok` only when both passed.
pub fn verdict(results: &[(&'static str, bool)]) -> Result<(), String> {
    let failed: Vec<&str> = results.iter().filter(|(_, ok)| !ok).map(|(name, _)| *name).collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("the filtered tests failed on {}", failed.join(" and ")))
    }
}

/// `dev golden`.
pub fn command(c: GoldenCommand) -> Result<Report, CliError> {
    let GoldenCommand::Confirm(a) = c;
    let max: Duration = parse_duration(&a.max).map_err(|e| CliError::usage(format!("dev golden confirm --max: {e}")))?;
    let dir = match a.manifest_dir {
        Some(d) => d,
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?,
    };
    let dir = PathBuf::from(
        paddock_env::winpath::real_for_tools(&dir).map_err(|e| CliError::failure(format!("dev golden confirm: {}: {e}", dir.display())))?,
    );
    let root = repo_root(&dir).ok_or_else(|| {
        CliError::failure(error_line(
            &format!(
                "dev golden confirm: {} is not inside a Paddock checkout (no ci-image/VERSION above it).",
                dir.display()
            ),
            "run it from the workspace of the test, or pass --manifest-dir.",
        ))
    })?;
    let steps = plan(&root, &dir, a.package.as_deref(), &a.filter).map_err(|e| CliError::usage(format!("dev golden confirm: {e}")))?;
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let mut results = Vec::new();
    for s in &steps {
        eprintln!(
            "golden confirm: running the filtered tests on {} ({} {})",
            s.name,
            s.program,
            s.args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
        );
        let ran = ToolCall::new(&s.program)
            .args(s.args.iter().cloned())
            .cwd(&s.cwd)
            .timeout(max)
            .output(Output::Stream)
            .pass(PASSED_ENV)
            .run(&env, &mut std::io::stderr());
        let (ok, no_tests) = match &ran {
            Ok(r) => (r.success, r.how == "exit 4"),
            Err(e) => {
                eprintln!("golden confirm: {e}");
                (false, false)
            }
        };
        eprintln!("golden confirm: {}: {}", s.name, if ok { "passed" } else { "FAILED" });
        if no_tests {
            // The same filter selects nothing in the image either: stop, and say what to check.
            return Err(CliError::failure(error_line(
                &format!(
                    "dev golden confirm: no test matched `{}` on {} (nextest exits 4).",
                    a.filter, s.name
                ),
                "check the name, or pass -p <package>.",
            )));
        }
        results.push((s.name, ok));
    }
    verdict(&results).map_err(|why| {
        CliError::failure(error_line(
            &format!("dev golden confirm: {why}."),
            "fix the test's norm() so the golden does not hold a value of one machine (conformance C131 lists such values), or the golden, and run it again.",
        ))
    })?;
    Ok(Report::new(
        json!({ "ok": true, "filter": a.filter, "ran": results.iter().map(|(n, _)| *n).collect::<Vec<_>>() }),
        "golden confirm: the filtered tests pass on this machine and in the CI image as root",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_is_a_test_name_never_shell_text() {
        for ok in ["golden", "a::b::c_1", "x-y.z"] {
            assert_eq!(parse_filter(ok), Ok(ok));
        }
        for bad in ["", "a b", "a;b", "a'b", "$(x)", "a|b", "a/b", "--no-run", "-E", "-x"] {
            assert!(parse_filter(bad).unwrap_err().contains("not a test filter"), "{bad}");
        }
    }

    #[test]
    fn two_runs_the_same_filter_in_the_same_workspace_one_in_the_ci_image() {
        let [host, linux] = plan(Path::new("/r"), Path::new("/r/tools/paddock-dev"), None, "check::c131").unwrap();
        assert_eq!(
            (host.program.as_str(), host.args.join(" ")),
            ("cargo", "nextest run check::c131".to_string())
        );
        assert_eq!(host.cwd, Path::new("/r/tools/paddock-dev"));
        assert_eq!(linux.cwd, Path::new("/r"));
        let line = linux.args.join(" ");
        assert!(
            line.starts_with("tools/linux-container.sh --id paddock --ci-image --root /r -- sh -c"),
            "{line}"
        );
        assert!(line.ends_with("cd tools/paddock-dev && cargo nextest run check::c131"), "{line}");
        assert_eq!(
            plan(Path::new("/r"), Path::new("/r"), None, "t").unwrap()[1].args.last().unwrap(),
            "cd . && cargo nextest run t"
        );
        assert!(
            plan(Path::new("/r"), Path::new("/elsewhere"), None, "t")
                .unwrap_err()
                .contains("not inside")
        );
    }

    #[test]
    fn a_package_and_a_quote_in_the_folder_are_passed_safely() {
        let [host, linux] = plan(Path::new("/r"), Path::new("/r/it's here"), Some("paddock-dev"), "t").unwrap();
        assert_eq!(host.args.join(" "), "nextest run -p paddock-dev t");
        assert!(
            linux
                .args
                .last()
                .unwrap()
                .ends_with("cd 'it'\\''s here' && cargo nextest run -p paddock-dev t"),
            "{:?}",
            linux.args
        );
        assert!(plan(Path::new("/r"), Path::new("/r"), Some("-x"), "t").is_err());
    }

    #[test]
    fn it_passes_only_when_both_runs_pass() {
        assert!(verdict(&[("this machine", true), ("the CI image as root", true)]).is_ok());
        assert!(
            verdict(&[("this machine", true), ("the CI image as root", false)])
                .unwrap_err()
                .contains("the CI image as root")
        );
        assert!(
            verdict(&[("this machine", false), ("the CI image as root", true)])
                .unwrap_err()
                .contains("this machine")
        );
        assert!(
            verdict(&[("this machine", false), ("the CI image as root", false)])
                .unwrap_err()
                .contains("this machine and the CI image as root")
        );
    }
}
