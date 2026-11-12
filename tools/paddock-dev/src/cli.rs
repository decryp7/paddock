use crate::{
    brief_check, check, ci_isolation, ci_runs, gates, hook, input_hash, ipc_endpoints, merge_check, plan, plan_cmd, release_isolation,
    slots, wait, worktree,
};
use clap::{Args, Parser, Subcommand};
use paddock_cli::{CliError, Exit, Format, Report, finish};
use std::io::Write;
use std::path::PathBuf;

/// `./dev`: the Paddock developer and CI tool.
#[derive(Parser)]
#[command(name = "dev", version, disable_help_subcommand = true)]
pub(crate) struct Cli {
    /// Write exactly one JSON object to standard output (errors included).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// The tool itself.
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommand,
    },
    /// CI helpers.
    Ci {
        #[command(subcommand)]
        command: CiCommand,
    },
    /// Wait, with a limit, for a plock job, a process, a line in a file or a CI run. Use it instead of `pgrep -f` loops.
    Wait(wait::WaitArgs),
    /// Briefs to agents: check one for the required fields and the known-bad patterns (docs/agents/brief-template.md).
    Brief {
        #[command(subcommand)]
        command: brief_check::BriefCommand,
    },
    /// C100 and the gate register (ADR 0060): is every check run somewhere?
    Gates {
        #[command(subcommand)]
        command: GatesCommand,
    },
    /// Repository checks of the family.
    Check {
        #[command(subcommand)]
        command: CheckCommand,
    },
    /// Agent worktrees (ADR 0083): add, claim, reassign, release, drop, list and clean. The caller's agent id is self-declared
    /// until the guard hook (RT-7) passes the real one.
    Worktree {
        #[command(subcommand)]
        command: worktree::WorktreeCommand,
    },
    /// The CI planner: which projects a change reaches, the CI matrix and the commit lint's range (the port of
    /// scripts/affected.mjs; the arguments are its: <base> [<head>], --files a,b, --all, --ci [--app x], --commit-range, --commit-flags, --explain).
    /// It prints no JSON object: `--json` is refused.
    Plan(PlanArgs),
    /// Merge checks.
    Merge {
        #[command(subcommand)]
        command: MergeCommand,
    },
    /// The project graph.
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
    /// Claude Code hooks (ADR 0083). Run by the installed copy, never through `./dev`.
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    /// Put the hook binary and its shim in place.
    Hooks {
        #[command(subcommand)]
        command: HooksCommand,
    },
}

#[derive(Subcommand)]
enum HookCommand {
    /// The PreToolUse guard: reads one tool call as JSON on standard input, prints a deny decision or nothing. Exit 0 always.
    PreToolUse,
}

#[derive(Subcommand)]
enum HooksCommand {
    /// Copy this binary to <git-common-dir>/paddock/hooks/ and write the shim tools/githooks/claude-pre-tool-use.
    Install(hook::install::InstallArgs),
}

