//! `./dev check run`: the host of Paddock's repository checks in Rust (RT-5, ADR 0022's conformance rules engine). It runs the
//! checks ported so far ([`c39`], [`c53`], [`c120`]) over a tree, with
//!
//! - findings and a report JSON in the shape of `conformance/src/report.mjs`'s result (`app`, `checks`, `findings`, `waived`,
//!   `errors`, `warnings`), so the conformance job can merge or diff the two hosts, plus `runs`, the run records;
//! - expiring waivers from `paddock.waivers.toml` ([`waivers`], the semantics of `util/waivers.mjs`);
//! - shrink-only baselines by text ([`baseline`]), for the checks that keep one (C71 later);
//! - `--only` (ids), `--files` (per-file scope, for `push-gate`) and `--json`;
//! - a run record per check ([`record`]): ran, completed, precondition, control, probe count, scope and OS, so `not measured` is
//!   told from `clean` (ER-33). A check whose control fails or which read nothing on a whole-tree run is an error no waiver covers.
//!
//! Shadow running: until the JavaScript host drops C39 and C53 (RT-5 retirement), both run in the conformance job and
//! `tests/it/check.rs` diffs them on the tree and on `tests/fixtures/check`. The wiring is listed in the RT-5 hand-off, not done here.

pub mod baseline;
pub mod c53_liveness;
pub mod config;
pub mod conformance;
pub mod finding;
pub mod inline_waiver;
pub mod jsre;
pub mod lex;
pub mod mask;
pub mod outcome;
pub mod record;
pub mod report;
pub mod tree;
pub mod util;
pub mod waivers;

/// Registers checks: one line per check, `module::Type`. It declares the module and adds the check to [`registry`], in the order of
/// `conformance/src/checks/index.mjs`. Nothing else names a check: the host, `--only`, `list` and the parity tests read the registry.
macro_rules! register_checks {
    ($($module:ident :: $check:ident),+ $(,)?) => {
        $(pub mod $module;)+

        /// The checks the host has, in the order of `conformance/src/checks/index.mjs`.
        pub fn registry() -> Vec<Box<dyn Check>> {
            vec![$(Box::new($module::$check)),+]
        }

        /// `(check id, source file)` of every registered check, for `list --json`.
        pub fn sources() -> Vec<(&'static str, &'static str)> {
            vec![$(($module::$check.id(), concat!("tools/paddock-dev/src/check/", stringify!($module), ".rs"))),+]
        }
    };
}

register_checks! {
    c39::C39,
    c53::C53,
    c119::C119,
    c120::C120,
    c121::C121,
}

use config::Config;
use finding::{Finding, Severity, Waived};
use outcome::Outcome;
use paddock_cli::{CliError, Exit, Report};
use record::{Precondition, RunRecord, Scope};
use serde_json::{Value, json};
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tree::Tree;

/// What a check sees: the tree, narrowed to the run's files, and the run's inputs.
pub struct Ctx<'a> {
    tree: &'a Tree,
    scoped: Rc<Vec<String>>,
    base_ref: Option<String>,
    config: &'a Config,
    scope: Option<String>,
    today: String,
    workflows: OnceCell<Rc<Vec<util::workflows::Workflow>>>,
}

