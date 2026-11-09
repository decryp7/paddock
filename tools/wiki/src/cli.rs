use crate::date::{self, Day};
use crate::lint::{self, Finding, MAX_AGE_DAYS, Request, RuleSource};
use crate::ops::{self, NewPage, OpError};
use crate::sources::{self, ReadError};
use crate::trace::{self, Trace};
use clap::{Args, Parser, Subcommand};
use paddock_cli::{CliError, Exit, Format, Report, finish};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `wiki`: lint a project wiki, append to its log, create a page.
#[derive(Parser)]
#[command(name = "wiki", version, disable_help_subcommand = true)]
struct Cli {
    /// Write exactly one JSON object to standard output (errors included).
    #[arg(long, global = true)]
    json: bool,
    /// The wiki folder (default: `wiki` of the config file, else docs/wiki, relative to the config file's folder).
    #[arg(long, global = true)]
    wiki: Option<PathBuf>,
    /// The settings file (default: wiki-lint.json under --repo, if it exists): `wiki`, `rules`, `citeByLine`, `maxAge`, `er`.
    /// Flags win over it.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// The repository folder: where `git show` runs for `<ref>:<path>` rules sources, and the base of the defaults
    /// (default: the current folder).
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    /// Today as YYYY-MM-DD (default: the system clock, local time, like `date +%F`).
    #[arg(long, global = true)]
    today: Option<String>,
    /// Troubleshooting lines on standard error: phases with durations, every git command with its exit code, every rules source.
    #[arg(long, global = true)]
    verbose: bool,
    /// Append the same lines to this file as CLEF JSON lines (one object per line, like paddock-log's files).
    #[arg(long, global = true)]
    log_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check the wiki: links, index, orphans, front matter, sources, log, copied rule text.
    Lint(LintArgs),
    /// Append `## [today] <kind> | <subject>` to log.md.
    Log(LogArgs),
    /// Create a page from the template and add its line to index.md.
    New(NewArgs),
}

#[derive(Args)]
struct LintArgs {
    /// A rules source for the copied-text check: a file, or `<git ref>:<path>` (repeatable; replaces the config file's `rules`).
    /// With none given anywhere: docs/rules.md and origin/main:docs/rules.md under the settings folder, each if readable.
    #[arg(long = "rules")]
    rules: Vec<String>,
    /// One more rules file (the engineering rules); a warning if it does not exist. Default: ../engineering-rules/RULES.md next
    /// to the settings folder, when that exists.
    #[arg(long)]
    er: Option<PathBuf>,
    /// A rules file on every branch matching a pattern: `<ref glob>:<path>`, for example `origin/docs/*:docs/rules.md`
    /// (repeatable; also `ruleBranches` of the config file). A branch that cannot be read only warns.
    #[arg(long = "rule-branches")]
    rule_branches: Vec<String>,
    /// A log whose entries must be cited by line number, not clock time: a line with this name and a time like 14:33 is an
    /// error (repeatable; also `citeByLine` of the config file).
    #[arg(long = "cite-by-line")]
    cite_by_line: Vec<String>,
    /// Pages not verified within this many days are flagged (default 30, or `maxAge` of the config file).
    #[arg(long)]
    max_age: Option<u32>,
    /// Stale pages fail the lint too.
    #[arg(long)]
    strict: bool,
}

#[derive(Args)]
struct LogArgs {
    /// ingest, lint or fix.
    kind: String,
    /// The rest of the line: `page | what changed`.
    subject: String,
}

#[derive(Args)]
struct NewArgs {
    /// The file name without .md: lower-case letters, digits and hyphens.
    slug: String,
    /// The page title.
    #[arg(long)]
    title: String,
    /// The index line's description (default: the title).
    #[arg(long)]
    summary: Option<String>,
    /// A `sources:` entry (repeatable); the lint asks for at least one.
    #[arg(long = "source")]
    sources: Vec<String>,
}

/// What the tool takes from its surroundings, so tests can inject it.
pub struct Context {
    /// Today's local calendar day.
    pub clock: fn() -> Day,
    /// Print `::error::` annotations (set when the `CI` variable is).
    pub ci: bool,
    /// The folder of the per-run log file; `None` writes none (tests).
    pub log_dir: Option<PathBuf>,
}

