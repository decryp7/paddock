// ci-runs-reader: ci cache-audit: has a cache key ever been saved (ADR 0115)
//! `ci cache-audit`: reads the `cache-report:` lines that every cache step of CI prints (`conformance/ci/cache-report.sh`, ADR 0115)
//! from the job logs of the last runs of a workflow, and names every cache key that was never saved: missed by several jobs, with no
//! exact hit and no confirmed save anywhere in the window. A cache that is never filled costs every job its full rebuild and shows
//! nowhere else (run 8576: the dev tool was rebuilt in 42 of 44 jobs, a green run each time).
//!
//! Report-only (ER-41): findings exit 0, and `--fail-on-never-saved` makes them exit 5. A window with no `cache-report:` line at all
//! is "check broken", exit 6, never "no problem" (ER-33): the lines are the check's only eyes, so a log that lost them must be loud.
//! A log that cannot be read is counted and named, never skipped quietly.
//!
//! How a key counts: a report is `name`, `key`, the restore `outcome` (exact, partial, miss) and `saved` (yes, no, pending, n/a). A
//! miss whose save the job reported (`yes` or `no`) counts once per job; a miss with a save pending in actions/cache's post step
//! counts once per run (the job cannot tell whether it worked, only a later run's hit can). A key is never saved when it has at
//! least `--min-misses` such misses (default 3), no exact hit and no `saved=yes`.

use super::client::Client;
use crate::log_text::plain;
use std::collections::{BTreeMap, BTreeSet};

/// One `cache-report:` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The cache (`dev-tool`, `npm`, `Angular`).
    pub name: String,
    /// `exact`, `partial` or `miss`.
    pub outcome: String,
    /// `yes`, `no`, `pending` or `n/a`.
    pub saved: String,
    /// The cache key.
    pub key: String,
}

/// Reads the `cache-report:` lines of one job log (timestamps and colours around the line are ignored).
pub fn parse(log: &str) -> Vec<Report> {
    let mut out = Vec::new();
    for line in log.lines() {
        let line = plain(line);
        let Some(at) = line.find("cache-report: ") else { continue };
        let mut get = std::collections::HashMap::new();
        // The key is last and may hold anything but a space; name, outcome and saved are single words.
        for part in line[at + "cache-report: ".len()..].split(' ') {
            if let Some((k, v)) = part.split_once('=') {
                get.entry(k).or_insert(v);
            }
        }
        let (Some(name), Some(outcome), Some(saved), Some(key)) = (get.get("name"), get.get("outcome"), get.get("saved"), get.get("key"))
        else {
            continue;
        };
        out.push(Report {
            name: (*name).into(),
            outcome: (*outcome).into(),
            saved: (*saved).into(),
            key: (*key).into(),
        });
    }
    out
}

/// What the window showed for one key.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KeyStats {
    /// The cache's name.
    pub name: String,
    /// Misses that count (see the module doc).
    pub misses: usize,
    /// Exact hits.
    pub hits: usize,
    /// Jobs that reported a confirmed save.
    pub saved: usize,
}

/// The reports of the window: `(run, job, report)`.
pub type Seen = Vec<(u64, u64, Report)>;

/// Per key: how it fared. Keys are those of the reports; a key of one cache name never mixes with another's.
pub fn stats(seen: &Seen) -> BTreeMap<String, KeyStats> {
    let mut by: BTreeMap<String, KeyStats> = BTreeMap::new();
    let mut pending_runs: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    for (run, _job, r) in seen {
        let s = by.entry(r.key.clone()).or_default();
        s.name = r.name.clone();
        match (r.outcome.as_str(), r.saved.as_str()) {
            ("exact", _) => s.hits += 1,
            // A save step's own report (the key it saved under): a success is a confirmed save, a failure counts as a miss.
            ("save", "yes") => s.saved += 1,
            ("save", _) => s.misses += 1,
            // This job never saves the key (a marker lookup, or a restore whose save is another key): nothing to expect.
            (_, "n/a") => {}
            (_, "yes") => {
                s.saved += 1;
                s.misses += 1;
            }
            (_, "pending") => {
                pending_runs.entry(r.key.clone()).or_default().insert(*run);
            }
            // A partial restore filled the folder from an older key; the exact key itself still was not there.
            ("miss" | "partial", _) => s.misses += 1,
            _ => {}
        }
    }
    for (key, runs) in pending_runs {
        if let Some(s) = by.get_mut(&key) {
            s.misses += runs.len();
        }
    }
    by
}

