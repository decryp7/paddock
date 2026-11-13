//! `dev ci build-folder <workspace> <name>`: a job's Rust build folder, set as cargo configuration and never with
//! `CARGO_TARGET_DIR` (sccache puts every `CARGO_*` variable into its cache key; Paddock lessons, "Test and build speed"). The
//! port of `conformance/ci/build-folder.sh`.
//!
//! Linux containers build in the workspace's `target/` (a new container every run; sccache keeps the compiled crates) and any
//! old `.cargo/ci.toml` is removed. Host runners (macOS, Windows) build in a folder that outlives the job, outside the checkout:
//! the kept folder `target` of the job's slot (`ci isolation path target <name>`: `<ci-home>/target-<name>-<slot>`), so two jobs on
//! one host never share one. It is written to `<workspace>/.cargo/ci.toml` (git-ignored, included by the workspace's
//! `.cargo/config.toml` as `include = [{ path = "ci.toml", optional = true }]`), so every cargo command (clippy, nextest, doc
//! tests, `tauri build`, cargo metadata) uses it with no flag and no variable. `PADDOCK_BUILD_FOLDER` (the folder, for the steps
//! that collect from it) goes to `$GITHUB_ENV`.

use super::Step;
use super::safe_remove;
use clap::Args as ClapArgs;
use paddock_env::{Os, winpath};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `dev ci build-folder`.
#[derive(ClapArgs, Debug)]
pub struct BuildFolderArgs {
    /// The Cargo workspace folder.
    pub workspace: PathBuf,
    /// The kept folder's name on a host runner, one per app and job (`<app>-<job>`); the job's slot is appended.
    pub name: String,
}

/// The short name the CLI uses.
pub type Args = BuildFolderArgs;

/// Whether the workspace's `.cargo/config.toml` includes the job's `ci.toml` (the script's `grep -Eq`).
pub fn includes_ci_toml(config: &str) -> bool {
    // The pattern is a constant, so it always compiles; a failure would read as "does not include", which fails the step safely.
    regex::Regex::new(r#"(?m)^include = \[.*\{ *path *= *"ci\.toml", *optional *= *true *\}"#).is_ok_and(|re| re.is_match(config))
}

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, windows: bool, args: &Args, out: &mut dyn Write) -> i32 {
    run_with(env, windows, args, out, &|p| winpath::real_verbatim(p), Os::current())
}

