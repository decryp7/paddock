use crate::{gates, input_hash, ipc_endpoints, plan, plan_cmd, slots, wait};
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
    /// The CI planner: which projects a change reaches, the CI matrix and the commit lint's range (the port of
    /// scripts/affected.mjs; the arguments are its: <base> [<head>], --files a,b, --all, --ci [--app x], --commit-range, --commit-flags, --explain).
    /// It prints no JSON object: `--json` is refused.
    Plan(PlanArgs),
    /// The project graph.
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
}

#[derive(Subcommand)]
enum CheckCommand {
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
    let result = dispatch(cli.command);
    finish(if cli.json { Format::Json } else { Format::Text }, result, out, err).code()
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
        Command::Wait(a) => wait::command(&a),
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
            command: CheckCommand::IpcEndpoints { tag, root },
        } => ipc_endpoints::command(root, tag),
        Command::Plan(a) => plan_cmd::command(&a.args, a.root),
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
