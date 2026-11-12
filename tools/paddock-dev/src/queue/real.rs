// ci-runs-reader: the queue lists the runs of its train head (running, finished, failed) to take its next step; the landing verdict is ./dev ci verdict (ci_runs::verdict)
//! The real adapters of the queue's ports: Gitea through [`crate::pr::gitea::Api`], git in the queue's working copy, the local
//! gates through this tool and the conformance line of ci.yml, and CI through `ci dispatch`, `ci verdict` and `ci triage`.
//! No logic of the queue lives here; the decisions are in [`super::tick`].

use super::ports::{
    Blame, Built, Ci, CiRun, Failure, GateFailure, GateOutcome, Gates, Ledger, Marker, RunState, Sent, TRAIN_BRANCH, Train,
};
use crate::ci_runs::client::Client;
use crate::ci_runs::triage::{self, Status, Test};
use crate::ci_runs::verdict::{self, Verdict};
use crate::ci_runs::{dispatch, time};
use crate::pr::gitea::{Api, Pull, approved_at};
use crate::pr::labels::{self, LabelSpec};
use crate::wait::Http;
use paddock_cli::{CliError, Report};
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
        let now = self
            .api
            .pulls("open")?
            .into_iter()
            .find(|p| p.number == number)
            .ok_or_else(|| format!("#{number} is not open"))?;
        let after = labels::apply(&now.labels, &changes, &self.known)?;
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

    fn comment(&mut self, number: u64, text: &str) -> Result<(), String> {
        self.api.comment(number, text)
    }

    fn approved_head(&mut self, number: u64, head: &str) -> Result<bool, String> {
        Ok(approved_at(&self.api.reviews(number)?, head))
    }
}

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

/// git in the queue's working copy, as queue-bot.
pub struct GitTrain {
    /// The checkout.
    pub root: PathBuf,
    /// The token for the calls that talk to the server (fetch, push), passed to git in the environment (`GIT_CONFIG_*`, never an
    /// argument). `None` in the tokenless half, which needs the branches already fetched.
    pub token: Option<String>,
    /// The merge check; [`crate::merge_check::command`] outside the tests.
    pub merge_check: MergeCheckFn,
}

/// The merge check as the train builder calls it.
pub type MergeCheckFn = fn(&crate::merge_check::Request) -> Result<Report, CliError>;

impl GitTrain {
    /// The real one.
    pub fn new(root: PathBuf, token: Option<String>) -> GitTrain {
        GitTrain {
            root,
            token,
            merge_check: crate::merge_check::command,
        }
    }
}

const IDENT: [&str; 4] = ["-c", "user.name=queue-bot", "-c", "user.email=queue-bot@dev.decryptology.net"];

