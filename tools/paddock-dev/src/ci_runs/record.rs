// ci-runs-reader: ci record-fixture: records finished runs as test fixtures
//! `ci record-fixture <run>...`: records finished runs of the Gitea Actions API as test fixtures, scrubbed on capture.
//!
//! A recording is kept in git for good, and the API's answers carry people and machines: full names, e-mail addresses, avatar
//! links, logins and runner host names. [`scrub`] replaces them with invented values before anything is written, and conformance
//! C110 fails any fixture that still holds one. The recorder writes, per run, `run-<id>.json`, `jobs-<id>.json` (the first page of
//! jobs) and `log-<job>.txt` for each failed job (only the lines the nextest reader needs, see [`trim_log`]), and one `runs-1.json`
//! listing the recorded runs, which is the layout of [`Fake::from_dir`](super::testutil::Fake::from_dir). Read-only on the server.

use crate::log_text::plain;
use crate::wait::{CurlHttp, Http};
use serde_json::Value;
use std::collections::BTreeMap;

/// The server name every recording carries instead of the real one.
pub const PLACEHOLDER_HOST: &str = "gitea.invalid";
/// The account and owner name a recording carries.
pub const PLACEHOLDER_OWNER: &str = "example";

/// Replaces the identities in an API answer with invented values, in place. `runners` keeps one `runner-N` per real runner name
/// across the answers of one recording, so a recording still tells two runners apart.
pub fn scrub(v: &mut Value, real_host: &str, real_owner: &str, runners: &mut BTreeMap<String, usize>) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(|x| scrub(x, real_host, real_owner, runners)),
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if let Value::String(s) = x {
                    if let Some(n) = identity(k, s, runners) {
                        *s = n;
                        continue;
                    }
                    *s = rename(s, real_host, real_owner);
                } else {
                    scrub(x, real_host, real_owner, runners);
                }
            }
        }
        _ => {}
    }
}

fn rename(s: &str, real_host: &str, real_owner: &str) -> String {
    let mut out = s.to_string();
    if !real_host.is_empty() {
        out = out.replace(real_host, PLACEHOLDER_HOST);
    }
    if !real_owner.is_empty() {
        out = out.replace(real_owner, PLACEHOLDER_OWNER);
    }
    // A person's own site (the owner's website field) is not the server's name.
    if let Some(i) = out.find("://").map(|i| i + 3) {
        let rest = &out[i..];
        if rest.ends_with(".net") && !rest.contains('/') && !rest.starts_with(PLACEHOLDER_HOST) {
            out = format!("{}{PLACEHOLDER_HOST}", &out[..i]);
        }
    }
    out
}

/// The placeholder for the value of an identity key, or `None` when the key holds no identity.
fn identity(key: &str, value: &str, runners: &mut BTreeMap<String, usize>) -> Option<String> {
    Some(match key {
        // A repository's `full_name` is `owner/name`; a person's is a name.
        "full_name" if value.contains('/') => format!("{PLACEHOLDER_OWNER}/{}", value.rsplit('/').next().unwrap_or("repo")),
        "full_name" => "Example User".into(),
        "email" => "user@example.com".into(),
        "avatar_url" if value.is_empty() => String::new(),
        "avatar_url" => format!("https://{PLACEHOLDER_HOST}/avatar.png"),
        "login" | "username" => "user".into(),
        "login_name" | "location" => String::new(),
        "website" if value.is_empty() => String::new(),
        "website" => format!("https://{PLACEHOLDER_HOST}"),
        "hostname" | "runner_name" if value.is_empty() || value == "undefined" => value.to_string(),
        "hostname" | "runner_name" => {
            let next = runners.len() + 1;
            format!("runner-{}", runners.entry(value.to_string()).or_insert(next))
        }
        _ => return None,
    })
}