impl Context {
    /// The real clock and environment.
    pub fn system() -> Context {
        Context {
            clock: date::today_local,
            ci: ["CI", "GITHUB_ACTIONS", "GITEA_ACTIONS"]
                .iter()
                .any(|k| std::env::var(k).is_ok_and(|v| v == "true")),
            log_dir: trace::default_log_dir(),
        }
    }
}

/// Runs the tool: `args` are the command line without the program name. Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    run_with(args, out, err, &Context::system())
}

/// [`run`] with the clock and CI mode given.
pub fn run_with(args: &[String], out: &mut dyn Write, err: &mut dyn Write, ctx: &Context) -> i32 {
    let format = if args.iter().any(|a| a == "--json") {
        Format::Json
    } else {
        Format::Text
    };
    let argv = std::iter::once("wiki".to_string()).chain(args.iter().cloned());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => {
            let _ = write!(out, "{e}");
            return 0;
        }
        Err(e) => {
            let message = e.render().to_string();
            let first = message.lines().next().unwrap_or("").trim_start_matches("error: ").to_string();
            let hint = "Run `wiki --help` for the commands.";
            return finish(format, Err(CliError::usage(first).with_hint(hint)), out, err).code();
        }
    };
    let operation_id = trace::new_operation_id();
    let log_file = match &cli.log_file {
        Some(p) => Some(p.clone()),
        None => ctx.log_dir.as_deref().and_then(|d| trace::default_log_file(d, &operation_id)),
    };
    let result = {
        let live: Option<&mut dyn Write> = if cli.verbose { Some(&mut *err) } else { None };
        match Trace::new(operation_id, live, log_file.as_deref()) {
            Ok(trace) => {
                trace.info(
                    &format!("wiki {} started", env!("CARGO_PKG_VERSION")),
                    &[
                        ("version", json!(env!("CARGO_PKG_VERSION"))),
                        ("args", json!(loggable_args(&cli, args))),
                        ("os", json!(std::env::consts::OS)),
                        ("arch", json!(std::env::consts::ARCH)),
                        ("ci", json!(ctx.ci)),
                        ("log_file", json!(log_file.as_ref().map(|p| p.display().to_string()))),
                    ],
                );
                let result = dispatch(&cli, ctx, &trace);
                let ms = trace.elapsed_ms();
                match &result {
                    Ok(r) => trace.info(
                        // Only the lint's summary line is safe to log; the others (`log`) echo the user's text.
                        &format!(
                            "summary: {}",
                            if matches!(cli.command, Command::Lint(_)) {
                                r.text.lines().last().unwrap_or("done")
                            } else {
                                "done"
                            }
                        ),
                        &[("exit", json!(r.exit.code())), ("ok", json!(true)), ("ms", json!(ms))],
                    ),
                    Err(e) => trace.event(
                        "Error",
                        &format!("summary: failed: {}", e.text().replace('\n', " ")),
                        &[("exit", json!(e.exit.code())), ("ok", json!(false)), ("ms", json!(ms))],
                    ),
                }
                result
            }
            Err(m) => Err(io_error(m).with_hint("Check the --log-file path and the permissions.")),
        }
    };
    // A failed run says where its log is (the log file is the evidence, ER-27).
    let result = match (result, &log_file) {
        (Err(e), Some(p)) => Err(CliError::new(e.exit, e.message).with_hint(format!("{} The log of this run: {}", e.hint, p.display()))),
        (other, _) => other,
    };
    emit_errors(&result, ctx, cli.json, err);
    finish(if cli.json { Format::Json } else { Format::Text }, result, out, err).code()
}

