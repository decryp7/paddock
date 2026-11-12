//! `pr`: the branch ledger on Gitea pull requests (user decision 2026-11-12; docs/plans/merge-queue.md).
//!
//! - `pr open <branch> [--labels a,b]`: a draft pull request (`WIP:` title, Gitea's draft prefix) against main, its body the
//!   branch's commits, labelled `state/built` unless a `state/` label is given;
//! - `pr label <branch> +x -y`: changes the labels of the branch's open pull request (an exclusive scope keeps one label);
//! - `pr list [--state approved]`: branch, head, labels and the newest CI run per workflow of the head;
//! - `pr sync [--dry-run]`: creates and updates the labels of `.gitea/labels.toml` on the server, never deletes one;
//! - `pr plan-status [--write]`: plan rows whose State cell disagrees with the ledger ([`plan_status`]).
//!
//! The token is read from `GITEA_TOKEN`, travels in a header only ([`crate::wait::CurlHttp`]) and is printed nowhere. Opening a
//! pull request starts no CI run: no workflow has a `pull_request` trigger (docs/plans/merge-queue.md; the contract test
//! `no_workflow_runs_on_a_pull_request` keeps it so).
//!
//! A pull request whose head reached main by a fast-forward push counts as landed from git (its head is on main), whether or
//! not the server's "autodetect manual merge" marked it merged; `pr sync` warns while that setting is off.

#[cfg(test)]
pub(crate) mod fake;
pub mod gitea;
pub mod labels;
pub mod ledger;
pub mod plan_status;

use crate::wait::CurlHttp;
use clap::{Args, Subcommand};
use gitea::Api;
use paddock_cli::{CliError, Exit, Report};
use serde_json::json;
use std::path::{Path, PathBuf};

const SERVER: &str = "https://dev.decryptology.net";