impl<'a> Ctx<'a> {
    /// A context over `tree` for the files `scoped`, with the run's config, `--scope` and today's date.
    pub fn new(
        tree: &'a Tree,
        scoped: Rc<Vec<String>>,
        base_ref: Option<String>,
        config: &'a Config,
        scope: Option<String>,
        today: String,
    ) -> Ctx<'a> {
        Ctx {
            tree,
            scoped,
            base_ref,
            config,
            scope,
            today,
            workflows: OnceCell::new(),
        }
    }

    /// The files this run looks at (all of the tree, or the `--files` that the tree knows), sorted.
    pub fn files(&self) -> Rc<Vec<String>> {
        self.scoped.clone()
    }

    /// The ref a baseline's growth is measured against: the run's own, else `PADDOCK_BASE_REF`, else `origin/main`.
    pub fn base_ref(&self) -> String {
        self.base_ref
            .clone()
            .or_else(|| std::env::var("PADDOCK_BASE_REF").ok())
            .unwrap_or_else(|| "origin/main".into())
    }

    /// The tree's root folder.
    pub fn root(&self) -> &std::path::Path {
        self.tree.root()
    }

    /// Every file of the tree, whatever the scope (for a precondition).
    pub fn tree_files(&self) -> Rc<Vec<String>> {
        self.tree.files()
    }

    /// A file's text, or `None` when it cannot be read.
    pub fn read(&self, rel: &str) -> Option<Rc<String>> {
        self.tree.read(rel)
    }

    /// Whether the path exists in the tree.
    pub fn exists(&self, rel: &str) -> bool {
        self.tree.exists(rel)
    }

    /// Whether the path is a folder of the tree.
    pub fn is_dir(&self, rel: &str) -> bool {
        self.tree.is_dir(rel)
    }

    /// The run's config (`paddock.conformance.json` over the defaults, the app name).
    pub fn config(&self) -> &Config {
        self.config
    }

    /// `--scope apps/<app>` (C99), if given.
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    /// Today, `YYYY-MM-DD` (UTC).
    pub fn today(&self) -> &str {
        &self.today
    }

    /// The workflow files (`<workflowsDir>/*.yml|yaml`) with their jobs and steps, read once.
    pub fn workflows(&self) -> Rc<Vec<util::workflows::Workflow>> {
        self.workflows
            .get_or_init(|| {
                let dir = format!("{}/", self.config.str("workflowsDir"));
                Rc::new(
                    self.tree
                        .files()
                        .iter()
                        .filter(|f| f.starts_with(&dir) && (f.ends_with(".yml") || f.ends_with(".yaml")))
                        .map(|f| {
                            let text = self.tree.read(f).map(|t| t.to_string()).unwrap_or_default();
                            util::workflows::read(f, &text)
                        })
                        .collect(),
                )
            })
            .clone()
    }
}

/// What a check returns to the host.
pub struct CheckOutput {
    /// The findings, before waivers.
    pub findings: Vec<Finding>,
    /// How many things it looked at.
    pub probes: usize,
    /// It read everything it meant to.
    pub completed: bool,
    /// What its input had to be.
    pub precondition: Precondition,
}

impl CheckOutput {
    /// A check that does not apply to this run (C119 judges the Paddock repository only): no findings, and no error for having
    /// looked at nothing. It counts as one probe so the host does not call the run empty; the precondition says why it is empty.
    pub fn not_applicable(why: &str) -> CheckOutput {
        CheckOutput {
            findings: Vec::new(),
            probes: 1,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("not applicable: {why}"),
            },
        }
    }
}

/// A check's positive control (ER-33): a small tree and the findings the check must report on it, as `(file, line)`. The host runs
/// the check itself over it, so the prefilters and the file loop are exercised, not only the scanner.
pub struct Control {
    /// `(path, text)` of the control tree: at least one site that must be found, one that must not, and a path the check skips.
    pub files: Vec<(&'static str, &'static str)>,
    /// The findings the check must report, sorted.
    pub expect: Vec<(&'static str, usize)>,
}

/// A repository check.
pub trait Check {
    /// `C39`.
    fn id(&self) -> &'static str;
    /// The rule it enforces.
    fn rule(&self) -> u32;
    /// A short title.
    fn title(&self) -> &'static str;
    /// Runs over the context.
    fn run(&self, ctx: &Ctx) -> CheckOutput;
    /// Its positive control.
    fn control(&self) -> Control;
    /// Where it runs: the repository root, the apps, or both (`scope` of `listChecks`).
    fn scope(&self) -> CheckScope {
        CheckScope::Both
    }
    /// Whether its findings can fail a run (`mode` of `listChecks`).
    fn mode(&self) -> Mode {
        Mode::Gate
    }
    /// What an app needs for the check to apply (`needs` of `listChecks`).
    fn needs(&self) -> Needs {
        Needs::All
    }
}

/// Where a check runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckScope {
    /// The repository root only.
    Root,
    /// Apps only.
    Apps,
    /// Both.
    Both,
}

impl CheckScope {
    /// The name in `list --json`.
    pub fn name(self) -> &'static str {
        match self {
            CheckScope::Root => "root",
            CheckScope::Apps => "apps",
            CheckScope::Both => "both",
        }
    }
}

