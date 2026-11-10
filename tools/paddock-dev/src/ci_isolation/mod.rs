//! `dev ci isolation`: the CI kit's job isolation on a host runner (ADR 0025 and its 2026-11-10 amendment), the Rust port of
//! `conformance/ci/job-isolation.sh`, `job-tmpdir.sh` and `ci-home.sh` (Rust tooling plan, package RT-1).
//!
//! ```text
//! dev ci isolation acquire | release | status | slots | temp
//! dev ci isolation path <kind> [<name>] [--source <dir>]
//! dev ci isolation owns <folder> [<kind>] | port <name> | pattern <regex> | sweep-slot [all]
//! dev ci isolation tmpdir setup | remove | sweep
//! ```
//!
//! The contract is the shell script's, case for case (`conformance/test/job-isolation.test.mjs` runs against both): a value
//! command (`path`, `temp`, `owns`, `port`, `pattern`, `slots`, `status`) prints exactly its value on standard output; every
//! message goes to standard error; `acquire` appends the same lines to `$GITHUB_ENV`; failures are one `::error::job-isolation:`
//! annotation with the cause and the next step. Exit codes: 0, 1 for a failure (also what a refused `owns` or `pattern` answers),
//! 2 for a usage error.
//!
//! The engine holds no Paddock names. The kept folders and port blocks are read from `kept-folders.txt` and `ports.txt`
//! (`--tables <dir>`, else `PADDOCK_CI_TABLES`, else `conformance/ci` of the repository found from the current folder).
//! `PADDOCK_DEV` is the program run for `ci slot ...` (tests); the default is this binary, so no wrapper or build is involved.
//!
//! Parts: [`table`] (the data files, [`table::glob_of`], the retired-layout rule), [`home`] (the CI home and paths),
//! [`folders`] (the folders of a slot, what is due), [`slotflow`] (acquire, release, status, the sweep), [`pattern`] (process
//! patterns), [`tmpdir`] (the job's temp and socket folders).

pub mod folders;
pub mod home;
pub mod pattern;
pub mod slotflow;
pub mod table;
pub mod tmpdir;

use clap::Args;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

/// The command line of `dev ci isolation`.
#[derive(Args)]
pub struct IsolationArgs {
    /// The folder with `kept-folders.txt` and `ports.txt` (default: `PADDOCK_CI_TABLES`, else `conformance/ci` of the repository).
    #[arg(long)]
    pub tables: Option<PathBuf>,
    /// The command (`acquire`, `release`, `status`, `slots`, `path`, `temp`, `owns`, `port`, `pattern`, `sweep-slot`, `tmpdir`)
    /// and its arguments.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Why a command ended without its value: the text for standard error (empty for a silent refusal) and the exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    /// Exit code.
    pub code: i32,
    /// What to print on standard error; empty when the answer is only the exit code.
    pub text: String,
}

impl Stop {
    /// `::error::job-isolation: <msg>`, exit 1 (the script's `die`).
    pub fn die(msg: impl AsRef<str>) -> Stop {
        Stop::die_as("job-isolation", msg)
    }

    /// The same for another tool name (`job-tmpdir`).
    pub fn die_as(who: &str, msg: impl AsRef<str>) -> Stop {
        Stop {
            code: 1,
            text: format!("::error::{who}: {}", msg.as_ref()),
        }
    }

    /// Exit `code` with nothing to say.
    pub fn silent(code: i32) -> Stop {
        Stop { code, text: String::new() }
    }

    /// A line exactly as given, exit `code`.
    pub fn line(code: i32, text: impl Into<String>) -> Stop {
        Stop { code, text: text.into() }
    }
}

/// A result that may end the command.
pub type R<T> = Result<T, Stop>;

/// The process's environment as the command sees it, and where its data lives. Built once; tests build their own.
pub struct Ctx {
    env: HashMap<String, String>,
    /// The folder with the two data files.
    pub tables: PathBuf,
    /// This program (what `PADDOCK_DEV` falls back to, and what a sweep runs under `idle-run`).
    pub exe: PathBuf,
}

impl Ctx {
    /// A context over explicit values.
    pub fn new(env: HashMap<String, String>, tables: PathBuf, exe: PathBuf) -> Ctx {
        Ctx { env, tables, exe }
    }

    /// A copy of the environment (for a command that runs as a later step of the job).
    pub fn env_snapshot(&self) -> HashMap<String, String> {
        self.env.clone()
    }

    /// A variable that is set and not empty (the script's `${X:-}` tests).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }

    /// A variable that must be set and not empty (`${X:?msg}`).
    pub fn need(&self, key: &str, msg: &str) -> R<&str> {
        self.get(key).ok_or_else(|| Stop::die(msg))
    }