/// The keys that were never saved: `misses >= min_misses`, no exact hit, no confirmed save.
pub fn never_saved(by: &BTreeMap<String, KeyStats>, min_misses: usize) -> Vec<(&String, &KeyStats)> {
    by.iter()
        .filter(|(_, s)| s.misses >= min_misses && s.hits == 0 && s.saved == 0)
        .collect()
}

/// The result of reading the window.
#[derive(Debug, Default)]
pub struct Window {
    /// Every report found.
    pub seen: Seen,
    /// Runs read.
    pub runs: Vec<u64>,
    /// Jobs whose log was read.
    pub logs_read: usize,
    /// Jobs whose log could not be read, with the reason.
    pub unreadable: Vec<String>,
}

/// Reads the last `runs` finished runs of `workflow` (newest first, the first page of the run list) and the report lines of each job's log.
pub fn read(client: &mut Client, workflow: &str, runs: usize) -> Result<Window, String> {
    let page = client.runs_page(1)?;
    let mut w = Window::default();
    for run in page
        .runs
        .iter()
        .filter(|r| r.workflow() == workflow && r.finished() && r.conclusion != "cancelled")
        .take(runs)
    {
        w.runs.push(run.id);
        let Some(jobs) = client.timed_jobs(run.id)? else {
            w.unreadable.push(format!("run {}: no jobs listed", run.id));
            continue;
        };
        for job in jobs.iter().filter(|j| !j.is_caller() && !j.never_ran()) {
            match client.log(job.id) {
                Ok(Some(text)) => {
                    w.logs_read += 1;
                    w.seen.extend(parse(&text).into_iter().map(|r| (run.id, job.id, r)));
                }
                Ok(None) => w.unreadable.push(format!("run {} job {} ({}): no log", run.id, job.id, job.name)),
                Err(e) => w.unreadable.push(format!("run {} job {} ({}): {e}", run.id, job.id, job.name)),
            }
        }
    }
    Ok(w)
}

/// The text and the verdict of an audit: findings, "check broken" (no report line at all) or clean.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Keys never saved.
    NeverSaved(Vec<String>),
    /// No `cache-report:` line in any log: the check cannot see.
    Broken(String),
    /// Every key that missed was saved or hit.
    Clean,
}

