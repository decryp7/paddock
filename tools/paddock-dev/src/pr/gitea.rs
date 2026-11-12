// ci-runs-reader: pr list: the newest run per workflow of a pull request head, shown in the table, no verdict
//! The Gitea API calls of the ledger: labels, pull requests and the CI runs of a commit. The token travels in a header only
//! (the [`Http`] contract) and is in no message.

use crate::wait::Http;
use serde_json::{Value, json};

/// A label on the server.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerLabel {
    /// Its id.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// `#rrggbb` (lower case).
    pub color: String,
    /// Its description.
    pub description: String,
    /// Whether its scope is exclusive.
    pub exclusive: bool,
}

impl ServerLabel {
    fn from_json(v: &Value) -> ServerLabel {
        let color = v["color"].as_str().unwrap_or("").trim_start_matches('#').to_ascii_lowercase();
        ServerLabel {
            id: v["id"].as_u64().unwrap_or(0),
            name: v["name"].as_str().unwrap_or("").to_string(),
            color: format!("#{color}"),
            description: v["description"].as_str().unwrap_or("").to_string(),
            exclusive: v["exclusive"].as_bool().unwrap_or(false),
        }
    }
}

/// A pull request as the ledger reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pull {
    /// Its number (the issue index).
    pub number: u64,
    /// Its title.
    pub title: String,
    /// `open` or `closed`.
    pub state: String,
    /// Whether it is a draft.
    pub draft: bool,
    /// Whether it was merged (Gitea also marks a pull request merged when its head reaches the base by a fast-forward).
    pub merged: bool,
    /// The merge commit, when merged.
    pub merge_commit: Option<String>,
    /// The head branch.
    pub branch: String,
    /// The head commit.
    pub head: String,
    /// The base branch.
    pub base: String,
    /// Its labels' names.
    pub labels: Vec<String>,
    /// Its web page.
    pub url: String,
}

impl Pull {
    fn from_json(v: &Value) -> Pull {
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        Pull {
            number: v["number"].as_u64().unwrap_or(0),
            title: s(&v["title"]),
            state: s(&v["state"]),
            draft: v["draft"].as_bool().unwrap_or(false),
            merged: v["merged"].as_bool().unwrap_or(false),
            merge_commit: v["merge_commit_sha"].as_str().filter(|c| !c.is_empty()).map(str::to_string),
            branch: s(&v["head"]["ref"]),
            head: s(&v["head"]["sha"]),
            base: s(&v["base"]["ref"]),
            labels: v["labels"]
                .as_array()
                .map(|a| a.iter().filter_map(|l| l["name"].as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            url: s(&v["html_url"]),
        }
    }
}

/// One CI run of a commit: workflow file, run id, and its result (`success`, `failure`, `cancelled`, or the status while it runs).
#[derive(Debug, Clone, PartialEq)]
pub struct CommitRun {
    /// `ci.yml`.
    pub workflow: String,
    /// The run's id.
    pub id: u64,
    /// The conclusion, or the status while it runs.
    pub result: String,
}

/// The ledger's API of one repository.
pub struct Api<'a> {
    http: &'a mut dyn Http,
    root: String,
    token: String,
}

/// Pages ask for 50; the server may give fewer (its own maximum page size), so a listing ends at the first empty page.
/// 80 pages bound a runaway listing.
const PAGE: usize = 50;
const MAX_PAGES: u32 = 80;

impl<'a> Api<'a> {
    /// `server` like `https://dev.decryptology.net`, `repo` like `decryp7/paddock`.
    pub fn new(http: &'a mut dyn Http, server: &str, repo: &str, token: &str) -> Api<'a> {
        Api {
            http,
            root: format!("{}/api/v1/repos/{repo}", server.trim_end_matches('/')),
            token: token.to_string(),
        }
    }

    fn pages(&mut self, path: &str) -> Result<Vec<Value>, String> {
        let sep = if path.contains('?') { '&' } else { '?' };
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let url = format!("{}{path}{sep}limit={PAGE}&page={page}", self.root);
            let body = self.http.get_json(&url, &self.token)?;
            let batch = body.as_array().cloned().unwrap_or_default();
            if batch.is_empty() {
                return Ok(all);
            }
            all.extend(batch);
        }
        Err(format!("more than {MAX_PAGES} pages under {path}; the listing was stopped"))
    }

