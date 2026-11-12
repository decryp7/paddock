//! Which projects a changed `.gitea/` file or shared CI script reaches: the port of `giteaTargets`, `giteaUsers`,
//! `scriptImporters`, `workflowJobs` and `changedJobs` of `scripts/affected.mjs`.
//!
//! A line-based reading of the workflow files, no YAML library, exactly as the script does it: whatever the split cannot
//! prove (anchors, odd indentation, an unknown job, a file that is not a workflow) answers `None`, and the caller plans
//! everything.

use regex::Regex;
use std::collections::{BTreeSet, VecDeque};
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// What each job of ci.yml runs for: the plan project(s) that make the job run (`apps` = every app leg).
pub const CI_JOBS: &[(&str, &[&str])] = &[
    ("plan", &["everything"]),
    ("ui", &["ui"]),
    ("e2e", &["ui"]),
    ("e2e-webkit", &["ui"]),
    ("conformance", &["conformance"]),
    ("c99", &["c99"]),
    ("ipc-endpoints", &["conformance"]),
    ("gitleaks", &["conformance"]),
    ("tools", &["tools"]),
    ("tools-macos", &["tools"]),
    ("wiki", &["tools"]),
    ("heavy", &["tools"]),
    ("icons", &["icons"]),
    ("eslint-preset", &["eslint"]),
    ("crates", &["crates"]),
    ("crate", &["crates"]),
    ("commits", &["tools"]),
    ("train-gates", &["tools"]),
    ("app", &["apps"]),
    ("paddock-dev", &["paddock-dev"]),
];

fn ci_job(name: &str) -> Option<&'static [&'static str]> {
    CI_JOBS.iter().find(|(j, _)| *j == name).map(|(_, p)| *p)
}

/// The files of a revision pair of the `.gitea/` and `conformance/` trees, as `affected` needs them.
pub trait GiteaSource {
    /// The apps (to tell an app's workflow from a shared one).
    fn app_names(&self) -> Vec<String>;
    /// Every file under `.gitea/` at the head revision (files mode: the tracked files).
    fn files(&self) -> Vec<String>;
    /// The shared scripts (`conformance/**.{mjs,cjs,js,sh}` except `test/` and `fixtures/`).
    fn scripts(&self) -> Vec<String>;
    /// Files mode: whether `f` differs from HEAD (a committed change cannot be diffed); `None` in base mode.
    fn dirty(&self, f: &str) -> Option<bool>;
    /// The file at the base revision (files mode: HEAD), `None` when it does not exist there.
    fn before(&self, f: &str) -> Option<String>;
    /// The file at the head revision (files mode: the working tree), `None` when it does not exist there.
    fn after(&self, f: &str) -> Option<String>;
}

/// The header (everything up to `jobs:`) and the text of each job, in file order.
#[derive(Debug, PartialEq)]
pub struct Workflow {
    /// Everything before and including `jobs:`.
    pub header: String,
    /// `(name, text)` per job.
    pub jobs: Vec<(String, String)>,
}

