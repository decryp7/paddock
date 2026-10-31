//! The external programs `dev notices` starts, behind a port so the command is testable without cargo or npm, and the steps that
//! use them: the pinned `cargo about` install, `npm ci`, and one `cargo about generate` per manifest. Every call is bounded.

use super::error::NoticesError;
use paddock_proc::{Limits, Outcome, command as proc_command, run_bounded};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// The cargo-about release the committed notices are made with: a new release can find more licence texts, so every machine and CI use this one.
pub const CARGO_ABOUT_VERSION: &str = "0.9.2";
/// One `cargo about generate` run (the slow step: it resolves every crate's licence files; 113 s in CI on 2026-10-27).
pub const ABOUT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// `cargo install cargo-about` compiles it.
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// `npm ci`.
pub const NPM_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// A quick query (`cargo about --version`).
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
/// Most bytes kept of a program's output; a `cargo about` document over it is an error, not a truncated document.
pub const MAX_OUTPUT: usize = 256 << 20;

/// What a program that ran printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRun {
    /// Exit status 0.
    pub success: bool,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

/// Runs programs: the real one goes through `paddock-proc` (timeouts, output caps, the whole tree ended).
pub trait Tools {
    /// Runs `program args...` in `cwd`; Err names why it could not run, timed out or wrote too much.
    fn run(&self, program: &str, args: &[&str], cwd: &Path, timeout: Duration) -> Result<ToolRun, String>;
}

/// The real thing.
pub struct Real;

impl Tools for Real {
    fn run(&self, program: &str, args: &[&str], cwd: &Path, timeout: Duration) -> Result<ToolRun, String> {
        let mut cmd = proc_command(program);
        cmd.args(args).current_dir(cwd);
        let out = run_bounded(cmd, Limits::new(timeout).max_output(MAX_OUTPUT).stop_on_cap()).map_err(|e| e.to_string())?;
        match out.outcome {
            Outcome::TimedOut => Err(format!("it did not finish within {} minutes", timeout.as_secs() / 60)),
            Outcome::OutputCap => Err(format!("it wrote more than {MAX_OUTPUT} bytes")),
            Outcome::Exited(_) => Ok(ToolRun {
                success: out.success(),
                stdout: out.stdout_text(),
                stderr: out.stderr_text(),
            }),
        }
    }
}

fn tool_error(command: &str, why: impl Into<String>) -> NoticesError {
    NoticesError::Tool {
        command: command.to_string(),
        why: why.into(),
    }
}

fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text.trim().lines().collect();
    lines[lines.len().saturating_sub(3)..].join(" ")
}

/// Installs the pinned cargo-about unless exactly that version is installed (any other installed version is replaced).
pub fn ensure_cargo_about(tools: &dyn Tools, cwd: &Path) -> Result<(), NoticesError> {
    let installed = tools
        .run("cargo", &["about", "--version"], cwd, QUERY_TIMEOUT)
        .ok()
        .filter(|r| r.success)
        .and_then(|r| r.stdout.split_whitespace().nth(1).map(String::from));
    if installed.as_deref() == Some(CARGO_ABOUT_VERSION) {
        return Ok(());
    }
    let args = [
        "install",
        "cargo-about",
        "--version",
        CARGO_ABOUT_VERSION,
        "--locked",
        "--force",
        "--features",
        "cli",
    ];
    let line = format!("cargo {}", args.join(" "));
    match tools.run("cargo", &args, cwd, INSTALL_TIMEOUT) {
        Ok(r) if r.success => Ok(()),
        Ok(r) => Err(tool_error(&line, last_lines(&r.stderr))),
        Err(why) => Err(tool_error(&line, why)),
    }
}

/// Installs the npm packages from the root lock when there is no `node_modules` yet (`npm ci --ignore-scripts`).
pub fn ensure_node_modules(tools: &dyn Tools, root: &Path) -> Result<(), NoticesError> {
    if root.join("node_modules").is_dir() {
        return Ok(());
    }
    match tools.run("npm", &["ci", "--ignore-scripts"], root, NPM_TIMEOUT) {
        Ok(r) if r.success => Ok(()),
        Ok(r) => Err(tool_error("npm ci --ignore-scripts", last_lines(&r.stderr))),
        Err(why) => Err(tool_error("npm ci --ignore-scripts", why)),
    }
}

