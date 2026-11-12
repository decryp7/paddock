// ci-runs-reader: the fake Gitea of the ledger tests
//! An in-memory Gitea for the unit tests: labels, pull requests and runs by commit, with every request recorded. It answers
//! the calls of [`super::gitea::Api`] only; anything else is a 404.

use crate::wait::Http;
use serde_json::{Value, json};

/// The fake server's state.
#[derive(Default)]
pub struct FakeGitea {
    /// Label objects as the API returns them.
    pub labels: Vec<Value>,
    /// Pull request objects as the API returns them.
    pub pulls: Vec<Value>,
    /// `(sha, run objects)`.
    pub runs: Vec<(String, Vec<Value>)>,
    /// `METHOD url` of every request, in order.
    pub requests: Vec<String>,
    /// The bodies of every write, in order.
    pub bodies: Vec<Value>,
    /// `(pull request number, text)` of every comment, in order.
    pub comments: Vec<(u64, String)>,
    /// The repository's `autodetect_manual_merge` (absent when `None`).
    pub autodetect: Option<bool>,
    /// The server's own maximum page size (Gitea's `MAX_RESPONSE_ITEMS`), below what the client asks for.
    pub max_page: Option<usize>,
}

const ROOT: &str = "https://gitea.invalid/api/v1/repos/o/r";

fn not_found(url: &str) -> String {
    format!("curl exited with 22: The requested URL returned error: 404 ({url})")
}

impl FakeGitea {
    /// The server root the tests give [`super::gitea::Api::new`].
    pub const SERVER: &'static str = "https://gitea.invalid";
    /// The repository the tests use.
    pub const REPO: &'static str = "o/r";

    /// Adds a label.
    pub fn label(&mut self, name: &str, color: &str, exclusive: bool) -> u64 {
        let id = self.labels.len() as u64 + 1;
        self.labels
            .push(json!({"id": id, "name": name, "color": color.trim_start_matches('#'), "description": "", "exclusive": exclusive}));
        id
    }

    /// Adds a pull request.
    pub fn pull(&mut self, number: u64, branch: &str, head: &str, state: &str, merged: Option<&str>, labels: &[&str]) {
        let labels: Vec<Value> = labels
            .iter()
            .map(|n| {
                self.labels
                    .iter()
                    .find(|l| l["name"] == *n)
                    .cloned()
                    .unwrap_or_else(|| json!({"name": n}))
            })
            .collect();
        self.pulls.push(json!({
            "number": number, "title": format!("WIP: {branch}"), "state": state, "draft": true,
            "merged": merged.is_some(), "merge_commit_sha": merged,
            "head": {"ref": branch, "sha": head}, "base": {"ref": "main"}, "labels": labels,
            "html_url": format!("https://gitea.invalid/o/r/pulls/{number}"),
        }));
    }

    fn page(&self, items: Vec<Value>, url: &str) -> Value {
        let q = |k: &str| {
            url.split(['?', '&'])
                .find_map(|p| p.strip_prefix(&format!("{k}=")))
                .and_then(|v| v.parse::<usize>().ok())
        };
        let (limit, page) = (q("limit").unwrap_or(50), q("page").unwrap_or(1));
        let limit = self.max_page.map_or(limit, |m| limit.min(m));
        Value::Array(items.into_iter().skip((page - 1) * limit).take(limit).collect())
    }
}

impl Http for FakeGitea {
    fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String> {
        assert!(!token.is_empty(), "the client must send its token");
        self.requests.push(format!("GET {url}"));
        let path = url.strip_prefix(ROOT).ok_or_else(|| not_found(url))?;
        if path.is_empty() {
            let mut repo = json!({"default_branch": "main"});
            if let Some(a) = self.autodetect {
                repo["autodetect_manual_merge"] = json!(a);
            }
            return Ok(repo);
        }
        if path.starts_with("/labels?") {
            return Ok(self.page(self.labels.clone(), url));
        }
        if let Some(rest) = path.strip_prefix("/pulls?state=") {
            let state = rest.split('&').next().unwrap_or("");
            let items = self
                .pulls
                .iter()
                .filter(|p| state == "all" || p["state"] == state)
                .cloned()
                .collect();
            return Ok(self.page(items, url));
        }
        if let Some(rest) = path.strip_prefix("/actions/runs?head_sha=") {
            let sha = rest.split('&').next().unwrap_or("");
            let runs = self.runs.iter().find(|(s, _)| s == sha).map(|(_, r)| r.clone()).unwrap_or_default();
            return Ok(json!({"total_count": runs.len(), "workflow_runs": runs}));
        }
        Err(not_found(url))
    }

    fn send_json(&mut self, method: &str, url: &str, token: &str, body: Option<&Value>) -> Result<Value, String> {
        assert!(!token.is_empty(), "the client must send its token");
        self.requests.push(format!("{method} {url}"));
        let body = body.cloned().unwrap_or(Value::Null);
        self.bodies.push(body.clone());
        let path = url.strip_prefix(ROOT).ok_or_else(|| not_found(url))?;
        match (method, path) {
            ("POST", "/labels") => {
                let id = self.labels.len() as u64 + 1;
                let mut l = body;
                l["id"] = json!(id);
                self.labels.push(l.clone());
                Ok(l)
            }
            ("PATCH", p) if p.starts_with("/labels/") => {
                let id: u64 = p["/labels/".len()..].parse().map_err(|_| not_found(url))?;
                let l = self.labels.iter_mut().find(|l| l["id"] == id).ok_or_else(|| not_found(url))?;
                for (k, v) in body.as_object().into_iter().flatten() {
                    l[k] = v.clone();
                }
                Ok(l.clone())
            }
            ("POST", "/pulls") => {
                let number = self.pulls.len() as u64 + 1;
                let ids: Vec<u64> = body["labels"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
                let names: Vec<String> = self
                    .labels
                    .iter()
                    .filter(|l| ids.contains(&l["id"].as_u64().unwrap_or(0)))
                    .filter_map(|l| l["name"].as_str().map(str::to_string))
                    .collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let branch = body["head"].as_str().unwrap_or("").to_string();
                self.pull(number, &branch, "0000000", "open", None, &refs);
                let p = self.pulls.last_mut().ok_or_else(|| not_found(url))?;
                p["title"] = body["title"].clone();
                Ok(p.clone())
            }
            ("POST", p) if p.starts_with("/issues/") && p.ends_with("/comments") => {
                let n: u64 = p["/issues/".len()..p.len() - "/comments".len()]
                    .parse()
                    .map_err(|_| not_found(url))?;
                self.comments.push((n, body["body"].as_str().unwrap_or("").to_string()));
                Ok(json!({}))
            }
            ("PUT", p) if p.starts_with("/issues/") && p.ends_with("/labels") => {
                let n: u64 = p["/issues/".len()..p.len() - "/labels".len()].parse().map_err(|_| not_found(url))?;
                let ids: Vec<u64> = body["labels"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
                let labels: Vec<Value> = self
                    .labels
                    .iter()
                    .filter(|l| ids.contains(&l["id"].as_u64().unwrap_or(0)))
                    .cloned()
                    .collect();
                let pr = self.pulls.iter_mut().find(|p| p["number"] == n).ok_or_else(|| not_found(url))?;
                pr["labels"] = Value::Array(labels.clone());
                Ok(Value::Array(labels))
            }
            _ => Err(not_found(url)),
        }
    }
}
