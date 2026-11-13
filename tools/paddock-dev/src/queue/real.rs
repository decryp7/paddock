// ci-runs-reader: the queue lists the runs of its train head (running, finished, failed) to take its next step; the landing verdict is ./dev ci verdict (ci_runs::verdict)
//! The real adapters of the queue's ports: Gitea through [`crate::pr::gitea::Api`], git in the queue's own clone, and CI through
//! `ci dispatch`, `ci verdict` and `ci triage`. No logic of the queue lives here; the decisions are in [`super::tick`].
//!
//! **No code of a pull request runs in this process or in any child of it.** git runs with hooks off (`core.hooksPath=/dev/null`),
//! no file transport (`protocol.file.allow=never`), no global or system configuration and no filter driver (a fresh clone has
//! none, and the repository cannot define one); the merges are made in the queue's own clone of an explicit URL, never in the job's
//! checkout; no cargo, node or npm runs from the merged tree (the planner runs in a checkout of `main`); and the token reaches
//! exactly one child, the push, through a `GIT_ASKPASS` program scoped to that command. The local gates (conformance, gate
//! register, commit lint, workspace hack, publish dry run, cargo metadata) are jobs of the dispatched workflow, which runs
//! without the queue's token.

use super::ports::{Blame, Built, Ci, CiRun, Failure, Ledger, Marker, RunState, Sent, TRAIN_BRANCH, Train, TrainMember};
use crate::ci_runs::client::Client;
use crate::ci_runs::triage::{self, Status, Test};
use crate::ci_runs::verdict::{self, Verdict};
use crate::ci_runs::{dispatch, time};
use crate::pr::gitea::{Api, Pull, approved_at};
use crate::pr::labels::{self, LabelSpec};
use crate::wait::Http;
use paddock_cli::{CliError, Report};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Lower case, letters and digits only, no plural `s`: `paddock-tools` and `PaddockTool` are one project.
pub fn norm(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    s.strip_suffix('s').map(str::to_string).unwrap_or(s)
}

/// The projects the planner's answer (`dev plan <base> <head>`) reaches, normalised: its true flags (`crates`, `ui`,
/// `paddockDev`), its `apps`, its package names, and `*` when it runs everything.
pub fn projects_of_plan(plan: &serde_json::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (k, v) in plan.as_object().into_iter().flatten() {
        match (k.as_str(), v) {
            ("all", serde_json::Value::Bool(true)) => {
                out.insert("*".to_string());
            }
            ("apps", serde_json::Value::Array(a)) => out.extend(a.iter().filter_map(|x| x.as_str()).map(norm)),
            ("packages", serde_json::Value::Object(m)) => {
                for names in m.values().filter_map(|v| v.as_array()) {
                    out.extend(names.iter().filter_map(|x| x.as_str()).map(norm));
                }
            }
            (_, serde_json::Value::Bool(true)) => {
                out.insert(norm(k));
            }
            _ => {}
        }
    }
    out
}

/// The projects a failed row belongs to: the crate of a nextest test (`paddock-service per_machine::x`), the app of an `app
/// <name> (...)` job, else the job's own name (`ui`, `conformance`).
pub fn projects_of_row(job: &str, test: &Test) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let head = job.split(" (").next().unwrap_or(job);
    let app = head.strip_prefix("app ");
    if let Some(app) = app {
        out.insert(norm(app));
    }
    match test {
        Test::Named(n) => {
            if let Some(binary) = n.split_whitespace().next() {
                out.insert(norm(binary.split("::").next().unwrap_or(binary)));
            }
        }
        _ if app.is_none() => {
            out.insert(norm(head));
        }
        _ => {}
    }
    out
}

/// Gitea as the queue's ledger.
pub struct GiteaLedger<'a> {
    /// The Gitea API.
    pub api: Api<'a>,
    /// `.gitea/labels.toml`.
    pub known: Vec<LabelSpec>,
}