#[derive(Subcommand)]
enum CheckCommand {
    /// The probe of the primary checkout (ADR 0083): on main, no local-only commit, no tracked change, owners match locks. Exit 5: a problem; exit 6: only checks that could not run (not measured). Liveness is measured only with --live.
    Primary(worktree::PrimaryArgs),
    /// The repository checks ported to Rust so far (C39, C53; RT-5): findings, expiring waivers from paddock.waivers.toml, a run
    /// record per check (`not measured` is told from `clean`). The report JSON has the shape of paddock-conformance's, plus `runs`.
    Run {
        /// Only these check ids, comma separated (for example C39,C53).
        #[arg(long, value_delimiter = ',')]
        only: Option<Vec<String>>,
        /// Only these repository-relative files, comma separated (the scope of a push gate).
        #[arg(long, value_delimiter = ',')]
        files: Option<Vec<String>>,
        /// The repository root (default: the folder containing .git, found from the current folder).
        #[arg(long)]
        root: Option<PathBuf>,
        /// The app name in the report.
        #[arg(long, default_value = "paddock")]
        app: String,
        /// Also write the report JSON to this file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// C102 (ADR 0072): every listen site is in docs/ipc-endpoints.toml with an ending test or a reason, and the ratchet of
    /// connection endings is well formed. With `--tag`, also refuses a release tag that a ratchet entry blocks.
    IpcEndpoints {
        /// A release tag to check against the ratchet's `blocks_tag` patterns (for example `dyno-v1.2.0`).
        #[arg(long)]
        tag: Option<String>,
        /// The repository root (default: the folder containing .git, found from the current folder).
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum MergeCommand {
    /// Did a merge silently drop one side's work? Flags files that go backwards against the first parent, hunks a parent added
    /// that the result lacks (reason: `Hunks-dropped: <path>: <reason>` in the merge message) and tests a parent has that the
    /// result lacks (`Tests-removed: <name>: <reason>`). Exit 5 on findings; a check that could not look is an error, never a pass.
    Check(MergeCheckArgs),
}

#[derive(Args)]
struct MergeCheckArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// A branch about to be merged (before the merge exists); needs --target.
    #[arg(long, requires = "target", conflicts_with = "commit")]
    branch: Option<String>,
    /// The branch it would be merged into.
    #[arg(long, requires = "branch")]
    target: Option<String>,
    /// The merge commit to check.
    commit: Option<String>,
}

#[derive(Subcommand)]
enum SelfCommand {
    /// Print the input hash this binary's sources, dependencies and compiler give, and the hash it was built from.
    Hash,
}

#[derive(Subcommand)]
enum CiCommand {
    /// Job-isolation slots held by kernel file locks.
    Slot {
        #[command(subcommand)]
        command: SlotCommand,
    },
    /// The CI kit's job isolation on a host runner: slots, kept folders, ports, process patterns, temp folders (the port of
    /// `conformance/ci/job-isolation.sh`). Values go to standard output, messages to standard error.
    Isolation(ci_isolation::IsolationArgs),
    /// Release mode (ADR 0085): a tag build reads nothing kept. `setup` makes the job's folders, `record` is the only
    /// ledger writer, `verify` proves it.
    ReleaseIsolation {
        #[command(subcommand)]
        command: release_isolation::Command,
    },
    /// The tests CI marked flaky (nextest, Playwright), vitest's failed specs and tests that failed and then passed on a
    /// rerun of the same commit, over a window of runs. An incomplete measurement ("not measured") exits 1 for now;
    /// flaky tests found exit 5 only with --fail-on-flakes;
    /// cancelled runs are counted and listed but never change the exit code.
    Flakes(ci_runs::FlakesArgs),
    /// Real CI timings of the family from the Gitea API (the port of scripts/ci-timings.mjs, same Markdown).
    Timings(ci_runs::TimingsArgs),
}

#[derive(Subcommand)]
enum SlotCommand {
    /// Take the first free slot of 1..=N and keep it until the owner folder is gone or `release` is called. Prints sN.
    Acquire(slots::AcquireArgs),
    /// Let a slot go and wait until it is free.
    Release(slots::ReleaseArgs),
    /// Show which slots are held.
    Status(slots::StatusArgs),
    /// Run a command only while a slot is idle, holding its lock until the command ends (skips a busy slot).
    IdleRun(slots::IdleRunArgs),
    /// The detached holder that `acquire` starts. Not for people.
    #[command(hide = true)]
    Hold(slots::HoldArgs),
}

#[derive(Subcommand)]
enum GatesCommand {
    /// Print the CI model: every command CI can run, with its folder, levels and status (for debugging C100).
    Model(GatesRootArgs),
    /// C100: every check the repository defines is gated by a reachable CI line, or registered with a reason.
    Check(GatesCheckArgs),
    /// Every check with its gating (use --json).
    List(GatesListArgs),
    /// The globs whose files change C100's answer (the planner's inputs).
    Inputs,
    /// Refuse a release tag while the lint-adoption ratchets of its app are not empty (nothing else of C100 can refuse it).
    Ratchet(GatesRatchetArgs),
}

#[derive(Args)]
struct GatesRatchetArgs {
    /// The repository root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// The release tag about to be made, `<app>-v<major>.<minor>.<patch>[-pre]`.
    #[arg(long)]
    release_tag: String,
    /// The revision the register may only shrink against.
    #[arg(long, default_value = "origin/main")]
    base: String,
}

#[derive(Args)]
struct GatesCheckArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// A release tag about to be made (debrief-v1.3.0): refused while a ratchet's target release is not met.
    #[arg(long)]
    release_tag: Option<String>,
    /// Print every finding but exit 0 (the rollback of ADR 0060: the model counts a --warn line as report-only).
    #[arg(long)]
    warn: bool,
    /// The revision the register may only shrink against.
    #[arg(long, default_value = "origin/main")]
    base: String,
}

#[derive(Args)]
struct GatesListArgs {
    /// The repository root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// The revision the register is compared with.
    #[arg(long, default_value = "origin/main")]
    base: String,
}

#[derive(Args)]
struct GatesRootArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Subcommand)]
enum GraphCommand {
    /// The projects a change reaches (the core of scripts/affected.mjs).
    Affected(AffectedArgs),
}

