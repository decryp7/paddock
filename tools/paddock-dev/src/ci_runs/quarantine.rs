//! The quarantine register of FLAKY tests, `docs/flaky-quarantine.toml` (user decision 2026-11-24: ER-12's grading is the verdict
//! reader's, as Google TAP and Chromium do it).
//!
//! The one grader ([`super::green`]) decides; this module only keeps the record:
//! - a test that failed both tries blocks, always; a FLAKY test (failed, then passed its one retry) in touched code blocks; a FLAKY test
//!   in untouched code is a fix item and does not block ([`super::green::grade_flaky`]). The register never changes a grade: a
//!   quarantined test that flakes again in touched code blocks, and one that fails both tries blocks.
//! - **Growth comes only from the tool's own recording in a landing step**: `./dev ci fix-items` writes the lessons bullet of the run
//!   ([`super::fix_items`]) and the register entries in the same step ([`record`]). The register is shrink-only against the merge base
//!   (ER-46, `policy = "shrink-only"` in its header): [`growth`] refuses an entry the merge base lacks unless the same diff adds the
//!   lessons bullet that recorded it (its run and its test, in the shape only `ci fix-items` writes). A guard rail against hand edits,
//!   not a security boundary (anyone can write both by hand, deliberately). Conformance C168 runs it ([`crate::check::c168`]: CI's
//!   conformance job, the train gates and `./dev check --pre-push`); `./dev ci quarantine-check` runs the same comparison by hand.
//! - **Release** ([`release`]): an entry whose test's jobs all passed in [`RELEASE_AFTER`] consecutive nightlies is removed by
//!   `./dev ci quarantine-release`. Its fix item stays in docs/lessons.md until a fix lands.
//! - **Report-only first (ER-41)**: the package-level grade decides only once its first green use is recorded here by run id
//!   (`[first_use]`, written by `./dev ci quarantine-record`, [`with_first_use`]); until then the job level decides and the package level
//!   is printed as `::notice::` lines ([`notice`]) in the verdict output and the job summary.

use super::green::{FixItem, nextest_parts};
use std::collections::BTreeSet;
use std::path::Path;

/// The register, relative to the repository root.
pub const REGISTER: &str = "docs/flaky-quarantine.toml";

/// Consecutive nightlies in which every job of a quarantined test passed before `quarantine-release` removes it. 5 is the escape
/// register's `required` (ADR 0117 D12: five consecutive clean nightlies, a working week). It is enough because a release ends only the
/// quarantine record: the fix item stays, and a test that flakes again is graded and recorded again (in touched code it blocks as always).
pub const RELEASE_AFTER: u32 = 5;

/// Who writes an entry: the one recorder.
pub const RECORDED_BY: &str = "paddock-dev ci fix-items";

const HEADER: &str = "\
# The flaky-test quarantine (ER-12, user decision 2026-11-24; tools/paddock-dev/src/ci_runs/quarantine.rs, the one reader and writer).
# A FLAKY test (failed, then passed its one retry) in code the change did not touch does not block a landing: `./dev ci verdict` grades
# it, and `./dev ci fix-items` records it here and as a fix item in docs/lessons.md in the same step. A test that failed both tries, or
# a FLAKY test in touched code, always blocks; an entry here never changes a grade.
#
# policy: shrink-only against the merge base (ER-46). An entry appears only through `./dev ci fix-items` in a landing step, with the
# lessons bullet of its run in the same diff; conformance C168 refuses any other growth. An entry leaves when its fix lands (delete it
# in the fix's commit) or through `./dev ci quarantine-release` after `release_after` consecutive nightlies in which every job of the
# test passed.
#
# [first_use] (ER-41): the package-level grade (the Cargo package that holds the test, from the planner's graph) decides only after its
# first green use is recorded by run id with `./dev ci quarantine-record --workflow ci.yml --run <id>` (after `ci fix-items` recorded
# that run; C168 refuses a hand-written one); until then the job level decides and the package level is printed as ::notice:: lines.
#
# id           workspace, binary and name of a nextest test; spec and title of a Playwright test
# test         the test as the runner names it (the lessons bullet holds the same text)
# jobs         the CI jobs it flaked in
# first_run    the run that recorded it; last_run the newest; count the runs it flaked in
# fix_item     where the fix item is (docs/lessons.md, the bullet of first_run)
# added        the date it was recorded
";