impl Ledger for GiteaLedger<'_> {
    fn open_pulls(&mut self) -> Result<Vec<Pull>, String> {
        self.api.pulls("open")
    }

    fn relabel(&mut self, number: u64, changes: &[&str]) -> Result<(), String> {
        let words: Vec<String> = changes.iter().map(|s| s.to_string()).collect();
        let changes = labels::parse_changes(&words)?;
        // Read through the issue, so a closed pull request works too, and just before the write: the change merges into what the
        // labels are now, never replaces them (a `queue/hold` set in between survives).
        let now = self.api.issue_labels(number)?;
        let after = labels::apply(&now, &changes, &self.known)?;
        let server = self.api.labels()?;
        let ids = after
            .iter()
            .map(|n| {
                server
                    .iter()
                    .find(|l| &l.name == n)
                    .map(|l| l.id)
                    .ok_or_else(|| format!("the server has no label `{n}`; run `./dev pr sync`"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.api.set_labels(number, &ids).map(|_| ())
    }

    fn closed_held(&mut self) -> Result<Vec<u64>, String> {
        self.api.closed_with_label("queue/hold")
    }

    fn comment(&mut self, number: u64, text: &str) -> Result<(), String> {
        self.api.comment(number, text)
    }

    fn approved_head(&mut self, pr: &Pull) -> Result<bool, String> {
        // Never the pull request's own author, even if review-bot opened it.
        let allowed: Vec<&str> = APPROVERS.iter().copied().filter(|a| *a != pr.author).collect();
        Ok(approved_at(&self.api.reviews(pr.number)?, &pr.head, &allowed))
    }
}

/// The accounts whose approvals count: review-bot alone (its approvals are the official ones, main's approvals whitelist). Reviewer
/// agents post an approval through it after a passing review round; the queue's and the release bots never approve.
pub const APPROVERS: &[&str] = &["review-bot"];

/// How triage's answer for a failed test counts for blame: flaky is still red (ER-12), so it is blamed, unless the same test also
/// failed on the baseline run (then it is main's).
pub fn blame_of(status: &Status) -> Blame {
    match status {
        Status::New => Blame::New,
        Status::Flaky {
            also_on_baseline: true, ..
        }
        | Status::OnBaseline => Blame::OnMain,
        Status::Flaky { .. } => Blame::Flaky,
        Status::Unknown(_) => Blame::Unknown,
    }
}

/// How a git child is started: with no token at all, or as the one push that gets it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Child {
    /// Everything but the push: no token in its environment.
    Local,
    /// The push (and the fetch, when asked): `GIT_ASKPASS` answers with the token, for this child only.
    Authed,
}

/// The planner as the train asks it: the projects `main..head` reaches, run from a checkout of `main` (never of the pull request).
pub type PlannerFn = fn(trusted: &Path, main: &str, head: &str) -> Result<serde_json::Value, String>;

/// The merge check as the train builder calls it.
pub type MergeCheckFn = fn(&crate::merge_check::Request) -> Result<Report, CliError>;

/// git in the queue's own clone, as queue-bot.
pub struct GitTrain {
    /// The clone the merges are made in.
    pub root: PathBuf,
    /// A checkout of `main` for the planner (a worktree of the clone); `None` where the planner is not used.
    pub trusted: Option<PathBuf>,
    /// The explicit URL of the repository (`origin` is set to it, whatever the job's checkout says).
    pub url: String,
    /// The token for the push (and the fetch with `authed_fetch`); `None` for a queue that cannot push.
    pub token: Option<String>,
    /// Whether fetch and clone also get the token (a private repository); the default reads without it.
    pub authed_fetch: bool,
    /// Whether the `file` transport is allowed (a local path as URL: the tests; never in production).
    pub allow_file: bool,
    /// The folder of the askpass program.
    pub scratch: PathBuf,
    /// The merge check; [`crate::merge_check::command`] outside the tests.
    pub merge_check: MergeCheckFn,
    /// The planner; [`run_planner`] outside the tests.
    pub planner: PlannerFn,
}

impl GitTrain {
    /// The queue's clone of `url` under `scratch` (made once, then fetched into), and a worktree of `main` beside it for the
    /// planner. Nothing here is the job's own checkout, and nothing in it ever runs.
    pub fn open(url: &str, scratch: &Path, token: Option<String>, authed_fetch: bool, allow_file: bool) -> Result<GitTrain, String> {
        std::fs::create_dir_all(scratch).map_err(|e| format!("cannot create {}: {e}", scratch.display()))?;
        let g = GitTrain {
            root: scratch.join("merge"),
            trusted: Some(scratch.join("trusted")),
            url: url.to_string(),
            token,
            authed_fetch,
            allow_file,
            scratch: scratch.to_path_buf(),
            merge_check: crate::merge_check::command,
            planner: run_planner,
        };
        if !g.root.join(".git").exists() {
            let kind = if authed_fetch { Child::Authed } else { Child::Local };
            let (ok, out) = g.run_in(
                Some(scratch),
                kind,
                &["clone", "--quiet", "--no-checkout", "--no-tags", url, &g.root.display().to_string()],
            )?;
            if !ok {
                return Err(format!("cannot clone {url}: {}", g.redact(&out)));
            }
        }
        // The URL is ours, not whatever an earlier run or the job's configuration said.
        g.ok(&["remote", "set-url", "origin", url])?;
        Ok(g)
    }

    /// An existing clone, for the tests (its `origin` is already right).
    pub fn for_clone(root: PathBuf, url: &str) -> GitTrain {
        GitTrain {
            scratch: root.clone(),
            root,
            trusted: None,
            url: url.to_string(),
            token: None,
            authed_fetch: false,
            allow_file: true,
            merge_check: crate::merge_check::command,
            planner: run_planner,
        }
    }

    fn redact(&self, text: &str) -> String {
        match self.token.as_deref() {
            Some(t) if !t.is_empty() => text.replace(t, "***"),
            _ => text.to_string(),
        }
    }

    /// The git child: hooks off, no file transport, no global or system configuration, no prompt, and no token unless this is the
    /// push. `dir` is the checkout to work in (the clone when `None`).
    pub fn command(&self, dir: Option<&Path>, kind: Child) -> std::process::Command {
        let mut c = paddock_proc::command("git");
        c.arg("-C").arg(dir.unwrap_or(&self.root));
        c.args(IDENT);
        c.args(["-c", "core.hooksPath=/dev/null"]);
        c.args([
            "-c",
            if self.allow_file {
                "protocol.file.allow=user"
            } else {
                "protocol.file.allow=never"
            },
        ]);
        c.args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.sshCommand=false",
            "-c",
            "credential.helper=",
        ]);
        let parent: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
        scrub(&mut c, &parent);
        if let (Child::Authed, Some(t)) = (kind, &self.token) {
            c.env("GIT_ASKPASS", self.scratch.join("askpass.sh"))
                .env("PADDOCK_QUEUE_PUSH_TOKEN", t);
        }
        c
    }

    /// Writes the askpass program: it answers a username prompt with the bot's name and anything else with the token from the
    /// environment of the one child it runs for.
    fn ensure_askpass(&self) -> Result<(), String> {
        let path = self.scratch.join("askpass.sh");
        let body =
            "#!/bin/sh\ncase \"$1\" in\n  Username*) echo queue-bot ;;\n  *) printf '%s\\n' \"$PADDOCK_QUEUE_PUSH_TOKEN\" ;;\nesac\n";
        std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("cannot protect the askpass program: {e}"))?;
        }
        Ok(())
    }

    fn run_in(&self, dir: Option<&Path>, kind: Child, args: &[&str]) -> Result<(bool, String), String> {
        if kind == Child::Authed {
            self.ensure_askpass()?;
        }
        let out = self
            .command(dir, kind)
            .args(args)
            .output()
            .map_err(|e| format!("cannot start git: {e}"))?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            text.push_str(&String::from_utf8_lossy(&out.stderr));
        }
        Ok((out.status.success(), text))
    }

    fn git(&self, args: &[&str]) -> Result<(bool, String), String> {
        self.run_in(None, Child::Local, args)
    }

    fn ok(&self, args: &[&str]) -> Result<String, String> {
        let (ok, out) = self.git(args)?;
        if ok {
            Ok(out)
        } else {
            Err(format!("git {} failed: {}", args.join(" "), out.trim()))
        }
    }

    fn ok_kind(&self, kind: Child, args: &[&str]) -> Result<String, String> {
        let (ok, out) = self.run_in(None, kind, args)?;
        if ok {
            Ok(out)
        } else {
            Err(format!("git {} failed: {}", args.first().unwrap_or(&""), self.redact(out.trim())))
        }
    }

    /// A checkout of `main` for the planner: the worktree beside the clone, moved to `main`. The planner reads the pull request's
    /// changes through git; it never runs from the pull request's tree.
    fn trusted_main(&self, main: &str) -> Result<PathBuf, String> {
        let dir = self
            .trusted
            .clone()
            .ok_or_else(|| "this train has no checkout of main for the planner".to_string())?;
        if !dir.join(".git").exists() {
            self.ok(&["worktree", "add", "--force", "--detach", &dir.display().to_string(), main])?;
        } else {
            let (ok, out) = self.run_in(Some(&dir), Child::Local, &["checkout", "--quiet", "--force", "--detach", main])?;
            if !ok {
                return Err(format!("cannot move the planner's checkout to {main}: {}", out.trim()));
            }
        }
        Ok(dir)
    }

    fn fetch_kind(&self) -> Child {
        if self.authed_fetch { Child::Authed } else { Child::Local }
    }
}

