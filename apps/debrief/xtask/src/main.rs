//! Repository chores for Debrief developers, run as `cargo xtask <task>` (alias in `.cargo/config.toml`).
//!
//! - `gen-samples`: regenerates every committed sample: the logs from `samples/generate.py`, then the
//!   profiler captures and the crash dump, which Rust tests write when `DEBRIEF_REGENERATE_SAMPLES=1`.
//! - `build-plugins [--release|--debug] [--target <triple>]`: builds the plugins binary and puts it where Tauri bundles it as a
//!   sidecar (`app/src-tauri/binaries/debrief-plugins-<triple>`); release builds do it through `app/scripts/build-sidecars.mjs`.
//! - `libduckdb`: fetches the pinned prebuilt libduckdb the store worker links and points Cargo at it
//!   (see [`libduckdb`]).

mod libduckdb;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// One command the task runs, kept as data so the plan can be tested without running anything.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl Step {
    fn new(program: &str, args: &[&str]) -> Self {
        Step {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: Vec::new(),
        }
    }

    fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    fn display(&self) -> String {
        let env: String = self.env.iter().map(|(k, v)| format!("{k}={v} ")).collect();
        format!("{env}{} {}", self.program, self.args.join(" "))
    }
}

/// Python launchers to try, in order. `python3` comes first (Linux, macOS); on Windows it may be the
/// Microsoft Store stub, which fails `--version`, so `python` and the `py -3` launcher follow.
const PYTHONS: &[&[&str]] = &[&["python3"], &["python"], &["py", "-3"]];

/// The Rust tests that write samples, as (crate, extra cargo args, module of the crate's `it` test binary).
const RUST_GENERATORS: &[(&str, &[&str], &str)] = &[
    ("debrief-plugin-profiling", &["--no-default-features"], "gen_json_samples"),
    ("debrief-plugin-profiling", &["--no-default-features"], "gen_nettrace"),
    ("debrief-plugin-profiling", &["--no-default-features"], "gen_pprof"),
    ("debrief-plugin-dumps", &[], "samples"),
];

/// The full `gen-samples` plan, given the Python launcher to use (program and leading arguments).
fn gen_samples_plan(python: &[&str], root: &Path) -> Vec<Step> {
    let script = root.join("samples").join("generate.py");
    let script = script.to_string_lossy();
    let mut py_args: Vec<&str> = python[1..].to_vec();
    py_args.push(&script);
    let mut steps = vec![Step::new(python[0], &py_args)];
    for (krate, extra, module) in RUST_GENERATORS {
        let mut args = vec!["test", "-p", krate];
        args.extend_from_slice(extra);
        // Only the ignored `regenerate_*` tests write files; the rest of the module runs in the normal test suite.
        let filter = format!("{module}::regenerate_");
        args.extend_from_slice(&["--test", "it", "--", "--ignored", &filter]);
        steps.push(Step::new("cargo", &args).with_env("DEBRIEF_REGENERATE_SAMPLES", "1"));
    }
    steps
}