/// The id of a test across runs and OS: `workspace binary name` for a nextest test (the workspace from its job), `spec › title` for a
/// Playwright one (the project and the line dropped: the job names the engine, and a line moves).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestId {
    /// A nextest test.
    Nextest {
        /// The workspace folder (`crates`, `tools/paddock-dev`, `apps/<app>`) or the job's project.
        workspace: String,
        /// The nextest binary id (`paddock-chromium::it`).
        binary: String,
        /// The test name.
        name: String,
    },
    /// A Playwright test.
    Spec {
        /// The spec file.
        spec: String,
        /// The title path.
        title: String,
    },
}

/// The workspace a job's nextest tests belong to.
fn workspace_of_job(job: &str) -> String {
    if job.starts_with("crate (") || job.starts_with("crates (") {
        "crates".into()
    } else if job.starts_with("paddock-dev (") {
        "tools/paddock-dev".into()
    } else if let Some(rest) = job.strip_prefix("app ") {
        format!("apps/{}", rest.split(" (").next().unwrap_or(rest))
    } else {
        job.split(" (").next().unwrap_or(job).to_string()
    }
}

impl TestId {
    /// The id of `test` as the log reader names it in job `job`.
    pub fn of(job: &str, test: &str) -> TestId {
        if let Some((binary, name)) = nextest_parts(test) {
            return TestId::Nextest {
                workspace: workspace_of_job(job),
                binary: binary.into(),
                name: name.into(),
            };
        }
        // `[project] › path:line:col › title`, the project optional.
        let rest = match test.trim().split_once(" › ") {
            Some((p, r)) if p.starts_with('[') && p.ends_with(']') => r,
            _ => test.trim(),
        };
        let (loc, title) = rest.split_once(" › ").unwrap_or((rest, ""));
        let mut spec = loc;
        for _ in 0..2 {
            if let Some((head, tail)) = spec.rsplit_once(':')
                && !tail.is_empty()
                && tail.bytes().all(|b| b.is_ascii_digit())
            {
                spec = head;
            }
        }
        TestId::Spec {
            spec: spec.into(),
            title: title.into(),
        }
    }

    /// The key of the register.
    pub fn key(&self) -> String {
        match self {
            TestId::Nextest { workspace, binary, name } => format!("{workspace} {binary} {name}"),
            TestId::Spec { spec, title } => format!("{spec} › {title}"),
        }
    }
}

/// One quarantined test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// [`TestId::key`].
    pub id: String,
    /// The test as the runner names it.
    pub test: String,
    /// The workflow of its runs.
    pub workflow: String,
    /// The jobs it flaked in.
    pub jobs: Vec<String>,
    /// The run that recorded it.
    pub first_run: u64,
    /// The newest run it flaked in.
    pub last_run: u64,
    /// How many runs it flaked in.
    pub count: u64,
    /// Where its fix item is.
    pub fix_item: String,
    /// The date it was recorded (`YYYY-MM-DD`).
    pub added: String,
    /// Who recorded it ([`RECORDED_BY`]).
    pub recorded_by: String,
}

/// The recorded first green use of the package-level grade (ER-41).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstUse {
    /// The workflow.
    pub workflow: String,
    /// The run.
    pub run: u64,
    /// The date it was recorded.
    pub recorded: String,
}

/// The register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    /// [`RELEASE_AFTER`] as the file states it.
    pub release_after: u32,
    /// Set once the package level decides.
    pub first_use: Option<FirstUse>,
    /// The entries, in the order they were recorded.
    pub entries: Vec<Entry>,
}

impl Default for Register {
    fn default() -> Register {
        Register {
            release_after: RELEASE_AFTER,
            first_use: None,
            entries: Vec::new(),
        }
    }
}

/// The keys of a `[[quarantine]]` entry.
const ENTRY_KEYS: [&str; 10] = [
    "id",
    "test",
    "workflow",
    "jobs",
    "first_run",
    "last_run",
    "count",
    "fix_item",
    "added",
    "recorded_by",
];

