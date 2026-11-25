//! How a CI job ended, parsed once from the Gitea Actions API (a job's `conclusion` in a run's job list), so that no reader
//! turns it into a "did it fail?" boolean and treats a skipped or cancelled job as a pass by accident (user decision 2026-11-25;
//! the two readers that did: `ci verdict` counted an all-skipped run as full and green, ADR 0089/0113, and `ci triage` called a
//! failure NEW because its comparison run skipped the job). Every reader matches on [`JobOutcome`] and decides each case on
//! purpose; there is deliberately no `failed()` or `passed()` helper.
//!
//! The words come from Gitea's `ToActionsStatus`: a job whose status is done gets the conclusion `success`, `failure`,
//! `cancelled` or `skipped`; one that is not done (unknown, waiting, blocked, running) gets an empty conclusion and the status
//! `queued`, `waiting` or `in_progress`. The recorded fixtures under `tests/` hold exactly these five (empty once, for a job
//! still running).
//!
//! Whether a job ever got a runner or ran a step is a separate fact (`TimedJob::never_ran` of the CI-run reader): a job
//! cancelled while queued and one cancelled mid-test are both [`JobOutcome::Cancelled`].

use std::fmt;

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// `success`.
    Passed,
    /// `failure`.
    Failed,
    /// `cancelled`: neither a pass nor a failure (ER-33: not measured).
    Cancelled,
    /// `skipped`: the job did not run (its `if:` was false, or a job it needs failed). Never a pass.
    Skipped,
    /// No conclusion yet: the job is queued, waiting, blocked or running.
    Unfinished,
    /// A conclusion this reader does not know (GitHub's `neutral` or `timed_out`, a newer server): never a pass. Holds the word.
    Unknown(String),
}

impl JobOutcome {
    /// Parses the API's `conclusion` word (empty or absent: [`JobOutcome::Unfinished`]).
    pub fn from_api(conclusion: &str) -> JobOutcome {
        match conclusion {
            "success" => JobOutcome::Passed,
            "failure" => JobOutcome::Failed,
            "cancelled" => JobOutcome::Cancelled,
            "skipped" => JobOutcome::Skipped,
            "" => JobOutcome::Unfinished,
            other => JobOutcome::Unknown(other.to_string()),
        }
    }

    /// The API's word for it (empty for [`JobOutcome::Unfinished`]): what messages and recorded JSON print.
    pub fn as_api(&self) -> &str {
        match self {
            JobOutcome::Passed => "success",
            JobOutcome::Failed => "failure",
            JobOutcome::Cancelled => "cancelled",
            JobOutcome::Skipped => "skipped",
            JobOutcome::Unfinished => "",
            JobOutcome::Unknown(w) => w,
        }
    }
}

impl fmt::Display for JobOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_api())
    }
}

#[cfg(test)]
mod tests {
    use super::JobOutcome;

    #[test]
    fn every_api_word_parses_to_its_own_outcome_and_prints_back() {
        for (word, outcome) in [
            ("success", JobOutcome::Passed),
            ("failure", JobOutcome::Failed),
            ("cancelled", JobOutcome::Cancelled),
            ("skipped", JobOutcome::Skipped),
            ("", JobOutcome::Unfinished),
            ("neutral", JobOutcome::Unknown("neutral".into())),
        ] {
            assert_eq!(JobOutcome::from_api(word), outcome, "{word:?}");
            assert_eq!(outcome.as_api(), word);
        }
    }

    /// Every conclusion in the recorded job lists is one of Gitea's known words: none parses to `Unknown`.
    #[test]
    fn the_recorded_job_lists_hold_only_known_conclusions() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut seen = std::collections::BTreeSet::new();
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "json") {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&p).unwrap_or_default()) else {
                        continue;
                    };
                    for j in v["jobs"].as_array().into_iter().flatten() {
                        if let Some(c) = j["conclusion"].as_str() {
                            assert!(
                                !matches!(JobOutcome::from_api(c), JobOutcome::Unknown(_)),
                                "{}: conclusion {c:?}",
                                p.display()
                            );
                            seen.insert(c.to_string());
                        }
                    }
                }
            }
        }
        // The probe can see its signal (ER-33): the fixtures hold the four done words.
        for w in ["success", "failure", "cancelled", "skipped"] {
            assert!(seen.contains(w), "no recorded job ended {w:?}: {seen:?}");
        }
    }
}