impl GitTrain {
    fn command(&self, authed: bool) -> std::process::Command {
        let mut c = paddock_proc::command("git");
        c.arg("-C").arg(&self.root).args(IDENT);
        if let (true, Some(t)) = (authed, &self.token) {
            c.env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "http.extraheader")
                .env("GIT_CONFIG_VALUE_0", format!("Authorization: token {t}"))
                .env("GIT_TERMINAL_PROMPT", "0");
        }
        c
    }

    fn run(&self, authed: bool, args: &[&str]) -> Result<(bool, String), String> {
        let out = self
            .command(authed)
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
        self.run(false, args)
    }

    fn ok(&self, args: &[&str]) -> Result<String, String> {
        let (ok, out) = self.git(args)?;
        if ok {
            Ok(out)
        } else {
            Err(format!("git {} failed: {}", args.join(" "), out.trim()))
        }
    }

    fn ok_authed(&self, args: &[&str]) -> Result<String, String> {
        let (ok, out) = self.run(true, args)?;
        if ok {
            Ok(out)
        } else {
            Err(format!(
                "git {} failed: {}",
                args.first().unwrap_or(&""),
                out.replace(self.token.as_deref().unwrap_or("\u{0}"), "***").trim()
            ))
        }
    }
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
            self.ok_authed(&[
                "fetch",
                "--quiet",
                "--no-tags",
                "origin",
                &format!("+refs/heads/{b}:refs/remotes/origin/{b}"),
            ])?;
        }
        let spec = format!("+refs/heads/{TRAIN_BRANCH}:refs/remotes/origin/{TRAIN_BRANCH}");
        if !self.run(true, &["fetch", "--quiet", "--no-tags", "origin", &spec])?.0 {
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

    fn affected(&mut self, main: &str, head: &str) -> Result<BTreeSet<String>, String> {
        let args = [main.to_string(), head.to_string()];
        let r = crate::plan_cmd::command(&args, Some(self.root.clone())).map_err(|e| e.message)?;
        Ok(projects_of_plan(&r.data))
    }

    fn reached_workflows(&mut self, main: &str, head: &str) -> Result<Vec<String>, String> {
        let args = [main.to_string(), head.to_string()];
        let plan = crate::plan_cmd::command(&args, Some(self.root.clone())).map_err(|e| e.message)?;
        let reached = projects_of_plan(&plan.data);
        let mut out = vec!["ci.yml".to_string()];
        // The apps and their workflows are read from main: a pull request cannot take its own app out of the gates.
        for line in self.ok(&["ls-tree", "--name-only", main, "apps/"])?.lines() {
            let Some(app) = line.strip_prefix("apps/") else { continue };
            let (ok, text) = self.git(&["show", &format!("{main}:apps/{app}/paddock.ci.json")])?;
            let wf = ok
                .then(|| serde_json::from_str::<serde_json::Value>(&text).ok())
                .flatten()
                .and_then(|v| v["workflow"].as_str().map(str::to_string));
            if let Some(wf) = wf.filter(|w| !w.is_empty() && !out.contains(w))
                && (reached.contains("*") || reached.contains(&norm(app)))
            {
                out.push(wf);
            }
        }
        Ok(out)
    }

    fn checkout(&mut self, rev: &str) -> Result<(), String> {
        self.ok(&["checkout", "--quiet", "--force", "--detach", rev]).map(|_| ())
    }

    fn push_train(&mut self, head: &str, old: Option<&str>) -> Result<(), String> {
        let lease = format!("--force-with-lease=refs/heads/{TRAIN_BRANCH}:{}", old.unwrap_or(""));
        self.ok_authed(&["push", "--quiet", &lease, "origin", &format!("{head}:refs/heads/{TRAIN_BRANCH}")])
            .map(|_| ())
    }

    fn land(&mut self, head: &str) -> Result<(), String> {
        // A plain push: the server refuses anything that is not a fast-forward.
        self.ok_authed(&["push", "--quiet", "origin", &format!("{head}:refs/heads/main")])?;
        self.ok_authed(&[
            "fetch",
            "--quiet",
            "--no-tags",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main",
        ])?;
        let now = self.main_head()?;
        if now == head {
            Ok(())
        } else {
            Err(format!("after the push origin/main is {now}, not the train head {head}"))
        }
    }
}

/// The local fast gates: the gate register, the conformance line of ci.yml (read from `origin/main`, never from the train: a
/// pull request must not choose its own gate) and the commit lint of the train's range, the workspace-hack diff and the publish
/// dry run. They run PR code, so every gate subprocess runs without the queue's credentials ([`scrub`]). Formatting and clippy
/// run in the queue's Linux job (slice 6).
pub struct LocalGates {
    /// The checkout, at the train head.
    pub root: PathBuf,
}

/// The variables and configuration a gate subprocess must not see: the queue's tokens, git credentials, and any other variable
/// whose name says it is a secret. `parent` is the environment names of this process.
pub fn scrub(cmd: &mut std::process::Command, parent: &[String]) {
    const NAMED: [&str; 10] = [
        "GITEA_TOKEN",
        "QUEUE_BOT_TOKEN",
        "PLAN_TOKEN",
        "GITHUB_TOKEN",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    ];
    const WORDS: [&str; 6] = ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH", "PRIVATE_KEY"];
    for name in parent {
        let up = name.to_ascii_uppercase();
        if NAMED.contains(&up.as_str())
            || up.starts_with("GIT_CONFIG_KEY_")
            || up.starts_with("GIT_CONFIG_VALUE_")
            || WORDS.iter().any(|w| up.contains(w))
        {
            cmd.env_remove(name);
        }
    }
    for n in NAMED {
        cmd.env_remove(n);
    }
    // No credential helper or extra header from any git configuration the job brought.
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    cmd.env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_SYSTEM", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
}

/// One local gate: its name, program, arguments and the folder it runs in (relative to the checkout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateCmd {
    /// The gate's name in comments.
    pub name: &'static str,
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Folder under the checkout.
    pub dir: &'static str,
}