fn str_of(t: &toml::Table, k: &str, at: &str) -> Result<String, String> {
    t.get(k)
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{REGISTER}: {at} has no string `{k}`"))
}

fn int_of(t: &toml::Table, k: &str, at: &str) -> Result<u64, String> {
    t.get(k)
        .and_then(toml::Value::as_integer)
        .and_then(|i| u64::try_from(i).ok())
        .ok_or_else(|| format!("{REGISTER}: {at} has no non-negative integer `{k}`"))
}

impl Register {
    /// Parses the file (ER-42: the one reader). Unknown keys, a policy other than shrink-only and malformed entries are refused.
    pub fn parse(text: &str) -> Result<Register, String> {
        let v: toml::Table = text.parse().map_err(|e| format!("{REGISTER} is not TOML: {e}"))?;
        for k in v.keys() {
            if !["version", "policy", "release_after", "first_use", "quarantine"].contains(&k.as_str()) {
                return Err(format!("{REGISTER}: unknown key `{k}`"));
            }
        }
        if v.get("version").and_then(toml::Value::as_integer) != Some(1) {
            return Err(format!("{REGISTER}: `version` must be 1"));
        }
        if v.get("policy").and_then(toml::Value::as_str) != Some("shrink-only") {
            return Err(format!("{REGISTER}: `policy` must be \"shrink-only\" (ER-46)"));
        }
        let release_after = u32::try_from(int_of(&v, "release_after", "the header")?)
            .ok()
            .filter(|n| *n >= RELEASE_AFTER)
            .ok_or_else(|| format!("{REGISTER}: `release_after` must be at least {RELEASE_AFTER}"))?;
        let first_use = match v.get("first_use") {
            None => None,
            Some(toml::Value::Table(t)) if t.keys().all(|k| ["workflow", "run", "recorded"].contains(&k.as_str())) => Some(FirstUse {
                workflow: str_of(t, "workflow", "[first_use]")?,
                run: int_of(t, "run", "[first_use]")?,
                recorded: str_of(t, "recorded", "[first_use]")?,
            }),
            Some(_) => return Err(format!("{REGISTER}: `first_use` must be a table of workflow, run and recorded")),
        };
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();
        for (i, e) in v
            .get("quarantine")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let at = format!("[[quarantine]] {}", i + 1);
            let t = e.as_table().ok_or_else(|| format!("{REGISTER}: {at} is not a table"))?;
            if let Some(k) = t.keys().find(|k| !ENTRY_KEYS.contains(&k.as_str())) {
                return Err(format!("{REGISTER}: {at} has an unknown key `{k}`"));
            }
            let jobs = t
                .get("jobs")
                .and_then(toml::Value::as_array)
                .map(|a| a.iter().filter_map(toml::Value::as_str).map(str::to_string).collect::<Vec<_>>())
                .filter(|j| !j.is_empty())
                .ok_or_else(|| format!("{REGISTER}: {at} has no `jobs`"))?;
            let entry = Entry {
                id: str_of(t, "id", &at)?,
                test: str_of(t, "test", &at)?,
                workflow: str_of(t, "workflow", &at)?,
                jobs,
                first_run: int_of(t, "first_run", &at)?,
                last_run: int_of(t, "last_run", &at)?,
                count: int_of(t, "count", &at)?,
                fix_item: str_of(t, "fix_item", &at)?,
                added: str_of(t, "added", &at)?,
                recorded_by: str_of(t, "recorded_by", &at)?,
            };
            if !seen.insert(entry.id.clone()) {
                return Err(format!("{REGISTER}: {at} repeats the id {:?}", entry.id));
            }
            entries.push(entry);
        }
        Ok(Register {
            release_after,
            first_use,
            entries,
        })
    }

    /// The file's text: the header, then the entries (the one writer).
    pub fn render(&self) -> String {
        let mut out = String::from(HEADER);
        out.push_str(&format!(
            "version = 1\npolicy = \"shrink-only\"\nrelease_after = {}\n",
            self.release_after
        ));
        if let Some(f) = &self.first_use {
            out.push_str(&format!(
                "\n[first_use]\nworkflow = {}\nrun = {}\nrecorded = {}\n",
                q(&f.workflow),
                f.run,
                q(&f.recorded)
            ));
        }
        for e in &self.entries {
            let jobs: Vec<String> = e.jobs.iter().map(|j| q(j)).collect();
            out.push_str(&format!(
                "\n[[quarantine]]\nid = {}\ntest = {}\nworkflow = {}\njobs = [{}]\nfirst_run = {}\nlast_run = {}\ncount = {}\nfix_item = {}\nadded = {}\nrecorded_by = {}\n",
                q(&e.id),
                q(&e.test),
                q(&e.workflow),
                jobs.join(", "),
                e.first_run,
                e.last_run,
                e.count,
                q(&e.fix_item),
                q(&e.added),
                q(&e.recorded_by)
            ));
        }
        out
    }
}