/// Judges a window.
pub fn judge(w: &Window, min_misses: usize) -> Verdict {
    if w.runs.is_empty() {
        return Verdict::Broken("no finished run of the workflow was found in the first page of the run list".into());
    }
    if w.seen.is_empty() {
        return Verdict::Broken(format!(
            "no `cache-report:` line in {} job log(s) of {} run(s): the cache steps do not report, or the logs lost the lines",
            w.logs_read,
            w.runs.len()
        ));
    }
    let by = stats(&w.seen);
    let bad: Vec<String> = never_saved(&by, min_misses)
        .into_iter()
        .map(|(k, s)| format!("{} cache `{k}`: {} miss(es), no exact hit, no confirmed save", s.name, s.misses))
        .collect();
    if bad.is_empty() { Verdict::Clean } else { Verdict::NeverSaved(bad) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(name: &str, outcome: &str, saved: &str, key: &str) -> Report {
        Report {
            name: name.into(),
            outcome: outcome.into(),
            saved: saved.into(),
            key: key.into(),
        }
    }

    #[test]
    fn parses_the_line_the_script_prints_through_a_timestamp() {
        let log = "2026-11-22T10:00:00.1Z cache-report: name=dev-tool outcome=miss saved=no key=dev-tool-v1-Linux-X64-abc\nother line\n";
        assert_eq!(parse(log), vec![rep("dev-tool", "miss", "no", "dev-tool-v1-Linux-X64-abc")]);
    }

    #[test]
    fn a_half_line_is_not_a_report() {
        assert!(parse("cache-report: name=x outcome=miss\n").is_empty());
    }

    fn many(n: usize, outcome: &str, saved: &str) -> Seen {
        (0..n).map(|i| (1, i as u64, rep("dev-tool", outcome, saved, "k"))).collect()
    }

    #[test]
    fn a_key_missed_by_many_jobs_and_never_saved_is_named() {
        // Run 8576: 42 jobs, every one a miss and nobody saved.
        let by = stats(&many(42, "miss", "no"));
        let bad = never_saved(&by, 3);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].1.misses, 42);
    }

    #[test]
    fn one_saved_key_or_one_hit_is_not_a_finding() {
        let mut seen = many(5, "miss", "no");
        seen.push((1, 99, rep("dev-tool", "miss", "yes", "k")));
        assert!(never_saved(&stats(&seen), 3).is_empty(), "a confirmed save clears the key");
        let mut seen = many(5, "miss", "no");
        seen.push((2, 100, rep("dev-tool", "exact", "n/a", "k")));
        assert!(never_saved(&stats(&seen), 3).is_empty(), "a later exact hit proves it was saved");
    }

    #[test]
    fn a_pending_save_counts_once_per_run_not_per_job() {
        // One run, 10 jobs: the post steps may all still work. Two more runs with no hit: they did not.
        let mut seen: Seen = (0..10).map(|i| (1, i, rep("npm", "miss", "pending", "k"))).collect();
        assert!(never_saved(&stats(&seen), 3).is_empty());
        seen.push((2, 20, rep("npm", "miss", "pending", "k")));
        seen.push((3, 30, rep("npm", "partial", "pending", "k")));
        assert_eq!(never_saved(&stats(&seen), 3).len(), 1);
    }

    #[test]
    fn a_restore_that_this_job_never_saves_is_not_a_finding_and_a_save_report_clears_a_key() {
        let seen: Seen = (0..5).map(|i| (1, i, rep("bench", "partial", "n/a", "k"))).collect();
        assert!(never_saved(&stats(&seen), 3).is_empty(), "n/a is not expected to be saved");
        let mut seen = many(5, "miss", "no");
        seen.push((1, 9, rep("dev-tool", "save", "yes", "k")));
        assert!(never_saved(&stats(&seen), 3).is_empty());
        let seen: Seen = (0..4).map(|i| (1, i, rep("x", "save", "no", "k"))).collect();
        assert_eq!(never_saved(&stats(&seen), 3).len(), 1, "a save that keeps failing is named");
    }

    #[test]
    fn a_window_without_report_lines_is_broken_never_clean() {
        let w = Window {
            runs: vec![1],
            logs_read: 4,
            ..Window::default()
        };
        assert!(
            matches!(judge(&w, 3), Verdict::Broken(_)),
            "positive control: the check that sees nothing says so"
        );
        assert!(matches!(judge(&Window::default(), 3), Verdict::Broken(_)));
        let w = Window {
            runs: vec![1],
            logs_read: 4,
            seen: many(1, "exact", "n/a"),
            ..Window::default()
        };
        assert_eq!(judge(&w, 3), Verdict::Clean);
        let w = Window {
            runs: vec![1],
            logs_read: 44,
            seen: many(42, "miss", "no"),
            ..Window::default()
        };
        assert!(matches!(judge(&w, 3), Verdict::NeverSaved(v) if v.len() == 1));
    }
}
