// ci-runs-reader: tests of the escape monitor: a fixture of the runs list answer for pick_green, no read of any run
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::register::{self, Register, Row, Table};
use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default)]
struct Mem(RefCell<BTreeMap<String, Load>>);

impl Sink for Mem {
    fn put(&self, r: &Record) -> Result<(), String> {
        self.0.borrow_mut().insert(r.commit.clone(), Load::Ok(Box::new(r.clone())));
        Ok(())
    }
}
impl Store for Mem {
    fn get(&self, c: &str) -> Load {
        self.0.borrow().get(c).cloned().unwrap_or(Load::Missing)
    }
}

fn c(n: usize) -> String {
    format!("{n:040x}")
}

/// A record for commit `n`: the change reaches `ui` (with a reason); `selected` says whether the plan ran it.
fn rec(n: usize, selected: bool) -> Record {
    let mut r = Record {
        commit: c(n),
        base: Some(c(n.saturating_sub(1))),
        at: 1_000_000,
        reach: BTreeMap::from([("ui".to_string(), vec!["npm ui: a dependency changed".to_string()])]),
        selected: BTreeMap::new(),
    };
    if selected {
        r.selected.insert(
            "ui".into(),
            Sel {
                os: vec!["ubuntu-latest".into()],
                runs: "all".into(),
                targets: None,
            },
        );
    }
    r
}

fn fail_ui() -> Failure {
    Failure {
        project: "ui".into(),
        os: "ubuntu-latest".into(),
        target: "vitest:ui/a.spec.ts".into(),
    }
}

struct Culprit(usize, usize);
impl Runner for Culprit {
    fn fails_at(&mut self, commit: &str, _: &Failure) -> Result<bool, String> {
        self.1 += 1;
        Ok(usize::from_str_radix(commit, 16).unwrap() >= self.0)
    }
}

fn store_of(rs: &[Record]) -> Mem {
    let m = Mem::default();
    for r in rs {
        save(&m, r).unwrap();
    }
    m
}

#[test]
fn an_escape_is_detected_when_a_dropped_target_fails() {
    let m = store_of(&[rec(1, true), rec(2, false), rec(3, true)]);
    let j = judge(&[c(1), c(2), c(3)], &m, &[fail_ui()], &mut NoRunner);
    let e = j.escapes();
    assert_eq!(e.len(), 1, "{j:?}");
    assert_eq!(e[0].commit, c(2));
    assert!(!e[0].bisected, "one commit dropped it: no bisect");
    assert!(e[0].reason.contains("a dependency changed"));
    assert!(!j.clean());
    let lines = j.lines().join("\n");
    assert!(lines.contains("::error::escape: commit 000000000"), "{lines}");
    assert!(
        lines.contains("vitest:ui/a.spec.ts") && lines.contains("npm ui: a dependency changed"),
        "{lines}"
    );
}

#[test]
fn no_escape_when_the_target_was_selected() {
    // Positive control of the test above: the same failure with the plan having run it.
    let m = store_of(&[rec(1, true), rec(2, true), rec(3, true)]);
    let j = judge(&[c(1), c(2), c(3)], &m, &[fail_ui()], &mut NoRunner);
    assert!(j.escapes().is_empty(), "{j:?}");
    assert_eq!(j.outcomes, vec![Outcome::Covered(fail_ui())]);
    assert!(j.clean(), "a red nightly whose targets all ran is no escape");
}

#[test]
fn a_commit_whose_change_could_not_reach_the_project_is_no_escape() {
    let mut r = rec(2, false);
    r.reach.clear();
    let m = store_of(&[rec(1, true), r]);
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut NoRunner);
    assert!(j.escapes().is_empty() && j.clean(), "{j:?}");
}

#[test]
fn a_selection_on_another_os_does_not_cover_the_failure() {
    let mut r = rec(1, true);
    r.selected.get_mut("ui").unwrap().os = vec!["windows-latest".into()];
    let m = store_of(&[r]);
    assert_eq!(judge(&[c(1)], &m, &[fail_ui()], &mut NoRunner).escapes().len(), 1);
}