const IDENT: [&str; 4] = ["-c", "user.name=queue-bot", "-c", "user.email=queue-bot@dev.decryptology.net"];

/// The planner child: `plan <main> <head>` in `trusted`, without the queue's token (`parent`: the names of this process's
/// environment).
pub fn planner_command(me: &Path, trusted: &Path, main: &str, head: &str, parent: &[String]) -> std::process::Command {
    let mut c = paddock_proc::command(me);
    c.args(["plan", main, head]).current_dir(trusted);
    scrub(&mut c, parent);
    c
}

/// The planner as a child of this tool in `trusted` (a checkout of main), without the queue's token.
pub fn run_planner(trusted: &Path, main: &str, head: &str) -> Result<serde_json::Value, String> {
    let me = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    let parent: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
    let mut c = planner_command(&me, trusted, main, head, &parent);
    let out = c.output().map_err(|e| format!("cannot start the planner: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "the planner failed on {main}..{head}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("the planner's answer is not JSON: {e}"))
}

pub use crate::child_env::scrub;

fn short_id(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// The repository files a gate's log names (`crates/x/src/lib.rs:12`, `ui/a.ts`), in order of first mention, at most 50.
pub fn files_named(text: &str) -> Vec<String> {
    static PATH: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        regex::Regex::new(r"(?:^|[\s'\x22`(\[=:])((?:[A-Za-z0-9_.@-]+/)+[A-Za-z0-9_.@-]+\.[A-Za-z0-9]{1,6})").expect("a valid pattern")
    });
    let mut out: Vec<String> = Vec::new();
    for m in PATH.captures_iter(text) {
        let f = m[1].trim_start_matches("./").to_string();
        if !out.contains(&f) {
            out.push(f);
        }
        if out.len() == 50 {
            break;
        }
    }
    out
}