/// The gates of a train, in order. Besides the register, conformance and commit lint, three catch what only a later CI gate saw
/// on a merged tree (the recurring class "a train-time conflict only a later gate sees": workspace-hack drift, a dependency
/// cycle in the publish dry run): `cargo hakari generate --diff`, `scripts/publish.mjs --skip-build --offline`.
pub fn gate_commands(me: &str, conformance: &str, range: &str) -> Vec<GateCmd> {
    let c = |name, program: &str, args: &[&str], dir| GateCmd {
        name,
        program: program.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        dir,
    };
    vec![
        c("gates check", me, &["gates", "check"], "."),
        c("conformance", "sh", &["-c", conformance], "."),
        c(
            "commit-lint",
            "node",
            &["conformance/bin/paddock-conformance.mjs", "commit-lint", "--range", range],
            ".",
        ),
        c(
            "workspace-hack (cargo hakari generate --diff)",
            "cargo",
            &["hakari", "generate", "--diff"],
            "crates",
        ),
        c(
            "publish dry run (scripts/publish.mjs)",
            "node",
            &["scripts/publish.mjs", "--skip-build", "--offline"],
            ".",
        ),
    ]
}

/// The repository files a gate's output names (`crates/x/src/lib.rs:12`, `ui/a.ts`), in order of first mention, at most 50.
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

pub(crate) fn conformance_line(root: &Path) -> Option<String> {
    let out = paddock_proc::command("git")
        .arg("-C")
        .arg(root)
        .args(["show", "origin/main:.gitea/workflows/ci.yml"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().trim_start_matches("- ").strip_prefix("run: "))
        .find(|l| l.starts_with("node conformance/bin/paddock-conformance.mjs --app paddock") && l.contains("--only"))
        .map(str::to_string)
}

impl LocalGates {
    fn command(&self, g: &GateCmd) -> std::process::Command {
        let mut c = paddock_proc::command(&g.program);
        c.args(&g.args).current_dir(self.root.join(g.dir));
        let parent: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
        scrub(&mut c, &parent);
        c
    }

    /// Removes the credentials a checkout persisted in its own git configuration (an extra header, a credential helper), so PR
    /// code reading `.git/config` finds none. The push step brings its own through the environment.
    fn scrub_local_config(&self) {
        let list = paddock_proc::command("git")
            .arg("-C")
            .arg(&self.root)
            .args([
                "config",
                "--local",
                "--name-only",
                "--get-regexp",
                r"^(http\..*extraheader|credential\..*)$",
            ])
            .output();
        if let Ok(o) = list {
            for key in String::from_utf8_lossy(&o.stdout).lines() {
                let _ = paddock_proc::command("git")
                    .arg("-C")
                    .arg(&self.root)
                    .args(["config", "--local", "--unset-all", key])
                    .output();
            }
        }
    }

    /// `Ok(None)` passed, `Ok(Some(f))` failed, `Err` could not start (not measured).
    pub(crate) fn run_one(&self, g: &GateCmd) -> Result<Option<GateFailure>, String> {
        match self.command(g).output() {
            Ok(o) if o.status.success() => Ok(None),
            Ok(o) => {
                let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let tail: Vec<&str> = text.lines().rev().take(12).collect();
                let detail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                Ok(Some(GateFailure {
                    name: g.name.into(),
                    detail,
                    files: files_named(&text),
                }))
            }
            Err(e) => Err(format!("{}: could not start {}: {e}", g.name, g.program)),
        }
    }
}

impl Gates for LocalGates {
    fn run(&mut self, main: &str, head: &str) -> GateOutcome {
        let Some(line) = conformance_line(&self.root) else {
            return GateOutcome::NotMeasured("origin/main's ci.yml has no conformance line to run".into());
        };
        self.scrub_local_config();
        let me = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "./dev".into());
        let mut fails = Vec::new();
        for g in gate_commands(&me, &line, &format!("{main}..{head}")) {
            match self.run_one(&g) {
                Ok(None) => {}
                Ok(Some(f)) => fails.push(f),
                Err(why) => return GateOutcome::NotMeasured(why),
            }
        }
        if fails.is_empty() {
            GateOutcome::Green
        } else {
            GateOutcome::Red(fails)
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
        let report = triage::collect(&mut c, &req, &mut |l| eprintln!("{l}"))?;
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
                blame: blame_of(&r.status),
            })
            .collect())
    }
}