#[test]
fn labels_cover_what_they_run() {
    let s = |runs: &str, targets: Option<Vec<&str>>| Sel {
        os: vec!["l".into()],
        runs: runs.into(),
        targets: targets.map(|t| t.into_iter().map(String::from).collect()),
    };
    assert!(s("rust", None).covers("l", "nextest:crates"));
    assert!(!s("rust", None).covers("l", "vitest:x"));
    assert!(s("ui", None).covers("l", "vitest:x") && !s("ui", None).covers("l", "nextest:x"));
    // `tests` and `smoke` run no enumerable target: only the named ones count.
    assert!(!s("smoke", None).covers("l", "vitest:x"));
    assert!(s("tests", Some(vec!["vitest:x"])).covers("l", "vitest:x"));
    assert!(!s("tests", Some(vec!["vitest:x"])).covers("l", "vitest:y"));
}

#[test]
fn several_dropping_commits_are_bisected_to_the_culprit() {
    let rs: Vec<Record> = (1..=9).map(|n| rec(n, n % 3 == 0)).collect(); // 1,2,4,5,7,8 dropped it
    let commits: Vec<String> = (1..=9).map(c).collect();
    let m = store_of(&rs);
    let mut runner = Culprit(5, 0);
    let j = judge(&commits, &m, &[fail_ui()], &mut runner);
    let e = j.escapes();
    assert_eq!(e.len(), 1, "{j:?}");
    assert_eq!(e[0].commit, c(5), "the first dropping commit at which the target fails");
    assert!(e[0].bisected);
    assert!(
        runner.1 <= 3 && j.runs_used == runner.1,
        "binary search, not one run per commit: {} runs",
        runner.1
    );
}

#[test]
fn the_bisect_stops_at_the_run_budget_and_says_not_measured() {
    struct Slow;
    impl Runner for Slow {
        fn fails_at(&mut self, _: &str, _: &Failure) -> Result<bool, String> {
            Ok(false)
        }
    }
    // Eleven failures, each needing a bisect of the same two dropping commits: the nightly's budget is MAX_RUNS runs in all.
    let m = store_of(&[rec(1, false), rec(2, false)]);
    let fs: Vec<Failure> = (0..MAX_RUNS + 2)
        .map(|i| Failure {
            project: "ui".into(),
            os: "ubuntu-latest".into(),
            target: format!("vitest:t{i}"),
        })
        .collect();
    let j = judge(&[c(1), c(2)], &m, &fs, &mut Slow);
    assert_eq!(j.runs_used, MAX_RUNS);
    assert_eq!(j.escapes().len(), MAX_RUNS);
    assert_eq!(j.not_measured(), 2, "the failures past the budget are not measured, never guessed");
    assert!(!j.clean());
    assert!(j.lines().join("\n").contains("more than the 10 runs"));
}

#[test]
fn more_than_the_commit_bound_is_not_measured_when_a_bisect_would_be_needed() {
    let n = MAX_COMMITS + 1;
    let rs: Vec<Record> = (1..=n).map(|i| rec(i, false)).collect();
    let commits: Vec<String> = (1..=n).map(c).collect();
    let m = store_of(&rs);
    let mut runner = Culprit(1, 0);
    let j = judge(&commits, &m, &[fail_ui()], &mut runner);
    assert_eq!(runner.1, 0, "no run past the bound");
    assert_eq!(j.not_measured(), 1);
    assert!(j.escapes().is_empty());
    assert!(j.lines().join("\n").contains("over the bisect bound of 30"));
    // Control: at the bound the same shape is bisected.
    let m = store_of(&rs[..MAX_COMMITS]);
    let j = judge(&commits[..MAX_COMMITS], &m, &[fail_ui()], &mut Culprit(7, 0));
    assert_eq!(j.escapes().len(), 1);
    assert_eq!(j.escapes()[0].commit, c(7));
}

#[test]
fn one_dropping_commit_needs_no_bisect() {
    let m = store_of(&[rec(1, true), rec(2, false)]);
    let mut runner = Culprit(0, 0);
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut runner);
    assert_eq!((runner.1, j.runs_used), (0, 0));
    assert_eq!(j.escapes().len(), 1);
}

#[test]
fn a_runner_that_cannot_run_is_not_measured() {
    let m = store_of(&[rec(1, false), rec(2, false)]);
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut NoRunner);
    assert_eq!(j.not_measured(), 1);
    assert!(j.escapes().is_empty() && !j.clean());
}

