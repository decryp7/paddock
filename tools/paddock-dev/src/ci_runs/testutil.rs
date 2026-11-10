//! A scripted [`Http`] for the tests: replies by URL, records every request.

use crate::wait::Http;
use serde_json::Value;
use std::collections::HashMap;

/// Recorded replies: JSON by URL, job logs by job id, and job logs that fail.
#[derive(Default)]
pub struct Fake {
    json: HashMap<String, Value>,
    logs: HashMap<u64, Result<String, String>>,
    urls: Vec<String>,
}

impl Fake {
    /// The replies recorded in a folder: `runs-1.json` (page 1 of the run list), `jobs-<run>.json` (page 1 of a run's jobs) and
    /// `log-<job>.txt`, for `repo` on `https://gitea.invalid`. Anything else is a 404.
    pub fn from_dir(dir: &std::path::Path, repo: &str) -> Fake {
        let api = format!("https://gitea.invalid/api/v1/repos/{repo}/actions");
        let mut f = Fake::default();
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        for n in names {
            let path = dir.join(&n);
            if n == "runs-1.json" {
                f.json(
                    &format!("{api}/runs?limit=50&page=1"),
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
                );
            } else if let Some(id) = n.strip_prefix("jobs-").and_then(|r| r.strip_suffix(".json")) {
                f.json(
                    &format!("{api}/runs/{id}/jobs?limit=50&page=1"),
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
                );
            } else if let Some(id) = n.strip_prefix("log-").and_then(|r| r.strip_suffix(".txt")) {
                f.log(id.parse().unwrap(), &std::fs::read_to_string(&path).unwrap());
            }
        }
        f
    }

    /// Answer `url` with `body`.
    pub fn json(&mut self, url: &str, body: Value) {
        self.json.insert(url.to_string(), body);
    }

    /// Forget the reply for `url` (it becomes a 404).
    pub fn remove_json(&mut self, url: &str) {
        self.json.remove(url);
    }

    /// Answer the log of `job` with `text`.
    pub fn log(&mut self, job: u64, text: &str) {
        self.logs.insert(job, Ok(text.to_string()));
    }

    /// Fail the log of `job` with `error`.
    pub fn fail(&mut self, job: u64, error: &str) {
        self.logs.insert(job, Err(error.to_string()));
    }

    /// The runs of page 1 of the run list.
    pub fn take_runs(&self) -> Vec<Value> {
        let page = &self.json[&self.runs_url()];
        page["workflow_runs"].as_array().cloned().unwrap_or_default()
    }

    /// Replace the runs of page 1 of the run list.
    pub fn set_runs(&mut self, runs: Vec<Value>) {
        let url = self.runs_url();
        self.json
            .insert(url, serde_json::json!({"total_count": runs.len(), "workflow_runs": runs}));
    }

    fn runs_url(&self) -> String {
        self.json
            .keys()
            .find(|u| u.ends_with("/runs?limit=50&page=1"))
            .cloned()
            .unwrap_or_default()
    }

    /// Every URL asked for, in order.
    pub fn requested(&self) -> &[String] {
        &self.urls
    }
}

impl Http for Fake {
    fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String> {
        assert!(!token.is_empty(), "the client must send its token");
        self.urls.push(url.to_string());
        self.json
            .get(url)
            .cloned()
            .ok_or_else(|| format!("curl exited with 22: The requested URL returned error: 404 ({url})"))
    }

    fn get_text(&mut self, url: &str, _token: &str) -> Result<String, String> {
        self.urls.push(url.to_string());
        let id: u64 = url
            .split("/jobs/")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        self.logs
            .get(&id)
            .cloned()
            .unwrap_or_else(|| Err(format!("curl exited with 22: The requested URL returned error: 404 ({url})")))
    }
}