/// The command over an explicit `canonicalize` and OS (tests fake a verbatim `\\?\C:\...` answer on any OS).
pub fn run_with(
    env: &HashMap<String, String>,
    windows: bool,
    args: &Args,
    out: &mut dyn Write,
    canon: &dyn Fn(&Path) -> std::io::Result<PathBuf>,
    os_kind: Os,
) -> i32 {
    let mut step = Step {
        env,
        out,
        windows,
        name: "build-folder",
    };
    if args.name.is_empty()
        || !args
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return step.fail(
            format!("the name '{}' may only hold letters, digits, '.', '_' and '-'", args.name),
            "pass <app>-<job> as the name.",
        );
    }
    let Some(os) = step.var("RUNNER_OS").map(str::to_string) else {
        return step.fail("RUNNER_OS is not set.", "run this step in a runner job.");
    };
    if step.var("GITHUB_ENV").is_none() {
        return step.fail("GITHUB_ENV is not set.", "run this step in a runner job.");
    }
    // The folders to read and write are the workspace as given, made absolute; the text that goes into ci.toml and $GITHUB_ENV is
    // the canonical path for programs: plain, with `/` separators (paddock_env::winpath, never the verbatim `\\?\` form).
    let ws = match std::path::absolute(&args.workspace) {
        Ok(p) => p,
        Err(e) => {
            return step.fail(
                format!("the Cargo workspace {} cannot be read: {e}", args.workspace.display()),
                "pass the workspace folder.",
            );
        }
    };
    let ws_text = match canon(&args.workspace) {
        Ok(c) => match winpath::for_tools(&c, os_kind) {
            Ok(t) => t,
            Err(e) => return step.fail(e.to_string(), "use a workspace folder with a plain path."),
        },
        Err(e) => {
            return step.fail(
                format!("the Cargo workspace {} cannot be resolved: {e}", args.workspace.display()),
                "pass the workspace folder.",
            );
        }
    };
    let config = ws.join(".cargo").join("config.toml");
    if !std::fs::read_to_string(&config).is_ok_and(|t| includes_ci_toml(&t)) {
        return step.fail(
            format!(
                "{} must include the job's build folder: include = [{{ path = \"ci.toml\", optional = true }}]",
                config.display()
            ),
            "add that line to the workspace's .cargo/config.toml.",
        );
    }
    let ci_toml = ws.join(".cargo").join("ci.toml");
    let dir: String;
    if os == "Linux" {
        let home_dir = step.var("HOME").map(PathBuf::from);
        if let Err(e) = safe_remove::remove_under(&ws.join(".cargo"), &ci_toml, home_dir.as_deref()) {
            return step.fail(format!("cannot remove the old {}: {e}", ci_toml.display()), "remove it by hand.");
        }
        dir = format!("{ws_text}/target");
    } else {
        let kept = match step.isolation(&["path", "target", &args.name]) {
            Ok(p) => p,
            Err(e) => {
                step.say(e);
                return 1;
            }
        };
        // Cargo on Windows wants a native path; forward slashes work in cargo, Node and Git bash alike.
        dir = if windows { kept.replace('\\', "/") } else { kept };
        if dir.contains('\'') {
            return step.fail(
                format!("the build folder {dir} has a quote in its path"),
                "use a CI home without a quote in its path.",
            );
        }
        let text = format!("# Written by `./dev ci build-folder` for this CI job (git-ignored).\n[build]\ntarget-dir = '{dir}'\n");
        if let Err(e) = write_file(&ci_toml, &text) {
            return step.fail(
                format!("cannot write {}: {e}", ci_toml.display()),
                "check the workspace's permissions.",
            );
        }
    }
    if let Err(e) = step.append_to("GITHUB_ENV", &format!("PADDOCK_BUILD_FOLDER={dir}")) {
        return step.fail(e, "run this step in a runner job.");
    }
    step.say(format!("build folder: {dir}"));
    0
}