#[test]
fn missing_and_unreadable_records_are_not_measured_never_clean() {
    let m = store_of(&[rec(1, true)]);
    m.0.borrow_mut().insert(c(2), Load::Unreadable("version 9 is not 1".into()));
    // No failures at all: still not clean, because the commits could not be judged.
    let j = judge(&[c(1), c(2), c(3)], &m, &[], &mut NoRunner);
    assert_eq!(j.unreadable.len(), 2, "{j:?}");
    assert!(!j.clean());
    let lines = j.lines().join("\n");
    assert!(
        lines.contains("unreadable: version 9") && lines.contains("no record was saved"),
        "{lines}"
    );
    // A failure with an unreadable record is not measured either.
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut NoRunner);
    assert_eq!(j.not_measured(), 1);
    // Control: every record readable and no failure is clean.
    let ok = store_of(&[rec(1, true), rec(2, true)]);
    assert!(judge(&[c(1), c(2)], &ok, &[], &mut NoRunner).clean());
}

#[test]
fn the_chain_follows_bases_to_the_previous_nightly_and_ends_at_a_missing_record() {
    let m = store_of(&[rec(5, true), rec(8, true)]);
    // 8's base is 7 in `rec`; make the ranges real: 8 covers 5..8, 5 covers 2..5.
    let (mut a, mut b) = (rec(8, true), rec(5, true));
    a.base = Some(c(5));
    b.base = Some(c(2));
    let m2 = store_of(&[a, b]);
    let reached = |x: &str| x == c(2);
    assert_eq!(chain(&m2, &c(8), &reached), vec![c(5), c(8)]);
    // A missing record in the middle ends the chain at that commit, which judge reports.
    let m3 = store_of(&[{
        let mut r = rec(8, true);
        r.base = Some(c(5));
        r
    }]);
    let ch = chain(&m3, &c(8), &reached);
    assert_eq!(ch, vec![c(5), c(8)]);
    assert!(!judge(&ch, &m3, &[], &mut NoRunner).clean());
    // A record with no base ran everything and ends the chain.
    let mut full = rec(8, true);
    full.base = None;
    assert_eq!(chain(&store_of(&[full]), &c(8), &reached), vec![c(8)]);
    // A cycle never loops.
    let mut x = rec(8, true);
    x.base = Some(c(8));
    assert_eq!(chain(&store_of(&[x]), &c(8), &reached), vec![c(8)]);
    let _ = m;
}