/// `(app, workflow)` of every app whose `paddock.ci.json` names its own workflow, as `main` has it.
pub fn app_workflows(ls_tree: &str, show: &mut dyn FnMut(&str) -> Option<String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in ls_tree.lines() {
        let Some(app) = line.strip_prefix("apps/") else { continue };
        let wf = show(&format!("apps/{app}/paddock.ci.json"))
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["workflow"].as_str().map(str::to_string));
        if let Some(wf) = wf.filter(|w| !w.is_empty() && w.ends_with(".yml") && !w.contains('/')) {
            out.insert(app.to_string(), wf);
        }
    }
    out
}

/// `ci.yml`, then each app workflow whose app the plan reaches (all of them when the plan runs everything).
pub fn select_workflows(apps: &BTreeMap<String, String>, reached: &BTreeSet<String>) -> Vec<String> {
    let mut out = vec!["ci.yml".to_string()];
    for (app, wf) in apps {
        if (reached.contains("*") || reached.contains(&norm(app))) && !out.contains(wf) {
            out.push(wf.clone());
        }
    }
    out
}

fn marker_text(m: &Marker) -> String {
    let prs = m.prs.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
    let mut s = format!("Queue-Prs: {prs}\nQueue-Round: {}\n", m.round);
    if let (Some(r), Some(w)) = (m.since_run, &m.since_workflow) {
        s.push_str(&format!("Queue-Since-Run: {r}\nQueue-Since-Workflow: {w}\n"));
    }
    s
}

