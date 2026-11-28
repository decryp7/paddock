// ci-runs-reader: tests of the escape monitor's CI edges: scripted Actions API answers (runs, artifacts) and the runs list for pick_green, no verdict of any run
//! The tests of the escape monitor's CI edges: the artifact store and the `ci.yml` side of the record, and the nightly command's readers.

use super::artifact::{ARTIFACT_NAME, ArtifactStore, FileSink, RECORD_FILE, unzip_one};
use crate::ci_runs::client::Client;
use crate::escape::testutil::{c, fail_ui, rec};
use crate::escape::{Load, NoRunner, Outcome, RETENTION_DAYS, Record, Store, WHOLE_JOB, judge, save};
use crate::http::Http;
use serde_json::{Value, json};
use std::collections::HashMap;
// ---- the artifact store (ADR 0117 amendment D13, 2026-11-27) ----

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
    ArtifactStore::new(Client::new(api, "https://gitea.invalid", "decryp7/paddock", "tok"))
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

/// The upload action's prefix: the Gitea fork of v4 (docs/research/2026-11-ci-artifacts-v4.md).
const UPLOAD_ACTION: &str = "ChristopherHX/gitea-upload-artifact@";

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
        .filter(|s| s.get_text("uses").is_some_and(|u| u.starts_with(UPLOAD_ACTION)))
        .collect();
    assert_eq!(uploads.len(), 1, "the plan job has one upload step");
    let with = uploads[0].get("with").expect("with");
    assert_eq!(with.get_text("name"), Some(ARTIFACT_NAME));
    assert_eq!(with.get_text("retention-days"), Some(RETENTION_DAYS.to_string().as_str()));
    // The Gitea fork of the v4 action: the REST API lists only v4 artifacts, and upstream v4 refuses Gitea. One pin in the file (ER-26).
    let pins: std::collections::BTreeSet<&str> = doc
        .get("jobs")
        .map(Node::entries)
        .unwrap_or_default()
        .iter()
        .flat_map(|(_, j)| j.get("steps").map(Node::items).unwrap_or_default())
        .filter_map(|s| s.get_text("uses"))
        .filter(|u| u.starts_with(UPLOAD_ACTION))
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
    let (f, un) = super::cmd::failures_of(&plan);
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
    let f =
        super::cmd::read_failures(r#"[{"project":"ui","os":"ubuntu-latest"},{"project":"crates","os":"x","target":"nextest:a"}]"#).unwrap();
    assert_eq!(f[0].target, WHOLE_JOB);
    assert!(super::cmd::read_failures(r#"[{"os":"x"}]"#).is_err() && super::cmd::read_failures("{}").is_err());
}

#[test]
fn the_previous_green_nightly_is_the_newest_green_run_of_the_workflow_but_not_this_one() {
    let body = serde_json::json!({"workflow_runs": [
        {"id": 9, "run_number": 9, "path": "nightly.yml@refs/heads/main", "conclusion": "success", "head_sha": "cur"},
        {"id": 8, "run_number": 8, "path": "nightly.yml@refs/heads/main", "conclusion": "failure", "head_sha": "red"},
        {"id": 7, "run_number": 7, "path": "ci.yml@refs/heads/main", "conclusion": "success", "head_sha": "other-workflow"},
        {"id": 6, "run_number": 6, "path": "nightly.yml@refs/heads/main", "conclusion": "success", "head_sha": "green"},
    ]});
    assert_eq!(super::cmd::pick_green(&body, 9).as_deref(), Some("green"));
    assert_eq!(super::cmd::pick_green(&serde_json::json!({"workflow_runs": []}), 9), None);
}