#[test]
fn a_record_round_trips_and_a_bad_one_is_refused_with_why() {
    let r = rec(3, false);
    assert_eq!(Record::parse(&serde_json::to_string(&r.to_json()).unwrap()).unwrap(), r);
    assert!(Record::parse("{").unwrap_err().contains("not JSON"));
    assert!(Record::parse(r#"{"version":2}"#).unwrap_err().contains("version"));
    let mut v = r.to_json();
    v["commit"] = "../x".into();
    assert!(Record::parse(&v.to_string()).unwrap_err().contains("commit"));
    assert!(
        save(
            &Mem::default(),
            &Record {
                commit: "--force".into(),
                ..r
            }
        )
        .is_err()
    );
}

// ---- the artifact store (ADR 0117 amendment D13, 2026-11-27) ----

use super::artifact::{ARTIFACT_NAME, ArtifactStore, FileSink, RECORD_FILE, unzip_one};
use crate::ci_runs::client::Client;
use crate::wait::Http;
use serde_json::{Value, json};
use std::collections::HashMap;

const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";

/// A scripted Actions API: JSON and zip bytes by URL, everything else is a 404.
#[derive(Default)]
struct Api {
    json: HashMap<String, Value>,
    zips: HashMap<String, Vec<u8>>,
}

impl Http for Api {
    fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
        self.json
            .get(url)
            .cloned()
            .ok_or_else(|| "curl exited with 22: error: 404".to_string())
    }
    fn get_bytes(&mut self, url: &str, _token: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        let z = self.zips.get(url).ok_or_else(|| "curl exited with 22: error: 404".to_string())?;
        if z.len() as u64 > max_bytes {
            return Err("curl exited with 63: maximum file size exceeded".into());
        }
        Ok(z.clone())
    }
}

/// A zip with one entry as a server's download holds it: stored (method 0) or deflated (8).
fn zip_of(name: &str, data: &[u8], deflate: bool) -> Vec<u8> {
    let (method, body) = if deflate {
        (8u16, miniz_oxide::deflate::compress_to_vec(data, 6))
    } else {
        (0u16, data.to_vec())
    };
    let mut z = Vec::new();
    let local = z.len() as u32;
    z.extend(0x0403_4b50u32.to_le_bytes());
    z.extend([20, 0, 0, 0]);
    z.extend(method.to_le_bytes());
    z.extend([0u8; 4]); // time, date
    z.extend(0u32.to_le_bytes()); // crc (not checked)
    z.extend((body.len() as u32).to_le_bytes());
    z.extend((data.len() as u32).to_le_bytes());
    z.extend((name.len() as u16).to_le_bytes());
    z.extend(0u16.to_le_bytes());
    z.extend(name.as_bytes());
    z.extend(&body);
    let cd = z.len() as u32;
    z.extend(0x0201_4b50u32.to_le_bytes());
    z.extend([20, 0, 20, 0, 0, 0]);
    z.extend(method.to_le_bytes());
    z.extend([0u8; 4]);
    z.extend(0u32.to_le_bytes());
    z.extend((body.len() as u32).to_le_bytes());
    z.extend((data.len() as u32).to_le_bytes());
    z.extend((name.len() as u16).to_le_bytes());
    z.extend([0u8; 8]); // extra, comment, disk, internal attributes
    z.extend(0u32.to_le_bytes()); // external attributes
    z.extend(local.to_le_bytes());
    z.extend(name.as_bytes());
    let cd_len = z.len() as u32 - cd;
    z.extend(0x0605_4b50u32.to_le_bytes());
    z.extend([0u8; 4]);
    z.extend(1u16.to_le_bytes());
    z.extend(1u16.to_le_bytes());
    z.extend(cd_len.to_le_bytes());
    z.extend(cd.to_le_bytes());
    z.extend(0u16.to_le_bytes());
    z
}

/// A run of ci.yml for `sha` in the list the API answers for `head_sha=<sha>`.
fn run_json(id: u64, sha: &str, workflow: &str) -> Value {
    json!({"id": id, "path": format!("{workflow}@refs/heads/main"), "event": "push", "status": "completed",
           "conclusion": "success", "head_sha": sha, "head_branch": "main", "created_at": "2026-11-27T01:00:00Z",
           "run_attempt": 1, "display_title": "t"})
}

fn serve_runs(api: &mut Api, sha: &str, runs: Vec<Value>) {
    api.json.insert(
        format!("{API}/runs?head_sha={sha}&limit=50&page=1"),
        json!({"workflow_runs": runs, "total_count": 1}),
    );
}

fn serve_artifacts(api: &mut Api, run: u64, arts: Vec<Value>) {
    api.json.insert(
        format!("{API}/runs/{run}/artifacts?name={ARTIFACT_NAME}"),
        json!({"artifacts": arts, "total_count": 1}),
    );
}

fn artifact(id: u64, expired: bool) -> Value {
    json!({"id": id, "name": ARTIFACT_NAME, "expired": expired})
}

fn store(api: &mut Api) -> ArtifactStore<'_> {
    let mut client = Client::new(api, "https://gitea.invalid", "decryp7/paddock", "tok");
    client.backoff_ms = 0;
    ArtifactStore::new(client)
}

fn record_zip(r: &Record) -> Vec<u8> {
    zip_of(RECORD_FILE, serde_json::to_string(&r.to_json()).unwrap().as_bytes(), true)
}

#[test]
fn a_record_round_trips_through_the_artifact_store() {
    // The writer: the plan job's file, exactly what the upload step sends.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plan-record").join(RECORD_FILE);
    save(&FileSink { path: file.clone() }, &rec(4, true)).unwrap();
    let written = std::fs::read(&file).unwrap();
    // The reader: the nightly, through the run list and the artifact API.
    let mut api = Api::default();
    serve_runs(&mut api, &c(4), vec![run_json(7, &c(4), "ci.yml")]);
    serve_artifacts(&mut api, 7, vec![artifact(9, false)]);
    api.zips
        .insert(format!("{API}/artifacts/9/zip"), zip_of(RECORD_FILE, &written, true));
    let s = store(&mut api);
    assert_eq!(s.get(&c(4)), Load::Ok(Box::new(rec(4, true))));
    // And judge reads it through the same seam.
    let j = judge(&[c(4)], &s, &[fail_ui()], &mut NoRunner);
    assert_eq!(j.outcomes, vec![Outcome::Covered(fail_ui())], "{j:?}");
}

