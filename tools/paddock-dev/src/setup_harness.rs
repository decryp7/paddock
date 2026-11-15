//! `./dev setup-harness build`: the stub `paddock-setup.exe` and the harness installer of `crates/paddock-setup/nsis/test`, built with
//! `makensis` (the port of `build.sh`, docs/plans/shell-to-rust.md WP-7). The include is staged beside them the way an app's
//! `src-tauri/windows/` holds it; `SETUP_EXE` names a real `paddock-setup.exe` to use instead of the stub.

use crate::ci_tool::{Output, ToolCall, error_line};
use clap::Subcommand;
use paddock_cli::{CliError, Report};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The files copied beside the build, relative to the harness folder (the include is one folder up).
const STAGED: &[&str] = &[
    "harness-hooks.nsh",
    "harness.nsi",
    "harness.txt",
    "stub-setup.nsi",
    "stub-server.nsi",
];
/// The stop list the include reads; only the harness' own programs.
const GENERATED: &str = "!define PADDOCK_STOP_IMAGES \"harness.exe,harness-agent.exe\"\n";
/// One makensis run, at most this long.
const MAKENSIS_TIMEOUT: Duration = Duration::from_secs(300);

/// `./dev setup-harness <command>`.
#[derive(Subcommand)]
pub enum SetupHarnessCommand {
    /// Build the stub setup program and the harness installer with makensis.
    Build {
        /// Where the built files go (default: `crates/paddock-setup/nsis/test/out`).
        outdir: Option<PathBuf>,
        /// The repository root (default: the current repository).
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

/// Runs one `setup-harness` command.
pub fn command(command: SetupHarnessCommand) -> Result<Report, CliError> {
    match command {
        SetupHarnessCommand::Build { outdir, root } => {
            let root = crate::gates::command::repo_root(root.as_deref())?;
            let env: HashMap<String, String> = std::env::vars().collect();
            build(&root, outdir.as_deref(), &env)
        }
    }
}

fn io_error(what: &str, path: &Path, e: &std::io::Error) -> CliError {
    CliError::failure(error_line(
        &format!("{what} {}: {e}.", path.display()),
        "check the folder and its rights.",
    ))
}

/// Stages the files into `outdir` and runs makensis three times, as `build.sh` did.
pub fn build(root: &Path, outdir: Option<&Path>, env: &HashMap<String, String>) -> Result<Report, CliError> {
    let here = root.join("crates/paddock-setup/nsis/test");
    let out = outdir.map_or_else(|| here.join("out"), Path::to_path_buf);
    std::fs::create_dir_all(&out).map_err(|e| io_error("Cannot create", &out, &e))?;
    let include = here.join("../paddock-per-machine.nsh");
    let sources = std::iter::once(include).chain(STAGED.iter().map(|f| here.join(f)));
    for src in sources {
        let name = src.file_name().unwrap_or_default().to_owned();
        std::fs::copy(&src, out.join(name)).map_err(|e| io_error("Cannot copy", &src, &e))?;
    }
    let generated = out.join("paddock-per-machine.generated.nsh");
    std::fs::write(&generated, GENERATED).map_err(|e| io_error("Cannot write", &generated, &e))?;
    if let Some(setup) = env.get("SETUP_EXE").filter(|v| !v.is_empty()) {
        std::fs::copy(setup, out.join("paddock-setup.exe")).map_err(|e| io_error("Cannot copy SETUP_EXE", Path::new(setup), &e))?;
    }
    let scripts: &[&str] = if env.get("SETUP_EXE").is_some_and(|v| !v.is_empty()) {
        &["stub-server.nsi", "harness.nsi"]
    } else {
        &["stub-setup.nsi", "stub-server.nsi", "harness.nsi"]
    };
    for script in scripts {
        let ran = ToolCall::new("makensis")
            .args(["-V2", script])
            .cwd(&out)
            .output(Output::Stream)
            .timeout(MAKENSIS_TIMEOUT)
            .run(env, &mut std::io::stderr())
            .map_err(|why| CliError::failure(error_line(&why, "install NSIS (makensis) and run it again.")))?;
        if !ran.success {
            return Err(CliError::failure(error_line(
                &format!("makensis failed on {script}: {}.", ran.how),
                "read its output above; the include is crates/paddock-setup/nsis/paddock-per-machine.nsh.",
            )));
        }
    }
    let line = format!("built in {}", out.display());
    Ok(Report::new(serde_json::json!({ "out": out.display().to_string() }), line))
}