fn write_file(p: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(p, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(include: bool) -> (tempfile::TempDir, PathBuf) {
        let d = crate::ci_host::canonical_tempdir();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(ws.join(".cargo")).unwrap();
        std::fs::write(
            ws.join(".cargo/config.toml"),
            if include {
                "include = [{ path = \"ci.toml\", optional = true }]\n"
            } else {
                "[alias]\n"
            },
        )
        .unwrap();
        std::fs::write(ws.join(".cargo/ci.toml"), "# left by an earlier job\n").unwrap();
        std::fs::create_dir_all(d.path().join("home")).unwrap();
        (d, ws)
    }

    fn go(d: &Path, ws: &Path, os: &str, name: &str, slot: &str) -> (i32, String, String, Option<String>) {
        let mut env: HashMap<String, String> = [
            ("RUNNER_OS", os.to_string()),
            ("HOME", d.join("home").display().to_string()),
            ("GITHUB_ENV", d.join("github-env").display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        if !slot.is_empty() {
            env.insert("PADDOCK_CI_SLOT".into(), slot.into());
        }
        let mut out = Vec::new();
        let code = run(
            &env,
            false,
            &Args {
                workspace: ws.to_path_buf(),
                name: name.into(),
            },
            &mut out,
        );
        let written = std::fs::read_to_string(d.join("github-env")).unwrap_or_default();
        let _ = std::fs::remove_file(d.join("github-env"));
        (
            code,
            String::from_utf8(out).unwrap(),
            written,
            std::fs::read_to_string(ws.join(".cargo/ci.toml")).ok(),
        )
    }

    #[test]
    fn a_host_job_builds_in_the_kept_folder_of_its_slot_through_the_included_ci_toml() {
        let (d, ws) = fixture(true);
        let (code, out, env, ci) = go(d.path(), &ws, "macOS", "spotter-rust", "s1");
        assert_eq!(code, 0, "{out}");
        let kept = format!(
            "{}/.paddock-ci/target-spotter-rust-s1",
            paddock_testkit::path::text(&d.path().join("home"))
        );
        let canon = paddock_env::winpath::real(&d.path().join("home")).unwrap();
        let kept = if Path::new(&kept).exists() {
            kept
        } else {
            format!("{}/.paddock-ci/target-spotter-rust-s1", paddock_testkit::path::text(&canon))
        };
        assert_eq!(
            ci.unwrap().split('\n').skip(1).collect::<Vec<_>>().join("\n"),
            format!("[build]\ntarget-dir = '{kept}'\n")
        );
        assert_eq!(env, format!("PADDOCK_BUILD_FOLDER={kept}\n"));
        assert!(Path::new(&kept).is_dir());
        assert_eq!(out, format!("build folder: {kept}\n"));
        let (_, _, env2, _) = go(d.path(), &ws, "macOS", "spotter-rust", "s2");
        assert!(
            env2.ends_with("/.paddock-ci/target-spotter-rust-s2\n"),
            "a concurrent job builds in its own slot: {env2}"
        );
    }

    #[test]
    fn a_container_builds_in_target_and_an_old_ci_toml_is_removed() {
        let (d, ws) = fixture(true);
        let (code, out, env, ci) = go(d.path(), &ws, "Linux", "x", "");
        assert_eq!(code, 0, "{out}");
        assert_eq!(ci, None);
        let canon = winpath::real_for_tools(&ws).unwrap();
        assert_eq!(env, format!("PADDOCK_BUILD_FOLDER={canon}/target\n"));
    }

    #[test]
    fn a_config_that_does_not_include_ci_toml_and_bad_names_and_a_missing_slot_fail() {
        let (d, ws) = fixture(false);
        let (code, out, _, _) = go(d.path(), &ws, "macOS", "x", "s1");
        assert_eq!(code, 1);
        assert!(
            out.contains("::error::build-folder:") && out.contains("must include the job's build folder") && out.contains("Next step:"),
            "{out}"
        );
        let (d, ws) = fixture(true);
        assert_eq!(go(d.path(), &ws, "macOS", "a b", "s1").0, 1);
        let (code, out, _, _) = go(d.path(), &ws, "macOS", "x", "");
        assert_eq!(code, 1, "a host job without a slot could share a folder");
        assert!(out.contains("::error::job-isolation:"), "{out}");
    }

    #[test]
    fn the_include_line_is_found_only_as_the_scripts_pattern_has_it() {
        assert!(includes_ci_toml("[build]\ninclude = [{ path = \"ci.toml\", optional = true }]\n"));
        assert!(includes_ci_toml("include = [{path=\"ci.toml\",optional=true}]"));
        assert!(!includes_ci_toml("# include = [{ path = \"ci.toml\", optional = true }]"));
        assert!(!includes_ci_toml("include = [{ path = \"other.toml\", optional = true }]"));
    }

    #[test]
    fn a_verbatim_canonical_path_never_reaches_the_build_folder_text() {
        // Windows' `canonicalize` answers `\\?\C:\...`; any OS fakes it (a container leg builds in `<workspace>/target`).
        let (d, ws) = fixture(true);
        let env: HashMap<String, String> = [
            ("RUNNER_OS", "Linux".to_string()),
            ("HOME", d.path().join("home").display().to_string()),
            ("GITHUB_ENV", d.path().join("github-env").display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let mut out = Vec::new();
        let args = Args {
            workspace: ws,
            name: "x".into(),
        };
        let code = run_with(&env, true, &args, &mut out, &|_| Ok(PathBuf::from(r"\\?\C:\ci\ws")), Os::Windows);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            std::fs::read_to_string(d.path().join("github-env")).unwrap(),
            "PADDOCK_BUILD_FOLDER=C:/ci/ws/target\n"
        );
        assert_eq!(out, "build folder: C:/ci/ws/target\n");
    }

    #[test]
    fn a_workspace_whose_canonical_path_has_no_plain_spelling_is_one_error_line() {
        let (d, ws) = fixture(true);
        let env: HashMap<String, String> = [
            ("RUNNER_OS", "Linux".to_string()),
            ("GITHUB_ENV", d.path().join("e").display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let mut out = Vec::new();
        let args = Args {
            workspace: ws,
            name: "x".into(),
        };
        let code = run_with(
            &env,
            true,
            &args,
            &mut out,
            &|_| Ok(PathBuf::from(r"\\?\Volume{1234}\ws")),
            Os::Windows,
        );
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 1);
        assert_eq!(out.lines().filter(|l| l.starts_with("::error::build-folder:")).count(), 1, "{out}");
        assert!(out.contains("Next step:"), "{out}");
    }
}
