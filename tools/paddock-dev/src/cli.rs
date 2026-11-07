use crate::{input_hash, plan, slots, wait};
use clap::{Args, Parser, Subcommand};
use paddock_cli::{CliError, Exit, Format, Report, finish};
use std::io::Write;
use std::path::PathBuf;

/// `./dev`: the Paddock developer and CI tool.
#[derive(Parser)]
#[command(name = "dev", version, disable_help_subcommand = true)]
struct Cli {
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
    /// The project graph.
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
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
enum GraphCommand {
    /// The projects a change reaches (the core of scripts/affected.mjs).
    Affected(AffectedArgs),
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