/// The command line as the log may show it: commands, flags and paths as given, but every free-text value (a log subject, a
/// page title, summary or source) replaced by its length, since user text does not belong in a log by default.
fn loggable_args(cli: &Cli, args: &[String]) -> Vec<String> {
    let free: Vec<&str> = match &cli.command {
        Command::Log(a) => vec![a.subject.as_str()],
        Command::New(a) => {
            let mut v = vec![a.title.as_str()];
            v.extend(a.summary.as_deref());
            v.extend(a.sources.iter().map(String::as_str));
            v
        }
        Command::Lint(_) => Vec::new(),
    };
    args.iter()
        .map(|a| {
            let masked = |v: &str| format!("<text: {} chars>", v.chars().count());
            if !a.starts_with("--") && free.contains(&a.as_str()) {
                masked(a)
            } else if let Some((flag, value)) = a.split_once('=')
                && flag.starts_with("--")
                && free.contains(&value)
            {
                // The `--title=Text` form of a free-text flag.
                format!("{flag}={}", masked(value))
            } else {
                a.clone()
            }
        })
        .collect()
}

/// The error findings of a lint (standard error: `error file:line: message`, or in CI an annotation), and in CI the failure of
/// any command. Warnings and the summary are the report's text (standard output).
fn emit_errors(result: &Result<Report, CliError>, ctx: &Context, json: bool, err: &mut dyn Write) {
    match result {
        Err(e) => {
            if ctx.ci {
                let _ = writeln!(err, "::error::wiki-lint: {}", e.text().replace('\n', " "));
            }
        }
        Ok(r) => {
            for f in r.data.get("errors").and_then(Value::as_array).into_iter().flatten() {
                let place = place(f);
                let message = f["message"].as_str().unwrap_or("");
                if ctx.ci {
                    let _ = writeln!(err, "::error::wiki-lint: {place}: {message}");
                } else if !json {
                    let _ = writeln!(err, "error {place}: {message}");
                }
            }
        }
    }
}

/// `file` or `file:line` of a finding object.
fn place(f: &Value) -> String {
    let file = f["file"].as_str().unwrap_or("");
    match f["line"].as_u64().unwrap_or(0) {
        0 => file.to_string(),
        n => format!("{file}:{n}"),
    }
}

fn io_error(message: impl Into<String>) -> CliError {
    CliError::new(Exit::Failure, message).with_hint("Check the path and the permissions.")
}

fn op_error(e: OpError) -> CliError {
    match e {
        OpError::Input(m) => CliError::usage(m),
        OpError::Refused(m) => CliError::refused(m),
        OpError::Io(m) => io_error(m),
    }
}

fn today(cli: &Cli, ctx: &Context) -> Result<Day, CliError> {
    match &cli.today {
        None => Ok((ctx.clock)()),
        Some(s) => date::parse_date(s).ok_or_else(|| CliError::usage(format!("--today must be a real date as YYYY-MM-DD, not `{s}`"))),
    }
}

/// Where things are: the folder the command was run from, and the folder settings and default paths resolve against (the
/// config file's, else the repository's top folder (the nearest ancestor with `.git`), else the start folder).
struct Layout {
    cwd: PathBuf,
    root: PathBuf,
    config_file: Option<PathBuf>,
}

impl Layout {
    fn discover(cli: &Cli) -> Result<Layout, CliError> {
        let cwd = std::env::current_dir().map_err(|e| io_error(format!("cannot read the current folder: {e}")))?;
        let start = lint::normalize(&cwd.join(cli.repo.as_deref().unwrap_or(Path::new("."))));
        let toplevel = start.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf);
        let config_file = match &cli.config {
            Some(p) => {
                let path = lint::normalize(&cwd.join(p));
                if !path.is_file() {
                    return Err(io_error(format!("cannot read the settings file {}", path.display()))
                        .with_hint("Check --config, or drop it to search upwards for wiki-lint.json."));
                }
                Some(path)
            }
            // Search from the start folder up to the repository's top folder; without a repository only the start folder.
            None => start
                .ancestors()
                .take_while(|d| toplevel.as_ref().is_none_or(|t| d.starts_with(t)))
                .take(if toplevel.is_some() { usize::MAX } else { 1 })
                .map(|d| d.join("wiki-lint.json"))
                .find(|f| f.is_file()),
        };
        let root = config_file
            .as_ref()
            .and_then(|f| f.parent())
            .map(Path::to_path_buf)
            .or_else(|| toplevel.clone())
            .unwrap_or_else(|| start.clone());
        Ok(Layout { cwd, root, config_file })
    }
}