/// The lines of a job log that the nextest reader needs (`Job:` key, `Starting`, a test's end, `Summary`, errors), as they were.
pub fn trim_log(log: &str) -> String {
    let keep = regex::Regex::new(
        r"^\s*(Job: |Starting \d+ tests?|(FAIL|TIMEOUT|TMT|SIG\w*|FLAKY|FLKY\S*|TRY \d+ (FAIL|TIMEOUT|PASS|SIG\w*))\b|Summary \[|.*tests? (were |was )?not run|Cancelling due to|##\[error\]|::error::)",
    );
    let Ok(keep) = keep else { return String::new() };
    // Runner work folders name the runner and the disk: `/Volumes/<disk>/gitea-runners/<runner>/work/...`.
    let paths = regex::Regex::new(r"/Volumes/[^/\s]+/gitea-runners/[^/\s]+");
    let mut out = String::new();
    for l in log.lines() {
        if keep.is_match(&plain(l)) {
            match &paths {
                Ok(p) => out.push_str(&p.replace_all(l, "/Volumes/disk/gitea-runners/runner-1")),
                Err(_) => out.push_str(l),
            }
            out.push('\n');
        }
    }
    out
}

/// What a recording of runs holds, ready to write: file name and content.
pub fn record(
    http: &mut dyn Http,
    server: &str,
    repo: &str,
    token: &str,
    runs: &[u64],
    log: &mut dyn FnMut(&str),
) -> Result<Vec<(String, String)>, String> {
    let api = format!("{}/api/v1/repos/{repo}/actions", server.trim_end_matches('/'));
    let host = server
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let owner = repo.split('/').next().unwrap_or("");
    let mut runners = BTreeMap::new();
    let mut files = Vec::new();
    let mut run_objects = Vec::new();
    for id in runs {
        let mut run = http.get_json(&format!("{api}/runs/{id}"), token)?;
        let mut jobs = http.get_json(&format!("{api}/runs/{id}/jobs?limit=50&page=1"), token)?;
        let failed: Vec<u64> = jobs["jobs"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|j| !matches!(j["conclusion"].as_str(), Some("success" | "skipped")))
                    .filter_map(|j| j["id"].as_u64())
                    .collect()
            })
            .unwrap_or_default();
        for job in failed {
            match http.get_text(&format!("{api}/jobs/{job}/logs"), token) {
                Ok(t) => files.push((format!("log-{job}.txt"), trim_log(&t))),
                Err(e) => log(&format!("::warning::job {job}: its log was not recorded ({e})")),
            }
        }
        scrub(&mut run, host, owner, &mut runners);
        scrub(&mut jobs, host, owner, &mut runners);
        log(&format!("run {id}: recorded"));
        files.push((format!("jobs-{id}.json"), format!("{jobs:#}\n")));
        files.push((format!("run-{id}.json"), format!("{run:#}\n")));
        run_objects.push(run);
    }
    run_objects.sort_by_key(|r| std::cmp::Reverse(r["id"].as_u64().unwrap_or(0)));
    let list = serde_json::json!({"total_count": run_objects.len(), "workflow_runs": run_objects});
    files.push(("runs-1.json".into(), format!("{list:#}\n")));
    Ok(files)
}