#[derive(Args)]
struct PlanArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The planner's own arguments, as scripts/affected.mjs took them.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Args)]
struct AffectedArgs {
    /// Changed files, comma separated (instead of a base).
    #[arg(long, value_delimiter = ',', conflicts_with_all = ["all", "base"])]
    files: Vec<String>,
    /// Everything (what a tag or nightly runs).
    #[arg(long, conflicts_with = "base")]
    all: bool,
    /// Give the reason for every project.
    #[arg(long)]
    explain: bool,
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The base revision.
    base: Option<String>,
    /// The head revision.
    #[arg(default_value = "HEAD", requires = "base")]
    head: String,
}

/// Runs the tool: `args` are the command line without the program name. Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    // What `./dev --print-binary` answers (the wrapper prints its cached binary): this binary's own path, so the CI kit can pin
    // whichever of the two `PADDOCK_DEV` names.
    if args == ["--print-binary"] {
        return match std::env::current_exe() {
            Ok(p) => {
                let _ = writeln!(out, "{}", p.display());
                Exit::Ok.code()
            }
            Err(e) => finish(
                Format::Text,
                Err(CliError::failure(format!("the program's own path is unknown: {e}."))),
                out,
                err,
            )
            .code(),
        };
    }
    let format = if args.iter().any(|a| a == "--json") {
        Format::Json
    } else {
        Format::Text
    };
    let argv = std::iter::once("dev".to_string()).chain(args.iter().cloned());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => {
            let _ = write!(out, "{e}");
            return Exit::Ok.code();
        }
        Err(e) => {
            let message = e.render().to_string();
            let first = message.lines().next().unwrap_or("").trim_start_matches("error: ").to_string();
            let hint = "Run `./dev --help` for the commands.";
            return finish(format, Err(CliError::usage(first).with_hint(hint)), out, err).code();
        }
    };
    if cli.json && matches!(cli.command, Command::Plan(_)) {
        let e = CliError::usage("`dev plan` has no --json mode.")
            .with_hint("Its output is the planner's own: JSON for a plan, true or false, a range.");
        return finish(Format::Json, Err(e), out, err).code();
    }
    if let Command::Ci {
        command: CiCommand::Isolation(a),
    } = &cli.command
    {
        // The script's contract: plain values on standard output, `::error::` lines on standard error, no `error:` prefix.
        return ci_isolation::run(a, out, err);
    }
    if matches!(cli.command, Command::Hook { .. }) {
        // The hook's answer is its own JSON on standard output, not a report, and its exit code is always 0.
        return hook::pre_tool_use(&mut std::io::stdin(), out, err);
    }
    let command = match cli.command {
        Command::Merge {
            command: MergeCommand::Check(a),
        } if cli.json => {
            return merge_check::command_json(
                &merge_check::Request {
                    root: a.root,
                    commit: a.commit,
                    branch: a.branch,
                    target: a.target,
                },
                out,
                err,
            );
        }
        other => other,
    };
    let result = dispatch(command);
    // A probe whose only problems are checks that could not run ends with `worktree::EXIT_NOT_MEASURED` (ER-33).
    let not_measured_only = result.as_ref().is_ok_and(|r| r.data["not_measured_only"] == true);
    let code = finish(if cli.json { Format::Json } else { Format::Text }, result, out, err);
    if not_measured_only && code == Exit::Findings {
        worktree::EXIT_NOT_MEASURED
    } else {
        code.code()
    }
}