#[test]
fn a_commit_with_no_artifact_is_not_measured_never_clean() {
    let mut api = Api::default();
    // No run of ci.yml at all (another workflow's run does not count).
    serve_runs(&mut api, &c(1), vec![run_json(5, &c(1), "nightly.yml")]);
    // A run without the artifact (the plan job did not finish).
    serve_runs(&mut api, &c(2), vec![run_json(6, &c(2), "ci.yml")]);
    serve_artifacts(&mut api, 6, vec![]);
    // An artifact the server has expired (retention).
    serve_runs(&mut api, &c(3), vec![run_json(7, &c(3), "ci.yml")]);
    serve_artifacts(&mut api, 7, vec![artifact(9, true)]);
    // c(4): the run list itself is not served (a 404).
    let s = store(&mut api);
    for n in 1..=4 {
        assert_eq!(s.get(&c(n)), Load::Missing, "commit {n}");
    }
    let j = judge(&[c(1), c(2)], &s, &[], &mut NoRunner);
    assert_eq!(j.unreadable.len(), 2, "{j:?}");
    assert!(!j.clean(), "no failures, and still not clean: the commits could not be judged");
    assert!(j.lines().join("\n").contains("no record was saved"));
    // With a failure the answer is not measured, never covered and never an escape.
    let j = judge(&[c(2)], &s, &[fail_ui()], &mut NoRunner);
    assert!(j.escapes().is_empty() && !j.clean(), "{j:?}");
}

#[test]
fn an_unreadable_artifact_is_unreadable_with_why_never_a_record() {
    let mut api = Api::default();
    for (n, zip) in [
        (1, zip_of(RECORD_FILE, b"not a record", false)),
        (2, record_zip(&rec(9, true))),          // a record for another commit
        (3, b"PK not really a zip".to_vec()),    // a damaged download
        (4, zip_of("other.json", b"{}", false)), // the file is not in it
    ] {
        serve_runs(&mut api, &c(n), vec![run_json(10 + n as u64, &c(n), "ci.yml")]);
        serve_artifacts(&mut api, 10 + n as u64, vec![artifact(20 + n as u64, false)]);
        api.zips.insert(format!("{API}/artifacts/{}/zip", 20 + n), zip);
    }
    let s = store(&mut api);
    for n in 1..=4 {
        assert!(matches!(s.get(&c(n)), Load::Unreadable(_)), "commit {n}: {:?}", s.get(&c(n)));
    }
    assert!(matches!(s.get(&c(2)), Load::Unreadable(w) if w.contains("is for")));
    // A bad commit id never reaches a URL.
    assert!(matches!(s.get("../x"), Load::Unreadable(_)));
}

#[test]
fn the_newest_run_with_a_readable_record_wins_and_a_bad_one_does_not_hide_it() {
    let mut api = Api::default();
    serve_runs(&mut api, &c(1), vec![run_json(8, &c(1), "ci.yml"), run_json(7, &c(1), "ci.yml")]);
    serve_artifacts(&mut api, 8, vec![artifact(18, false)]);
    serve_artifacts(&mut api, 7, vec![artifact(17, false)]);
    api.zips.insert(format!("{API}/artifacts/18/zip"), b"junk".to_vec());
    api.zips.insert(format!("{API}/artifacts/17/zip"), record_zip(&rec(1, true)));
    assert_eq!(store(&mut api).get(&c(1)), Load::Ok(Box::new(rec(1, true))));
}