/// Whether a check's findings can fail a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Errors fail the run.
    Gate,
    /// Reported only (warnings; `--strict` makes them fail).
    ReportOnly,
}

impl Mode {
    /// The name in `list --json`.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Gate => "gate",
            Mode::ReportOnly => "report-only",
        }
    }
}

/// What an app needs for a check to apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    /// Nothing.
    All,
    /// A Rust workspace.
    Rust,
    /// A UI.
    Ui,
}

impl Needs {
    /// The name in `list --json`.
    pub fn name(self) -> &'static str {
        match self {
            Needs::All => "all",
            Needs::Rust => "rust",
            Needs::Ui => "ui",
        }
    }
}

/// The inputs of a run.
pub struct RunOptions {
    /// The repository root.
    pub root: PathBuf,
    /// The run's config: the app name in the report (`paddock` for the monorepo itself) and the keys of `paddock.conformance.json`.
    pub config: Config,
    /// `--scope apps/<app>` (C99), or none.
    pub scope: Option<String>,
    /// Only these check ids (upper case), or all.
    pub only: Option<Vec<String>>,
    /// Only these files (repository-relative, POSIX), or the whole tree.
    pub files: Option<Vec<String>>,
    /// Today, `YYYY-MM-DD` (UTC), for waiver expiry.
    pub today: String,
    /// The ref baselines are compared with (tests pin it); `None` reads `PADDOCK_BASE_REF`, then `origin/main`.
    pub base_ref: Option<String>,
}

/// The result of a run.
pub struct RunResult {
    /// The app name.
    pub app: String,
    /// The ids that ran.
    pub checks: Vec<String>,
    /// Kept findings, including the waiver engine's.
    pub findings: Vec<Finding>,
    /// Findings a waiver covered.
    pub waived: Vec<Waived>,
    /// The run records, one per check that ran.
    pub runs: Vec<RunEntry>,
    /// `--files` entries the tree does not have (a deleted file is fine; a typo is not), as given.
    pub unknown_files: Vec<String>,
}

/// A run record with the number of findings the check produced before and after waivers.
pub struct RunEntry {
    /// The record.
    pub record: RunRecord,
    /// Findings before waivers.
    pub raw: usize,
    /// Findings left after waivers (the unwaivable ones included).
    pub kept: usize,
}

impl RunResult {
    /// Findings of severity error.
    pub fn errors(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Error).count()
    }

    /// Findings of severity warn.
    pub fn warnings(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Warn).count()
    }

    /// The report JSON: the JavaScript host's shape, then `runs`.
    pub fn json(&self) -> Value {
        json!({
            "app": self.app,
            "checks": self.checks,
            "findings": self.findings.iter().map(Finding::json).collect::<Vec<_>>(),
            "waived": self.waived.iter().map(Waived::json).collect::<Vec<_>>(),
            "errors": self.errors(),
            "warnings": self.warnings(),
            "runs": self.runs.iter().map(|e| e.record.json(e.raw, e.kept)).collect::<Vec<_>>(),
            "unknownFiles": self.unknown_files,
        })
    }

    /// The log: one `::error` annotation and one plain line per finding (as `formatText` of the JavaScript host), the waived ones,
    /// the run records and the summary.
    pub fn text(&self) -> String {
        let mut out = Vec::new();
        let where_ = |f: &Finding| {
            if f.file.is_empty() {
                "-".to_string()
            } else {
                format!("{}:{}", f.file, f.line)
            }
        };
        for f in &self.findings {
            let level = if f.severity == Severity::Error { "::error" } else { "::warning" };
            out.push(format!(
                "{level} file={},line={}::{} (rule {}) {} Fix: {}",
                f.file, f.line, f.check, f.rule, f.message, f.fix
            ));
            out.push(format!(
                "{:<5} {} {}  {}\n        fix: {}",
                f.severity.name(),
                f.check,
                where_(f),
                f.message,
                f.fix
            ));
        }
        for w in &self.waived {
            out.push(format!(
                "waived {} {}  {} (until {}: {})",
                w.finding.check,
                where_(&w.finding),
                w.finding.message,
                w.expires,
                w.reason
            ));
        }
        if !self.unknown_files.is_empty() {
            out.push(format!(
                "::warning::--files names {} file(s) the tree does not have: {}. A deleted file is fine; a typo narrows the run silently.",
                self.unknown_files.len(),
                self.unknown_files.join(", ")
            ));
        }
        for e in &self.runs {
            let (r, kept) = (&e.record, e.kept);
            out.push(format!(
                "run {}: {} (ran {}, completed {}, precondition {}, control {}, probes {}, scope {}, os {}, raw findings {}, kept {})",
                r.check,
                r.status(kept > 0).name(),
                r.ran,
                r.completed,
                if r.precondition.met { "met" } else { "NOT met" },
                r.control,
                r.probes,
                r.scope.name(),
                r.os,
                e.raw,
                kept
            ));
        }
        out.push(format!(
            "{}: {} checks, {} errors, {} warnings, {} waived",
            self.app,
            self.checks.len(),
            self.errors(),
            self.warnings(),
            self.waived.len()
        ));
        out.join("\n")
    }
}