fn dispatch(command: Command) -> Result<Report, CliError> {
    match command {
        Command::SelfCmd {
            command: SelfCommand::Hash,
        } => input_hash::command(),
        Command::Ci {
            command: CiCommand::Slot { command },
        } => match command {
            SlotCommand::Acquire(a) => slots::acquire(&a),
            SlotCommand::Release(a) => slots::release(&a),
            SlotCommand::Status(a) => slots::status(&a),
            SlotCommand::IdleRun(a) => slots::idle_run(&a),
            SlotCommand::Hold(a) => slots::hold(&a),
        },
        Command::Ci {
            command: CiCommand::Isolation(_),
        } => Err(CliError::failure("`ci isolation` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::ReleaseIsolation { command },
        } => release_isolation::command(command),
        Command::Ci {
            command: CiCommand::Flakes(a),
        } => ci_runs::flakes(&a),
        Command::Ci {
            command: CiCommand::Timings(a),
        } => ci_runs::timings(&a),
        Command::Wait(a) => wait::command(&a),
        Command::Brief { command } => brief_check::command(command),
        Command::Gates {
            command: GatesCommand::Model(a),
        } => gates::command::model(a.root.as_deref()),
        Command::Gates {
            command: GatesCommand::Check(a),
        } => gates::command::check(&gates::command::CheckArgs {
            root: a.root,
            release_tag: a.release_tag,
            warn: a.warn,
            base: a.base,
        }),
        Command::Gates {
            command: GatesCommand::List(a),
        } => gates::command::list(a.root.as_deref(), &a.base),
        Command::Gates {
            command: GatesCommand::Inputs,
        } => gates::command::inputs(),
        Command::Gates {
            command: GatesCommand::Ratchet(a),
        } => gates::command::ratchet(a.root.as_deref(), &a.base, &a.release_tag),
        Command::Check {
            command:
                CheckCommand::Run {
                    only,
                    files,
                    root,
                    app,
                    out,
                },
        } => check::command(check::Args {
            only,
            files,
            root,
            app,
            out,
        }),
        Command::Check {
            command: CheckCommand::IpcEndpoints { tag, root },
        } => ipc_endpoints::command(root, tag),
        Command::Check {
            command: CheckCommand::Primary(a),
        } => worktree::check_primary(&a),
        Command::Worktree { command } => worktree::command(command),
        Command::Merge {
            command: MergeCommand::Check(a),
        } => merge_check::command(&merge_check::Request {
            root: a.root,
            commit: a.commit,
            branch: a.branch,
            target: a.target,
        }),
        Command::Plan(a) => plan_cmd::command(&a.args, a.root),
        Command::Hook { .. } => Ok(Report::new(serde_json::json!({}), String::new())),
        Command::Hooks {
            command: HooksCommand::Install(a),
        } => hook::install::command(&a),
        Command::Graph {
            command: GraphCommand::Affected(a),
        } => plan::command(&plan::Request {
            files: a.files,
            all: a.all,
            explain: a.explain,
            root: a.root,
            base: a.base,
            head: a.head,
        }),
    }
}
