//! `ci test-cache probe --shared`: a fixed set of scenarios against the shared store, to test and debug it from any runner (user, 2026-11-28,
//! after run 9350 lost every record of a Windows leg to one timed-out PUT). Each scenario prints one line: how many requests, p50, p90 and max
//! in milliseconds, the HTTP statuses, the curl exits, and how many took longer than each production limit. The requests run with a generous limit
//! of their own ([`PROBE_REQUEST_SECS`]) so a slow answer is measured, not cut off. Every object goes under the probe's own scratch prefix
//! (`v<schema>/selfcheck/<token>/`) and is deleted at the end, whatever happened (ER-45); a key left behind is reported.

use super::key::{RunnerClass, SCHEMA};
use super::s3::{Block, PARALLEL, REQUEST_SECS, S3Store, WRITE_PARALLEL, WRITE_REQUEST_SECS};
use super::store::Record;
use std::path::PathBuf;

/// The limit of one probe request: wide, so the scenario measures the answer instead of cutting it off.
const PROBE_REQUEST_SECS: u64 = 120;
/// The limit of one probe call (all requests of a scenario).
const PROBE_CALL_SECS: u64 = 900;
/// What curl prints per transfer: status, the output file, seconds, curl's exit code.
const TIMED_OUT: &str = "%{http_code}|%{redirect_url}|%{filename_effective}|%{time_total}|%{exitcode}";

/// One answered request.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Req {
    pub code: u16,
    pub ms: u64,
    pub exit: i32,
}

/// Reads the per-transfer lines of [`TIMED_OUT`]; `Err` names the line it could not read.
pub(super) fn parse_timed(text: &str) -> Result<Vec<(String, Req)>, String> {
    text.lines()
        .map(|l| {
            let p: Vec<&str> = l.split('|').collect();
            let [code, _, file, secs, exit] = p[..] else {
                return Err(format!("curl said {l:?}"));
            };
            let secs: f64 = secs.parse().map_err(|_| format!("curl said {l:?}"))?;
            Ok((
                file.to_string(),
                Req {
                    code: code.parse().unwrap_or(0),
                    ms: (secs * 1000.0).round() as u64,
                    exit: exit.trim().parse().unwrap_or(-1),
                },
            ))
        })
        .collect()
}

/// The p50, p90 and max of `ms` (nearest rank).
pub(super) fn percentiles(ms: &[u64]) -> (u64, u64, u64) {
    let mut v = ms.to_vec();
    v.sort_unstable();
    let at = |q: f64| {
        v.get((((v.len() as f64) * q).ceil() as usize).saturating_sub(1))
            .copied()
            .unwrap_or(0)
    };
    (at(0.5), at(0.9), v.last().copied().unwrap_or(0))
}

/// The scenario's one line.
pub(super) fn line(name: &str, reqs: &[Req], note: &str) -> String {
    let (p50, p90, max) = percentiles(&reqs.iter().map(|r| r.ms).collect::<Vec<_>>());
    let count = |f: &dyn Fn(&Req) -> String| {
        let mut m: Vec<(String, usize)> = Vec::new();
        for r in reqs {
            let k = f(r);
            match m.iter_mut().find(|(x, _)| *x == k) {
                Some((_, n)) => *n += 1,
                None => m.push((k, 1)),
            }
        }
        m.sort();
        m.iter().map(|(k, n)| format!("{k} x{n}")).collect::<Vec<_>>().join(", ")
    };
    let over = |secs: u64| reqs.iter().filter(|r| r.ms > secs * 1000).count();
    format!(
        "probe {name}: n={} p50 {p50} ms, p90 {p90} ms, max {max} ms; status {}; curl exit {}; over {REQUEST_SECS} s: {}, over {WRITE_REQUEST_SECS} s: {}{note}",
        reqs.len(),
        count(&|r| r.code.to_string()),
        count(&|r| r.exit.to_string()),
        over(REQUEST_SECS),
        over(WRITE_REQUEST_SECS),
    )
}

/// Percent-encodes what a key may not hold raw in a URL path (a space, a backslash, anything not unreserved except `/`).
pub(super) fn encode_key(key: &str) -> String {
    key.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

struct Probe<'a> {
    s: &'a S3Store,
    prefix: String,
    /// Every key put, for the cleanup.
    keys: Vec<String>,
    lines: Vec<String>,
    seq: usize,
}