static YAML_REUSE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)(^|[\s:\[{,-])[&*][A-Za-z0-9_-]+|<<:"));
static JOBS_KEY: LazyLock<Regex> = LazyLock::new(|| re(r"^jobs:\s*$"));
static JOB_KEY: LazyLock<Regex> = LazyLock::new(|| re(r"^ {2}([A-Za-z0-9_-]+):\s*(#.*)?$"));

fn trim_end(s: &str) -> String {
    s.trim_end().to_string()
}

/// Splits a workflow file into its header and jobs, or `None` when that shape is not found, when a two-space line is
/// neither a job key nor a comment, or when the file uses anchors or aliases (so a job may depend on text outside it).
pub fn workflow_jobs(text: &str) -> Option<Workflow> {
    if YAML_REUSE.is_match(text) {
        return None;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let at = lines.iter().position(|l| JOBS_KEY.is_match(l))?;
    let mut jobs: Vec<(String, Vec<&str>)> = Vec::new();
    let mut cur: Option<usize> = None;
    for l in &lines[at + 1..] {
        if let Some(m) = JOB_KEY.captures(l) {
            let name = m[1].to_string();
            let i = match jobs.iter().position(|(k, _)| *k == name) {
                Some(i) => {
                    jobs[i].1.clear(); // a Map.set of an existing key keeps its place and replaces the lines
                    i
                }
                None => {
                    jobs.push((name, Vec::new()));
                    jobs.len() - 1
                }
            };
            cur = Some(i);
            continue;
        }
        let two_space_other = l.starts_with("  ") && l.chars().nth(2).is_some_and(|c| !c.is_whitespace()) && !l.starts_with("  #");
        if two_space_other {
            return None; // a flow mapping, a quoted key...: not the shape we know
        }
        if l.chars().next().is_some_and(|c| !c.is_whitespace()) && !l.starts_with('#') {
            return None; // another top-level key after jobs
        }
        if l.starts_with('\t') {
            return None;
        }
        if let Some(i) = cur {
            jobs[i].1.push(l);
        } else if !l.trim().is_empty() && !l.trim().starts_with('#') {
            return None;
        }
    }
    if jobs.is_empty() {
        return None;
    }
    Some(Workflow {
        header: trim_end(&lines[..=at].join("\n")),
        jobs: jobs.into_iter().map(|(k, v)| (k, trim_end(&v.join("\n")))).collect(),
    })
}

/// The jobs whose text differs between two revisions of a workflow (added or removed ones included), or `None` when either
/// cannot be split or the header changed.
pub fn changed_jobs(before: Option<&str>, after: Option<&str>) -> Option<Vec<String>> {
    let b = workflow_jobs(before?)?;
    let a = workflow_jobs(after?)?;
    if a.header != b.header {
        return None;
    }
    let get = |w: &Workflow, k: &str| w.jobs.iter().find(|(n, _)| n == k).map(|(_, t)| t.clone());
    let mut names: Vec<&String> = Vec::new();
    for (k, _) in a.jobs.iter().chain(b.jobs.iter()) {
        if !names.contains(&k) {
            names.push(k);
        }
    }
    Some(names.into_iter().filter(|k| get(&a, k) != get(&b, k)).cloned().collect())
}

static GITEA_FILE: LazyLock<Regex> = LazyLock::new(|| re(r"^\.gitea/(workflows|actions)/.+"));
static CONFORMANCE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| re(r"^conformance/(ci|release)/.+"));
/// The scripts traced to the actions that run them: `conformance/ci`, `conformance/release` and the helpers they import.
pub static TRACED_SCRIPT: LazyLock<Regex> = LazyLock::new(|| re(r"^conformance/((ci|release)|src/util)/.+"));
static USER_FILE: LazyLock<Regex> = LazyLock::new(|| re(r"^\.gitea/(actions/[^/]+/action\.ya?ml|workflows/[^/]+\.ya?ml)$"));
static WORKFLOW_CALL: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^\s*workflow_call:"));

/// A text without its comment lines: a mention in a comment is no use.
fn uncommented(t: &str) -> String {
    t.split('\n')
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// Whether `text` holds `needle` not followed by a name character (JavaScript's `needle(?![A-Za-z0-9_.-])`).
fn mentions(text: &str, needle: &str) -> bool {
    text.match_indices(needle)
        .any(|(i, _)| !text[i + needle.len()..].chars().next().is_some_and(name_char))
}

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// The scripts that reach `f` by being named in another script's text (`import ... from './x.mjs'`), transitively, `f`
/// included. A match on the file name is enough: over-selecting is safe.
pub fn script_importers(f: &str, gitea: &dyn GiteaSource) -> Vec<String> {
    let scripts: Vec<String> = gitea.scripts().into_iter().filter(|x| x != f).collect();
    let mut out: Vec<String> = vec![f.to_string()];
    loop {
        let mut grew = false;
        for x in &scripts {
            if out.contains(x) {
                continue;
            }
            let text = gitea.after(x).unwrap_or_default();
            let hit = out.iter().any(|y| {
                let needle = format!("/{}", basename(y));
                mentions(&text, &needle)
            });
            if hit {
                out.push(x.clone());
                grew = true;
            }
        }
        if !grew {
            return out;
        }
    }
}

/// What a changed file is called in the workflows and actions that use it.
fn ref_of(path: &str) -> Option<String> {
    static WORKFLOW: LazyLock<Regex> = LazyLock::new(|| re(r"^\.gitea/workflows/([^/]+)$"));
    static ACTION: LazyLock<Regex> = LazyLock::new(|| re(r"^\.gitea/actions/([^/]+)/"));
    if let Some(w) = WORKFLOW.captures(path) {
        return Some(format!(".gitea/workflows/{}", &w[1]));
    }
    if let Some(a) = ACTION.captures(path) {
        return Some(format!(".gitea/actions/{}", &a[1]));
    }
    CONFORMANCE_SCRIPT.is_match(path).then(|| path.to_string())
}

/// A user of a changed file: a workflow (after revision) that references it directly or through an action that does.
#[derive(Debug, PartialEq)]
pub struct User {
    /// The workflow file.
    pub file: String,
    /// The jobs that reference it, `None` when the workflow cannot be split into jobs (the caller fails safe).
    pub jobs: Option<Vec<String>>,
}

/// Who uses a changed file (a workflow, an action, or a shared script under `conformance/ci` or `conformance/release`).
pub fn gitea_users(changed: &str, gitea: &dyn GiteaSource, seeds: &[String]) -> Vec<User> {
    let files = gitea.files();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut refs: Vec<String> = Vec::new();
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = seeds.iter().filter_map(|s| ref_of(s)).collect();
    while let Some(r) = queue.pop_front() {
        if !seen.insert(r.clone()) {
            continue;
        }
        refs.push(r.clone());
        for f in &files {
            if f == changed || !USER_FILE.is_match(f) {
                continue;
            }
            let Some(text) = gitea.after(f).filter(|t| !t.is_empty()) else {
                continue;
            };
            let live = uncommented(&text);
            if !mentions(&live, &r) {
                continue;
            }
            let own = ref_of(f);
            // Actions and reusable workflows (`workflow_call`, except ci.yml, whose jobs are mapped) are followed to their callers.
            if f.starts_with(".gitea/actions/") || (f != ".gitea/workflows/ci.yml" && WORKFLOW_CALL.is_match(&live)) {
                if let Some(o) = own {
                    queue.push_back(o);
                }
            } else {
                found.insert(f.clone());
            }
        }
    }
    found
        .into_iter()
        .map(|file| {
            let split = gitea.after(&file).and_then(|t| workflow_jobs(&t));
            let jobs = split.map(|w| {
                w.jobs
                    .into_iter()
                    .filter(|(_, t)| {
                        let live = uncommented(t);
                        refs.iter().any(|r| mentions(&live, r))
                    })
                    .map(|(k, _)| k)
                    .collect()
            });
            User { file, jobs }
        })
        .collect()
}

/// The app a workflow belongs to (shared with the planner).
fn app_of(path: &str, apps: &[String]) -> Option<String> {
    let set: BTreeSet<String> = apps.iter().cloned().collect();
    crate::plan::app_of_workflow(path, &set).map(str::to_string)
}

/// The `[project, reason]` pairs a changed `.gitea` file or shared script reaches, or `None` when that cannot be told (fail
/// safe: everything). ci.yml: its changed jobs through [`CI_JOBS`]; other workflows, actions and scripts: their users (a ci.yml
/// job, an app's workflow). Users that are no pull-request gate (nightly, publish, canary) are reported as such and mark tools.
pub fn gitea_targets(f: &str, gitea: &dyn GiteaSource) -> Option<Vec<(String, String)>> {
    let is_script = TRACED_SCRIPT.is_match(f);
    if !is_script && !GITEA_FILE.is_match(f) {
        return Some(Vec::new()); // workflow-templates, READMEs: only the conformance run added by the caller
    }
    // Files mode compares HEAD with the working tree: a committed change shows no difference, so nothing can be narrowed.
    if gitea.dirty(f) == Some(false) {
        return None;
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let via_job = |job: &str, why: &str, out: &mut Vec<(String, String)>| -> bool {
        let Some(ps) = ci_job(job) else {
            return false;
        };
        for p in ps {
            out.push((p.to_string(), format!("{why}, job {job}")));
        }
        true
    };
    if f == ".gitea/workflows/ci.yml" {
        let before = gitea.before(f);
        let after = gitea.after(f);
        if before == after {
            return None;
        }
        let jobs = changed_jobs(before.as_deref(), after.as_deref())?;
        if jobs.is_empty() || !jobs.iter().all(|j| via_job(j, f, &mut out)) {
            return None;
        }
        return Some(out);
    }
    let seeds = if is_script {
        script_importers(f, gitea)
    } else {
        vec![f.to_string()]
    };
    let users = gitea_users(f, gitea, &seeds);
    // An action nobody uses (or whose users are all comments) cannot be traced: external consumers are possible.
    if users.is_empty() && f.starts_with(".gitea/actions/") {
        return None;
    }
    let apps = gitea.app_names();
    for u in users {
        if let Some(app) = app_of(&u.file, &apps) {
            out.push((format!("app:{app}"), format!("{f} used by {}", u.file)));
        } else if u.file == ".gitea/workflows/ci.yml" {
            let jobs = u.jobs.filter(|j| !j.is_empty())?;
            if !jobs.iter().all(|j| via_job(j, &format!("{f} used by ci.yml"), &mut out)) {
                return None;
            }
        } else {
            out.push((
                "tools".to_string(),
                format!(
                    "{f} used by {}, which runs only on a schedule, a tag or a manual run (not a pull request)",
                    u.file
                ),
            ));
        }
    }
    Some(out)
}

/// A [`GiteaSource`] over fixed texts, for tests.
#[cfg(test)]
/// Fixed texts.
pub struct Fixed {
    /// The apps.
    pub apps: Vec<String>,
    /// Files at the base revision.
    pub before: std::collections::BTreeMap<String, String>,
    /// Files at the head revision.
    pub after: std::collections::BTreeMap<String, String>,
    /// Files mode: the dirty paths.
    pub dirty: Option<BTreeSet<String>>,
}

#[cfg(test)]
impl GiteaSource for Fixed {
    fn app_names(&self) -> Vec<String> {
        self.apps.clone()
    }
    fn files(&self) -> Vec<String> {
        self.after.keys().filter(|k| k.starts_with(".gitea/")).cloned().collect()
    }
    fn scripts(&self) -> Vec<String> {
        self.after
            .keys()
            .filter(|k| k.starts_with("conformance/") && re(r"\.(mjs|cjs|js|sh)$").is_match(k))
            .cloned()
            .collect()
    }
    fn dirty(&self, f: &str) -> Option<bool> {
        self.dirty.as_ref().map(|d| d.contains(f))
    }
    fn before(&self, f: &str) -> Option<String> {
        self.before.get(f).cloned()
    }
    fn after(&self, f: &str) -> Option<String> {
        self.after.get(f).cloned()
    }
}