fn dispatch(cli: &Cli, ctx: &Context, trace: &Trace<'_>) -> Result<Report, CliError> {
    let (layout, config) = trace.phase("config discovery", || -> Result<(Layout, Config), CliError> {
        let layout = Layout::discover(cli)?;
        let config = Config::load(layout.config_file.as_deref())?;
        match &layout.config_file {
            Some(f) => trace.info(
                &format!("settings file: {}", f.display()),
                &[("settings", json!(f.display().to_string()))],
            ),
            None => trace.info("settings file: none found, using defaults", &[]),
        }
        trace.info(
            &format!("settings folder: {}", layout.root.display()),
            &[("root", json!(layout.root.display().to_string()))],
        );
        Ok((layout, config))
    })?;
    let wiki = match (&cli.wiki, &config.wiki) {
        (Some(w), _) => lint::normalize(&layout.cwd.join(w)),
        (None, Some(w)) => lint::normalize(&layout.root.join(w)),
        (None, None) => layout.root.join("docs/wiki"),
    };
    match &cli.command {
        Command::Lint(a) => lint_command(cli, a, ctx, &config, &layout, &wiki, trace),
        Command::Log(a) => {
            let line = ops::log(&wiki, &a.kind, &a.subject, today(cli, ctx)?).map_err(op_error)?;
            Ok(Report::new(json!({"line": line}), format!("added to log.md: {line}")))
        }
        Command::New(a) => {
            let page = NewPage {
                slug: &a.slug,
                title: &a.title,
                summary: a.summary.as_deref(),
                sources: &a.sources,
            };
            let name = ops::new_page(&wiki, &page, today(cli, ctx)?).map_err(op_error)?;
            Ok(Report::new(
                json!({"page": name}),
                format!(
                    "created {name} and listed it in index.md; fill in the sources and claims, link it from another page, then run `wiki lint`"
                ),
            ))
        }
    }
}

fn finding_json(f: &Finding) -> Value {
    json!({"file": f.file, "line": f.line, "message": f.message})
}

/// The settings file. Unknown keys are ignored; a key of the wrong type is an error naming the file.
#[derive(Default)]
struct Config {
    wiki: Option<String>,
    rules: Vec<String>,
    rule_branches: Vec<String>,
    cite_by_line: Vec<String>,
    max_age: Option<u32>,
    er: Option<String>,
}

/// Largest settings file read.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// Most rules sources read in one run (branches times files), so a broad `ruleBranches` pattern cannot run away.
const MAX_RULE_SOURCES: usize = 256;

impl Config {
    fn load(file: Option<&Path>) -> Result<Config, CliError> {
        let Some(path) = file else { return Ok(Config::default()) };
        let bad = |why: String| {
            io_error(format!("{}: {why}", path.display())).with_hint(
                "The settings file is JSON: {\"wiki\": \"docs/wiki\", \"rules\": [...], \"ruleBranches\": [\"origin/docs/*:docs/rules.md\"], \"citeByLine\": [...], \"maxAge\": 30}.",
            )
        };
        let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if len > MAX_CONFIG_BYTES {
            return Err(bad("larger than 1 MiB".into()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| bad(format!("cannot read: {e}")))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| bad(format!("not valid JSON: {e}")))?;
        let strings = |key: &str| -> Result<Vec<String>, CliError> {
            match v.get(key) {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|i| {
                        i.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| bad(format!("`{key}` must be a list of strings")))
                    })
                    .collect(),
                Some(_) => Err(bad(format!("`{key}` must be a list of strings"))),
            }
        };
        let string = |key: &str| -> Result<Option<String>, CliError> {
            match v.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(bad(format!("`{key}` must be a string"))),
            }
        };
        let max_age = match v.get("maxAge") {
            None | Some(Value::Null) => None,
            Some(n) => Some(
                n.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| bad("`maxAge` must be a whole number of days".into()))?,
            ),
        };
        Ok(Config {
            wiki: string("wiki")?,
            rules: strings("rules")?,
            rule_branches: strings("ruleBranches")?,
            cite_by_line: strings("citeByLine")?,
            max_age,
            er: string("er")?,
        })
    }
}

