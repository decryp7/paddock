//! `dev ci <host step>`: the host-runner steps of the CI kit, the Rust port of the actions `stable-workspace`, `node-caches`,
//! `host-tools`, `sccache-stats`, the script `conformance/ci/build-folder.sh`, the prune scripts (`tools/prune-build-folders.sh`,
//! `tools/prune-ci-build-folder.sh`, `tools/prune-build-folders.ps1`) and `tools/host-cleanup.sh` (docs/plans/ci-in-rust.md, WP-4).
//!
//! ```text
//! dev ci stable-workspace --name <name>          move the checkout to the job slot's kept folder, leave a link
//! dev ci node-caches [--angular-dir <dir>] --name <name>
//! dev ci host-tools                              the pinned cargo-nextest
//! dev ci sccache-stats
//! dev ci build-folder <workspace> <name>         the job's build folder as cargo configuration
//! dev ci prune-build-folder --max-gb <n> [--drop-bundles]   cap the job's own kept build folder (CI form)
//! dev ci prune-folder <folder> <max-gb>          cap one folder (incremental caches first, then the folder)
//! dev ci prune-agent-builds --app <id> [--remove] [--keep <suffix>...]   list or remove agents' build folders
//! dev ci host-cleanup --id <app> [--process-pattern <p>]... [--temp-prefix <p>] [--build-outputs]
//! ```
//!
//! The contract is the shell's, case for case (`tests/it/host_steps.rs` runs both on the same inputs, `conformance/test/*` keep the
//! scripts' own cases): plain lines and `::warning::` / `::notice::` lines on standard output, `$GITHUB_ENV` and `$GITHUB_PATH`
//! appended to, and one `::error::` line (the cause, then `Next step:`) when a step fails. A step that only saves time (the stable
//! workspace, the caches, the prune, the clean-up) never fails the job: it warns and exits 0, as the shell did.
//!
//! Everything that deletes goes through [`safe_remove`]: a path is removed only under an explicit root that is not a home folder,
//! a drive root or a top-level folder, and a link is removed as a link, never followed (ER-14). Programs that are not ours
//! (`git`, `curl`, `tar`, `sccache`, `ps`, PowerShell) run through [`ToolCall`](crate::ci_tool::ToolCall); a process is ended
//! through `paddock_proc` after the identity check (rule 53). The folders of the job (its slot, the kept folders) come from
//! `ci isolation` in process.

pub mod build_folder;
pub mod cleanup;
pub mod host_tools;
pub mod links;
pub mod node_caches;
pub mod prune;
pub mod safe_remove;
pub mod sccache_stats;
pub mod stable_workspace;

use crate::ci_isolation::{self, Ctx, Sinks};
use crate::ci_tool::error_line;
use clap::Subcommand;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// The host steps, as subcommands of `dev ci`.
#[derive(Subcommand)]
pub enum HostStep {
    /// Moves the checkout to a fixed folder of the job's slot in the CI home and leaves a link at the old path (a clone where it
    /// cannot move), sets PADDOCK_WORKSPACE (the port of the stable-workspace action). Never fails the job.
    StableWorkspace(stable_workspace::Args),
    /// Keeps npm's download cache and the Angular build cache between runs (the port of the node-caches action).
    NodeCaches(node_caches::Args),
    /// The pinned cargo-nextest on every OS: kept in the CI home on a host, checked everywhere (the port of the host-tools action).
    HostTools,
    /// Prints sccache's statistics and stops the job's own server (the port of the sccache-stats action).
    SccacheStats,
    /// A job's Rust build folder as cargo configuration, `<workspace>/.cargo/ci.toml` (the port of conformance/ci/build-folder.sh).
    BuildFolder(build_folder::Args),
    /// Caps this job's kept build folder (`$PADDOCK_BUILD_FOLDER`, only a target-* folder of the job's slot): the CI form of
    /// prune-folder (the port of tools/prune-ci-build-folder.sh). Always exits 0.
    PruneBuildFolder(prune::CiArgs),
    /// Caps one folder: over the cap the incremental caches go, still over the folder goes (the port of tools/prune-build-folders.sh).
    PruneFolder(prune::FolderArgs),
    /// Lists, or with --remove deletes, the build folders agents left (`~/.<app>-target*`, `<repo>/target/agent-*`; the port of
    /// tools/prune-build-folders.ps1).
    PruneAgentBuilds(prune::AgentArgs),
    /// Leaves nothing of this job behind: its stray processes, its temp folders, optionally its build outputs (the port of
    /// tools/host-cleanup.sh). Always exits 0 once the arguments are valid.
    HostCleanup(cleanup::Args),
}

/// Runs a host step for the real process.
pub fn run(step: &HostStep, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let env: HashMap<String, String> = std::env::vars().collect();
    let windows = cfg!(windows) || env.get("PADDOCK_CI_ASSUME_WINDOWS").is_some_and(|v| v == "1");
    match step {
        HostStep::StableWorkspace(a) => stable_workspace::run(&env, windows, a, out),
        HostStep::NodeCaches(a) => node_caches::run(&env, windows, a, out),
        HostStep::HostTools => host_tools::run(&env, windows, out),
        HostStep::SccacheStats => sccache_stats::run(&env, out),
        HostStep::BuildFolder(a) => build_folder::run(&env, windows, a, out),
        HostStep::PruneBuildFolder(a) => prune::run_ci(&env, windows, a, out),
        HostStep::PruneFolder(a) => prune::run_folder(&env, a, out, err),
        HostStep::PruneAgentBuilds(a) => prune::run_agent(&env, a, out, err),
        HostStep::HostCleanup(a) => cleanup::run(&env, windows, a, out, err),
    }
}