/// A run that cannot start: an id no check has.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownCheck(pub Vec<String>);

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// Runs the check over its own control tree: it must report exactly the expected findings, read at least one file and complete.
fn control_holds(check: &dyn Check) -> bool {
    let c = check.control();
    let tree = Tree::in_memory(&c.files);
    let config = Config::bare("paddock");
    let ctx = Ctx::new(&tree, tree.files(), None, &config, None, String::new());
    let Ok(out) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check.run(&ctx))) else {
        return false;
    };
    let mut got: Vec<(String, usize)> = out
        .findings
        .iter()
        .filter(|f| f.inline_waiver.is_none() || f.unwaivable)
        .map(|f| (f.file.clone(), f.line))
        .collect();
    got.sort();
    let mut want: Vec<(String, usize)> = c.expect.iter().map(|(f, l)| (f.to_string(), *l)).collect();
    want.sort();
    out.completed && out.precondition.met && out.probes > 0 && got == want
}

/// Runs the selected checks of `registry` over the tree. A check that panics, did not complete, failed its control, had no
/// precondition or looked at nothing adds an error no waiver covers, whatever else it found; waivers are applied last.
pub fn run_checks(opts: &RunOptions, registry: &[Box<dyn Check>]) -> Result<RunResult, UnknownCheck> {
    run_checks_in(&Tree::open(&opts.root), opts, registry)
}