impl Probe<'_> {
    fn key(&mut self, name: &str) -> String {
        let k = format!("{}{name}", self.prefix);
        self.keys.push(k.clone());
        k
    }

    fn file(&mut self, dir: &std::path::Path, tag: &str) -> PathBuf {
        self.seq += 1;
        dir.join(format!("{}-{tag}-{}", std::process::id(), self.seq))
    }

    /// Runs the blocks in one curl at `parallel`; one answer per block, in order.
    fn timed(&self, blocks: &[Block], parallel: usize) -> Result<Vec<Req>, String> {
        let (_, stdout, stderr) = self
            .s
            .curl(blocks, parallel, PROBE_CALL_SECS, TIMED_OUT, Some(PROBE_REQUEST_SECS))
            .map_err(|e| e.0)?;
        let got = parse_timed(&stdout).map_err(|e| format!("{e}; curl said: {}", stderr.trim().chars().take(200).collect::<String>()))?;
        blocks
            .iter()
            .map(|b| {
                let f = b.1.display().to_string();
                got.iter().find(|(g, _)| *g == f).map(|(_, r)| r.clone()).ok_or_else(|| {
                    format!(
                        "curl gave no line for a request; {}",
                        stderr.trim().chars().take(200).collect::<String>()
                    )
                })
            })
            .collect()
    }

    fn say(&mut self, name: &str, r: Result<Vec<Req>, String>, note: &str) -> Option<Vec<Req>> {
        match r {
            Ok(reqs) => {
                self.lines.push(line(name, &reqs, note));
                Some(reqs)
            }
            Err(e) => {
                self.lines.push(format!("probe {name}: no answer: {e}"));
                None
            }
        }
    }

    fn puts(&mut self, name: &str, n: usize, parallel: usize, body: &str) {
        let dir = self.s.scratch.clone();
        let mut blocks = Vec::new();
        for i in 0..n {
            let up = self.file(&dir, "up");
            if std::fs::write(&up, body).is_err() {
                self.lines.push(format!("probe {name}: cannot write {}", up.display()));
                return;
            }
            let key = self.key(&format!("{}-{i}.json", name.replace(' ', "-")));
            blocks.push((self.s.url(&key), self.file(&dir, "out"), Some(up), "PUT"));
        }
        let r = self.timed(&blocks, parallel);
        for b in &blocks {
            let _ = std::fs::remove_file(&b.1);
            if let Some(up) = &b.2 {
                let _ = std::fs::remove_file(up);
            }
        }
        self.say(name, r, &format!("; parallel {parallel}, body {} bytes", body.len()));
    }

    fn gets(&mut self, name: &str, keys: &[String], parallel: usize) {
        let dir = self.s.scratch.clone();
        let blocks: Vec<Block> = keys.iter().map(|k| (self.s.url(k), self.file(&dir, "get"), None, "GET")).collect();
        let r = self.timed(&blocks, parallel);
        for b in &blocks {
            let _ = std::fs::remove_file(&b.1);
        }
        self.say(name, r, &format!("; parallel {parallel}"));
    }

    /// Deletes every key put, with the wide limit, and lists the prefix: the keys it still names.
    fn clean(&mut self) -> Vec<String> {
        let dir = self.s.scratch.clone();
        let keys = std::mem::take(&mut self.keys);
        let blocks: Vec<Block> = keys
            .iter()
            .map(|k| (self.s.url(&encode_key(k)), self.file(&dir, "del"), None, "DELETE"))
            .collect();
        let r = self.timed(&blocks, WRITE_PARALLEL);
        for b in &blocks {
            let _ = std::fs::remove_file(&b.1);
        }
        self.say("cleanup deletes", r, "");
        match self.s.list(&self.prefix) {
            Ok(left) => left,
            Err(e) => vec![format!("(the listing failed: {e})")],
        }
    }
}

/// A record of the real shape and size (a job name, a ref, a commit, an OS build).
fn real_record() -> Record {
    Record {
        key: "k".repeat(64),
        target: "rust:crates/paddock-example-crate".into(),
        class: RunnerClass("Windows-X64".into()),
        filter: "package(=paddock-example-crate) | package(=paddock-another)".into(),
        run: "9350".into(),
        job: "crate (windows-latest)".into(),
        attempt: 1,
        git_ref: "refs/heads/train/2026-11-28-ab".into(),
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        tests: 412,
        secs: 38.5,
        at: 1_800_000_000,
        os_build: "10.0.26100".into(),
    }
}