/// A TOML basic string.
fn q(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The register of the checkout at `root`: an absent file is an empty register (nothing quarantined, the package level not recorded).
pub fn read_in(root: &Path) -> Result<Register, String> {
    match std::fs::read_to_string(root.join(REGISTER)) {
        Ok(text) => Register::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Register::default()),
        Err(e) => Err(format!("{REGISTER} cannot be read: {e}")),
    }
}

/// Whether the package level decides in the checkout at `root` ([`Register::first_use`] is recorded). A register that cannot be read
/// keeps the job level, the stricter one, with a warning.
pub fn narrow_in(root: &Path) -> bool {
    match read_in(root) {
        Ok(r) => r.first_use.is_some(),
        Err(why) => {
            eprintln!("::warning::flaky quarantine: {why}; the job level decides (the stricter grade)");
            false
        }
    }
}

thread_local! {
    static NOTICES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Prints one grade as a `::notice::` line and keeps it for the job summary ([`take_notices`]): the verdict output and the summary
/// show what the package level says during the report-only phase (ER-41) and after it.
pub fn notice(line: &str) {
    eprintln!("::notice::{line}");
    NOTICES.with(|n| n.borrow_mut().push(line.to_string()));
}

/// The notices printed since the last call; a command appends them to its job summary.
pub fn take_notices() -> Vec<String> {
    NOTICES.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

/// Appends the notices to `$GITHUB_STEP_SUMMARY` when it is set (a CI step); a missing or unwritable file is a warning, never an error.
pub fn write_summary(heading: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY").filter(|p| !p.is_empty()) else {
        return;
    };
    let mut text = format!("\n### {heading}\n\n");
    for l in lines {
        text.push_str(&format!("- {l}\n"));
    }
    let res = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
    if let Err(e) = res {
        eprintln!("::warning::the job summary cannot be written ({e}); the notices above are the record");
    }
}

/// The fix-item reference of a run, as [`super::fix_items::lessons_entry`] heads its bullet.
pub fn fix_item_ref(workflow: &str, run: u64, sha: &str) -> String {
    format!(
        "{}: full {workflow} run {run} ({})",
        super::fix_items::LESSONS,
        sha.get(..9).unwrap_or(sha)
    )
}

/// What [`record`] changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Recorded {
    /// New entries.
    pub added: Vec<String>,
    /// Entries whose count, newest run or jobs grew.
    pub updated: Vec<String>,
}

/// Records the fix items of `workflow` run `run` on `sha` (`today`: `YYYY-MM-DD`): a new test is a new entry, a known one counts the
/// run once and adds its job. Idempotent: the same run again changes nothing.
pub fn record(reg: &mut Register, workflow: &str, run: u64, sha: &str, items: &[FixItem], today: &str) -> Recorded {
    let mut done = Recorded::default();
    for i in items {
        let id = TestId::of(&i.job, &i.test).key();
        match reg.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                let mut changed = false;
                if run > e.last_run {
                    e.last_run = run;
                    e.count += 1;
                    changed = true;
                }
                if run >= e.last_run && !e.jobs.contains(&i.job) {
                    e.jobs.push(i.job.clone());
                    changed = true;
                }
                if changed && !done.added.contains(&id) && !done.updated.contains(&id) {
                    done.updated.push(id);
                }
            }
            None => {
                reg.entries.push(Entry {
                    id: id.clone(),
                    test: i.test.clone(),
                    workflow: workflow.into(),
                    jobs: vec![i.job.clone()],
                    first_run: run,
                    last_run: run,
                    count: 1,
                    fix_item: fix_item_ref(workflow, run, sha),
                    added: today.into(),
                    recorded_by: RECORDED_BY.into(),
                });
                done.added.push(id);
            }
        }
    }
    done
}

