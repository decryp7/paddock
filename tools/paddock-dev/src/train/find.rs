// ci-runs-reader: train release: finds the run it dispatched and waits for its end; a red round is judged by ci triage, never by its conclusion alone
//! Finds the run a dispatch started. Gitea's dispatch answers with no run id, so the run is the newest run of that workflow on
//! the train's ref and commit that is newer than the newest one seen before the dispatch.

use crate::ci_runs::client::RunInfo;
use std::time::Duration;

/// How many times the run list is read after a dispatch, and the pause between reads: 10 minutes in all, the script's bound.
pub const TRIES: u32 = 40;
/// The pause between reads.
pub const PAUSE: Duration = Duration::from_secs(15);

/// The newest run id of `workflow` on `git_ref` at `sha` in `runs`, or 0.
pub fn newest(runs: &[RunInfo], workflow: &str, git_ref: &str, sha: &str) -> u64 {
    let path = format!("{workflow}@refs/heads/{git_ref}");
    runs.iter()
        .filter(|r| r.path == path && r.head_sha == sha)
        .map(|r| r.id)
        .max()
        .unwrap_or(0)
}

/// Reads the list until a run newer than `before` shows, at most `tries` times with `pause` between reads.
pub fn find_new_run(
    list: &mut dyn FnMut() -> Result<Vec<RunInfo>, String>,
    sleep: &mut dyn FnMut(Duration),
    (workflow, git_ref, sha): (&str, &str, &str),
    before: u64,
    tries: u32,
    pause: Duration,
) -> Result<u64, String> {
    for attempt in 0..tries {
        let id = newest(&list()?, workflow, git_ref, sha);
        if id > before {
            return Ok(id);
        }
        if attempt + 1 < tries {
            sleep(pause);
        }
    }
    Err(format!(
        "no new run of {workflow} on {git_ref} at {} showed after {tries} reads ({} s apart)",
        &sha[..sha.len().min(9)],
        pause.as_secs()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: u64, wf: &str, sha: &str) -> RunInfo {
        RunInfo {
            id,
            path: format!("{wf}@refs/heads/train/x"),
            event: String::new(),
            status: String::new(),
            conclusion: String::new(),
            head_sha: sha.into(),
            head_branch: String::new(),
            created_at: String::new(),
            run_attempt: 1,
        }
    }

    #[test]
    fn the_new_run_is_found_once_it_shows_and_older_or_foreign_runs_are_ignored() {
        let mut reads = 0;
        let mut slept = Vec::new();
        let got = find_new_run(
            &mut || {
                reads += 1;
                let mut v = vec![run(10, "ci.yml", "s"), run(99, "ci.yml", "other-sha"), run(98, "debrief.yml", "s")];
                if reads >= 3 {
                    v.push(run(11, "ci.yml", "s"));
                }
                Ok(v)
            },
            &mut |d| slept.push(d),
            ("ci.yml", "train/x", "s"),
            10,
            4,
            Duration::from_secs(15),
        );
        assert_eq!(got, Ok(11));
        assert_eq!(slept.len(), 2);
    }

    #[test]
    fn it_gives_up_after_its_reads_and_says_so() {
        let mut reads = 0;
        let got = find_new_run(
            &mut || {
                reads += 1;
                Ok(vec![run(10, "ci.yml", "s")])
            },
            &mut |_| {},
            ("ci.yml", "train/x", "s"),
            10,
            3,
            Duration::from_secs(15),
        );
        assert_eq!(reads, 3);
        assert!(
            got.unwrap_err()
                .contains("no new run of ci.yml on train/x at s showed after 3 reads")
        );
    }

    #[test]
    fn an_unreadable_list_is_an_error_not_a_missing_run() {
        let got = find_new_run(
            &mut || Err("500".into()),
            &mut |_| {},
            ("ci.yml", "train/x", "s"),
            0,
            3,
            Duration::ZERO,
        );
        assert_eq!(got, Err("500".to_string()));
    }
}