/// `~/x` is the home folder's `x`.
fn expand_home(spec: &str) -> String {
    match (
        spec.strip_prefix("~/"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    ) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest).to_string_lossy().into_owned(),
        _ => spec.to_string(),
    }
}

/// A rules spec as a thing to read: a file relative to `base` when it is one, else a git `<ref>:<path>` as written.
fn resolve_spec(spec: &str, base: &Path) -> String {
    let spec = expand_home(spec);
    let joined = lint::normalize(&base.join(&spec));
    if Path::new(&spec).is_absolute() || joined.exists() || !sources::looks_like_git_ref(&spec) {
        joined.to_string_lossy().into_owned()
    } else {
        spec
    }
}

fn read_error(e: &ReadError) -> CliError {
    io_error(e.cause.clone()).with_hint(e.next_step.clone())
}

/// A warning about a `<ref>:<path>` that cannot be read.
fn unreadable_ref(name: &str, e: &ReadError) -> Finding {
    Finding {
        file: name.to_string(),
        line: 0,
        message: format!("{} {} The copied-rule check skips it.", e.cause, e.next_step),
    }
}

/// [`sources::read`], recording whether the source was found.
fn read_logged(name: &str, target: &str, repo: &Path, trace: &Trace<'_>) -> Result<RuleSource, ReadError> {
    let r = sources::read(name, target, repo, trace);
    match &r {
        Ok(s) => trace.info(
            &format!("rules source {name}: found"),
            &[("source", json!(name)), ("found", json!(true)), ("bytes", json!(s.text.len()))],
        ),
        Err(e) => trace.event(
            "Warning",
            &format!("rules source {name}: not found: {}", e.cause),
            &[("source", json!(name)), ("found", json!(false))],
        ),
    }
    r
}

/// Reads a rules source: an unreadable file is an error, an unreadable `<ref>:<path>` a warning.
fn read_or_warn(
    name: &str,
    target: &str,
    repo: &Path,
    rules: &mut Vec<RuleSource>,
    notes: &mut Vec<Finding>,
    trace: &Trace,
) -> Result<(), CliError> {
    match read_logged(name, target, repo, trace) {
        Ok(src) => rules.push(src),
        Err(e) if e.is_ref => notes.push(unreadable_ref(name, &e)),
        Err(e) => return Err(read_error(&e)),
    }
    Ok(())
}

/// A finding as the contract prints it.
fn line_of(f: &Finding) -> String {
    f.to_string()
}