/// Where the ledger lives.
#[derive(Args, Clone)]
pub struct Common {
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The repository checkout (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

/// `pr`'s commands.
#[derive(Subcommand)]
pub enum PrCommand {
    /// Open a DRAFT pull request of a pushed branch against main; its body lists the branch's commits. Labelled `state/built`
    /// unless --labels names a `state/` label. Starts no CI run.
    Open {
        /// The branch (pushed: origin/<branch> must exist here).
        branch: String,
        /// Labels to add, comma separated (from .gitea/labels.toml).
        #[arg(long, value_delimiter = ',')]
        labels: Vec<String>,
        /// The base branch.
        #[arg(long, default_value = "main")]
        base: String,
        /// Server, repository and checkout.
        #[command(flatten)]
        common: Common,
    },
    /// Change the labels of the branch's open pull request: `+state/approved -owes/windows-tests`. Adding a label of an
    /// exclusive scope (state/, review-round/) removes the others of that scope.
    Label {
        /// The branch.
        branch: String,
        /// The changes: +name adds, -name removes.
        #[arg(required = true, allow_hyphen_values = true, num_args = 1..)]
        changes: Vec<String>,
        /// Server, repository and checkout.
        #[command(flatten)]
        common: Common,
    },
    /// The open pull requests as a table: number, branch, head, labels, the newest CI run per workflow of the head.
    List {
        /// Only those labelled state/<state> (`approved` or `state/approved`).
        #[arg(long)]
        state: Option<String>,
        /// Do not read the CI runs of each head (one request less per pull request).
        #[arg(long)]
        no_ci: bool,
        /// Server, repository and checkout.
        #[command(flatten)]
        common: Common,
    },
    /// Create the labels of .gitea/labels.toml that the server lacks and update changed ones. Never deletes a label.
    Sync {
        /// Print what would change; write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Server, repository and checkout.
        #[command(flatten)]
        common: Common,
    },
    /// Plan rows (docs/plans/*.md, apps/*/docs/plans/*.md) whose State cell disagrees with the ledger: "open" while the pull
    /// request is merged or state/landed, "done" while it is not. Exit 5 on findings; a row the ledger cannot place is
    /// "not measured" and also a finding.
    PlanStatus {
        /// Set the State cell of each landed row to `done (<commit>)`. Other findings stay for a person.
        #[arg(long)]
        write: bool,
        /// Only these plan files (repository-relative, comma separated).
        #[arg(long, value_delimiter = ',')]
        files: Vec<String>,
        /// The branch rows land on.
        #[arg(long, default_value = "main")]
        base: String,
        /// Server, repository and checkout.
        #[command(flatten)]
        common: Common,
    },
}

fn token() -> Result<String, CliError> {
    std::env::var("GITEA_TOKEN").ok().filter(|t| !t.is_empty()).ok_or_else(|| {
        CliError::usage("GITEA_TOKEN is not set.")
            .with_hint("Run `source ~/.zshenv`, or export a Gitea token that can write issues and pull requests of the repository.")
    })
}

pub(crate) fn root_of(given: &Option<PathBuf>) -> Result<PathBuf, CliError> {
    if let Some(r) = given {
        return Ok(r.clone());
    }
    let here = std::env::current_dir().map_err(|e| CliError::failure(format!("the current folder is unknown: {e}")))?;
    here.ancestors()
        .find(|d| d.join(".git").exists())
        .map(PathBuf::from)
        .ok_or_else(|| CliError::usage("no repository checkout was found from the current folder.").with_hint("Pass --root <checkout>."))
}

/// The app folders (`apps/<app>/paddock.ci.json`), sorted.
fn apps(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root.join("apps"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("paddock.ci.json").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

pub(crate) fn known(root: &Path) -> Result<Vec<labels::LabelSpec>, CliError> {
    let path = root.join(".gitea/labels.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        CliError::failure(format!("cannot read {}: {e}", path.display())).with_hint("Run from a checkout that has .gitea/.")
    })?;
    labels::parse(&text, &apps(root)).map_err(|e| CliError::failure(format!("{}: {e}", path.display())))
}

/// git in the checkout.
struct RepoGit {
    root: PathBuf,
}

impl RepoGit {
    fn run(&self, args: &[&str]) -> Result<(bool, String), String> {
        let out = paddock_proc::command("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| format!("cannot start git: {e}"))?;
        Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
    }
}

impl ledger::Git for RepoGit {
    fn remote_head(&self, branch: &str) -> Result<Option<String>, String> {
        let (ok, out) = self.run(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}^{{commit}}"),
        ])?;
        Ok(ok.then(|| out.trim().to_string()).filter(|s| !s.is_empty()))
    }

    fn commits(&self, base: &str, branch: &str) -> Result<Vec<(String, String)>, String> {
        let range = format!("refs/remotes/origin/{base}..refs/remotes/origin/{branch}");
        let (ok, out) = self.run(&["log", "--reverse", "--no-merges", "--format=%h%x09%s", &range, "--"])?;
        if !ok {
            return Err(format!("git log {range} failed; fetch origin"));
        }
        Ok(out
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect())
    }

    fn fetch(&self, branches: &[&str]) -> Result<(), String> {
        let specs: Vec<String> = branches
            .iter()
            .map(|b| format!("+refs/heads/{b}:refs/remotes/origin/{b}"))
            .collect();
        let mut args = vec!["fetch", "--quiet", "--no-tags", "origin"];
        args.extend(specs.iter().map(String::as_str));
        let (ok, _) = self.run(&args)?;
        if !ok {
            return Err(format!(
                "git fetch origin {} failed; check the remote and that the branch is pushed",
                branches.join(" ")
            ));
        }
        Ok(())
    }

    fn on_base(&self, commit: &str, base: &str) -> Result<Option<bool>, String> {
        if commit.starts_with('-') {
            return Ok(None);
        }
        let (exists, _) = self.run(&["cat-file", "-e", &format!("{commit}^{{commit}}")])?;
        if !exists {
            return Ok(None);
        }
        let (ok, _) = self.run(&["merge-base", "--is-ancestor", commit, &format!("refs/remotes/origin/{base}")])?;
        Ok(Some(ok))
    }
}

fn failed(what: &str) -> impl Fn(String) -> CliError + '_ {
    move |e| {
        CliError::failure(format!("{what}: {e}"))
            .with_hint("Check the network, that GITEA_TOKEN can write the repository's pull requests, and the branch.")
    }
}

/// The plan files under `root`, repository-relative and sorted.
fn plan_files(root: &Path) -> Vec<String> {
    let mut dirs = vec!["docs/plans".to_string()];
    dirs.extend(apps_any(root).into_iter().map(|a| format!("apps/{a}/docs/plans")));
    let mut out = Vec::new();
    for d in dirs {
        for e in std::fs::read_dir(root.join(&d)).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.ends_with(".md") && e.path().is_file() {
                out.push(format!("{d}/{n}"));
            }
        }
    }
    out.sort();
    out
}