/// [`run_checks`] over a tree the caller opened (a test passes one that reports a listing error).
pub fn run_checks_in(tree: &Tree, opts: &RunOptions, registry: &[Box<dyn Check>]) -> Result<RunResult, UnknownCheck> {
    let only = opts.only.clone().unwrap_or_default();
    let unknown: Vec<String> = only
        .iter()
        .filter(|id| !registry.iter().any(|c| c.id() == id.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(UnknownCheck(unknown));
    }
    let selected: Vec<&Box<dyn Check>> = registry
        .iter()
        .filter(|c| opts.only.is_none() || only.iter().any(|id| id == c.id()))
        .collect();
    let (scoped, scope, unknown_files) = match &opts.files {
        None => (tree.files(), Scope::Root, Vec::new()),
        Some(wanted) => {
            let norm = |f: &str| f.replace('\\', "/").trim_start_matches("./").to_string();
            let set: BTreeSet<String> = wanted.iter().map(|f| norm(f)).collect();
            let known: Vec<String> = tree.files().iter().filter(|f| set.contains(f.as_str())).cloned().collect();
            let unknown: Vec<String> = wanted.iter().filter(|f| !known.contains(&norm(f))).cloned().collect();
            (Rc::new(known.clone()), Scope::Files(known), unknown)
        }
    };
    let ctx = Ctx::new(
        tree,
        scoped.clone(),
        opts.base_ref.clone(),
        &opts.config,
        opts.scope.clone(),
        opts.today.clone(),
    );

    let mut findings: Vec<Finding> = unknown_files
        .iter()
        .map(|f| {
            Finding::warn(
                "FILES",
                0,
                f,
                1,
                "--files names a file the tree does not have (a deleted file is fine; a typo narrows the run silently)",
                "Fix the path, or drop it if the file was deleted.",
            )
        })
        .collect();
    let mut runs = Vec::new();
    let mut inline_waived: Vec<Waived> = Vec::new();
    for check in &selected {
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check.run(&ctx)));
        let control = control_holds(check.as_ref());
        let mut record = RunRecord {
            check: check.id().into(),
            ran: true,
            completed: false,
            crash: None,
            unlisted: tree.unlisted(),
            precondition: Precondition {
                met: true,
                detail: String::new(),
            },
            control,
            probes: 0,
            scope: scope.clone(),
            os: RunRecord::this_os(),
            matched: Vec::new(),
        };
        let found = match attempt {
            Ok(out) => {
                record.completed = out.completed;
                record.precondition = out.precondition;
                record.probes = out.probes;
                out.findings
            }
            Err(p) => {
                record.crash = Some(panic_text(p.as_ref()));
                Vec::new()
            }
        };
        // A finding waived by a comment at its site is waived, not found (the JavaScript runner does the same).
        let (inline, mut found): (Vec<Finding>, Vec<Finding>) = found.into_iter().partition(|f| f.inline_waiver.is_some() && !f.unwaivable);
        inline_waived.extend(inline.into_iter().map(|f| {
            let reason = f.inline_waiver.clone().unwrap_or_default();
            Waived {
                finding: f,
                reason,
                expires: "inline".into(),
            }
        }));
        record.matched = found
            .iter()
            .filter(|f| !f.file.is_empty())
            .map(|f| format!("{}:{}", f.file, f.line))
            .collect();
        let raw = found.len();
        // ER-33: the record decides its status and the error together; the error is added whatever else the check found, and no
        // waiver covers it (a waived finding next to an unreadable file must not read as a pass).
        if let (status, Some(message)) = record.judge(false) {
            let outcome = if status == record::Status::CheckBroken || record.crash.is_some() {
                Outcome::CheckBroken
            } else {
                Outcome::NotMeasured
            };
            found.push(
                Finding::error(
                    check.id(),
                    check.rule(),
                    "",
                    1,
                    message,
                    "Fix the check or its input; a run that did not look is not a pass.",
                )
                .unwaivable()
                .with_outcome(outcome),
            );
        }
        findings.extend(found);
        runs.push(RunEntry { record, raw, kept: 0 });
    }

    let waiver_text = tree.read(waivers::FILE);
    let (parsed, problems) = waiver_text.as_ref().map_or((Vec::new(), Vec::new()), |t| waivers::parse(t));
    let checks_run: Vec<String> = selected.iter().map(|c| c.id().to_string()).collect();
    let scoped_set: Rc<Vec<String>> = scoped.clone();
    let narrowed = matches!(scope, Scope::Files(_));
    // A waiver the run could not have used: a scoped run that did not scan any file the waiver covers.
    let skip_unused = move |w: &waivers::Waiver| {
        narrowed
            && !scoped_set
                .iter()
                .any(|f| waivers::path_matches(w.path.as_deref().unwrap_or_default(), f))
    };
    let (kept, mut waived) = waivers::apply(waivers::Applying {
        findings,
        waivers: &parsed,
        problems: &problems,
        today: &opts.today,
        checks_run: &checks_run,
        skip_unused: &skip_unused,
    });
    waived.extend(inline_waived);
    for e in &mut runs {
        e.kept = kept.iter().filter(|f| f.check == e.record.check).count();
    }
    Ok(RunResult {
        app: opts.config.app().to_string(),
        checks: checks_run,
        findings: kept,
        waived,
        runs,
        unknown_files,
    })
}

/// Today as `YYYY-MM-DD` (UTC), from the system clock.
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil_date((secs / 86_400) as i64)
}

/// The civil date of a day count since 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// The arguments of `./dev check run`.
pub struct Args {
    /// Only these check ids.
    pub only: Option<Vec<String>>,
    /// Only these files.
    pub files: Option<Vec<String>>,
    /// The repository root.
    pub root: Option<PathBuf>,
    /// The app name for the report.
    pub app: String,
    /// Write the report JSON here as well.
    pub out: Option<PathBuf>,
}