    /// `kept-folders.txt`, parsed.
    pub fn kept(&self) -> R<table::Kept> {
        let p = self.tables.join("kept-folders.txt");
        std::fs::read_to_string(&p).map(|t| table::parse_kept(&t)).map_err(|e| {
            Stop::die(format!(
                "cannot read {}: {e}. Pass --tables <folder with kept-folders.txt and ports.txt>.",
                p.display()
            ))
        })
    }

    /// `ports.txt`, parsed.
    pub fn ports(&self) -> R<Vec<table::PortBlock>> {
        let p = self.tables.join("ports.txt");
        std::fs::read_to_string(&p).map(|t| table::parse_ports(&t)).map_err(|e| {
            Stop::die(format!(
                "cannot read {}: {e}. Pass --tables <folder with kept-folders.txt and ports.txt>.",
                p.display()
            ))
        })
    }
}

/// Standard output and standard error of the command.
pub struct Sinks<'a> {
    /// Values.
    pub out: &'a mut dyn Write,
    /// Messages.
    pub err: &'a mut dyn Write,
}

impl Sinks<'_> {
    /// A message line on standard error.
    pub fn say(&mut self, text: impl AsRef<str>) {
        let _ = writeln!(self.err, "{}", text.as_ref());
    }

    /// A value line on standard output.
    pub fn value(&mut self, text: impl AsRef<str>) {
        let _ = writeln!(self.out, "{}", text.as_ref());
    }
}

const USAGE: &str = "usage: dev ci isolation acquire|release|status|slots|path <kind> [<name>] [--source <dir>]|temp|owns <folder> [<kind>]|port <name>|pattern <regex>|sweep-slot [all]|tmpdir setup|remove|sweep";

/// The tables folder: the flag, `PADDOCK_CI_TABLES`, else `conformance/ci` of the repository above the current folder.
pub fn find_tables(flag: Option<PathBuf>, env: &HashMap<String, String>, cwd: &std::path::Path) -> PathBuf {
    if let Some(t) = flag {
        return t;
    }
    if let Some(t) = env.get("PADDOCK_CI_TABLES").filter(|t| !t.is_empty()) {
        return PathBuf::from(t);
    }
    let mut at = Some(cwd);
    while let Some(d) = at {
        let c = d.join("conformance/ci");
        if c.join("kept-folders.txt").is_file() {
            return c;
        }
        at = d.parent();
    }
    cwd.join("conformance/ci")
}

/// Runs the command line `args` (the part after `ci isolation`) and returns the exit code.
pub fn dispatch(ctx: &Ctx, args: &[String], io: &mut Sinks<'_>) -> i32 {
    match run_command(ctx, args, io) {
        Ok(()) => 0,
        Err(stop) => {
            if !stop.text.is_empty() {
                io.say(&stop.text);
            }
            stop.code
        }
    }
}

fn run_command(ctx: &Ctx, args: &[String], io: &mut Sinks<'_>) -> R<()> {
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        Some("acquire") => slotflow::acquire(ctx, io),
        Some("release") => slotflow::release(ctx, io),
        Some("status") => slotflow::status(ctx, io),
        Some("slots") => {
            io.value(folders::max_slots(ctx)?.to_string());
            Ok(())
        }
        Some("sweep-slot") => slotflow::sweep_slot(ctx, rest.first().map(String::as_str), io),
        Some("path") => home::cmd_path(ctx, rest, io),
        Some("temp") => {
            io.value(home::job_temp(ctx)?);
            Ok(())
        }
        Some("owns") => folders::cmd_owns(ctx, rest),
        Some("port") => pattern::cmd_port(ctx, rest, io),
        Some("pattern") => pattern::cmd_pattern(ctx, rest, io),
        Some("tmpdir") => tmpdir::command(ctx, rest.first().map(String::as_str), io),
        _ => Err(Stop::line(2, USAGE)),
    }
}

/// `dev ci isolation` for the real process: its environment, current folder and standard streams' writers.
pub fn run(a: &IsolationArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let env: HashMap<String, String> = std::env::vars().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let tables = find_tables(a.tables.clone(), &env, &cwd);
    let exe = std::env::current_exe().unwrap_or_default();
    dispatch(&Ctx::new(env, tables, exe), &a.args, &mut Sinks { out, err })
}

#[cfg(test)]
pub(crate) mod testkit {
    use super::*;

    /// A context over `env` pairs with the repository's real data files.
    pub fn ctx(pairs: &[(&str, &str)]) -> Ctx {
        let tables = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/ci");
        Ctx::new(
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            tables,
            PathBuf::from("/nonexistent/paddock-dev"),
        )
    }

    /// Runs a command, returns (code, stdout, stderr).
    pub fn go(ctx: &Ctx, args: &[&str]) -> (i32, String, String) {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let code = dispatch(ctx, &argv, &mut Sinks { out: &mut o, err: &mut e });
        (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
    }
}