/// `ci record-fixture`, against the real server.
pub fn command(a: &super::RecordArgs) -> Result<paddock_cli::Report, paddock_cli::CliError> {
    let token = super::token()?;
    let mut http = CurlHttp;
    let files = record(&mut http, &a.server, &a.repo, &token, &a.runs, &mut |l| eprintln!("{l}")).map_err(|e| {
        paddock_cli::CliError::failure(format!("the runs could not be recorded: {e}")).with_hint("Check the network and GITEA_TOKEN.")
    })?;
    std::fs::create_dir_all(&a.out).map_err(|e| paddock_cli::CliError::failure(format!("cannot create {}: {e}", a.out.display())))?;
    for (name, text) in &files {
        std::fs::write(a.out.join(name), text).map_err(|e| paddock_cli::CliError::failure(format!("cannot write {name}: {e}")))?;
    }
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    Ok(paddock_cli::Report::new(
        serde_json::json!({"out": a.out.display().to_string(), "files": names}),
        format!("{} file(s) written to {}, identities scrubbed", files.len(), a.out.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;
    use serde_json::json;

    /// The shape of the API's answer, with invented people.
    fn sample() -> Value {
        let person = |login: &str, name: &str| {
            json!({"login": login, "login_name": "", "full_name": name, "email": format!("1+{login}@noreply@real.example.net"),
                "avatar_url": "https://real.example.net/avatars/0123abcd", "website": "https://janedoe.net", "location": "Town", "html_url": format!("https://real.example.net/{login}"),
                "username": login, "last_login": "0001-01-01T00:00:00Z"})
        };
        json!({"id": 7, "path": "ci.yml@refs/heads/main", "actor": person("jdoe", "Jane Doe"), "trigger_actor": person("jdoe", "Jane Doe"),
            "repository": {"owner": person("jdoe", "Jane Doe"), "full_name": "jdoe/paddock", "name": "paddock",
                "clone_url": "https://real.example.net/jdoe/paddock.git", "avatar_url": ""},
            "jobs": [{"runner_name": "Gitea Action Runner (box-1)"}, {"runner_name": "Gitea Action Runner (box-2)"},
                {"runner_name": "Gitea Action Runner (box-1)"}, {"runner_name": ""}, {"runner_name": "undefined"}]})
    }

    #[test]
    fn scrub_removes_every_identity_and_keeps_the_structure() {
        let mut v = sample();
        scrub(&mut v, "real.example.net", "jdoe", &mut BTreeMap::new());
        let text = v.to_string();
        for real in [
            "jdoe",
            "Jane",
            "Doe",
            "real.example.net",
            "janedoe",
            "Town",
            "0123abcd",
            "box-1",
            "box-2",
            "noreply",
        ] {
            assert!(!text.contains(real), "{real} survived: {text}");
        }
        assert_eq!(v["actor"]["login"], "user");
        assert_eq!(v["actor"]["full_name"], "Example User");
        assert_eq!(v["actor"]["email"], "user@example.com");
        assert_eq!(v["actor"]["avatar_url"], "https://gitea.invalid/avatar.png");
        assert_eq!(v["repository"]["full_name"], "example/paddock");
        assert_eq!(v["repository"]["clone_url"], "https://gitea.invalid/example/paddock.git");
        assert_eq!(v["repository"]["avatar_url"], "");
        assert_eq!(v["id"], 7);
        assert_eq!(v["path"], "ci.yml@refs/heads/main");
    }

    #[test]
    fn runners_keep_one_stable_name_each_and_empty_or_undefined_stay() {
        let mut v = sample();
        scrub(&mut v, "real.example.net", "jdoe", &mut BTreeMap::new());
        let names: Vec<&str> = v["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|j| j["runner_name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["runner-1", "runner-2", "runner-1", "", "undefined"]);
    }

    #[test]
    fn the_recorder_scrubs_what_it_writes_and_trims_the_logs() {
        let mut f = Fake::default();
        let api = "https://real.example.net/api/v1/repos/jdoe/paddock/actions";
        f.json(&format!("{api}/runs/7"), sample());
        f.json(
            &format!("{api}/runs/7/jobs?limit=50&page=1"),
            json!({"total_count": 1, "jobs": [{"id": 9, "name": "j", "conclusion": "failure", "runner_name": "box-1", "steps": []}]}),
        );
        f.log(9, "2026-01-01T00:00:00.0Z Job: rust-test\nnoise from box-1 and Jane Doe\n##[error]failed in /Volumes/d7sst7/gitea-runners/box-1/work/abc\n2026-01-01T00:00:01.0Z      Summary [ 1.0s] 2 tests run: 1 passed, 1 failed\n");
        let files = record(&mut f, "https://real.example.net", "jdoe/paddock", "t", &[7], &mut |_| {}).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["log-9.txt", "jobs-7.json", "run-7.json", "runs-1.json"]);
        for (n, text) in &files {
            for real in ["jdoe", "Jane", "real.example.net", "box-1", "d7sst7", "noreply"] {
                assert!(!text.contains(real), "{n} holds {real}");
            }
        }
        let log = &files[0].1;
        assert!(
            log.contains("Job: rust-test") && log.contains("Summary") && !log.contains("noise"),
            "{log}"
        );
        assert!(files[3].1.contains("\"total_count\": 1"));
    }
}