#[test]
fn a_zip_entry_is_read_stored_or_deflated_and_bounded() {
    let data = b"{\"a\":1}".repeat(50);
    for deflate in [false, true] {
        assert_eq!(
            unzip_one(&zip_of("record.json", &data, deflate), "record.json", 1 << 20).unwrap(),
            data
        );
    }
    assert!(
        unzip_one(&zip_of("record.json", &data, true), "record.json", 10)
            .unwrap_err()
            .contains("larger")
    );
    assert!(
        unzip_one(&zip_of("x.json", &data, false), "record.json", 1 << 20)
            .unwrap_err()
            .contains("record.json")
    );
    assert!(unzip_one(b"", "record.json", 1 << 20).is_err());
    let z = zip_of("record.json", &data, false);
    assert!(unzip_one(&z[..z.len() - 5], "record.json", 1 << 20).is_err(), "a truncated zip");
}

// ---- ci.yml writes nothing to the repository ----

use paddock_ci_config::yaml::{self, Node};

/// What in a workflow gives write access to the repository or pushes a ref: a `contents: write` (or `write-all`) permission at any
/// level, and a step that runs `git push` or `update-ref` (read through the YAML model, not by a text match, ER-42).
fn repository_writes(doc: &Node) -> Vec<String> {
    let mut out = Vec::new();
    let mut perms: Vec<(String, &Node)> = Vec::new();
    if let Some(p) = doc.get("permissions") {
        perms.push(("the workflow".into(), p));
    }
    for (id, job) in doc.get("jobs").map(Node::entries).unwrap_or_default() {
        if let Some(p) = job.get("permissions") {
            perms.push((format!("job {id}"), p));
        }
        for step in job.get("steps").map(Node::items).unwrap_or_default() {
            let run = step.get_text("run").unwrap_or("");
            if run.contains("git push") || run.contains("update-ref") || run.contains("refs/paddock/plan-records") {
                out.push(format!("job {id} pushes a ref (line {})", step.line));
            }
        }
    }
    for (who, p) in perms {
        match p.text() {
            Some("write-all") => out.push(format!("{who} has write-all")),
            Some(_) => {}
            None => {
                if p.get_text("contents") == Some("write") {
                    out.push(format!("{who} has contents: write"));
                }
            }
        }
    }
    out
}