/// Reads the trailers of a marker commit message.
pub fn parse_marker(message: &str) -> Option<Marker> {
    let field = |k: &str| message.lines().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_string());
    let prs = field("Queue-Prs:")?;
    let mut nums: Vec<u64> = prs
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().parse().ok())
        .collect::<Option<_>>()?;
    nums.sort_unstable();
    Some(Marker {
        prs: nums,
        round: field("Queue-Round:")?.parse().ok()?,
        since_run: field("Queue-Since-Run:").and_then(|v| v.parse().ok()),
        since_workflow: field("Queue-Since-Workflow:"),
    })
}

impl Train for GitTrain {
    fn fetch(&mut self, branches: &[String]) -> Result<(), String> {
        for b in branches.iter().filter(|b| b.as_str() != TRAIN_BRANCH) {
            self.ok_kind(
                self.fetch_kind(),
                &[
                    "fetch",
                    "--quiet",
                    "--no-tags",
                    "origin",
                    &format!("+refs/heads/{b}:refs/remotes/origin/{b}"),
                ],
            )?;
        }
        let spec = format!("+refs/heads/{TRAIN_BRANCH}:refs/remotes/origin/{TRAIN_BRANCH}");
        if !self
            .run_in(None, self.fetch_kind(), &["fetch", "--quiet", "--no-tags", "origin", &spec])?
            .0
        {
            // No train branch on the server (or it was deleted): forget our copy.
            let _ = self.git(&["update-ref", "-d", &format!("refs/remotes/origin/{TRAIN_BRANCH}")])?;
        }
        Ok(())
    }

    fn main_head(&mut self) -> Result<String, String> {
        Ok(self
            .ok(&["rev-parse", "--verify", "refs/remotes/origin/main^{commit}"])?
            .trim()
            .to_string())
    }

