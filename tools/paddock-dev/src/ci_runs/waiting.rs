//! A train's waiting intent (2026-11-15): while `ci dispatch` waits for a busy single-machine runner before dispatching a train
//! run, nothing of the train is queued yet, so `ci schedule-skip` saw no train and the next scheduled run took the runner again;
//! a waiting train could be starved for hours (2026-11-14 11:0x, and 2026-11-15 05:03 to 07:07 behind runs 8112 to 8114).
//!
//! The dispatcher records the intent in the repository Actions variable [`VAR`] through the same Actions API it dispatches with
//! (`/actions/variables`, the dispatch token can write it), and clears it when it dispatched or gave up. Scheduled runs read it
//! as `${{ vars.PADDOCK_TRAIN_WAITING }}` (the job token cannot read variables through the API; the runner hands them to every
//! job) and pass it to `ci schedule-skip`, which skips while it is live, with the same bounds as for a queued train.
//!
//! The value is `<train branch> <workflow> <expiry, Unix seconds>`. The expiry is the dispatcher's start plus its `--max`, so an
//! intent left behind by a dispatcher that was killed stops counting when that dispatcher would have given up anyway.

use crate::wait::Http;
use serde_json::json;

/// The repository Actions variable that holds the intent.
pub const VAR: &str = "PADDOCK_TRAIN_WAITING";

/// The longest an intent may last, whatever the value says: a dispatcher's `--max` is at most a day.
pub const MAX_SECS: i64 = 24 * 3600;

/// A train dispatch waiting for a runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    /// The train branch (`train/2026-11-15-12`).
    pub branch: String,
    /// The workflow it will dispatch (`ci.yml`).
    pub workflow: String,
    /// When the intent ends (Unix seconds).
    pub expires: i64,
}

impl Waiting {
    /// The variable's value.
    pub fn value(&self) -> String {
        format!("{} {} {}", self.branch, self.workflow, self.expires)
    }

    /// Reads a value; `Ok(None)` for an empty one (no intent), `Err` for one that is not `<train/...> <workflow> <seconds>`.
    pub fn parse(v: &str) -> Result<Option<Waiting>, String> {
        let v = v.trim();
        if v.is_empty() {
            return Ok(None);
        }
        let parts: Vec<&str> = v.split_whitespace().collect();
        match parts[..] {
            [branch, workflow, expires] if branch.starts_with("train/") && !workflow.is_empty() => {
                let expires = expires
                    .parse::<i64>()
                    .map_err(|_| format!("{VAR}={v:?}: the expiry is not Unix seconds"))?;
                Ok(Some(Waiting {
                    branch: branch.into(),
                    workflow: workflow.into(),
                    expires,
                }))
            }
            _ => Err(format!("{VAR}={v:?} is not `<train/...> <workflow> <expiry seconds>`")),
        }
    }

    /// Whether it still counts at `now` (Unix seconds): not expired, and not further ahead than a dispatcher can wait.
    pub fn live(&self, now: i64) -> bool {
        now < self.expires && self.expires - now <= MAX_SECS
    }

    /// The expiry as `YYYY-MM-DDTHH:MMZ`.
    pub fn until(&self) -> String {
        let s = self.expires.rem_euclid(86_400);
        format!("{}T{:02}:{:02}Z", super::time::date_of(self.expires), s / 3600, s % 3600 / 60)
    }
}

/// Whether a dispatch of `git_ref` is a train's (only those record an intent: a scheduled run skips for a train only).
pub fn is_train(git_ref: &str) -> bool {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref).starts_with("train/")
}

fn url(server: &str, repo: &str) -> String {
    format!("{}/api/v1/repos/{repo}/actions/variables/{VAR}", server.trim_end_matches('/'))
}

/// Writes the intent (update, or create when there is none). Errors are texts without the token.
pub fn record(http: &mut dyn Http, server: &str, repo: &str, token: &str, w: &Waiting) -> Result<(), String> {
    let u = url(server, repo);
    let body = json!({"name": VAR, "value": w.value()});
    match http.send_json("PUT", &u, token, Some(&body)) {
        Ok(_) => Ok(()),
        Err(e) if e.contains("404") => http.send_json("POST", &u, token, Some(&json!({"value": w.value()}))).map(|_| ()),
        Err(e) => Err(e),
    }
}

/// Removes the intent when it is still `w` (another dispatcher may have written its own since). `Ok(false)`: not ours or gone.
pub fn clear(http: &mut dyn Http, server: &str, repo: &str, token: &str, w: &Waiting) -> Result<bool, String> {
    let u = url(server, repo);
    let current = match http.get_json(&u, token) {
        Ok(v) => v["data"].as_str().or_else(|| v["value"].as_str()).map(|s| s.trim().to_string()),
        Err(e) if e.contains("404") => None,
        Err(e) => return Err(e),
    };
    if current.as_deref() != Some(w.value().as_str()) {
        return Ok(false);
    }
    http.send_json("DELETE", &u, token, None).map(|_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_reads_back_and_a_bad_one_is_refused() {
        let w = Waiting {
            branch: "train/2026-11-15-12".into(),
            workflow: "ci.yml".into(),
            expires: 1_794_268_800,
        };
        assert_eq!(w.value(), "train/2026-11-15-12 ci.yml 1794268800");
        assert_eq!(Waiting::parse(&w.value()), Ok(Some(w.clone())));
        assert_eq!(Waiting::parse("  "), Ok(None));
        assert!(Waiting::parse("main ci.yml 1").is_err(), "only a train waits");
        assert!(Waiting::parse("train/x ci.yml soon").is_err());
        assert!(Waiting::parse("train/x").is_err());
        assert_eq!(w.until(), "2026-11-10T00:00Z");
    }

    #[test]
    fn an_intent_counts_until_its_expiry_and_never_more_than_a_day_ahead() {
        let w = |expires| Waiting {
            branch: "train/t".into(),
            workflow: "ci.yml".into(),
            expires,
        };
        assert!(w(1_000).live(999));
        assert!(!w(1_000).live(1_000), "it ends with the dispatcher's --max");
        assert!(!w(1_000 + MAX_SECS + 1).live(1_000), "a value too far ahead is not trusted");
        assert!(is_train("train/2026-11-15-12") && is_train("refs/heads/train/x"));
        assert!(!is_train("main") && !is_train("trial/fix/x"));
    }
}