    /// Whether the repository marks a pull request merged when its head reaches the base outside Gitea (a fast-forward
    /// push): the setting `autodetect_manual_merge`. `None` when the server does not say.
    pub fn autodetects_manual_merge(&mut self) -> Result<Option<bool>, String> {
        let v = self.http.get_json(&self.root, &self.token)?;
        Ok(v["autodetect_manual_merge"].as_bool())
    }

    /// Every label of the repository.
    pub fn labels(&mut self) -> Result<Vec<ServerLabel>, String> {
        Ok(self.pages("/labels")?.iter().map(ServerLabel::from_json).collect())
    }

    /// Creates a label.
    pub fn create_label(&mut self, name: &str, color: &str, description: &str, exclusive: bool) -> Result<ServerLabel, String> {
        let body = json!({"name": name, "color": color, "description": description, "exclusive": exclusive});
        let url = format!("{}/labels", self.root);
        Ok(ServerLabel::from_json(&self.http.send_json(
            "POST",
            &url,
            &self.token,
            Some(&body),
        )?))
    }

    /// Changes a label's colour, description and exclusivity.
    pub fn edit_label(&mut self, id: u64, color: &str, description: &str, exclusive: bool) -> Result<(), String> {
        let body = json!({"color": color, "description": description, "exclusive": exclusive});
        let url = format!("{}/labels/{id}", self.root);
        self.http.send_json("PATCH", &url, &self.token, Some(&body)).map(|_| ())
    }

    /// The pull requests in `state` (`open`, `closed`, `all`).
    pub fn pulls(&mut self, state: &str) -> Result<Vec<Pull>, String> {
        Ok(self.pages(&format!("/pulls?state={state}"))?.iter().map(Pull::from_json).collect())
    }

    /// Opens a pull request of `branch` against `base` with these label ids.
    pub fn create_pull(&mut self, branch: &str, base: &str, title: &str, body: &str, labels: &[u64]) -> Result<Pull, String> {
        let req = json!({"head": branch, "base": base, "title": title, "body": body, "labels": labels});
        let url = format!("{}/pulls", self.root);
        Ok(Pull::from_json(&self.http.send_json("POST", &url, &self.token, Some(&req))?))
    }

    /// Replaces the labels of pull request `number` with these ids; returns the names the server now has.
    pub fn set_labels(&mut self, number: u64, ids: &[u64]) -> Result<Vec<String>, String> {
        let url = format!("{}/issues/{number}/labels", self.root);
        let v = self.http.send_json("PUT", &url, &self.token, Some(&json!({"labels": ids})))?;
        Ok(v.as_array()
            .map(|a| a.iter().filter_map(|l| l["name"].as_str().map(str::to_string)).collect())
            .unwrap_or_default())
    }

    /// Adds a comment to pull request (issue) `number`.
    pub fn comment(&mut self, number: u64, body: &str) -> Result<(), String> {
        let url = format!("{}/issues/{number}/comments", self.root);
        self.http
            .send_json("POST", &url, &self.token, Some(&json!({"body": body})))
            .map(|_| ())
    }

    /// The newest run of each workflow for commit `sha` (first page of 50, newest first), in the server's order.
    pub fn commit_runs(&mut self, sha: &str) -> Result<Vec<CommitRun>, String> {
        if sha.is_empty() || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("`{sha}` is not a commit id"));
        }
        let url = format!("{}/actions/runs?head_sha={sha}&limit={PAGE}&page=1", self.root);
        let body = self.http.get_json(&url, &self.token)?;
        let mut out: Vec<CommitRun> = Vec::new();
        for r in body["workflow_runs"].as_array().into_iter().flatten() {
            let workflow = r["path"].as_str().unwrap_or("").split('@').next().unwrap_or("").to_string();
            if out.iter().any(|o| o.workflow == workflow) {
                continue; // an older run of the same workflow
            }
            let conclusion = r["conclusion"].as_str().unwrap_or("");
            let result = if conclusion.is_empty() {
                r["status"].as_str().unwrap_or("")
            } else {
                conclusion
            };
            out.push(CommitRun {
                workflow,
                id: r["id"].as_u64().unwrap_or(0),
                result: result.to_string(),
            });
        }
        Ok(out)
    }
}