/// Records the fix items in the register of the checkout at `root` (an absent register is created).
pub fn record_in(root: &Path, workflow: &str, run: u64, sha: &str, items: &[FixItem], today: &str) -> Result<Recorded, String> {
    let mut reg = read_in(root)?;
    let done = record(&mut reg, workflow, run, sha, items, today);
    if !done.added.is_empty() || !done.updated.is_empty() {
        std::fs::write(root.join(REGISTER), reg.render()).map_err(|e| format!("{REGISTER} cannot be written: {e}"))?;
    }
    Ok(done)
}

/// The register text with the first green use recorded; refuses a register that has one already (the switch is made once).
pub fn with_first_use(text: Option<&str>, workflow: &str, run: u64, today: &str) -> Result<String, String> {
    let mut reg = match text {
        Some(t) => Register::parse(t)?,
        None => Register::default(),
    };
    if let Some(f) = &reg.first_use {
        return Err(format!(
            "{REGISTER} records the first use already ({} run {}): the package level decides",
            f.workflow, f.run
        ));
    }
    reg.first_use = Some(FirstUse {
        workflow: workflow.into(),
        run,
        recorded: today.into(),
    });
    Ok(reg.render())
}

/// The register's growth against the merge base (ER-46): every entry the base lacks must be the tool's own recording, witnessed by
/// the lessons bullet of its run that the same diff adds ([`super::fix_items::lessons_entry`]: `full <workflow> run <n> (` and the
/// test in backticks), recorded by [`RECORDED_BY`], first seen in the run that recorded it. One finding per entry that is not; an
/// absent base register is an empty one. A shrink, and a count or run that grows on a known entry, are not growth.
pub fn growth(base: Option<&str>, now: &str, base_lessons: &str, lessons: &str) -> Result<Vec<String>, String> {
    let now = Register::parse(now)?;
    let known: BTreeSet<String> = match base {
        Some(t) => Register::parse(t)
            .map(|r| r.entries.into_iter().map(|e| e.id).collect())
            .map_err(|e| format!("the merge base's register cannot be read: {e}"))?,
        None => BTreeSet::new(),
    };
    let mut out = Vec::new();
    for e in now.entries.iter().filter(|e| !known.contains(&e.id)) {
        let head = format!("full {} run {} (", e.workflow, e.first_run);
        let witnessed = e.recorded_by == RECORDED_BY
            && e.first_run == e.last_run
            && e.count == 1
            && lessons.contains(&head)
            && !base_lessons.contains(&head)
            && lessons.contains(&format!("`{}` (job ", e.test));
        if !witnessed {
            out.push(format!(
                "{REGISTER}: the entry {:?} is not in the register at the merge base and no lessons bullet of run {} added in the same diff records it: the register grows only through `./dev ci fix-items` (ER-46); remove the entry, or record the run with `./dev ci fix-items --workflow {} --sha <landed sha>`",
                e.id, e.first_run, e.workflow
            ));
        }
    }
    Ok(out)
}

/// Whether `lessons` holds the fix-item bullet of `workflow` run `run` ([`super::fix_items::lessons_entry`]'s head).
pub fn has_fix_items_of(lessons: &str, workflow: &str, run: u64) -> bool {
    lessons.contains(&format!("full {workflow} run {run} ("))
}

/// The `[first_use]` growth (ER-41's switch): a first use the merge base lacks, or one it records differently, must name a run whose
/// fix-item bullet is in `lessons` (`./dev ci quarantine-record` validates the run and refuses one that `ci fix-items` has not recorded,
/// so the switch is made only on a run the tool graded). `None`: no finding.
pub fn first_use_growth(base: Option<&str>, now: &str, lessons: &str) -> Result<Option<String>, String> {
    let now = Register::parse(now)?;
    let was = match base {
        Some(t) => Register::parse(t)
            .map(|r| r.first_use)
            .map_err(|e| format!("the merge base's register cannot be read: {e}"))?,
        None => None,
    };
    Ok(match now.first_use {
        Some(f) if was.as_ref() != Some(&f) && !has_fix_items_of(lessons, &f.workflow, f.run) => Some(format!(
            "{REGISTER}: [first_use] names {} run {}, which is not in the register at the merge base and has no fix-item bullet in {}: the package-level grade is switched on only by `./dev ci quarantine-record` on a run `ci fix-items` recorded (ER-41); remove it, or record the run with the tool",
            f.workflow,
            f.run,
            super::fix_items::LESSONS
        )),
        _ => None,
    })
}