/// Runs `cargo about generate` for each manifest and returns the parsed JSON documents.
pub fn cargo_about(tools: &dyn Tools, manifests: &[&Path], about_toml: &Path, cwd: &Path) -> Result<Vec<Value>, NoticesError> {
    manifests
        .iter()
        .map(|manifest| {
            let (about, manifest) = (about_toml.to_string_lossy(), manifest.to_string_lossy());
            let args = ["about", "generate", "--locked", "-c", &about, "--format", "json", "--manifest-path", &manifest];
            let line = format!("cargo about generate for {manifest}");
            let run = tools.run("cargo", &args, cwd, ABOUT_TIMEOUT).map_err(|why| {
                if why.contains("not found") || why.contains("not installed") {
                    tool_error(&line, "cargo is not installed")
                } else {
                    tool_error(&line, why)
                }
            })?;
            if !run.success {
                let lower = run.stderr.to_lowercase();
                if lower.contains("no such command") || lower.contains("not found") {
                    return Err(tool_error(&line, format!("cargo-about is not installed: cargo install cargo-about --version {CARGO_ABOUT_VERSION} --locked --features cli")));
                }
                return Err(tool_error(&line, last_lines(&run.stderr)));
            }
            serde_json::from_str(&run.stdout).map_err(|e| tool_error(&line, format!("its output is not valid JSON: {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::{FakeRunner, Reply};

    struct Fake(FakeRunner);
    impl Tools for Fake {
        fn run(&self, program: &str, args: &[&str], _cwd: &Path, _timeout: Duration) -> Result<ToolRun, String> {
            match self.0.run(program, args) {
                Reply::Ran { success, stdout, stderr } => Ok(ToolRun { success, stdout, stderr }),
                Reply::Failed(m) => Err(m),
            }
        }
    }

    #[test]
    fn the_pinned_version_is_not_reinstalled_any_other_is() {
        let runner = FakeRunner::new();
        runner.on("cargo", &["about", "--version"]).reply(Reply::ok("cargo-about 0.9.2\n"));
        let tools = Fake(runner.clone());
        ensure_cargo_about(&tools, Path::new(".")).unwrap();
        assert_eq!(runner.lines(), ["cargo about --version"]);

        let runner = FakeRunner::new();
        runner.on("cargo", &["about", "--version"]).reply(Reply::ok("cargo-about 0.8.0\n"));
        runner.on("cargo", &["install"]).reply(Reply::ok(""));
        ensure_cargo_about(&Fake(runner.clone()), Path::new(".")).unwrap();
        assert_eq!(
            runner.lines()[1],
            "cargo install cargo-about --version 0.9.2 --locked --force --features cli"
        );

        let runner = FakeRunner::new();
        runner
            .on("cargo", &["about", "--version"])
            .reply(Reply::fails("error: no such command: `about`"));
        runner
            .on("cargo", &["install"])
            .reply(Reply::fails("a\nb\nc\nd\nerror: linker failed"));
        let err = ensure_cargo_about(&Fake(runner), Path::new(".")).unwrap_err();
        assert!(err.to_string().ends_with("failed: c d error: linker failed"), "{err}");
    }

    #[test]
    fn npm_ci_runs_only_without_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::new();
        runner.on("npm", &["ci", "--ignore-scripts"]).reply(Reply::ok(""));
        ensure_node_modules(&Fake(runner.clone()), dir.path()).unwrap();
        assert_eq!(runner.count("npm", &[]), 1);
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        ensure_node_modules(&Fake(runner.clone()), dir.path()).unwrap();
        assert_eq!(runner.count("npm", &[]), 1);
        let failing = FakeRunner::new();
        failing
            .on("npm", &[])
            .reply(Reply::cannot_start("it did not finish within 20 minutes"));
        std::fs::remove_dir(dir.path().join("node_modules")).unwrap();
        assert!(
            ensure_node_modules(&Fake(failing), dir.path())
                .unwrap_err()
                .to_string()
                .contains("within 20 minutes")
        );
    }

    #[test]
    fn cargo_about_runs_once_per_manifest_and_explains_its_failures() {
        let runner = FakeRunner::new();
        runner.on("cargo", &["about"]).reply(Reply::ok(r#"{"crates":[],"licenses":[]}"#));
        let docs = cargo_about(
            &Fake(runner.clone()),
            &[Path::new("/m/a/Cargo.toml"), Path::new("/m/b/Cargo.toml")],
            Path::new("/r/about.toml"),
            Path::new("/w"),
        )
        .unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(
            runner.lines()[1],
            "cargo about generate --locked -c /r/about.toml --format json --manifest-path /m/b/Cargo.toml"
        );
        let fail = |reply: Reply| {
            let r = FakeRunner::new();
            r.on("cargo", &[]).reply(reply);
            cargo_about(&Fake(r), &[Path::new("/m/Cargo.toml")], Path::new("x"), Path::new("."))
                .unwrap_err()
                .to_string()
        };
        assert!(fail(Reply::cannot_start("cargo is not installed")).contains("cargo is not installed"));
        assert!(fail(Reply::fails("error: no such command: `about`")).contains("cargo install cargo-about --version 0.9.2"));
        assert!(fail(Reply::fails("line1\nline2\nerror: bad manifest")).ends_with("failed: line1 line2 error: bad manifest"));
        assert!(fail(Reply::ok("not json")).contains("not valid JSON"));
        assert!(fail(Reply::cannot_start("it did not finish within 10 minutes")).contains("within 10 minutes"));
    }
}
