//! `./dev check run`: the host of Paddock's repository checks in Rust (RT-5, ADR 0022's conformance rules engine). It runs the
//! checks ported so far ([`c39`], [`c53`]) over a tree, with
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
pub mod c39;
pub mod c53;
pub mod finding;
pub mod jsre;
pub mod mask;
pub mod record;
pub mod tree;
pub mod waivers;

use finding::{Finding, Severity, Waived};
use paddock_cli::{CliError, Exit, Report};
use record::{Precondition, RunRecord, Scope};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tree::Tree;

/// What a check sees: the tree, narrowed to the run's files.
pub struct Ctx<'a> {
    tree: &'a Tree,
    scoped: Rc<Vec<String>>,
}

impl Ctx<'_> {
    /// The files this run looks at (all of the tree, or the `--files` that the tree knows), sorted.
    pub fn files(&self) -> Rc<Vec<String>> {
        self.scoped.clone()
    }

    /// Every file of the tree, whatever the scope (for a precondition).
    pub fn tree_files(&self) -> Rc<Vec<String>> {
        self.tree.files()
    }

    /// A file's text, or `None` when it cannot be read.
    pub fn read(&self, rel: &str) -> Option<Rc<String>> {
        self.tree.read(rel)
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
    /// Its positive control saw what it is built to see.
    pub control: bool,
    /// What its input had to be.
    pub precondition: Precondition,
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
}

/// The checks the host has, in the order of `conformance/src/checks/index.mjs`.
pub fn registry() -> Vec<Box<dyn Check>> {
    vec![Box::new(c39::C39), Box::new(c53::C53)]
}

/// The inputs of a run.
pub struct RunOptions {
    /// The repository root.
    pub root: PathBuf,
    /// The app name in the report (`paddock` for the monorepo itself).
    pub app: String,
    /// Only these check ids (upper case), or all.
    pub only: Option<Vec<String>>,
    /// Only these files (repository-relative, POSIX), or the whole tree.
    pub files: Option<Vec<String>>,
    /// Today, `YYYY-MM-DD` (UTC), for waiver expiry.
    pub today: String,
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
    /// The run records, one per check that ran, with whether the check produced a finding.
    pub runs: Vec<(RunRecord, bool)>,
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
            "runs": self.runs.iter().map(|(r, found)| r.json(*found)).collect::<Vec<_>>(),
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
        for (r, found) in &self.runs {
            out.push(format!(
                "run {}: {} (ran {}, completed {}, precondition {}, control {}, probes {}, scope {}, os {})",
                r.check,
                r.status(*found).name(),
                r.ran,
                r.completed,
                if r.precondition.met { "met" } else { "NOT met" },
                r.control,
                r.probes,
                r.scope.name(),
                r.os
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

/// Runs the selected checks of `registry` over the tree. A check that panics is an error finding (it fails closed), a check that
/// could not look is an error no waiver covers, and waivers are applied last.
pub fn run_checks(opts: &RunOptions, registry: &[Box<dyn Check>]) -> Result<RunResult, UnknownCheck> {
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
    let tree = Tree::open(&opts.root);
    let (scoped, scope) = match &opts.files {
        None => (tree.files(), Scope::Root),
        Some(wanted) => {
            let set: BTreeSet<&str> = wanted.iter().map(|f| f.trim_start_matches("./")).collect();
            let known: Vec<String> = tree.files().iter().filter(|f| set.contains(f.as_str())).cloned().collect();
            let n = known.len();
            (Rc::new(known), Scope::Files(n))
        }
    };
    let ctx = Ctx {
        tree: &tree,
        scoped: scoped.clone(),
    };

    let mut findings: Vec<Finding> = Vec::new();
    let mut runs = Vec::new();
    for check in &selected {
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check.run(&ctx)));
        let (mut found, record) = match attempt {
            Ok(out) => {
                let record = RunRecord {
                    check: check.id().into(),
                    ran: true,
                    completed: out.completed,
                    precondition: out.precondition,
                    control: out.control,
                    probes: out.probes,
                    scope: scope.clone(),
                    os: RunRecord::this_os(),
                };
                (out.findings, record)
            }
            Err(p) => {
                let record = RunRecord {
                    check: check.id().into(),
                    ran: true,
                    completed: false,
                    precondition: Precondition {
                        met: true,
                        detail: "the check panicked".into(),
                    },
                    control: true,
                    probes: 0,
                    scope: scope.clone(),
                    os: RunRecord::this_os(),
                };
                let f = Finding::error(
                    check.id(),
                    check.rule(),
                    "",
                    1,
                    format!("the check crashed: {}", panic_text(p.as_ref())),
                    "Fix the check or the input it could not read; a check that cannot run never passes.",
                );
                (vec![f], record)
            }
        };
        // ER-33: a check that could not look never reads as clean, and no waiver covers that.
        let why = if !record.control {
            Some((
                "check broken",
                "its positive control no longer sees what the check is built to see".to_string(),
            ))
        } else if !record.precondition.met {
            Some(("not measured", format!("its precondition failed: {}", record.precondition.detail)))
        } else if !record.completed && found.is_empty() {
            Some(("not measured", "it did not complete (a file could not be read)".to_string()))
        } else if record.probes == 0 && scope == Scope::Root {
            Some(("not measured", "it looked at no file on a whole-tree run".to_string()))
        } else {
            None
        };
        if let Some((what, detail)) = why {
            found.push(
                Finding::error(
                    check.id(),
                    check.rule(),
                    "",
                    1,
                    format!("{what}: {} {detail}", check.id()),
                    "Fix the check or its input; a run that did not look is not a pass.",
                )
                .unwaivable(),
            );
        }
        let any = !found.is_empty();
        findings.extend(found);
        runs.push((record, any));
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
    let (kept, waived) = waivers::apply(waivers::Applying {
        findings,
        waivers: &parsed,
        problems: &problems,
        today: &opts.today,
        checks_run: &checks_run,
        skip_unused: &skip_unused,
    });
    Ok(RunResult {
        app: opts.app.clone(),
        checks: checks_run,
        findings: kept,
        waived,
        runs,
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
    let opts = RunOptions {
        root,
        app: args.app,
        only: args.only.map(|v| v.iter().map(|s| s.trim().to_uppercase()).collect()),
        files: args.files,
        today: today_utc(),
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
