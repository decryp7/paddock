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
use crate::curl_config;
use paddock_ci_config::settings::sccache::{TestCacheAddress, test_cache_address, test_cache_key};
use paddock_ci_config::settings::{EnvMap, Settings};
use paddock_proc::{Limits, run_bounded_with_input};
use paddock_secrets::Secret;
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

/// One transfer: URL, file the answer goes to, file to upload, method.
/// What one curl run said per transfer, and curl's own words when it exited non-zero.
type Each = (Vec<(u16, String)>, Option<String>);
/// A try of some blocks, as [`S3Store::run_each`].
type TryFn<'a> = &'a mut dyn FnMut(&[Block]) -> Result<Each, StoreError>;

pub(super) type Block = (String, PathBuf, Option<PathBuf>, &'static str);

/// The store's address and keys. `Debug` never shows the keys.
#[derive(Clone)]
pub struct S3Store {
    /// `http://host:port` or `https://host:port`.
    base: String,
    region: String,
    keys: (Secret, Secret),
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
        // CI's own key first (user decision 2026-11-26: a separate RustFS key for the test cache; C141 lets only `./dev ci test-cache`
        // steps name its secrets). Elsewhere the machine's usual source: the key file outside the repository, else `AWS_*`.
        let vars = EnvMap(env);
        let settings = Settings::new(&vars);
        let (id, secret) = match test_cache_key(&settings).map_err(|e| e.0)? {
            Some((id, secret)) => (id, secret),
            None => {
                let creds = crate::local_sccache::read_creds(env)?;
                let get = |k: &str| creds.pairs().iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                let (Some(id), Some(secret)) = (get("AWS_ACCESS_KEY_ID"), get("AWS_SECRET_ACCESS_KEY")) else {
                    return Err("no S3 credentials (CI: the secrets PADDOCK_TEST_CACHE_KEY_ID and PADDOCK_TEST_CACHE_SECRET; elsewhere the key file outside the repository, or AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY)".into());
                };
                (
                    Secret::new(id).map_err(|e| e.to_string())?,
                    Secret::new(secret).map_err(|e| e.to_string())?,
                )
            }
        };
        // The address exactly as the compile cache reads the endpoint (`parse_endpoint`, ER-22): the repository variable is
        // `http://host:port`, so the scheme is taken from it and never added a second time (run 9135: `http://http://...` answered 308).
        let TestCacheAddress { base, region } = test_cache_address(&settings)?;
        Ok(S3Store {
            base,
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
    fn config(&self, blocks: &[Block]) -> String {
        let mut out = String::new();
        for (i, (url, file, up, method)) in blocks.iter().enumerate() {
            if i > 0 {
                out.push_str("next\n");
            }
            out.push_str(&curl_config::line("url", url));
            out.push_str(&format!("aws-sigv4 = \"aws:amz:{}:s3\"\n", self.region));
            out.push_str(&curl_config::line(
                "user",
                &format!("{}:{}", self.keys.0.expose(), self.keys.1.expose()),
            ));
            out.push_str(&format!(
                "connect-timeout = {REQUEST_SECS}\nmax-time = {REQUEST_SECS}\nsilent\nshow-error\n"
            ));
            out.push_str(&curl_config::line("output", &file.display().to_string()));
            out.push_str("write-out = \"%{http_code}|%{redirect_url}|%{filename_effective}\\n\"\n");
            if let Some(up) = up {
                out.push_str(&curl_config::line("upload-file", &up.display().to_string()));
            }
            if *method != "GET" && up.is_none() {
                out.push_str(&format!("request = \"{method}\"\n"));
            }
        }
        out
    }

    /// Runs the blocks in one curl; the status of each transfer, by its output file. Any transfer that failed is an error for the call (a
    /// write, a delete or a listing is atomic); [`S3Store::run_each`] is the form a lookup uses.
    fn run(&self, blocks: &[Block]) -> Result<Vec<(u16, String)>, StoreError> {
        let (done, failed) = self.run_each(blocks)?;
        match failed {
            Some(why) => Err(StoreError(why)),
            None => Ok(done),
        }
    }

    /// Runs the blocks in one curl and keeps what each transfer said even when curl exits non-zero (one request timed out): status 0
    /// is a transfer that got no answer. The second answer is curl's own words when it exited non-zero. `Err` only when curl said
    /// nothing readable for the blocks (it did not start, the call's budget ended, a redirect).
    fn run_each(&self, blocks: &[Block]) -> Result<Each, StoreError> {
        if blocks.is_empty() {
            return Ok((Vec::new(), None));
        }
        let cmd = {
            let mut c = paddock_proc::command("curl");
            c.args(["-q", "--parallel", "--parallel-max", &PARALLEL.to_string(), "-K", "-"]);
            c
        };
        let out = run_bounded_with_input(cmd, self.config(blocks).into_bytes(), Limits::new(Duration::from_secs(CALL_SECS)))
            .map_err(|e| StoreError(self.mask(&format!("the store did not answer: {e}"))))?;
        let why = || {
            self.mask(&format!(
                "curl failed: {}",
                out.stderr_text().trim().chars().take(200).collect::<String>()
            ))
        };
        let done = parse_done(&out.stdout_text());
        if out.success() {
            return Ok((done?, None));
        }
        match done {
            Ok(d) if d.len() == blocks.len() => Ok((d, Some(why()))),
            _ => Err(StoreError(why())),
        }
    }

    fn mask(&self, t: &str) -> String {
        t.replace(self.keys.1.expose(), "***").replace(self.keys.0.expose(), "***")
    }

    fn body_path(&self, i: usize) -> PathBuf {
        self.scratch.join(format!("{}-{i}", std::process::id()))
    }
}

impl Store for S3Store {
    fn delete_many(&self, lookups: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        let found = self.get_many(lookups).into_iter().collect::<Result<Vec<_>, _>>()?;
        let rels: Vec<String> = lookups.iter().map(|l| rel_path(&l.target, &l.class, &l.key)).collect();
        self.delete_many_rels(&rels)?;
        Ok(found)
    }

    fn get_many(&self, lookups: &[Lookup]) -> Vec<Result<Option<Record>, StoreError>> {
        let all_fail = |e: StoreError| lookups.iter().map(|_| Err(StoreError(e.0.clone()))).collect::<Vec<_>>();
        if let Err(e) = std::fs::create_dir_all(&self.scratch) {
            return all_fail(StoreError(format!("{} cannot be made: {e}", self.scratch.display())));
        }
        let blocks: Vec<_> = lookups
            .iter()
            .enumerate()
            .map(|(i, l)| (self.url(&rel_path(&l.target, &l.class, &l.key)), self.body_path(i), None, "GET"))
            .collect();
        let codes = fetch_each(&blocks, &mut |b| self.run_each(b));
        let out = lookups
            .iter()
            .zip(codes)
            .enumerate()
            .map(|(i, (l, code))| match code? {
                200 => Ok(std::fs::read_to_string(self.body_path(i))
                    .ok()
                    .and_then(|t| Record::parse(&t, &l.key, &l.target, &l.class))),
                404 => Ok(None),
                other => Err(StoreError(format!(
                    "the store answered {other} for a record (credentials, bucket or a down server)"
                ))),
            })
            .collect();
        for i in 0..lookups.len() {
            let _ = std::fs::remove_file(self.body_path(i));
        }
        out
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
                "PUT",
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

/// What the live probe saw, step by step (`ci test-cache selfcheck`).
#[derive(Debug, PartialEq, Eq)]
pub struct Probe {
    /// The object came back with the bytes that were put.
    pub round_trip: bool,
    /// The listing under the scratch prefix named it.
    pub listed: bool,
    /// After the delete it was gone (404) and the listing no longer named it.
    pub deleted: bool,
}

impl S3Store {
    fn one(&self, rel: &str, method: &'static str, up: Option<&std::path::Path>, tag: &str) -> Result<(u16, String), StoreError> {
        std::fs::create_dir_all(&self.scratch).map_err(|e| StoreError(format!("{} cannot be made: {e}", self.scratch.display())))?;
        let out = self.scratch.join(format!("{}-{tag}", std::process::id()));
        let done = self.run(&[(self.url(rel), out.clone(), up.map(PathBuf::from), method)]);
        let body = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        let (code, _) = done?
            .into_iter()
            .next()
            .ok_or_else(|| StoreError("curl reported no transfer".into()))?;
        Ok((code, body))
    }

    /// Deletes the objects under `rels` (paths below the bucket); a missing object is fine (S3 answers 204).
    pub fn delete_many_rels(&self, rels: &[String]) -> Result<(), StoreError> {
        let blocks: Vec<Block> = rels
            .iter()
            .enumerate()
            .map(|(i, r)| (self.url(r), self.body_path(i), None, "DELETE"))
            .collect();
        let done = self.run(&blocks);
        for i in 0..rels.len() {
            let _ = std::fs::remove_file(self.body_path(i));
        }
        match done?.iter().find(|(c, _)| !(200..300).contains(c)) {
            Some((c, _)) => Err(StoreError(format!("the store answered {c} to a delete"))),
            None => Ok(()),
        }
    }

    /// The keys under `prefix` (one page, at most 1000: the probe's and `audit`'s need).
    pub fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let url = format!("{}/{BUCKET}?list-type=2&prefix={prefix}", self.base);
        std::fs::create_dir_all(&self.scratch).map_err(|e| StoreError(format!("{} cannot be made: {e}", self.scratch.display())))?;
        let out = self.scratch.join(format!("{}-list", std::process::id()));
        let done = self.run(&[(url, out.clone(), None, "GET")]);
        let body = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        match done?.first() {
            Some((200, _)) => Ok(body
                .split("<Key>")
                .skip(1)
                .filter_map(|p| p.split("</Key>").next())
                .map(str::to_string)
                .collect()),
            Some((c, _)) => Err(StoreError(format!("the store answered {c} to a listing{}", s3_error(&body)))),
            None => Err(StoreError("curl reported no transfer".into())),
        }
    }

    /// Puts, gets, lists and deletes one scratch object under `v<schema>/selfcheck/<token>/` and says what worked. Positive control of the store
    /// itself (ER-33): a probe that cannot see the object reports it, never a pass. The object is removed on every path.
    pub fn probe(&self, token: &str) -> Result<Probe, StoreError> {
        let rel = format!("v{}/selfcheck/{token}/probe.json", super::key::SCHEMA);
        let body = format!("{{\"probe\":\"{token}\"}}");
        let up = self.scratch.join(format!("{}-probe-up", std::process::id()));
        std::fs::create_dir_all(&self.scratch).map_err(|e| StoreError(format!("{} cannot be made: {e}", self.scratch.display())))?;
        std::fs::write(&up, &body).map_err(|e| StoreError(format!("{} cannot be written: {e}", up.display())))?;
        let result = (|| {
            let (c, answer) = self.one(&rel, "PUT", Some(&up), "put")?;
            if !(200..300).contains(&c) {
                return Err(StoreError(format!("the store answered {c} to a put{}", s3_error(&answer))));
            }
            let (c, got) = self.one(&rel, "GET", None, "get")?;
            let round_trip = c == 200 && got == body;
            let listed = self.list(&format!("v{}/selfcheck/{token}/", super::key::SCHEMA))?.contains(&rel);
            self.delete_many_rels(std::slice::from_ref(&rel))?;
            let (c, _) = self.one(&rel, "GET", None, "gone")?;
            let deleted = c == 404 && !self.list(&format!("v{}/selfcheck/{token}/", super::key::SCHEMA))?.contains(&rel);
            Ok(Probe {
                round_trip,
                listed,
                deleted,
            })
        })();
        if result.is_err() {
            let _ = self.delete_many_rels(std::slice::from_ref(&rel));
        }
        let _ = std::fs::remove_file(&up);
        result
    }
}

/// The status of every block, from one try and at most one more for the blocks that got no answer or a server error (status 0 or 5xx):
/// per item, never per call (PRINCIPLES, bulk APIs: errors are per item unless the operation is atomic; ADR 0116 §5, amended
/// 2026-11-28). `Err` is that item's cause. `try_` runs blocks and answers as [`S3Store::run_each`] does.
pub(super) fn fetch_each(blocks: &[Block], try_: TryFn<'_>) -> Vec<Result<u16, StoreError>> {
    // The answers per block, and whether the call failed whole.
    let one = |blocks: &[Block], try_: TryFn<'_>| match try_(blocks) {
        Ok((done, why)) => (
            blocks
                .iter()
                .map(|b| {
                    let file = b.1.display().to_string();
                    match done.iter().find(|(_, f)| *f == file).map(|(c, _)| *c) {
                        Some(0) | None => Err(StoreError(why.clone().unwrap_or_else(|| "the store gave no answer".into()))),
                        Some(c) => Ok(c),
                    }
                })
                .collect::<Vec<_>>(),
            false,
        ),
        Err(e) => (blocks.iter().map(|_| Err(StoreError(e.0.clone()))).collect(), true),
    };
    let (mut first, whole) = one(blocks, try_);
    // A call that failed whole (curl did not start, the call's budget ended, a redirect) is not retried: another 60 s would not help.
    let retry: Vec<usize> = (0..blocks.len())
        .filter(|&i| !whole && first[i].as_ref().map_or(true, |c| *c >= 500))
        .collect();
    if retry.is_empty() {
        return first;
    }
    let again: Vec<Block> = retry.iter().map(|&i| blocks[i].clone()).collect();
    for (&i, r) in retry.iter().zip(one(&again, try_).0) {
        first[i] = r;
    }
    first
}

/// The error code and message of an S3 error body (`<Code>AccessDenied</Code><Message>..</Message>`), for a log line: both are the
/// server's words about the request, never the key.
fn s3_error(body: &str) -> String {
    let tag = |t: &str| {
        body.split(&format!("<{t}>"))
            .nth(1)
            .and_then(|r| r.split(&format!("</{t}>")).next())
            .map(str::to_string)
    };
    match (tag("Code"), tag("Message")) {
        (Some(c), Some(m)) => format!(" ({c}: {})", m.chars().take(200).collect::<String>()),
        (Some(c), None) => format!(" ({c})"),
        _ => String::new(),
    }
}

/// Reads curl's per-transfer `write-out` lines, `<code>|<redirect url>|<output file>`. A redirect (3xx) is never followed: a signed request
/// does not survive one, and a store that redirects every request (run 9135: a doubled `http://` in the address) must say so, not look like a
/// refusal. It is an error for the whole call, with the address it was sent to, so the caller runs everything.
pub(super) fn parse_done(text: &str) -> Result<Vec<(u16, String)>, StoreError> {
    let mut done = Vec::new();
    for l in text.lines() {
        let mut p = l.splitn(3, '|');
        let (Some(code), Some(redirect), Some(file)) = (p.next(), p.next(), p.next()) else {
            return Err(StoreError(format!("curl said {l:?}")));
        };
        let code: u16 = code.parse().unwrap_or(0);
        if (300..400).contains(&code) {
            let to = if redirect.is_empty() {
                "an address it did not give".to_string()
            } else {
                redirect.to_string()
            };
            return Err(StoreError(format!(
                "the store answered {code}, a redirect to {to} (not followed: the endpoint is `http://host:port` with no path, the scheme is not added twice)"
            )));
        }
        done.push((code, file.to_string()));
    }
    Ok(done)
}

#[cfg(test)]
mod config_tests {
    use super::*;

    fn store(scratch: &str) -> S3Store {
        S3Store {
            base: "http://127.0.0.1:9".into(),
            region: "us-east-1".into(),
            keys: (
                Secret::new(String::from("AKIDEXAMPLE")).unwrap(),
                Secret::new(String::from("s3cr3t-value")).unwrap(),
            ),
            scratch: PathBuf::from(scratch),
        }
    }

    /// Run 9265: on the Windows runner the scratch folder is `D:\a\_temp\...`; curl drops a backslash before a letter and reads `\t` and `\r`
    /// as a tab and a return, so it wrote the answer to another file, the call failed and nothing was recorded.
    #[test]
    fn a_windows_path_reaches_curl_with_its_backslashes_doubled() {
        let s = store("D:\\a\\_temp\\test-cache\\scratch");
        let blocks = [(s.url("v2/a/b.json"), s.body_path(0), Some(s.body_path(1)), "PUT")];
        let c = s.config(&blocks);
        let file = format!("{}", s.body_path(0).display()).replace('\\', "\\\\");
        assert!(c.contains(&format!("output = \"{file}\"\n")), "{c}");
        assert!(c.contains("output = \"D:\\\\a\\\\_temp\\\\test-cache\\\\scratch"), "{c}");
        assert!(c.contains("upload-file = \"D:\\\\a\\\\_temp"), "{c}");
        // One writer for every curl config of the tool (curl_config): the lines are exactly its lines.
        assert!(
            c.contains(&curl_config::line("output", &s.body_path(0).display().to_string())),
            "{c}"
        );
        assert!(c.contains(&curl_config::line("url", &s.url("v2/a/b.json"))), "{c}");
    }
}