fn ci_yml() -> Node {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitea/workflows/ci.yml");
    yaml::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn ci_yml_has_no_contents_write_and_pushes_no_ref() {
    let doc = ci_yml();
    assert_eq!(repository_writes(&doc), Vec::<String>::new());
    assert_eq!(doc.get("permissions").and_then(|p| p.get_text("contents")), Some("read"));
    // Positive controls: the check sees each way of writing.
    for (bad, what) in [
        ("permissions:\n  contents: write\njobs: {}\n", "contents: write"),
        ("permissions: write-all\njobs: {}\n", "write-all"),
        (
            "jobs:\n  a:\n    permissions:\n      contents: write\n    steps: []\n",
            "job a has contents: write",
        ),
        ("jobs:\n  a:\n    steps:\n      - run: git push origin HEAD\n", "pushes a ref"),
        ("jobs:\n  a:\n    steps:\n      - run: git update-ref refs/x HEAD\n", "pushes a ref"),
    ] {
        let found = repository_writes(&yaml::parse(bad).unwrap()).join("; ");
        assert!(found.contains(what), "{bad:?} gave {found:?}");
    }
}

#[test]
fn the_plan_job_uploads_the_record_as_the_runs_artifact_for_the_retention() {
    let doc = ci_yml();
    let plan = doc.get("jobs").and_then(|j| j.get("plan")).expect("the plan job");
    let uploads: Vec<&Node> = plan
        .get("steps")
        .map(Node::items)
        .unwrap_or_default()
        .iter()
        .filter(|s| s.get_text("uses").is_some_and(|u| u.starts_with("actions/upload-artifact@")))
        .collect();
    assert_eq!(uploads.len(), 1, "the plan job has one upload step");
    let with = uploads[0].get("with").expect("with");
    assert_eq!(with.get_text("name"), Some(ARTIFACT_NAME));
    assert_eq!(with.get_text("retention-days"), Some(RETENTION_DAYS.to_string().as_str()));
    // The pin is the one the other jobs use (v3: v4 aborts on Gitea; ER-26).
    let pins: std::collections::BTreeSet<&str> = doc
        .get("jobs")
        .map(Node::entries)
        .unwrap_or_default()
        .iter()
        .flat_map(|(_, j)| j.get("steps").map(Node::items).unwrap_or_default())
        .filter_map(|s| s.get_text("uses"))
        .filter(|u| u.starts_with("actions/upload-artifact@"))
        .collect();
    assert_eq!(pins.len(), 1, "one pin of upload-artifact: {pins:?}");
    // The file it uploads is the one the plan step writes.
    let path = with.get_text("path").unwrap_or("");
    let plan_step_env = plan
        .get("steps")
        .map(Node::items)
        .unwrap_or_default()
        .iter()
        .find(|s| s.get_text("id") == Some("plan"))
        .and_then(|s| s.get("env"))
        .and_then(|e| e.get_text("PLAN_RECORD_FILE"))
        .expect("the plan step names PLAN_RECORD_FILE");
    assert!(plan_step_env.ends_with(&format!("/plan-record/{RECORD_FILE}")), "{plan_step_env}");
    assert!(path.contains("plan-record"), "{path}");
}

fn row(run: u64, escapes: u64, nm: u64, un: u64) -> Row {
    Row {
        run,
        date: "2026-11-24".into(),
        head: c(1),
        commits: 3,
        escapes,
        not_measured: nm,
        unreadable: un,
    }
}

fn reg(rows: Vec<Row>) -> Register {
    Register {
        required: register::REQUIRED,
        tables: vec![Table {
            file: "plan_gitea.rs".into(),
            contains: None,
        }],
        nightly: rows,
    }
}

#[test]
fn five_consecutive_clean_nightlies_free_the_tables_and_not_four() {
    let four: Vec<Row> = (1..=4).map(|i| row(i, 0, 0, 0)).collect();
    let gone = |_: &Table| false;
    let e = register::gate(&reg(four.clone()), &gone).unwrap_err();
    assert!(e.contains("only 4 of 5") && e.contains("plan_gitea.rs"), "{e}");
    let mut five = four;
    five.push(row(5, 0, 0, 0));
    assert!(register::gate(&reg(five), &gone).is_ok());
}

#[test]
fn an_escape_a_not_measured_or_an_unreadable_record_starts_the_count_again() {
    for bad in [row(3, 1, 0, 0), row(3, 0, 1, 0), row(3, 0, 0, 1)] {
        let mut rows: Vec<Row> = (1..=6).map(|i| row(i, 0, 0, 0)).collect();
        rows[2] = bad;
        let r = reg(rows);
        assert_eq!(register::consecutive_clean(&r), 3, "runs 4 to 6 only");
        assert!(register::gate(&r, &|_| false).is_err());
    }
}

#[test]
fn the_tables_present_pass_whatever_the_count_and_an_empty_register_is_refused() {
    assert!(register::gate(&reg(vec![]), &|_| true).is_ok());
    let mut r = reg(vec![]);
    r.tables.clear();
    assert!(register::gate(&r, &|_| true).unwrap_err().contains("guard nothing"));
}

#[test]
fn the_register_parses_and_refuses_a_lowered_count() {
    let text = "version = 1\nrequired = 5\n[[table]]\nfile = \"a.rs\"\ncontains = \"static X\"\n[[nightly]]\nrun = 1\ndate = \"2026-11-24\"\nhead = \"abc\"\ncommits = 2\nescapes = 0\nnot_measured = 0\nunreadable = 0\n";
    let r = register::parse(text).unwrap();
    assert_eq!((r.nightly.len(), r.tables[0].contains.as_deref()), (1, Some("static X")));
    let e = register::parse(&text.replace("required = 5", "required = 1")).unwrap_err();
    assert!(e.contains("below the 5"), "{e}");
    assert!(register::parse(&text.replace("escapes = 0\n", "")).unwrap_err().contains("escapes"));
}

#[test]
fn the_register_writer_appends_one_row_per_run() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("docs")).unwrap();
    std::fs::write(d.path().join(register::FILE), "version = 1\n[[table]]\nfile = \"a.rs\"\n").unwrap();
    register::append(d.path(), &row(7, 0, 0, 0)).unwrap();
    assert_eq!(register::load(d.path()).unwrap().nightly, vec![row(7, 0, 0, 0)]);
    assert!(register::append(d.path(), &row(7, 0, 0, 0)).unwrap_err().contains("already"));
    assert!(
        register::append(
            d.path(),
            &Row {
                date: "x\"".into(),
                ..row(8, 0, 0, 0)
            }
        )
        .is_err()
    );
}