    fn train_head(&mut self) -> Result<Option<String>, String> {
        let (ok, out) = self.git(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{TRAIN_BRANCH}^{{commit}}"),
        ])?;
        Ok(ok.then(|| out.trim().to_string()).filter(|s| !s.is_empty()))
    }

    fn marker(&mut self, head: &str) -> Result<Option<Marker>, String> {
        Ok(parse_marker(&self.ok(&["show", "-s", "--format=%B", head])?))
    }

    fn contains(&mut self, ancestor: &str, descendant: &str) -> Result<bool, String> {
        let (exists, _) = self.git(&["cat-file", "-e", &format!("{ancestor}^{{commit}}")])?;
        if !exists {
            return Ok(false);
        }
        Ok(self.git(&["merge-base", "--is-ancestor", ancestor, descendant])?.0)
    }

    fn build(&mut self, main: &str, prs: &[Pull], marker: &Marker) -> Result<Built, String> {
        self.ok(&["checkout", "--quiet", "--force", "--detach", main])?;
        let (mut merged, mut dropped) = (Vec::new(), Vec::new());
        for pr in prs {
            let before = self.ok(&["rev-parse", "HEAD"])?.trim().to_string();
            let msg = format!("Merge {} ({}) into train", pr.branch, pr.head.get(..9).unwrap_or(&pr.head));
            let (ok, out) = self.git(&["merge", "--no-ff", "--quiet", "-m", &msg, &pr.head])?;
            if !ok {
                let files = self.git(&["diff", "--name-only", "--diff-filter=U"])?.1;
                let _ = self.git(&["merge", "--abort"])?;
                let files: Vec<&str> = files.lines().take(20).collect();
                let why = if files.is_empty() {
                    out.trim().to_string()
                } else {
                    format!("conflicts with the train or main in: {}", files.join(", "))
                };
                dropped.push((pr.number, why));
                continue;
            }
            let after = self.ok(&["rev-parse", "HEAD"])?.trim().to_string();
            if after == before {
                dropped.push((
                    pr.number,
                    "its head is already contained in main or an earlier pull request of the train; there is nothing to merge".into(),
                ));
                continue;
            }
            // ADR 0089: a merge that dropped a side's hunks or tests; a check that could not look stops the queue.
            let req = crate::merge_check::Request {
                root: Some(self.root.clone()),
                commit: Some("HEAD".into()),
                branch: None,
                target: None,
                // The queue runs no code of any pull request (docs/plans/merge-queue.md); the generator checks run the
                // registry's commands in the tree, so they are the landing gates' job, not the queue's.
                generators: false,
                gates: false,
            };
            match (self.merge_check)(&req) {
                Ok(_) => merged.push(pr.number),
                Err(e) if e.exit == paddock_cli::Exit::Findings => {
                    self.ok(&["reset", "--quiet", "--hard", &before])?;
                    dropped.push((pr.number, format!("the merge check found what the merge dropped:\n{}", e.message)));
                }
                Err(e) => return Err(format!("merge check of #{} did not look: {}", pr.number, e.message)),
            }
        }
        let m = Marker {
            prs: merged.clone(),
            ..marker.clone()
        };
        let subject = format!("chore(queue): train round {}", m.round);
        self.ok(&["commit", "--quiet", "--allow-empty", "-m", &subject, "-m", &marker_text(&m)])?;
        let head = self.ok(&["rev-parse", "HEAD"])?.trim().to_string();
        Ok(Built { head, merged, dropped })
    }

    fn pr_files(&mut self, main: &str, head: &str) -> Result<Vec<String>, String> {
        Ok(self
            .ok(&["diff", "--name-only", &format!("{main}...{head}")])?
            .lines()
            .map(str::to_string)
            .collect())
    }

    fn verify_train(&mut self, base: Option<&str>, head: &str, members: &[TrainMember], marker: &Marker) -> Result<Option<String>, String> {
        if members.is_empty() {
            return Ok(Some("it merges no pull request".into()));
        }
        let numbers: Vec<u64> = members.iter().map(|m| m.number).collect();
        if marker.prs != numbers {
            return Ok(Some("its marker lists other pull requests than the train's".into()));
        }
        if marker.round == 0 || (marker.round == 1 && (marker.since_run.is_some() || marker.since_workflow.is_some())) {
            return Ok(Some("its marker has a since-run on round 1, or round 0".into()));
        }
        let parents = |g: &GitTrain, c: &str| -> Result<Vec<String>, String> {
            Ok(g.ok(&["rev-list", "--parents", "-n", "1", c])?
                .split_whitespace()
                .skip(1)
                .map(str::to_string)
                .collect())
        };
        let tree =
            |g: &GitTrain, c: &str| -> Result<String, String> { Ok(g.ok(&["rev-parse", &format!("{c}^{{tree}}")])?.trim().to_string()) };
        // The message of a commit, byte for byte (what follows the headers' blank line).
        let message = |g: &GitTrain, c: &str| -> Result<String, String> {
            let raw = g.ok(&["cat-file", "commit", c])?;
            Ok(raw.split_once("\n\n").map(|(_, m)| m.to_string()).unwrap_or_default())
        };
        // The marker commit.
        let mut cur = head.to_string();
        let expected = format!("chore(queue): train round {}\n\n{}", marker.round, marker_text(marker));
        if message(self, &cur)? != expected {
            return Ok(Some(format!(
                "the marker commit {} has a message the queue did not write",
                short_id(&cur)
            )));
        }
        let ps = parents(self, &cur)?;
        if ps.len() != 1 || tree(self, &cur)? != tree(self, &ps[0])? {
            return Ok(Some(format!(
                "the marker commit {} does not have exactly one parent and its parent's tree",
                short_id(&cur)
            )));
        }
        cur = ps[0].clone();
        // The merges, last first: every first-parent commit down to the base is one of them.
        for m in members.iter().rev() {
            let ps = parents(self, &cur)?;
            if ps.len() != 2 || ps[1] != m.head {
                return Ok(Some(format!(
                    "commit {} is not a --no-ff merge of #{}'s approved head {}",
                    short_id(&cur),
                    m.number,
                    short_id(&m.head)
                )));
            }
            let want = format!("Merge {} ({}) into train\n", m.branch, m.head.get(..9).unwrap_or(&m.head));
            if message(self, &cur)? != want {
                return Ok(Some(format!(
                    "the merge {} of #{} has a message the queue did not write",
                    short_id(&cur),
                    m.number
                )));
            }
            let (ok, out) = self.git(&["merge-tree", "--write-tree", "--no-messages", &ps[0], &ps[1]])?;
            if !ok || out.lines().next().map(str::trim) != Some(tree(self, &cur)?.as_str()) {
                return Ok(Some(format!(
                    "the tree of the merge of #{} ({}) is not what merging its parents gives",
                    m.number,
                    short_id(&cur)
                )));
            }
            cur = ps[0].clone();
        }
        if let Some(b) = base
            && cur != b
        {
            return Ok(Some(format!("it does not start at {} but at {}", short_id(b), short_id(&cur))));
        }
        Ok(None)
    }

    fn affected(&mut self, main: &str, head: &str) -> Result<BTreeSet<String>, String> {
        let trusted = self.trusted_main(main)?;
        Ok(projects_of_plan(&(self.planner)(&trusted, main, head)?))
    }

    fn reached_workflows(&mut self, main: &str, head: &str) -> Result<Vec<String>, String> {
        let reached = self.affected(main, head)?;
        // The apps and their workflows are read from main: a pull request cannot take its own app out of the gates.
        let ls = self.ok(&["ls-tree", "--name-only", main, "apps/"])?;
        let apps = app_workflows(&ls, &mut |path| {
            let (ok, text) = self.git(&["show", &format!("{main}:{path}")]).ok()?;
            ok.then_some(text)
        });
        Ok(select_workflows(&apps, &reached))
    }

    fn push_train(&mut self, head: &str, old: Option<&str>) -> Result<(), String> {
        let lease = format!("--force-with-lease=refs/heads/{TRAIN_BRANCH}:{}", old.unwrap_or(""));
        self.ok_kind(
            Child::Authed,
            &["push", "--quiet", &lease, "origin", &format!("{head}:refs/heads/{TRAIN_BRANCH}")],
        )
        .map(|_| ())
    }

    fn land(&mut self, head: &str) -> Result<(), String> {
        // A plain push: the server refuses anything that is not a fast-forward.
        self.ok_kind(Child::Authed, &["push", "--quiet", "origin", &format!("{head}:refs/heads/main")])?;
        self.ok_kind(
            self.fetch_kind(),
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "origin",
                "+refs/heads/main:refs/remotes/origin/main",
            ],
        )?;
        let now = self.main_head()?;
        if now == head {
            Ok(())
        } else {
            Err(format!("after the push origin/main is {now}, not the train head {head}"))
        }
    }
}

