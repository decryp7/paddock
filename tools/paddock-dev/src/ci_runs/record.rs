// ci-runs-reader: ci record-fixture: records finished runs as test fixtures
//! `ci record-fixture <run>...`: records finished runs of the Gitea Actions API as test fixtures, scrubbed on capture.
//!
//! A recording is kept in git for good, and the API's answers carry people and machines: full names, e-mail addresses, avatar
//! links, logins and runner host names. [`scrub`] replaces them with invented values before anything is written, and conformance
//! C110 fails any fixture that still holds one. The recorder writes, per run, `run-<id>.json`, `jobs-<id>.json` (the first page of
//! jobs) and `log-<job>.txt` for each job that ran (only the lines the readers need, see [`trim_log`]: the plan job's scope line, the
//! nextest and Playwright result lines), `attempt-<run>-<n>.json` for each earlier attempt of a rerun run, `commit-<sha>.json` (the
//! parents only) for the head of a `trial/` run, and one `runs-1.json` listing the recorded runs, which is the layout of
//! [`Fake::from_dir`](super::testutil::Fake::from_dir). `--name <shape>` records into `tests/fixtures/runs/<shape>/`, the shapes the
//! replay suite reads (`ci_runs::replay`, docs/testing/ci-replay.md). Read-only on the server.

use crate::job_outcome::JobOutcome;
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

