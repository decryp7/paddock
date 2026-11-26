// ci-runs-reader: the fake Gitea of the tests
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
    errors: HashMap<String, String>,
}

impl Fake {
    /// The replies recorded in a folder: `runs-1.json` (page 1 of the run list), `run-<id>.json` (one run), `jobs-<run>.json` (page 1 of a run's jobs) and
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
            } else if let Some(id) = n.strip_prefix("run-").and_then(|r| r.strip_suffix(".json")) {
                f.json(
                    &format!("{api}/runs/{id}"),
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

    /// Fail `url` with `error` (a server error, not a 404).
    pub fn error(&mut self, url: &str, error: &str) {
        self.errors.insert(url.to_string(), error.to_string());
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
        if let Some(e) = self.errors.get(url) {
            return Err(e.clone());
        }
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

/// A fixture repository built once per machine and recipe, not once per test (ER-20, rule 85): nextest runs each test in its own
/// process, so a per-binary `OnceLock` shares nothing, and the dozens of git starts of a fixture (about 45 ms each on the Windows
/// runner) were paid by every test. The template lives in `<temp>/paddock-dev-test-templates/<name>-<hash of recipe>`; `recipe` is the
/// text of the source that builds it (`include_str!` of the test file), so any change to the recipe is a new template. `build` gets a
/// scratch folder to fill and the final folder the template will have (for an absolute path it must record); the scratch folder is
/// renamed into place, and a second builder that loses the race drops its own. Owner: the tests of `ci_runs`; size: under 1 MB each;
/// eviction: a template of the same name and another hash is removed when a new one is built, and the OS clears the temp folder.
/// Callers copy it with [`copy_tree`] and never write into it.
pub fn template(name: &str, recipe: &str, build: impl FnOnce(&std::path::Path, &std::path::Path)) -> std::path::PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    recipe.hash(&mut h);
    let root = std::env::temp_dir().join("paddock-dev-test-templates");
    let dir_name = format!("{name}-{:016x}", h.finish());
    let ready = root.join(&dir_name);
    if ready.join(".ready").is_file() {
        return ready;
    }
    std::fs::create_dir_all(&root).unwrap();
    let scratch = tempfile::Builder::new().prefix("building-").tempdir_in(&root).unwrap();
    build(scratch.path(), &ready);
    std::fs::write(scratch.path().join(".ready"), "").unwrap();
    for old in std::fs::read_dir(&root).unwrap().flatten() {
        let n = old.file_name().to_string_lossy().into_owned();
        if n.starts_with(&format!("{name}-")) && n != dir_name {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
    let built = scratch.keep();
    if std::fs::rename(&built, &ready).is_err() {
        let _ = std::fs::remove_dir_all(&built); // another test process won the race: use its template
    }
    ready
}

/// Copies a folder tree: a fixture's private copy of its template, with no git start.
pub fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    // per-item: a file copy, not a process start
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).unwrap();
        }
    }
}