#[test]
fn the_tree_gate_sees_a_deleted_table_and_its_control_a_present_one() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("docs")).unwrap();
    std::fs::write(
        d.path().join(register::FILE),
        "version = 1\n[[table]]\nfile = \"plan_gitea.rs\"\n[[table]]\nfile = \"plan.rs\"\ncontains = \"static GLOBAL\"\n",
    )
    .unwrap();
    // Both deleted: refused.
    assert!(register::check_tree(d.path()).is_err());
    // The file there but the table in it gone (GLOBAL removed): refused.
    std::fs::write(d.path().join("plan_gitea.rs"), "").unwrap();
    std::fs::write(d.path().join("plan.rs"), "// no table\n").unwrap();
    assert!(register::check_tree(d.path()).unwrap_err().contains("plan.rs"));
    // Control: all there.
    std::fs::write(d.path().join("plan.rs"), "static GLOBAL: X = 1;\n").unwrap();
    assert!(register::check_tree(d.path()).is_ok());
}

#[test]
fn a_failed_job_maps_to_its_project_and_an_unknown_one_is_unmapped() {
    use crate::plan_since::{Leg, Rerun, Scope, SincePlan};
    let job = |key: &str, leg: Leg| Rerun {
        id: 1,
        name: format!("job {key}"),
        key: Some(key.into()),
        leg,
        scope: Scope::Job("test".into()),
        vitest: vec![],
    };
    let plan = SincePlan {
        run: 1,
        workflow: "nightly.yml".into(),
        head: c(1),
        reruns: vec![
            job("ui", Leg::default()),
            job(
                "app",
                Leg {
                    os: Some("macos-14".into()),
                    app: Some("debrief".into()),
                    ..Leg::default()
                },
            ),
            job("mystery", Leg::default()),
            job("plan", Leg::default()),
        ],
        not_rerun: 0,
        everything: None,
        full_ran: false,
        expected: ["ui", "plan"]
            .iter()
            .map(|n| {
                let projects = if *n == "ui" {
                    ["ui", "eslint", "icons"].map(String::from).into()
                } else {
                    ["everything".to_string()].into()
                };
                (
                    format!("job {n}"),
                    crate::run_scope::Expected {
                        job: n.to_string(),
                        projects,
                        ..Default::default()
                    },
                )
            })
            .collect(),
    };
    let (f, un) = cmd::failures_of(&plan);
    assert!(
        f.iter()
            .any(|x| x.project == "ui" && x.os == "ubuntu-latest" && x.target == WHOLE_JOB)
    );
    assert!(f.iter().any(|x| x.project == "app:debrief" && x.os == "macos-14"));
    assert_eq!(un.len(), 1, "{un:?}");
    assert_eq!(f.len(), 2, "the plan job is no failure of a project");
}

#[test]
fn a_failures_file_is_parsed_and_a_bad_one_refused() {
    let f = cmd::read_failures(r#"[{"project":"ui","os":"ubuntu-latest"},{"project":"crates","os":"x","target":"nextest:a"}]"#).unwrap();
    assert_eq!(f[0].target, WHOLE_JOB);
    assert!(cmd::read_failures(r#"[{"os":"x"}]"#).is_err() && cmd::read_failures("{}").is_err());
}

#[test]
fn the_previous_green_nightly_is_the_newest_green_run_of_the_workflow_but_not_this_one() {
    let body = serde_json::json!({"workflow_runs": [
        {"id": 9, "run_number": 9, "path": "nightly.yml@refs/heads/main", "conclusion": "success", "head_sha": "cur"},
        {"id": 8, "run_number": 8, "path": "nightly.yml@refs/heads/main", "conclusion": "failure", "head_sha": "red"},
        {"id": 7, "run_number": 7, "path": "ci.yml@refs/heads/main", "conclusion": "success", "head_sha": "other-workflow"},
        {"id": 6, "run_number": 6, "path": "nightly.yml@refs/heads/main", "conclusion": "success", "head_sha": "green"},
    ]});
    assert_eq!(cmd::pick_green(&body, 9).as_deref(), Some("green"));
    assert_eq!(cmd::pick_green(&serde_json::json!({"workflow_runs": []}), 9), None);
}
