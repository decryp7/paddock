//! The shared store: the user's RustFS (S3 API), bucket [`BUCKET`] (ADR 0116 §4; the user chose a new bucket on 2026-11-26).
//!
//! The same endpoint and credentials as the compile cache ([`crate::local_sccache::read_creds`]: the machine's key file outside the
//! repository, else `AWS_*`; endpoint `PADDOCK_SCCACHE_S3_ENDPOINT`, default [`crate::local_sccache::DEFAULT_S3_ENDPOINT`]). Keys are
//! never printed, never in an argument list: they go to curl on standard input, in its config, which also signs the request
//! (`aws-sigv4`, the same common practice as the other tools' S3 clients, no signing crate). One curl run serves a whole call, its
//! transfers in parallel ([`PARALLEL`]); each request has its own budget ([`REQUEST_SECS`]) and the call [`CALL_SECS`] (§5). Any
//! answer that is not a record, a miss (404) or a success is an error for the whole call, and the caller then runs everything.

use super::key::RunnerClass;
use super::store::{Lookup, Record, Store, StoreError, rel_path};
use paddock_proc::{Limits, run_bounded_with_input};
use std::path::PathBuf;
use std::time::Duration;

/// The bucket of the test-result records: the one name local runs and CI use.
pub const BUCKET: &str = "test-results-paddock";
/// Transfers of one call in parallel.
pub const PARALLEL: usize = 16;
/// The budget of one request.
pub const REQUEST_SECS: u64 = 5;
/// The budget of one call (the whole lookup).
pub const CALL_SECS: u64 = 60;

/// The store's address and keys. `Debug` never shows the keys.
#[derive(Clone)]
pub struct S3Store {
    /// `http://host:port` or `https://host:port`.
    base: String,
    region: String,
    keys: (String, String),
    scratch: PathBuf,
}

impl std::fmt::Debug for S3Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "S3Store({}/{BUCKET})", self.base)
    }
}

impl S3Store {
    /// The store from `env` (a map of the process environment); `scratch` is a folder the store owns for the bodies of one call.
    /// `Err` says what is missing, without a value.
    pub fn from_env(env: &std::collections::HashMap<String, String>, scratch: PathBuf) -> Result<S3Store, String> {
        let creds = crate::local_sccache::read_creds(env)?;
        let get = |k: &str| creds.pairs().iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let (Some(id), Some(secret)) = (get("AWS_ACCESS_KEY_ID"), get("AWS_SECRET_ACCESS_KEY")) else {
            return Err("no S3 credentials (the key file outside the repository, or AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY)".into());
        };
        let endpoint = env
            .get("PADDOCK_SCCACHE_S3_ENDPOINT")
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| crate::local_sccache::DEFAULT_S3_ENDPOINT.to_string());
        let ssl = env.get("PADDOCK_SCCACHE_S3_SSL").is_some_and(|v| v == "true");
        let region = env
            .get("PADDOCK_SCCACHE_S3_REGION")
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| "auto".into());
        Ok(S3Store {
            base: format!("{}://{endpoint}", if ssl { "https" } else { "http" }),
            region,
            keys: (id, secret),
            scratch,
        })
    }

    /// The address of one object.
    fn url(&self, rel: &str) -> String {
        format!("{}/{BUCKET}/{rel}", self.base)
    }

    /// The curl config of a call: one block per transfer. `blocks` are `(url, output file, upload file)`.
    /// Pure, so a test reads it; the keys are in it (it goes to curl's standard input only).
    fn config(&self, blocks: &[(String, PathBuf, Option<PathBuf>)]) -> String {
        let mut out = String::new();
        for (i, (url, file, up)) in blocks.iter().enumerate() {
            if i > 0 {
                out.push_str("next\n");
            }
            out.push_str(&format!(
                "url = \"{url}\"\naws-sigv4 = \"aws:amz:{}:s3\"\nuser = \"{}:{}\"\nconnect-timeout = {REQUEST_SECS}\nmax-time = {REQUEST_SECS}\nsilent\nshow-error\noutput = \"{}\"\nwrite-out = \"%{{http_code}} %{{filename_effective}}\\n\"\n",
                self.region,
                self.keys.0,
                self.keys.1,
                file.display()
            ));
            if let Some(up) = up {
                out.push_str(&format!("upload-file = \"{}\"\n", up.display()));
            }
        }
        out
    }

    /// Runs the blocks in one curl; the status of each transfer, by its output file.
    fn run(&self, blocks: &[(String, PathBuf, Option<PathBuf>)]) -> Result<Vec<(u16, String)>, StoreError> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }
        let cmd = {
            let mut c = paddock_proc::command("curl");
            c.args(["-q", "--parallel", "--parallel-max", &PARALLEL.to_string(), "-K", "-"]);
            c
        };
        let out = run_bounded_with_input(cmd, self.config(blocks).into_bytes(), Limits::new(Duration::from_secs(CALL_SECS)))
            .map_err(|e| StoreError(self.mask(&format!("the store did not answer: {e}"))))?;
        if !out.success() {
            return Err(StoreError(self.mask(&format!(
                "curl failed: {}",
                out.stderr_text().trim().chars().take(200).collect::<String>()
            ))));
        }
        let text = out.stdout_text();
        let mut done = Vec::new();
        for l in text.lines() {
            let (code, file) = l.split_once(' ').ok_or_else(|| StoreError(format!("curl said {l:?}")))?;
            done.push((code.parse().unwrap_or(0), file.to_string()));
        }
        Ok(done)
    }

    fn mask(&self, t: &str) -> String {
        t.replace(&self.keys.1, "***").replace(&self.keys.0, "***")
    }

    fn body_path(&self, i: usize) -> PathBuf {
        self.scratch.join(format!("{}-{i}", std::process::id()))
    }
}