/// [`growth`] and [`first_use_growth`] in the checkout at `root` against the merge base with `base_ref` (on the target branch itself, against the first
/// parent, as [`crate::check::baseline::growth`] measures). `Err`: not measured.
pub fn growth_in(root: &Path, base_ref: &str) -> Result<Vec<String>, String> {
    use crate::plan_repo::git_text;
    let head = git_text(root, &["rev-parse", "HEAD"]).ok_or("git cannot read HEAD")?;
    let mut base = git_text(root, &["merge-base", "HEAD", base_ref])
        .ok_or_else(|| format!("no merge base with {base_ref}: fetch it with full history, or set PADDOCK_BASE_REF"))?;
    if base.trim() == head.trim() {
        base = git_text(root, &["rev-parse", "HEAD^1"]).ok_or("HEAD has no first parent to compare with")?;
    }
    let base = base.trim();
    let at = |p: &str| git_text(root, &["show", &format!("{base}:{p}")]);
    let now = match std::fs::read_to_string(root.join(REGISTER)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{REGISTER} cannot be read: {e}")),
    };
    let lessons = std::fs::read_to_string(root.join(super::fix_items::LESSONS))
        .map_err(|e| format!("{} cannot be read: {e}", super::fix_items::LESSONS))?;
    let base_reg = at(REGISTER);
    let mut found = growth(
        base_reg.as_deref(),
        &now,
        &at(super::fix_items::LESSONS).unwrap_or_default(),
        &lessons,
    )?;
    found.extend(first_use_growth(base_reg.as_deref(), &now, &lessons)?);
    Ok(found)
}

/// One job of a nightly run: its name and conclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NightlyJob {
    /// The job's display name.
    pub name: String,
    /// Its conclusion (`success`, `failure`, `skipped`, `cancelled`...).
    pub conclusion: String,
}

/// A finished nightly run, as the release reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nightly {
    /// Its id.
    pub run: u64,
    /// Its jobs.
    pub jobs: Vec<NightlyJob>,
}

/// The consecutive nightlies, newest first, in which every job of `e` passed, and why the count stopped (`None`: it ran out of
/// nightlies). A nightly that ran none of the entry's jobs (the nightly's gate skipped the work: main unchanged) neither counts nor
/// stops the count; one that ran some but not all, or any of them not `success`, stops it (a FLAKY makes a job fail: flaky-as-failure).
pub fn streak(e: &Entry, newest_first: &[Nightly]) -> (u32, Option<String>) {
    let mut n = 0;
    for night in newest_first {
        let of: Vec<&NightlyJob> = night.jobs.iter().filter(|j| e.jobs.contains(&j.name)).collect();
        if of.is_empty() {
            continue;
        }
        if of.len() < e.jobs.len() {
            return (n, Some(format!("nightly run {} did not run every job of the test", night.run)));
        }
        if let Some(j) = of.iter().find(|j| j.conclusion != "success") {
            return (
                n,
                Some(format!("nightly run {}: job {:?} ended {}", night.run, j.name, j.conclusion)),
            );
        }
        n += 1;
    }
    (n, None)
}

/// What [`release`] did per entry: `(id, streak, released)`.
pub type Released = Vec<(String, u32, bool)>;

/// Removes the entries whose streak reached `release_after`; reports every entry's streak.
pub fn release(reg: &mut Register, newest_first: &[Nightly]) -> Released {
    let mut out = Vec::new();
    let need = reg.release_after;
    reg.entries.retain(|e| {
        let (n, _) = streak(e, newest_first);
        let go = n >= need;
        out.push((e.id.clone(), n, go));
        !go
    });
    out
}

#[cfg(test)]
mod tests;