fn lint_command(
    cli: &Cli,
    a: &LintArgs,
    ctx: &Context,
    config: &Config,
    layout: &Layout,
    wiki: &Path,
    trace: &Trace,
) -> Result<Report, CliError> {
    let today = today(cli, ctx)?;
    // Git runs in the repository that holds the settings file (else where the command was started).
    let git_dir = &layout.root;
    let mut rules = Vec::new();
    let mut notes: Vec<Finding> = Vec::new();
    // Flags are relative to where the command was run; the settings file's entries to the settings file's folder. Flags win.
    let (given, base): (&[String], &Path) = if !a.rules.is_empty() {
        (&a.rules, &layout.cwd)
    } else {
        (&config.rules, &layout.root)
    };
    trace.phase("rules sources", || -> Result<(), CliError> {
        if given.is_empty() {
            // Nothing listed in the flags or the settings: the defaults, each used only when readable.
            for spec in ["docs/rules.md", "origin/main:docs/rules.md"] {
                if let Ok(src) = read_logged(spec, &resolve_spec(spec, &layout.root), git_dir, trace) {
                    rules.push(src);
                }
            }
        }
        for spec in given {
            let target = resolve_spec(spec, base);
            read_or_warn(spec, &target, git_dir, &mut rules, &mut notes, trace)?;
        }
        // A rules file on each branch a pattern matches (open rule branches); an unreadable one only warns.
        let patterns = if a.rule_branches.is_empty() {
            &config.rule_branches
        } else {
            &a.rule_branches
        };
        for pattern in patterns {
            let (glob, path) = pattern
                .rsplit_once(':')
                .filter(|(g, p)| !g.is_empty() && !p.is_empty())
                .ok_or_else(|| {
                    CliError::usage(format!("ruleBranches entry `{pattern}` must be `<ref glob>:<path>`"))
                        .with_hint("For example `origin/docs/*:docs/rules.md`.")
                })?;
            for branch in sources::branches(glob, git_dir, trace).map_err(|e| read_error(&e))? {
                if rules.len() + notes.len() >= MAX_RULE_SOURCES {
                    return Err(io_error(format!("more than {MAX_RULE_SOURCES} rules sources."))
                        .with_hint("Narrow the patterns in `ruleBranches`."));
                }
                let name = format!("{branch}:{path}");
                read_or_warn(&name, &name, git_dir, &mut rules, &mut notes, trace)?;
            }
        }
        let er = match (&a.er, &config.er) {
            (Some(p), _) => Some(lint::normalize(&layout.cwd.join(expand_home(&p.to_string_lossy())))),
            (None, Some(e)) => Some(lint::normalize(&layout.root.join(expand_home(e)))),
            (None, None) => None,
        };
        // The engineering rules are one more source: the file named by `--er` / `er` (a warning when it is missing), else
        // `../engineering-rules/RULES.md` next to the settings folder when that exists. One that exists but cannot be read is
        // an error.
        let explicit = er.is_some();
        let er = er.unwrap_or_else(|| layout.root.join("../engineering-rules/RULES.md"));
        if er.exists() {
            let name = er.to_string_lossy();
            rules.push(read_logged(&name, &name, git_dir, trace).map_err(|e| read_error(&e))?);
        } else if explicit {
            trace.event("Warning", &format!("engineering rules file {} does not exist", er.display()), &[]);
            notes.push(Finding {
                file: er.to_string_lossy().into_owned(),
                line: 0,
                message: "the engineering rules file does not exist, so the copied-rule check skips it. Check out engineering-rules there, or fix --er / `er`."
                    .to_string(),
            });
        }
        Ok(())
    })?;
    let outcome = trace
        .phase("pages", || {
            lint::lint(&Request {
                wiki_dir: wiki,
                cwd: &layout.cwd,
                rules: &rules,
                today,
                max_age: i64::from(a.max_age.or(config.max_age).unwrap_or(MAX_AGE_DAYS as u32)),
                cite_by_line: if a.cite_by_line.is_empty() {
                    &config.cite_by_line
                } else {
                    &a.cite_by_line
                },
            })
        })
        .map_err(|e| io_error(e.to_string()).with_hint("Check --wiki, or the `wiki` key of wiki-lint.json."))?;
    trace.info(
        &format!(
            "pages checked: {} pages ({} files) in {}",
            outcome.pages,
            outcome.files,
            wiki.display()
        ),
        &[("pages", json!(outcome.pages)), ("files", json!(outcome.files))],
    );
    let mut warnings = notes;
    warnings.extend(outcome.warnings);
    // Standard output: warnings, a note, the summary. The errors go to standard error (see `emit_errors`).
    let mut text = String::new();
    for w in &warnings {
        if ctx.ci {
            text.push_str(&format!("::warning::wiki-lint: {}\n", line_of(w)));
        } else {
            text.push_str(&format!("warning {}\n", line_of(w)));
        }
    }
    if rules.is_empty() {
        text.push_str("note: no rules source given or readable, so the copied-rule check is skipped (see --rules)\n");
    }
    text.push_str(&format!(
        "wiki-lint: {} error(s), {} warning(s); {} rules source(s)",
        outcome.errors.len(),
        warnings.len(),
        rules.len()
    ));
    let data = json!({
        "errors": outcome.errors.iter().map(finding_json).collect::<Vec<_>>(),
        "warnings": warnings.iter().map(finding_json).collect::<Vec<_>>(),
        "error_count": outcome.errors.len(),
        "warning_count": warnings.len(),
        "rules_sources": rules.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),
    });
    let failed = !outcome.errors.is_empty() || (a.strict && !warnings.is_empty());
    Ok(Report::new(data, text).with_exit(if failed { Exit::Findings } else { Exit::Ok }))
}