/// What every step shares: the environment it reads, where its lines go, and the helpers for the job's files.
pub struct Step<'a> {
    /// The environment of the job.
    pub env: &'a HashMap<String, String>,
    /// Standard output: every line of a step.
    pub out: &'a mut dyn Write,
    /// Windows semantics (junctions, drive paths); `cfg!(windows)` unless a test says otherwise.
    pub windows: bool,
    /// The step's name, the prefix of its lines.
    pub name: &'static str,
}

impl Step<'_> {
    /// A variable that is set and not empty (`${X:-}` tests).
    pub fn var(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }

    /// One line on standard output.
    pub fn say(&mut self, text: impl AsRef<str>) {
        let _ = writeln!(self.out, "{}", text.as_ref());
    }

    /// `::warning::<step>: <msg>`.
    pub fn warn(&mut self, msg: impl AsRef<str>) {
        let line = format!("::warning::{}: {}", self.name, msg.as_ref());
        self.say(line);
    }

    /// The step's one `::error::` line (cause, then the next step) and the failure exit code.
    pub fn fail(&mut self, cause: impl AsRef<str>, next: impl AsRef<str>) -> i32 {
        let line = error_line(&format!("{}: {}", self.name, cause.as_ref()), next.as_ref());
        self.say(line);
        1
    }

    /// Appends one line to the file `$<var>` names (`GITHUB_ENV`, `GITHUB_PATH`).
    pub fn append_to(&mut self, var: &str, line: &str) -> Result<(), String> {
        let path = self
            .var(var)
            .ok_or_else(|| format!("{var} is not set (the runner's file for the steps that follow)"))?;
        append_line(Path::new(path), line).map_err(|e| format!("cannot append to {path} ({var}): {e}"))
    }

    /// A value of `ci isolation` (a kept folder, ...), asked in process; `Err` is the isolation's own error text.
    pub fn isolation(&self, args: &[&str]) -> Result<String, String> {
        isolation(self.env, self.windows, args)
    }
}

/// Appends `line` and a newline to the file at `path`, creating it.
pub fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(f, "{line}")
}

/// `dev ci isolation <args>` in process: its standard output without the last newline, or its error text.
pub fn isolation(env: &HashMap<String, String>, windows: bool, args: &[&str]) -> Result<String, String> {
    let (code, o, e) = isolation_full(env, windows, args);
    if code == 0 { Ok(o) } else { Err(e) }
}

/// `dev ci isolation <args>` in process: the exit code, standard output and standard error (each without the last newline).
pub fn isolation_full(env: &HashMap<String, String>, windows: bool, args: &[&str]) -> (i32, String, String) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let tables = ci_isolation::find_tables(None, env, &cwd);
    let mut ctx = Ctx::new(env.clone(), tables, std::env::current_exe().unwrap_or_default());
    ctx.windows |= windows;
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let code = ci_isolation::dispatch(&ctx, &argv, &mut Sinks { out: &mut o, err: &mut e });
    let text = |b: Vec<u8>| String::from_utf8_lossy(&b).trim_end_matches(['\n', '\r']).to_string();
    (code, text(o), text(e))
}

/// `text` with every character outside `[A-Za-z0-9._-]` replaced by `_` (`tr -c 'A-Za-z0-9._-' '_'`).
pub fn folder_name(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The last `n` bytes of `text` with line breaks as spaces (`tail -c 300 | tr '\r\n' '  '`), for an `::error::` line.
pub fn tail_flat(text: &str, n: usize) -> String {
    let mut start = text.len().saturating_sub(n);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].replace(['\r', '\n'], " ")
}

/// Whether `program` is on `PATH` (`command -v`), by looking at the folders of the job's own `PATH`.
pub fn on_path(env: &HashMap<String, String>, program: &str) -> bool {
    let Some(path) = env.get("PATH") else { return false };
    std::env::split_paths(path).any(|d| {
        let base = d.join(program);
        if cfg!(windows) {
            ["", ".exe", ".cmd", ".bat"]
                .iter()
                .any(|e| std::path::PathBuf::from(format!("{}{e}", base.display())).is_file())
        } else {
            is_executable(&base)
        }
    })
}

/// A file with an execute bit (any file on Windows).
pub fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_keep_only_the_safe_characters() {
        assert_eq!(folder_name("decryp7/paddock-crate"), "decryp7_paddock-crate");
        assert_eq!(folder_name("a b\nc.d_e-f"), "a_b_c.d_e-f");
    }

    #[test]
    fn the_tail_is_flat_and_cut_on_a_character() {
        assert_eq!(tail_flat("a\r\nb\nc", 100), "a  b c");
        assert_eq!(tail_flat("abcdef", 3), "def");
        assert_eq!(tail_flat("aé", 1), "", "a cut inside a character moves on");
    }

    #[test]
    fn lines_append_to_the_runners_files() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("env");
        append_line(&f, "A=1").unwrap();
        append_line(&f, "B=2").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "A=1\nB=2\n");
        let env: HashMap<String, String> = [("GITHUB_ENV".to_string(), f.display().to_string())].into();
        let mut out = Vec::new();
        let mut s = Step {
            env: &env,
            out: &mut out,
            windows: false,
            name: "t",
        };
        s.append_to("GITHUB_ENV", "C=3").unwrap();
        assert!(s.append_to("GITHUB_PATH", "x").unwrap_err().contains("GITHUB_PATH is not set"));
        assert_eq!(s.fail("boom", "fix it"), 1);
        assert_eq!(String::from_utf8(out).unwrap(), "::error::t: boom Next step: fix it\n");
        assert!(std::fs::read_to_string(&f).unwrap().ends_with("C=3\n"));
    }
}