/// The lines of a job log that the nextest reader needs (`Job:` key, `Starting`, a test's end, `Summary`, errors), scrubbed as the
/// API answers are: the server and owner names ([`rename`]), runner and disk names, and the user folder of a home path.
pub fn trim_log(log: &str, real_host: &str, real_owner: &str) -> String {
    let keep = regex::Regex::new(
        r"^\s*(Job: |plan (scope|verdict): |Starting \d+ tests?|(FAIL|TIMEOUT|TMT|SIG\w*|FLAKY|FLKY\S*|TRY \d+ (FAIL|TIMEOUT|PASS|SIG\w*))\b|Summary \[|.*tests? (were |was )?not run|Cancelling due to|##\[error\]|::error::)",
    );
    let Ok(keep) = keep else { return String::new() };
    // (pattern, replacement), in order. Runner work folders name the runner and the disk: `/Volumes/<disk>/gitea-runners/<runner>`;
    // any other folder on a named disk (the CI home `/Volumes/<disk>/ci/home/...`) names the disk too. Home folders name the user:
    // `/Users/<name>` (macOS), `/home/<name>` (Linux), `C:\Users\<name>` and `C:/Users/<name>` (Windows, also JSON-escaped). A
    // name ends at a separator, white space or a quote.
    const NAME: &str = r#"[^/\\\s'`"]+"#;
    let rules: Vec<(regex::Regex, &str)> = [
        (
            format!("/Volumes/{NAME}/gitea-runners/{NAME}"),
            "/Volumes/disk/gitea-runners/runner-1",
        ),
        (format!("/Volumes/{NAME}"), "/Volumes/disk"),
        (format!("/Users/{NAME}"), "/Users/user"),
        (format!("/home/{NAME}"), "/home/user"),
        (
            format!(r"(?i)([A-Z]:)(\\{{1,2}}|/)Users(\\{{1,2}}|/){NAME}"),
            "${1}${2}Users${3}user",
        ),
    ]
    .into_iter()
    .filter_map(|(p, r)| regex::Regex::new(&p).ok().map(|re| (re, r)))
    .collect();
    if rules.len() != 5 {
        return String::new();
    }
    let mut out = String::new();
    for l in log.lines() {
        if keep.is_match(&plain(l)) {
            let mut l = rename(l, real_host, real_owner);
            for (re, with) in &rules {
                l = re.replace_all(&l, *with).into_owned();
            }
            out.push_str(&l);
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
        let logged: Vec<u64> = jobs["jobs"]
            .as_array()
            .map(|a| {
                a.iter()
                    // Record the log of every job that ran (the readers grade a pass by its log too, and the plan job's holds the
                    // scope line): a skip has no log.
                    .filter(|j| !matches!(JobOutcome::from_api(j["conclusion"].as_str().unwrap_or("")), JobOutcome::Skipped))
                    .filter_map(|j| j["id"].as_u64())
                    .collect()
            })
            .unwrap_or_default();
        // per-item: the server has no bulk log call; each trimmed log is a few lines, and a recording is made once per shape.
        for job in logged {
            match http.get_text(&format!("{api}/jobs/{job}/logs"), token) {
                Ok(t) => files.push((format!("log-{job}.txt"), trim_log(&t, host, owner))),
                Err(e) => log(&format!("::warning::job {job}: its log was not recorded ({e})")),
            }
        }
        // The jobs of each earlier attempt of a rerun (the verdict reads them to see a green that rests on a retry).
        let attempts = run["run_attempt"].as_u64().unwrap_or(1);
        for n in 1..attempts {
            // per-item: one list per earlier attempt; a rerun has one or two.
            let mut earlier = http.get_json(&format!("{api}/runs/{id}/attempts/{n}/jobs?limit=50&page=1"), token)?;
            scrub(&mut earlier, host, owner, &mut runners);
            files.push((format!("attempt-{id}-{n}.json"), format!("{earlier:#}\n")));
        }
        // A trial's merge commit: only its parents are read (the member is the second), so only they are kept.
        if run["head_branch"].as_str().is_some_and(|b| b.starts_with("trial/"))
            && let Some(sha) = run["head_sha"].as_str()
        {
            let commit = http.get_json(
                &format!("{}/git/commits/{sha}", api.strip_suffix("/actions").unwrap_or(&api)),
                token,
            )?;
            let parents: Vec<Value> = commit["parents"]
                .as_array()
                .map(|a| a.iter().map(|p| serde_json::json!({"sha": p["sha"]})).collect())
                .unwrap_or_default();
            files.push((
                format!("commit-{sha}.json"),
                format!("{:#}\n", serde_json::json!({"sha": sha, "parents": parents})),
            ));
        }
        // The run's artifacts (the escape monitor's `plan-record`): the list as the API answers it, each download's redirect and its zip
        // as hex. A run with none, or a server that cannot say, records none: this is no failure of the recording.
        match http.get_json(&format!("{api}/runs/{id}/artifacts"), token) {
            Ok(mut list) => {
                let ids: Vec<(u64, bool)> = list["artifacts"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| Some((x["id"].as_u64()?, x["expired"].as_bool().unwrap_or(false))))
                            .collect()
                    })
                    .unwrap_or_default();
                scrub(&mut list, host, owner, &mut runners);
                if !ids.is_empty() {
                    files.push((format!("artifacts-{id}.json"), format!("{list:#}\n")));
                }
                // per-item: the server has no bulk artifact call; a run has a few artifacts, and a recording is made once per shape.
                for (aid, _) in ids.into_iter().filter(|(_, expired)| !expired) {
                    let zip_url = format!("{api}/artifacts/{aid}/zip");
                    match http.get_redirect(&zip_url, token) {
                        Ok(Some(l)) => files.push((
                            format!("artifact-{aid}.redirect.txt"),
                            format!("{}\n", redirect_shape(&l, host, owner)),
                        )),
                        Ok(None) => files.push((format!("artifact-{aid}.redirect.txt"), "none\n".to_string())),
                        Err(e) => log(&format!("::warning::artifact {aid}: its redirect was not recorded ({e})")),
                    }
                    match http.get_bytes(&zip_url, token, 4 << 20) {
                        Ok(b) => files.push((format!("artifact-{aid}.zip.hex"), format!("{}\n", hex(&b)))),
                        Err(e) => log(&format!("::warning::artifact {aid}: its zip was not recorded ({e})")),
                    }
                }
            }
            Err(e) => log(&format!("::warning::run {id}: its artifacts were not recorded ({e})")),
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

/// The path of a download's `Location`, with the host and owner renamed and the signature and expiry (a secret that lapses) dropped:
/// what the replay pins is where the server sends the download, not who signed it.
pub fn redirect_shape(location: &str, real_host: &str, real_owner: &str) -> String {
    rename(location.split_once('?').map_or(location, |(path, _)| path), real_host, real_owner)
}

/// Bytes as lower-case hex (a recording is text: a zip is kept as hex and read back by [`unhex`]).
pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The bytes of [`hex`]; `None` for anything else.
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    (s.len().is_multiple_of(2) && s.is_ascii())
        .then(|| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect())
        .flatten()
}

/// The folder of the replay suite's shapes, below the repository root.
pub const SHAPES: &str = "tools/paddock-dev/tests/fixtures/runs";

/// Whether `name` is a shape name: lower-case words joined by `-`, so it is one folder name and never a path.
pub fn valid_shape(name: &str) -> bool {
    !name.is_empty()
        && name
            .split('-')
            .all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// The folder to write: `--out`, or the shape's folder below the repository root.
fn out_dir(a: &super::RecordArgs) -> Result<std::path::PathBuf, paddock_cli::CliError> {
    use paddock_cli::CliError;
    match (&a.out, &a.name) {
        (Some(out), None) => Ok(out.clone()),
        (None, Some(name)) if valid_shape(name) => {
            let here = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
            let root = super::find_root(&here).ok_or_else(|| {
                CliError::failure("no repository root above the current folder.").with_hint("Run it inside the checkout.")
            })?;
            Ok(root.join(SHAPES).join(name))
        }
        (None, Some(name)) => Err(CliError::usage(format!("{name:?} is not a shape name."))
            .with_hint("Use lower-case words joined by `-`, for example green-train.")),
        _ => Err(CliError::usage("give exactly one of --name and --out.").with_hint("`--name <shape>` records a replay shape.")),
    }
}

/// `ci record-fixture`, against the real server.
pub fn command(a: &super::RecordArgs) -> Result<paddock_cli::Report, paddock_cli::CliError> {
    let out = out_dir(a)?;
    let token = super::token()?;
    let mut http = CurlHttp;
    let files = record(&mut http, &a.server, &a.repo, &token, &a.runs, &mut |l| crate::log_text::eline(l)).map_err(|e| {
        paddock_cli::CliError::failure(format!("the runs could not be recorded: {e}")).with_hint("Check the network and GITEA_TOKEN.")
    })?;
    std::fs::create_dir_all(&out).map_err(|e| paddock_cli::CliError::failure(format!("cannot create {}: {e}", out.display())))?;
    for (name, text) in &files {
        std::fs::write(out.join(name), text).map_err(|e| paddock_cli::CliError::failure(format!("cannot write {name}: {e}")))?;
    }
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    Ok(paddock_cli::Report::new(
        serde_json::json!({"out": out.display().to_string(), "files": names}),
        format!("{} file(s) written to {}, identities scrubbed", files.len(), out.display()),
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
        let vol = "/Volumes";
        f.log(9, &format!("2026-01-01T00:00:00.0Z Job: rust-test\nnoise from box-1 and Jane Doe\n##[error]failed in {vol}/realdisk7/gitea-runners/box-1/work/abc\n::error::move the checkout to {vol}/realdisk7/ci/home/src/job-s3\n2026-01-01T00:00:01.0Z      Summary [ 1.0s] 2 tests run: 1 passed, 1 failed\n"));
        let files = record(&mut f, "https://real.example.net", "jdoe/paddock", "t", &[7], &mut |_| {}).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["log-9.txt", "jobs-7.json", "run-7.json", "runs-1.json"]);
        for (n, text) in &files {
            for real in ["jdoe", "Jane", "real.example.net", "box-1", "realdisk7", "noreply"] {
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

    #[test]
    fn an_identity_inside_a_kept_error_line_is_scrubbed() {
        let mut f = Fake::default();
        let api = "https://real.example.net/api/v1/repos/jdoe/paddock/actions";
        f.json(&format!("{api}/runs/8"), sample());
        f.json(
            &format!("{api}/runs/8/jobs?limit=50&page=1"),
            json!({"total_count": 1, "jobs": [{"id": 10, "conclusion": "failure"}]}),
        );
        let vol = "/Volumes";
        f.log(
            10,
            &format!(
                "::error::push to https://real.example.net/jdoe/paddock failed for jdoe in /Users/janedoe/src and /home/jdoe2/x and \
                 C:\\Users\\JaneD\\AppData and C:/Users/JaneD/y and \"{vol}/realdisk8\"/z\n"
            ),
        );
        let files = record(&mut f, "https://real.example.net", "jdoe/paddock", "t", &[8], &mut |_| {}).unwrap();
        let log = &files[0].1;
        for real in ["real.example.net", "jdoe", "janedoe", "JaneD", "realdisk8"] {
            assert!(!log.contains(real), "the kept line holds {real}: {log}");
        }
        assert!(log.contains("::error::") && log.contains("gitea.invalid/example/paddock"), "{log}");
        assert!(log.contains("/Users/user/src") && log.contains("/home/user/x"), "{log}");
        assert!(log.contains("\"/Volumes/disk\"/z"), "the quote ends the disk name: {log}");
    }

    #[test]
    fn the_plan_scope_line_and_the_logs_of_passed_jobs_are_recorded_and_a_skipped_job_has_none() {
        let mut f = Fake::default();
        let api = "https://real.example.net/api/v1/repos/jdoe/paddock/actions";
        f.json(&format!("{api}/runs/11"), sample());
        f.json(
            &format!("{api}/runs/11/jobs?limit=50&page=1"),
            json!({"total_count": 3, "jobs": [{"id": 1, "name": "plan", "conclusion": "success"},
                {"id": 2, "name": "crates", "conclusion": "success"}, {"id": 3, "name": "ui", "conclusion": "skipped"}]}),
        );
        f.log(
            1,
            "2026-01-01T00:00:00.0Z noise\nplan scope: {\"v\":2}\nplan verdict: complete=true\n",
        );
        f.log(2, "Job: crates\nnoise\n     Summary [ 1.0s] 2 tests run: 2 passed\n");
        let files = record(&mut f, "https://real.example.net", "jdoe/paddock", "t", &[11], &mut |_| {}).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["log-1.txt", "log-2.txt", "jobs-11.json", "run-11.json", "runs-1.json"]);
        assert!(
            files[0].1.contains("plan scope: {\"v\":2}")
                && files[0].1.contains("plan verdict: complete=true")
                && !files[0].1.contains("noise")
        );
        assert!(files[1].1.contains("Summary") && !files[1].1.contains("noise"));
    }

    #[test]
    fn a_rerun_records_the_jobs_of_each_earlier_attempt() {
        let mut f = Fake::default();
        let api = "https://real.example.net/api/v1/repos/jdoe/paddock/actions";
        let mut run = sample();
        run["run_attempt"] = json!(3);
        f.json(&format!("{api}/runs/12"), run);
        f.json(
            &format!("{api}/runs/12/jobs?limit=50&page=1"),
            json!({"total_count": 0, "jobs": []}),
        );
        for n in 1..=2 {
            f.json(
                &format!("{api}/runs/12/attempts/{n}/jobs?limit=50&page=1"),
                json!({"total_count": 1, "jobs": [{"id": n, "name": "crates", "conclusion": "failure", "runner_name": "box-1"}]}),
            );
        }
        let files = record(&mut f, "https://real.example.net", "jdoe/paddock", "t", &[12], &mut |_| {}).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "attempt-12-1.json",
                "attempt-12-2.json",
                "jobs-12.json",
                "run-12.json",
                "runs-1.json"
            ]
        );
        assert!(!files[0].1.contains("box-1"), "the attempt's runner is scrubbed: {}", files[0].1);
    }

    #[test]
    fn a_trials_head_commit_is_recorded_with_its_parents_only() {
        let mut f = Fake::default();
        let api = "https://real.example.net/api/v1/repos/jdoe/paddock";
        let mut run = sample();
        run["head_branch"] = json!("trial/feat/x");
        run["head_sha"] = json!("aaaa");
        f.json(&format!("{api}/actions/runs/13"), run);
        f.json(
            &format!("{api}/actions/runs/13/jobs?limit=50&page=1"),
            json!({"total_count": 0, "jobs": []}),
        );
        f.json(
            &format!("{api}/git/commits/aaaa"),
            json!({"sha": "aaaa", "author": {"name": "Jane Doe", "email": "jane@real.example.net"}, "parents": [{"sha": "bbbb", "url": "x"}, {"sha": "cccc", "url": "y"}]}),
        );
        let files = record(&mut f, "https://real.example.net", "jdoe/paddock", "t", &[13], &mut |_| {}).unwrap();
        let (name, text) = &files[0];
        assert_eq!(name, "commit-aaaa.json");
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(v, json!({"sha": "aaaa", "parents": [{"sha": "bbbb"}, {"sha": "cccc"}]}));
    }

    /// A server with one run that has one artifact: the list, the redirect of its download (with a signature) and its zip.
    struct WithArtifact;
    impl Http for WithArtifact {
        fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
            let api = "https://real.example.net/api/v1/repos/jdoe/paddock/actions";
            if url == format!("{api}/runs/5") {
                Ok(json!({"id": 5, "run_attempt": 1, "head_branch": "main", "head_sha": "aaaa"}))
            } else if url == format!("{api}/runs/5/jobs?limit=50&page=1") {
                Ok(json!({"total_count": 0, "jobs": []}))
            } else if url == format!("{api}/runs/5/artifacts") {
                Ok(json!({"total_count": 2, "artifacts": [
                    {"id": 41, "name": "plan-record", "expired": false, "archive_download_url": format!("{api}/artifacts/41/zip")},
                    {"id": 42, "name": "gone", "expired": true}]}))
            } else {
                Err("curl exited with 22: error: 404".into())
            }
        }
        fn get_redirect(&mut self, url: &str, _token: &str) -> Result<Option<String>, String> {
            assert!(url.ends_with("/artifacts/41/zip"), "an expired artifact is never downloaded: {url}");
            Ok(Some(format!("{}/raw?sig=SECRET&expires=1", url)))
        }
        fn get_bytes(&mut self, url: &str, _token: &str, _max: u64) -> Result<Vec<u8>, String> {
            assert!(url.ends_with("/artifacts/41/zip"), "{url}");
            Ok(vec![0x50, 0x4b, 0x00, 0xff])
        }
    }

    #[test]
    fn a_runs_artifacts_are_recorded_as_list_redirect_and_hex_without_secrets() {
        let files = record(
            &mut WithArtifact,
            "https://real.example.net",
            "jdoe/paddock",
            "t",
            &[5],
            &mut |_| {},
        )
        .unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "artifacts-5.json",
                "artifact-41.redirect.txt",
                "artifact-41.zip.hex",
                "jobs-5.json",
                "run-5.json",
                "runs-1.json"
            ]
        );
        let get = |n: &str| files.iter().find(|(x, _)| x == n).unwrap().1.clone();
        assert_eq!(
            get("artifact-41.redirect.txt").trim(),
            "https://gitea.invalid/api/v1/repos/example/paddock/actions/artifacts/41/zip/raw"
        );
        assert_eq!(unhex(&get("artifact-41.zip.hex")), Some(vec![0x50, 0x4b, 0x00, 0xff]));
        for (n, text) in &files {
            for secret in ["SECRET", "jdoe", "real.example.net"] {
                assert!(!text.contains(secret), "{n} holds {secret}");
            }
        }
        // The hex reader refuses what is no hex.
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn a_shape_name_is_one_folder_name() {
        for ok in ["green-train", "push-reused", "run9167"] {
            assert!(valid_shape(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "A", "a/b", "../x", "a b", "a_b"] {
            assert!(!valid_shape(bad), "{bad}");
        }
    }
}