/// The first launcher in `candidates` for which `works` holds.
fn pick_python<'a>(candidates: &[&'a [&'a str]], works: impl Fn(&[&str]) -> bool) -> Option<&'a [&'a str]> {
    candidates.iter().copied().find(|c| works(c))
}

/// A child process sharing this console: xtask is a terminal tool, so the app's `quiet_command` (no console
/// window) is not wanted here.
#[allow(clippy::disallowed_methods)]
fn command(program: &str) -> Command {
    Command::new(program)
}

fn python_works(launcher: &[&str]) -> bool {
    command(launcher[0])
        .args(&launcher[1..])
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The repository root: the parent of this crate's directory.
fn repo_root() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    dir.parent().unwrap_or(dir).to_path_buf()
}

fn run(step: &Step, root: &Path) -> Result<(), String> {
    eprintln!("xtask: {}", step.display());
    let status = command(&step.program)
        .args(&step.args)
        .envs(step.env.iter().map(|(k, v)| (k, v)))
        .current_dir(root)
        .status()
        .map_err(|e| format!("could not start `{}`: {e}", step.program))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{}` failed ({status})", step.display()))
    }
}

/// Where `build-plugins` builds and what it copies: the cargo step, the built file and the sidecar path.
#[derive(Debug, PartialEq, Eq)]
struct PluginsBuild {
    step: Step,
    built: PathBuf,
    sidecar: PathBuf,
}

/// The `build-plugins` plan for a profile (`release` or `debug`) and a target triple, with the host triple standing in for none.
fn build_plugins_plan(release: bool, target: Option<&str>, host: &str, target_dir: &Path, root: &Path) -> PluginsBuild {
    let target = target.unwrap_or(host);
    let profile = if release { "release" } else { "debug" };
    let mut args = vec!["build", "-p", "debrief-plugins", "--bin", "debrief-plugins", "--target", target];
    if release {
        args.push("--release");
    }
    let ext = if target.contains("-windows-") { ".exe" } else { "" };
    PluginsBuild {
        step: Step::new("cargo", &args),
        built: target_dir.join(target).join(profile).join(format!("debrief-plugins{ext}")),
        sidecar: root.join("app/src-tauri/binaries").join(format!("debrief-plugins-{target}{ext}")),
    }
}

/// The arguments of `build-plugins`: (release, target).
fn parse_build_plugins(args: &[String]) -> Result<(bool, Option<String>), String> {
    let (mut release, mut target) = (true, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--release" => release = true,
            "--debug" => release = false,
            "--target" => target = Some(it.next().ok_or("--target needs a triple")?.clone()),
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }
    Ok((release, target))
}

fn build_plugins(args: &[String]) -> Result<(), String> {
    let (release, target) = parse_build_plugins(args)?;
    let root = repo_root();
    let rustc = command("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("could not start `rustc`: {e}"))?;
    let host = String::from_utf8_lossy(&rustc.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or("`rustc -vV` printed no host line")?;
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let plan = build_plugins_plan(release, target.as_deref(), &host, &target_dir, &root);
    run(&plan.step, &root)?;
    if let Some(dir) = plan.sidecar.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::copy(&plan.built, &plan.sidecar).map_err(|e| {
        format!(
            "could not copy {} to {}: {e}; check the build output above",
            plan.built.display(),
            plan.sidecar.display()
        )
    })?;
    eprintln!("plugins: {}", plan.sidecar.display());
    Ok(())
}

fn gen_samples() -> Result<(), String> {
    let root = repo_root();
    let python = pick_python(PYTHONS, python_works)
        .ok_or("no working Python 3 found (tried python3, python, py -3); install Python 3 and put it on PATH")?;
    for step in gen_samples_plan(python, &root) {
        run(&step, &root)?;
    }
    eprintln!("xtask: samples regenerated; review and commit the changes under samples/ (and fuzz seeds)");
    Ok(())
}

const USAGE: &str = "usage: cargo xtask <task>\n\ntasks:\n  gen-samples   regenerate every committed sample (samples/generate.py and the Rust sample generators)\n  build-plugins build the plugins sidecar into app/src-tauri/binaries ([--release|--debug] [--target <triple>])\n  libduckdb     fetch the pinned prebuilt libduckdb and point Cargo at it (.cargo/libduckdb.toml)\n  help         show this message";

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let result = match task.as_deref() {
        Some("gen-samples") => gen_samples(),
        Some("build-plugins") => build_plugins(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("libduckdb") => libduckdb::run(&repo_root()),
        Some("help" | "-h" | "--help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown task `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_first_working_python() {
        let picked = pick_python(PYTHONS, |c| c[0] != "python3");
        assert_eq!(picked, Some(&["python"][..]));
        let picked = pick_python(PYTHONS, |c| c[0] == "py");
        assert_eq!(picked, Some(&["py", "-3"][..]));
        assert_eq!(pick_python(PYTHONS, |_| false), None);
    }

    #[test]
    fn plan_runs_the_python_generator_then_every_rust_generator() {
        let root = Path::new("repo");
        let plan = gen_samples_plan(&["py", "-3"], root);
        assert_eq!(plan.len(), 1 + RUST_GENERATORS.len());
        assert_eq!(plan[0].program, "py");
        assert_eq!(plan[0].args[0], "-3");
        assert!(plan[0].args[1].ends_with("generate.py"));
        assert!(plan[0].env.is_empty());
        for step in &plan[1..] {
            assert_eq!(step.program, "cargo");
            assert_eq!(step.env, vec![("DEBRIEF_REGENERATE_SAMPLES".to_owned(), "1".to_owned())]);
            assert!(step.args.windows(2).any(|w| w == ["--test", "it"]), "{:?}", step.args);
            let filter = step.args.last().unwrap();
            assert!(filter.ends_with("::regenerate_"), "{filter}");
            assert_eq!(step.args[step.args.len() - 3..step.args.len() - 1], ["--", "--ignored"]);
        }
        assert_eq!(plan.last().map(|s| s.args.last().unwrap().as_str()), Some("samples::regenerate_"));
        assert!(plan[1]
            .display()
            .starts_with("DEBRIEF_REGENERATE_SAMPLES=1 cargo test -p debrief-plugin-profiling --no-default-features"));
        assert_eq!(plan.last().map(|s| s.args[2].as_str()), Some("debrief-plugin-dumps"));
    }

    #[test]
    fn build_plugins_builds_the_bin_for_the_target_and_names_the_sidecar_like_the_script() {
        let p = build_plugins_plan(true, None, "aarch64-apple-darwin", Path::new("/t"), Path::new("/r"));
        assert_eq!(
            p.step.display(),
            "cargo build -p debrief-plugins --bin debrief-plugins --target aarch64-apple-darwin --release"
        );
        assert_eq!(p.built, Path::new("/t/aarch64-apple-darwin/release/debrief-plugins"));
        assert_eq!(
            p.sidecar,
            Path::new("/r/app/src-tauri/binaries/debrief-plugins-aarch64-apple-darwin")
        );
        let w = build_plugins_plan(
            false,
            Some("x86_64-pc-windows-msvc"),
            "aarch64-apple-darwin",
            Path::new("/t"),
            Path::new("/r"),
        );
        assert!(!w.step.args.contains(&"--release".to_owned()));
        assert_eq!(w.built, Path::new("/t/x86_64-pc-windows-msvc/debug/debrief-plugins.exe"));
        assert_eq!(
            w.sidecar,
            Path::new("/r/app/src-tauri/binaries/debrief-plugins-x86_64-pc-windows-msvc.exe")
        );
    }

    #[test]
    fn build_plugins_arguments_are_the_scripts() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_build_plugins(&a(&[])).unwrap(), (true, None));
        assert_eq!(
            parse_build_plugins(&a(&["--debug", "--target", "t"])).unwrap(),
            (false, Some("t".to_owned()))
        );
        assert!(parse_build_plugins(&a(&["--target"])).is_err());
        assert!(parse_build_plugins(&a(&["--wat"])).is_err());
    }

    #[test]
    fn repo_root_holds_the_sample_generator() {
        assert!(repo_root().join("samples").join("generate.py").is_file());
    }
}