/// Runs every scenario. Returns the lines and the keys left behind (empty when clean).
pub fn run(s: &S3Store, token: &str) -> (Vec<String>, Vec<String>) {
    let mut p = Probe {
        s,
        prefix: format!("v{SCHEMA}/selfcheck/{token}/"),
        keys: Vec::new(),
        lines: Vec::new(),
        seq: 0,
    };
    if let Err(e) = std::fs::create_dir_all(&s.scratch) {
        p.lines.push(format!("probe: {} cannot be made: {e}", s.scratch.display()));
        return (p.lines, Vec::new());
    }
    let real = serde_json::to_string(&real_record().json()).unwrap_or_default();
    let small = format!("{{\"probe\":\"{token}\"}}");
    let dir = s.scratch.clone();

    // (1) one GET of a missing key.
    let missing = format!("{}missing.json", p.prefix);
    p.gets("1 get missing", std::slice::from_ref(&missing), 1);

    // (2) one PUT, GET and DELETE of a small record, as selfcheck does.
    let k = p.key("small.json");
    let (out_put, up) = (p.file(&dir, "out"), p.file(&dir, "up"));
    if std::fs::write(&up, &small).is_ok() {
        let put = p.timed(&[(s.url(&k), out_put.clone(), Some(up.clone()), "PUT")], 1);
        p.say("2 put small", put, &format!("; body {} bytes", small.len()));
        p.gets("2 get small", std::slice::from_ref(&k), 1);
        let out_del = p.file(&dir, "out");
        let del = p.timed(&[(s.url(&k), out_del.clone(), None, "DELETE")], 1);
        p.say("2 delete small", del, "");
        for f in [&out_put, &up, &out_del] {
            let _ = std::fs::remove_file(f);
        }
    }

    // (3) one PUT of a record of the real size.
    p.puts("3 put real", 1, 1, &real);

    // (4) N PUTs in parallel; the last two are the record step's count at the old and at the production parallelism.
    p.puts("4 put x4", 4, 4, &real);
    p.puts("4 put x16", 16, 16, &real);
    p.puts("4 put x50 at 16", 50, 16, &real);
    p.puts(&format!("4 put x50 at {WRITE_PARALLEL}"), 50, WRITE_PARALLEL, &real);

    // (5) N GETs in parallel of keys that are there (the 4 and 16 PUT scenarios' keys), as a lookup does.
    for (n, name) in [(4, "x4"), (16, "x16")] {
        let keys: Vec<String> = p.keys.iter().filter(|k| k.contains(&format!("4-put-{name}-"))).cloned().collect();
        let keys: Vec<String> = keys.into_iter().take(n).collect();
        p.gets(&format!("5 get {name}"), &keys, PARALLEL);
    }
    let keys: Vec<String> = p.keys.iter().filter(|k| k.contains("x50-at-16-")).cloned().collect();
    p.gets("5 get x50", &keys, PARALLEL);

    // (6) a key and a scratch path with a space and a backslash (the Windows escaping of the curl config).
    let odd = s.scratch.join("sp ace").join("back\\slash");
    if std::fs::create_dir_all(&odd).is_ok() {
        let key = p.key("esc ape\\back.json");
        let url = s.url(&encode_key(&key));
        let (out_put, up, out_get, out_del) = (p.file(&odd, "out"), p.file(&odd, "up"), p.file(&odd, "got"), p.file(&odd, "del"));
        let ok = std::fs::write(&up, &small).is_ok();
        let note = format!("; path {}", odd.display());
        if ok {
            let put = p.timed(&[(url.clone(), out_put.clone(), Some(up.clone()), "PUT")], 1);
            p.say("6 put odd name", put, &note);
            let get = p.timed(&[(url.clone(), out_get.clone(), None, "GET")], 1);
            let same = std::fs::read_to_string(&out_get).map(|t| t == small).unwrap_or(false);
            p.say(
                "6 get odd name",
                get,
                &format!("; body back {}", if same { "identical" } else { "DIFFERENT" }),
            );
            let del = p.timed(&[(url, out_del.clone(), None, "DELETE")], 1);
            p.say("6 delete odd name", del, "");
        } else {
            p.lines.push(format!("probe 6: cannot write {}", up.display()));
        }
        let _ = crate::safe_remove::remove(&s.scratch.join("sp ace"), std::slice::from_ref(&s.scratch));
    }

    // (7) the same as (4), one at a time.
    p.puts("7 put x4 sequential", 4, 1, &real);
    p.puts("7 put x16 sequential", 16, 1, &real);
    p.puts("7 put x50 sequential", 50, 1, &real);

    let left = p.clean();
    (p.lines, left)
}