fn find_root(given: Option<PathBuf>) -> Result<PathBuf, CliError> {
    if let Some(r) = given {
        return Ok(r);
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let out = crate::input_hash::Exec::run(&crate::input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
    Ok(Path::new(out.trim()).to_path_buf())
}

/// `./dev check run`: runs the checks and reports. Exit 5 (findings) when an error was found.
pub fn command(args: Args) -> Result<Report, CliError> {
    let root = find_root(args.root)?;
    let config = Config::load(&root, Some(&args.app)).map_err(CliError::failure)?;
    let opts = RunOptions {
        root,
        config,
        scope: None,
        only: args.only.map(|v| v.iter().map(|s| s.trim().to_uppercase()).collect()),
        files: args.files,
        today: today_utc(),
        base_ref: None,
    };
    let result = run_checks(&opts, &registry()).map_err(|u| {
        let known: Vec<&str> = registry().iter().map(|c| c.id()).collect();
        CliError::usage(format!("unknown check: {}.", u.0.join(", "))).with_hint(format!(
            "`./dev check run --only` knows {} so far; the rest still run in conformance/.",
            known.join(", ")
        ))
    })?;
    let json = result.json();
    if let Some(out) = &args.out {
        std::fs::write(out, format!("{}\n", serde_json::to_string_pretty(&json).unwrap_or_default())).map_err(|e| {
            CliError::failure(format!("::error::cannot write {}: {e}.", out.display())).with_hint("Choose a folder you can write to.")
        })?;
    }
    let exit = if result.errors() > 0 { Exit::Findings } else { Exit::Ok };
    Ok(Report::new(json, result.text()).with_exit(exit))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A check that reads every `.rs` file and finds nothing.
    struct Reader;

    impl Check for Reader {
        fn id(&self) -> &'static str {
            "S1"
        }
        fn rule(&self) -> u32 {
            1
        }
        fn title(&self) -> &'static str {
            "reader"
        }
        fn control(&self) -> Control {
            Control {
                files: vec![("c.rs", "x")],
                expect: vec![],
            }
        }
        fn run(&self, ctx: &Ctx) -> CheckOutput {
            CheckOutput {
                findings: vec![],
                probes: ctx.files().iter().filter(|f| f.ends_with(".rs")).count(),
                completed: true,
                precondition: Precondition {
                    met: true,
                    detail: String::new(),
                },
            }
        }
    }

    fn run_over_unlisted(files: Option<Vec<String>>, unlisted: &[&str]) -> RunResult {
        let tree = Tree::in_memory_with_unlisted(&[("a.rs", "x")], unlisted);
        let opts = RunOptions {
            root: PathBuf::new(),
            config: Config::bare("t"),
            scope: None,
            only: None,
            files,
            today: "2026-11-10".into(),
            base_ref: None,
        };
        run_checks_in(&tree, &opts, &[Box::new(Reader)]).unwrap()
    }

    #[test]
    fn a_tree_that_could_not_be_listed_is_an_unwaivable_error_next_to_a_clean_looking_run() {
        let r = run_over_unlisted(None, &["locked/dir"]);
        let broken: Vec<&Finding> = r.findings.iter().filter(|f| f.unwaivable).collect();
        assert_eq!(broken.len(), 1, "{:?}", r.findings);
        assert!(broken[0].message.contains("could not be listed"), "{}", broken[0].message);
        assert!(
            broken[0].message.contains("locked/dir"),
            "the folder is named: {}",
            broken[0].message
        );
        assert!(r.errors() >= 1);
        assert_eq!(r.runs[0].record.status(r.runs[0].kept > 0), record::Status::NotMeasured);
        assert_eq!(
            run_over_unlisted(None, &[]).errors(),
            0,
            "the same run over a listable tree is clean"
        );
    }

    #[test]
    fn a_scoped_run_fails_closed_on_an_unlistable_folder_and_names_it() {
        let r = run_over_unlisted(Some(vec!["a.rs".into()]), &["elsewhere"]);
        let broken: Vec<&Finding> = r.findings.iter().filter(|f| f.unwaivable).collect();
        assert_eq!(broken.len(), 1, "{:?}", r.findings);
        let m = &broken[0].message;
        assert!(m.contains("elsewhere") && m.contains("outside the scoped files"), "{m}");
    }
}