impl Store for S3Store {
    fn get_many(&self, lookups: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        std::fs::create_dir_all(&self.scratch).map_err(|e| StoreError(format!("{} cannot be made: {e}", self.scratch.display())))?;
        let blocks: Vec<_> = lookups
            .iter()
            .enumerate()
            .map(|(i, l)| (self.url(&rel_path(&l.target, &l.class, &l.key)), self.body_path(i), None))
            .collect();
        let done = self.run(&blocks);
        let result = done.and_then(|done| {
            let mut out = Vec::new();
            for (i, l) in lookups.iter().enumerate() {
                let file = self.body_path(i);
                let code = done.iter().find(|(_, f)| *f == file.display().to_string()).map(|(c, _)| *c);
                match code {
                    Some(200) => out.push(
                        std::fs::read_to_string(&file)
                            .ok()
                            .and_then(|t| Record::parse(&t, &l.key, &l.target, &l.class)),
                    ),
                    Some(404) => out.push(None),
                    other => {
                        return Err(StoreError(format!(
                            "the store answered {} for a record (credentials, bucket or a down server)",
                            other.map_or("nothing".into(), |c| c.to_string())
                        )));
                    }
                }
            }
            Ok(out)
        });
        for i in 0..lookups.len() {
            let _ = std::fs::remove_file(self.body_path(i));
        }
        result
    }

    fn put_many(&self, records: &[Record]) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.scratch).map_err(|e| StoreError(format!("{} cannot be made: {e}", self.scratch.display())))?;
        let mut blocks = Vec::new();
        for (i, r) in records.iter().enumerate() {
            let up = self.scratch.join(format!("{}-up-{i}", std::process::id()));
            std::fs::write(&up, serde_json::to_string(&r.json()).unwrap_or_default())
                .map_err(|e| StoreError(format!("{} cannot be written: {e}", up.display())))?;
            blocks.push((
                self.url(&rel_path(&r.target, &RunnerClass(r.class.0.clone()), &r.key)),
                self.body_path(i),
                Some(up),
            ));
        }
        let done = self.run(&blocks);
        for (i, _) in records.iter().enumerate() {
            let _ = std::fs::remove_file(self.scratch.join(format!("{}-up-{i}", std::process::id())));
            let _ = std::fs::remove_file(self.body_path(i));
        }
        let done = done?;
        if done.len() != records.len() || done.iter().any(|(c, _)| !(200..300).contains(c)) {
            return Err(StoreError(
                "the store refused a record (credentials, bucket or a down server)".into(),
            ));
        }
        Ok(())
    }
}