/// CI through the tool's own readers.
pub struct GiteaCi<'a> {
    /// Gitea.
    pub server: String,
    /// `decryp7/paddock`.
    pub repo: String,
    /// The token.
    pub token: String,
    /// The checkout (workflow files for the runner classes).
    pub root: PathBuf,
    /// How long a dispatch waits for a busy runner.
    pub max_wait: Duration,
    /// The reader; real or a test's.
    pub http: &'a mut dyn Http,
}

impl Ci for GiteaCi<'_> {
    fn runs(&mut self, workflow: &str, head: &str) -> Result<Vec<CiRun>, String> {
        let mut c = Client::new(self.http, &self.server, &self.repo, &self.token);
        let page = c.runs_page_where(1, &format!("head_sha={head}"))?;
        let mut runs: Vec<_> = page
            .runs
            .into_iter()
            .filter(|r| r.workflow() == workflow && r.head_sha == head)
            .collect();
        runs.sort_by_key(|r| std::cmp::Reverse(r.id));
        Ok(runs
            .into_iter()
            .map(|r| CiRun {
                id: r.id,
                state: if !r.finished() {
                    RunState::Running
                } else if r.conclusion == "success" || r.status == "success" {
                    RunState::Success
                } else {
                    RunState::Failed(if r.conclusion.is_empty() {
                        r.status.clone()
                    } else {
                        r.conclusion.clone()
                    })
                },
            })
            .collect())
    }

    fn verdict(&mut self, workflow: &str, head: &str) -> Verdict {
        let mut c = Client::new(self.http, &self.server, &self.repo, &self.token);
        verdict::verdict(&mut c, workflow, head)
    }

    fn dispatch(&mut self, workflow: &str, inputs: &[(String, String)]) -> Result<Sent, String> {
        let kv: Vec<String> = inputs.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let req = dispatch::parse_request(workflow, TRAIN_BRANCH, &kv, &format!("{}s", self.max_wait.as_secs().max(1)), None)?;
        let root = self.root.clone();
        let read = move |name: &str| std::fs::read_to_string(root.join(".gitea/workflows").join(name)).ok();
        let mut clock = dispatch::system_clock();
        let target = dispatch::Target {
            server: &self.server,
            repo: &self.repo,
            token: &self.token,
        };
        match dispatch::run(self.http, &target, &req, &read, &mut clock, &mut std::io::stderr())? {
            dispatch::Dispatched::Yes { .. } => Ok(Sent::Yes),
            dispatch::Dispatched::StillBusy { last } => Ok(Sent::Busy(last)),
        }
    }

    fn failures(&mut self, run: u64) -> Result<Vec<Failure>, String> {
        let report = {
            let mut c = Client::new(self.http, &self.server, &self.repo, &self.token);
            let req = triage::Request {
                run,
                against: None,
                app: None,
                days: 14,
                history: true,
                repo: self.repo.clone(),
                now_secs: time::now_secs(),
            };
            triage::collect(&mut c, &req, &mut |l| eprintln!("{l}"))?
        };
        // The gate jobs (train gates, commit messages) fail on files: read their logs for the paths.
        let is_gate = |job: &str| job.starts_with("train gates") || job.starts_with("commit messages");
        let mut files: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if report.rows.iter().any(|r| is_gate(&r.job)) {
            let mut runs = crate::plan_since::GiteaRuns::new(self.http, &self.server, &self.repo, self.token.clone());
            let jobs = runs.jobs(run).unwrap_or_default();
            for r in report.rows.iter().filter(|r| is_gate(&r.job)) {
                if let Some(j) = jobs.iter().find(|j| j.name == r.job)
                    && let Ok(log) = runs.log(j.id)
                {
                    files.insert(
                        r.job.clone(),
                        files_named(&log.lines().map(crate::log_text::plain).collect::<Vec<_>>().join("\n")),
                    );
                }
            }
        }
        Ok(report
            .rows
            .iter()
            .map(|r| Failure {
                what: format!(
                    "{}: {}",
                    r.job,
                    match &r.test {
                        Test::Named(n) => n.clone(),
                        Test::WholeJob => "the job failed outside any test".into(),
                        Test::Unread(w) => format!("failing tests not readable ({w})"),
                    }
                ),
                projects: projects_of_row(&r.job, &r.test),
                files: files.get(&r.job).cloned().unwrap_or_default(),
                blame: blame_of(&r.status),
            })
            .collect())
    }
}