/// Every folder under `apps/` (a plan may live in an app without a CI contract).
fn apps_any(root: &Path) -> Vec<String> {
    std::fs::read_dir(root.join("apps"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Runs one `pr` command.
pub fn command(c: PrCommand) -> Result<Report, CliError> {
    match c {
        PrCommand::Open {
            branch,
            labels,
            base,
            common,
        } => {
            let root = root_of(&common.root)?;
            let known = known(&root)?;
            let token = token()?;
            let mut http = CurlHttp;
            let mut api = Api::new(&mut http, &common.server, &common.repo, &token);
            let v = ledger::open(&mut api, &RepoGit { root }, &known, &branch, &base, &labels).map_err(failed("pr open"))?;
            let text = format!(
                "Opened draft #{} for {} ({} commit(s)), labels {}\n{}\n",
                v["number"],
                branch,
                v["commits"],
                v["labels"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join(","))
                    .unwrap_or_default(),
                v["url"].as_str().unwrap_or("")
            );
            Ok(Report::new(v, text))
        }
        PrCommand::Label { branch, changes, common } => {
            let root = root_of(&common.root)?;
            let known = known(&root)?;
            let token = token()?;
            let mut http = CurlHttp;
            let mut api = Api::new(&mut http, &common.server, &common.repo, &token);
            let v = ledger::label(&mut api, &known, &branch, &changes).map_err(failed("pr label"))?;
            let names = |k: &str| {
                v[k].as_array()
                    .map(|a| a.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join(","))
                    .unwrap_or_default()
            };
            let text = format!("#{} {branch}: {} -> {}\n", v["number"], names("before"), names("labels"));
            Ok(Report::new(v, text))
        }
        PrCommand::List { state, no_ci, common } => {
            let token = token()?;
            let mut http = CurlHttp;
            let mut api = Api::new(&mut http, &common.server, &common.repo, &token);
            let rows = ledger::list(&mut api, state.as_deref(), !no_ci).map_err(failed("pr list"))?;
            let text = ledger::list_text(&rows);
            Ok(Report::new(json!({"pulls": rows}), text))
        }
        PrCommand::Sync { dry_run, common } => {
            let root = root_of(&common.root)?;
            let known = known(&root)?;
            let token = token()?;
            let mut http = CurlHttp;
            let mut api = Api::new(&mut http, &common.server, &common.repo, &token);
            let v = ledger::sync(&mut api, &known, dry_run).map_err(failed("pr sync"))?;
            let list = |k: &str| {
                v[k].as_array()
                    .map(|a| a.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            };
            let verb = if dry_run { "would create" } else { "created" };
            if let Some(w) = v["warning"].as_str() {
                eprintln!("warning: {w}");
            }
            let text = format!(
                "{verb}: {}\n{}: {}\nunchanged: {}\non the server but not in .gitea/labels.toml (kept): {}\n",
                list("created"),
                if dry_run { "would update" } else { "updated" },
                list("updated"),
                v["unchanged"],
                list("extra")
            );
            Ok(Report::new(v, text))
        }
        PrCommand::PlanStatus {
            write,
            files,
            base,
            common,
        } => {
            let root = root_of(&common.root)?;
            let token = token()?;
            let files = if files.is_empty() { plan_files(&root) } else { files };
            let mut http = CurlHttp;
            let mut api = Api::new(&mut http, &common.server, &common.repo, &token);
            let pulls = api.pulls("all").map_err(failed("pr plan-status"))?;
            let git = RepoGit { root: root.clone() };
            let (mut all, mut text, mut written) = (Vec::new(), String::new(), 0usize);
            for f in &files {
                let path = root.join(f);
                let body = std::fs::read_to_string(&path).map_err(|e| CliError::failure(format!("cannot read {f}: {e}")))?;
                let (findings, new) = plan_status::check_file(&body, &pulls, &git, &base, write).map_err(failed("pr plan-status"))?;
                if let Some(new) = new {
                    std::fs::write(&path, new).map_err(|e| CliError::failure(format!("cannot write {f}: {e}")))?;
                }
                for x in &findings {
                    let j = plan_status::finding_json(f, x);
                    written += usize::from(x.written);
                    text.push_str(&format!(
                        "{f}:{}: state {:?}; ledger {} ({}){}\n",
                        x.row.line,
                        x.row.state,
                        j["ledger"].as_str().unwrap_or(""),
                        j["detail"].as_str().unwrap_or(""),
                        if x.written { " -> written" } else { "" }
                    ));
                    all.push(j);
                }
            }
            let open = all.iter().filter(|j| j["written"] != true).count();
            text.push_str(&format!(
                "{} plan file(s), {} finding(s), {written} written, {open} left for a person\n",
                files.len(),
                all.len()
            ));
            let exit = if open > 0 { Exit::Findings } else { Exit::Ok };
            Ok(Report::new(
                json!({"files": files.len(), "findings": all, "written": written, "open": open}),
                text,
            )
            .with_exit(exit))
        }
    }
}
