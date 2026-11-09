//! The Gitea API from the app, with the user's read token (the service never sees it): the current job's run
//! details and the outcome and queue time of finished jobs. URLs and parsing are `pitcrew_core::gitea_api`.
//!
//! Job outcomes are cached in the app's data folder (`job-outcomes.json`, bounded, versioned, rebuilt when damaged)
//! so the Activity view asks the server only about jobs it has not seen; each refresh looks up at most
//! [`MAX_LOOKUPS`] jobs.

use pitcrew_core::gitea_api::{self, JobDetails, RunDetails, MAX_API_BODY};
use pitcrew_core::jobs::{FinishedJob, JobInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// Most job lookups per refresh.
pub const MAX_LOOKUPS: usize = 25;
/// Most outcomes kept in the cache.
pub const MAX_CACHED: usize = 5000;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(url: &str, token: &str) -> Result<Option<String>, String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("not an http(s) address".into());
    }
    let resp = agent()
        .get(url)
        .header("Authorization", &format!("token {token}"))
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("the server could not be reached: {e}"))?;
    let status = resp.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if status == 401 || status == 403 {
        return Err("the server refused the read token; enter a new one in Settings".into());
    }
    if !(200..300).contains(&status) {
        return Err(format!("the server answered HTTP {status}"));
    }
    let mut body = String::new();
    resp.into_body()
        .into_reader()
        .take(MAX_API_BODY as u64 + 1)
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(Some(body))
}

/// The run of the current job, when the server can tell.
pub fn run_details(job: &JobInfo, token: &str) -> Result<Option<RunDetails>, String> {
    let Some(url) = gitea_api::run_api_url(job) else { return Ok(None) };
    match get(&url, token)? {
        Some(body) => gitea_api::parse_run(&body).map(Some),
        None => Ok(None),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    version: u32,
    #[serde(default)]
    jobs: HashMap<u64, JobDetails>,
}

fn load(path: &Path) -> Cache {
    std::fs::read_to_string(path)
        .ok()
        .filter(|t| t.len() < 4 * 1024 * 1024)
        .and_then(|t| serde_json::from_str::<Cache>(&t).ok())
        .filter(|c| c.version == 1)
        .unwrap_or(Cache {
            version: 1,
            jobs: HashMap::new(),
        })
}

fn save(path: &Path, c: &Cache) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(c) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// The outcomes of `jobs`: cached ones, plus up to [`MAX_LOOKUPS`] new lookups with `token`.
pub fn outcomes(cache_file: &Path, jobs: &[FinishedJob], token: Option<&str>) -> HashMap<u64, JobDetails> {
    let mut cache = load(cache_file);
    let mut changed = false;
    if let Some(token) = token {
        let mut looked = 0;
        for j in jobs.iter().rev() {
            let Some(id) = j.info.job_id else { continue };
            if cache.jobs.contains_key(&id) || looked >= MAX_LOOKUPS {
                continue;
            }
            let Some(url) = gitea_api::job_api_url(&j.info) else { continue };
            looked += 1;
            match get(&url, token) {
                Ok(Some(body)) => {
                    if let Ok(d) = gitea_api::parse_job(&body) {
                        if d.conclusion != gitea_api::Conclusion::Unknown {
                            cache.jobs.insert(id, d);
                            changed = true;
                        }
                    }
                }
                Ok(None) => {}
                // The server is unreachable or refuses the token: stop asking this time.
                Err(_) => break,
            }
        }
    }
    if cache.jobs.len() > MAX_CACHED {
        let mut ids: Vec<u64> = cache.jobs.keys().copied().collect();
        ids.sort_unstable();
        for id in ids.iter().take(cache.jobs.len() - MAX_CACHED) {
            cache.jobs.remove(id);
        }
        changed = true;
    }
    if changed {
        save(cache_file, &cache);
    }
    cache.jobs
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::time::Timestamp;

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn a_damaged_cache_starts_over_and_masked_jobs_are_not_looked_up() {
        let f = std::env::temp_dir().join(format!("pitcrew-it-jobcache-{}.json", std::process::id()));
        std::fs::write(&f, "{broken").unwrap();
        let job = FinishedJob {
            info: JobInfo {
                job_id: Some(1),
                repository: Some("***/app".into()),
                server: Some("https://git.example.com".into()),
                ..JobInfo::default()
            },
            started_at: Timestamp(0),
            finished_at: Timestamp(1),
            outcome: None,
            peak_cpu_permille: 0,
            peak_mem_bytes: 0,
            leftovers: 0,
        };
        assert!(outcomes(&f, &[job], Some("t")).is_empty());
        let mut c = Cache {
            version: 1,
            jobs: HashMap::new(),
        };
        c.jobs.insert(
            5,
            JobDetails {
                conclusion: gitea_api::Conclusion::Success,
                queued_ms: Some(1),
            },
        );
        save(&f, &c);
        assert_eq!(outcomes(&f, &[], None).len(), 1);
        assert!(get("ftp://x", "t").is_err());
        let _ = std::fs::remove_file(&f);
    }
}
