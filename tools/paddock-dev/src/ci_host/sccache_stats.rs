//! `dev ci sccache-stats`: prints `sccache --show-stats` (the hit rate) in the job log and stops the job's own server (the port
//! of the `sccache-stats` action; the last step of a job, with `if: always()`). Does nothing when sccache is absent. A server on
//! the default port is never stopped: only CI's own server, whose port setup-rust sets (`SCCACHE_SERVER_PORT`).
//!
//! A job whose cache missed everywhere gets one `::warning::` ([`miss_warning`]; report-only until [`MISS_WARNING_REVIEW_BY`]).
//! A job on a kept build folder whose few executed requests all missed gets one `::notice::` ([`warm_label`]): its 0% is not a fault.
//!
//! sccache gets `SCCACHE_*` only (the server's port and settings), never the job's tokens ([`ToolCall`]). Never fails the job.

use super::{Step, on_path};
use crate::ci_tool::ToolCall;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

const STATS_TIMEOUT: Duration = Duration::from_secs(120);
/// How many lines of the server's error log the job log shows.
const ERROR_LOG_LINES: usize = 20;

/// ER-41: the warning below is report-only and is reviewed by this date (ER-41, user 2026-11-25): if no real run showed it by then it is
/// deleted, not extended; one that fired on a cold job stays only with the user's yes. It is recorded here and in the warning's own text, not in
/// docs/gates.toml: the register holds checks (C100 names an entry for anything else a nonexistent check, and one for the gated
/// `action:sccache-stats` a stale entry).
pub const MISS_WARNING_REVIEW_BY: &str = "2026-12-09";

/// The most S3 (L1) misses a job may have with no hit at any level before [`miss_warning`] speaks: a small job misses a few crates
/// by itself (jobs 50162 and 50136, 2026-11-25: 62 and 90 misses; only the second had no hit at all).
const MISS_WARNING_ABOVE: u64 = 20;

/// The value of the stats line that starts with exactly `label` (`Cache hits   277`, not `Cache hits rate   82.69 %` or
/// `Cache hits (Rust)   271`), as `sccache --show-stats` prints it. `None` when no such line has a whole number.
fn stat(text: &str, label: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.trim_start().strip_prefix(label).and_then(|rest| rest.trim().parse().ok()))
}

/// The report-only miss warning (ER-21: the third time a changing path in the cache keys broke sccache; runs 9054, 9065 and 9066
/// had 0 hits of 90 on a job that moved to another slot): `Some(line)` when no level had a hit, the S3 level (L1) had 0 hits and
/// more than [`MISS_WARNING_ABOVE`] misses. A job whose disk level (L0) hit is warm even though L1 never sees those requests (job 50162:
/// L0 277 hits, L1 0 hits and 62 misses), so L1 alone would warn on a healthy job. `None` when the text has no such counters (not
/// measured: never a pass or a failure, ER-33) or the job hit.
pub(super) fn miss_warning(stats: &str, leg: &str, slot: &str) -> Option<String> {
    let (hits, l1_hits, l1_misses) = (
        stat(stats, "Cache hits")?,
        stat(stats, "L1 (s3) hits")?,
        stat(stats, "L1 (s3) misses")?,
    );
    (hits == 0 && l1_hits == 0 && l1_misses > MISS_WARNING_ABOVE).then(|| {
        format!(
            "::warning::sccache-stats: leg {leg} (slot {slot}): 0 cache hits, S3 (L1) 0 hits and {l1_misses} misses; not judged: the counters cannot tell a \
             cold cache, a change cascade (first-party crates downstream of a changed crate miss by design) or a key fault (a changing \
             path in the keys) apart. Report-only, reviewed by {MISS_WARNING_REVIEW_BY}."
        )
    })
}

/// The most requests a job may execute and still be a warm job (the miss warning above speaks over [`MISS_WARNING_ABOVE`] misses).
const WARM_EXECUTED_AT_MOST: u64 = MISS_WARNING_ABOVE;

/// The label of a job on a kept build folder whose hit rate reads 0% and is not a fault (run 9248: the Mac and Windows paddock-dev legs,
/// 3 requests executed, 101 non-cacheable calls): cargo asks rustc only for the crates that changed, so the few requests sccache
/// executed are the changed first-party crates, all misses, and the rest were fresh in the kept folder. `Some(notice line)` when no
/// request hit and 1 to [`WARM_EXECUTED_AT_MOST`] were executed, all of them misses. `None` when the counters are missing (not
/// measured, ER-33), a request hit (the rate speaks for itself), nothing executed, or more executed than a warm job needs (a cold
/// job: [`miss_warning`]'s case).
pub(super) fn warm_label(stats: &str) -> Option<String> {
    let (executed, hits, misses) = (
        stat(stats, "Compile requests executed")?,
        stat(stats, "Cache hits")?,
        stat(stats, "Cache misses")?,
    );
    let probes = stat(stats, "Non-cacheable calls").unwrap_or(0);
    (hits == 0 && misses == executed && (1..=WARM_EXECUTED_AT_MOST).contains(&executed)).then(|| {
        format!(
            "::notice::sccache-stats: warm build folder: {executed} executed, all changed first-party crates; the rest fresh in the kept folder \
             ({probes} non-cacheable calls are not compiles). A hit rate of 0% here is not a fault."
        )
    })
}

/// How much of the end of the server's error log is read for the causes ([`error_causes`]): bounded, as a bad night writes one line per
/// refused write.
const ERROR_LOG_READ_BYTES: u64 = 64 * 1024;
/// The longest a cause is shown.
const CAUSE_CHARS: usize = 200;

/// A signed URL loses its query, a secret-named value is replaced: what the log lines show never carries a signature or a key.
fn masked(line: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;
    // The patterns are literals of this file, run by the tests below.
    #[allow(clippy::expect_used)]
    fn re(pattern: &str) -> Regex {
        Regex::new(pattern).expect("a valid pattern")
    }
    static QUERY: LazyLock<Regex> = LazyLock::new(|| re(r#"(https?://[^\s"'<>?,)}]+)\?[^\s"'<>,)}]*"#));
    static SECRET: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r#"(?i)((?:secret|token|password|signature|credential|authorization|access[_-]?key)[a-z_-]*["']?\s*[=:]\s*)("[^"]*"|[^\s,&"'}]+)"#,
        )
    });
    let no_query = QUERY.replace_all(line, "${1}?...");
    SECRET.replace_all(&no_query, "${1}***").into_owned()
}

/// The cause one error-log line names: the store's own message (opendal's `message: "..."`, or an XML `<Message>`), else the line after
/// its level word, both masked and cut. `None` for a blank line.
fn cause_of(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let between = |from: &str, to: &str| {
        line.split_once(from)
            .and_then(|(_, r)| r.split_once(to))
            .map(|(m, _)| m.to_string())
    };
    let cause = between("message: \\\"", "\\\"")
        .or_else(|| between("message: \"", "\""))
        .or_else(|| between("<Message>", "</Message>"))
        .unwrap_or_else(|| {
            ["ERROR ", "WARN "]
                .iter()
                .find_map(|lvl| line.split_once(lvl).map(|(_, r)| r))
                .unwrap_or(line)
                .to_string()
        });
    Some(masked(&cause).chars().take(CAUSE_CHARS).collect())
}

/// The distinct causes in the server's error log, most frequent first (ties in first-seen order), each with its count.
pub(super) fn error_causes(log: &str) -> Vec<(String, usize)> {
    let mut seen: Vec<(String, usize)> = Vec::new();
    for cause in log.lines().filter_map(cause_of) {
        match seen.iter_mut().find(|(c, _)| *c == cause) {
            Some((_, n)) => *n += 1,
            None => seen.push((cause, 1)),
        }
    }
    seen.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    seen
}

/// ER-27, ER-33: the one warning for a job whose S3 level (L1) stored nothing although it tried (runs 9381 and 9388: RustFS answered
/// every PUT with "Bucket quota exceeded" and the log said only "writes 0"). `Some(line)` when L1 writes are 0 and its write failures
/// are above 0; the top cause comes from the error log (`log` is its path when the job has one). `None` when the counters are missing
/// (not measured) or some write went through.
pub(super) fn write_refusal_warning(stats: &str, causes: &[(String, usize)], log: Option<&str>) -> Option<String> {
    let (writes, failures) = (stat(stats, "L1 (s3) writes")?, stat(stats, "L1 (s3) write failures")?);
    if writes != 0 || failures == 0 {
        return None;
    }
    let tail = match (causes.first(), log) {
        (Some((cause, _)), _) if cause.to_lowercase().contains("quota") => {
            format!("{cause}; free space or raise the quota on the sccache bucket")
        }
        (Some((cause, _)), _) => format!("{cause}; check the S3 store (bucket, key policy, server)"),
        (None, Some(log)) => {
            format!("cause unknown: the error log {log} has no line about it; check the S3 store (bucket, quota, key policy)")
        }
        (None, None) => "cause unknown: no error log (set SCCACHE_ERROR_LOG, which setup-rust sets when the S3 level is on)".to_string(),
    };
    Some(format!("::warning::sccache: {failures} S3 writes refused: {tail}"))
}

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, out: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows: cfg!(windows),
        name: "sccache-stats",
    };
    if !on_path(env, "sccache") {
        return 0;
    }
    // ER-33, ER-51: only the job's own server is asked, the one whose port setup-rust set. Without it (the server did not start, or setup-rust
    // never ran) the default port may hold a developer's server on a host runner, whose numbers would print as the job's.
    let cause = match step.var(crate::ci_setup_rust::STATE_VAR).and_then(|v| v.strip_prefix("off:")) {
        Some(c) => Some(c.to_string()),
        None if step.var("SCCACHE_SERVER_PORT").is_none() => Some("setup-rust set no server port".to_string()),
        None => None,
    };
    if let Some(cause) = cause {
        step.say(format!("not measured: the job's server did not start ({cause})"));
        job_end(env, &mut step, false);
        return 0;
    }
    let mut log = Vec::new();
    let stats = ToolCall::new("sccache")
        .args(["--show-stats"])
        .pass(["SCCACHE_*"])
        .timeout(STATS_TIMEOUT);
    let ran = stats.run(env, &mut log);
    step.out.write_all(&log).ok();
    match ran {
        Ok(r) if r.success => {
            step.say(r.stdout.trim_end());
            let leg = format!(
                "{}/{}",
                step.var("GITHUB_WORKFLOW").unwrap_or("?"),
                step.var("GITHUB_JOB").unwrap_or("?")
            );
            if let Some(line) = warm_label(&r.stdout) {
                step.say(line);
            }
            if let Some(line) = miss_warning(&r.stdout, &leg, step.var("PADDOCK_CI_SLOT").unwrap_or("none")) {
                step.say(line);
            }
            let log = step.var("SCCACHE_ERROR_LOG").map(str::to_string);
            let causes = log
                .as_deref()
                .map(|l| error_causes(&crate::ci_tool::read_tail(Path::new(l), ERROR_LOG_READ_BYTES)))
                .unwrap_or_default();
            if let Some(line) = write_refusal_warning(&r.stdout, &causes, log.as_deref()) {
                step.say(line);
            }
        }
        _ => step.say("::notice::sccache statistics are unavailable (the cache server was not reachable)."),
    }
    job_end(env, &mut step, true);
    0
}

/// What every job ends with, measured or not: the server's error log, the S3 guard's log and its stop, then the job's own server's stop
/// (only when `stop_server`: a port in the environment of a job whose server never started is not this job's).
fn job_end(env: &HashMap<String, String>, step: &mut Step, stop_server: bool) {
    // What the server logged as an error (setup-rust puts it in the job's temp folder when the S3 level is on): a failed S3 write
    // never fails a build, so this is where it shows (ER-27).
    if let Some(log) = step.var("SCCACHE_ERROR_LOG")
        && let Ok(text) = std::fs::read_to_string(log)
        && !text.trim().is_empty()
    {
        let lines: Vec<&str> = text.trim_end().lines().collect();
        let from = lines.len().saturating_sub(ERROR_LOG_LINES);
        let causes = error_causes(&crate::ci_tool::read_tail(Path::new(log), ERROR_LOG_READ_BYTES))
            .iter()
            .map(|(c, n)| format!("  (x{n}) {c}"))
            .collect::<Vec<_>>()
            .join("\n");
        step.say(format!(
            "::warning::sccache's error log {log} is not empty ({} lines); the distinct causes:\n{causes}\nthe last {}:\n{}",
            lines.len(),
            lines.len() - from,
            lines[from..].iter().map(|l| masked(l)).collect::<Vec<_>>().join("\n")
        ));
    }
    // The S3 timeout guard (setup-rust starts it when the S3 level is on): what it cut, then its stop.
    if let Some(log) = step.var("PADDOCK_S3_GUARD_LOG").map(str::to_string)
        && let Ok(text) = std::fs::read_to_string(&log)
        && !text.trim().is_empty()
    {
        // An idle keep-alive connection closed after the stall is routine (trial run 8363: 185 of them, no request cut): counted in a
        // notice. A cut request or a refused connection is what a person must see: a warning with the last lines.
        let (quiet, cuts): (Vec<&str>, Vec<&str>) = text.trim_end().lines().partition(|l| l.contains("was quiet for"));
        if !quiet.is_empty() {
            step.say(format!(
                "::notice::the S3 timeout guard closed {} idle connection(s), as designed; no request was cut",
                quiet.len()
            ));
        }
        if !cuts.is_empty() {
            let from = cuts.len().saturating_sub(ERROR_LOG_LINES);
            step.say(format!(
                "::warning::the S3 timeout guard cut or refused requests ({} lines, {log}); the last {}:\n{}",
                cuts.len(),
                cuts.len() - from,
                cuts[from..].join("\n")
            ));
        }
    }
    // The guard and the server that holds the S3 secret end here (ci_s3_guard::stop_job_cache; `ci isolation release` is the net).
    crate::ci_s3_guard::stop_job_cache(env, step.out);
    // Only CI's own server (setup-rust sets its port); a server on the default port is never stopped.
    if stop_server && step.var("SCCACHE_SERVER_PORT").is_some() {
        let stop = ToolCall::new("sccache")
            .args(["--stop-server"])
            .pass(["SCCACHE_*"])
            .timeout(STATS_TIMEOUT);
        let mut log = Vec::new();
        let _ = stop.run(env, &mut log);
        step.out.write_all(&log).ok();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use paddock_testkit::{Actions, StandIn};
    use std::path::Path;

    /// What every fake `sccache` appends to `calls` for a call: the arguments and the two variables that must (not) arrive.
    const CALL_LINE: &str = "sccache {args} port={env:SCCACHE_SERVER_PORT|} token={env:GITHUB_TOKEN|}\n";

    fn go(dir: &Path, vars: &[(&str, &str)]) -> (i32, String, String) {
        let mut env: HashMap<String, String> = [
            ("PATH".to_string(), format!("{}:/usr/bin:/bin", dir.display())),
            ("GITHUB_TOKEN".into(), "secret".into()),
        ]
        .into();
        env.extend(vars.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let mut out = Vec::new();
        let code = run(&env, &mut out);
        (
            code,
            String::from_utf8(out).unwrap(),
            std::fs::read_to_string(dir.join("calls")).unwrap_or_default(),
        )
    }

    /// The counters of job 50136's stats (a Mac job on a slot it did not hold last time; run 9054), as sccache prints them, without its
    /// cache location lines (those carry machine paths, ER-46).
    const COLD: &str = "Compile requests                    415
Cache hits                            0
Cache misses                         90
Cache hits rate                    0.00 %
Multi-level cache levels              2
  L0 (disk) hits                      0
  L0 (disk) misses                   90
  L1 (s3) hits                        0
  L1 (s3) misses                     90
  L1 (s3) hit rate                 0.00 %
";
    /// The counters of job 50162's stats (a Windows job on its slot): the disk level answered, so the S3 level saw only the misses.
    const WARM: &str = "Compile requests                    513
Cache hits                          277
Cache hits (Rust)                   271
Cache misses                         58
Cache hits rate                  82.69 %
Multi-level cache levels              2
  L0 (disk) hits                    277
  L0 (disk) misses                   62
  L1 (s3) hits                        0
  L1 (s3) misses                     62
  L1 (s3) hit rate                 0.00 %
";

    #[test]
    fn a_job_with_no_hit_at_any_level_and_many_s3_misses_gets_one_warning_naming_leg_and_slot() {
        let w = miss_warning(COLD, "CI/crate", "s2").unwrap();
        assert!(w.starts_with("::warning::") && !w.contains('\n'), "{w}");
        assert!(
            w.contains("leg CI/crate") && w.contains("slot s2") && w.contains("90 misses"),
            "{w}"
        );
        assert!(
            w.contains("not judged") && w.contains("change cascade") && !w.contains("cache keys may contain"),
            "no key fault is claimed from counters alone: {w}"
        );
        assert!(w.contains(MISS_WARNING_REVIEW_BY), "the review date is in the line: {w}");
    }

    /// The counters of a Mac paddock-dev leg on its kept build folder (run 9248: 3 requests executed, 101 non-cacheable calls): only
    /// the 2 changed first-party crates and the tool's own crate compiled, so the hit rate reads 0% and is not a fault. As sccache
    /// 0.20.1 prints them, without the cache location lines (machine paths, ER-46).
    const KEPT_FOLDER: &str = "Compile requests                    104
Compile requests executed             3
Cache hits                            0
Cache misses                          3
Cache hits rate                    0.00 %
Cache timeouts                        0
Compilations                          3
Compilation failures                  0
Non-cacheable compilations            0
Non-cacheable calls                 101
Non-compilation calls                 0
Multi-level cache levels              2
  L0 (disk) hits                      0
  L0 (disk) misses                    3
  L1 (s3) hits                        0
  L1 (s3) misses                      3
";

    #[test]
    fn a_zero_percent_job_with_few_executed_requests_is_labelled_warm_not_a_fault() {
        let l = warm_label(KEPT_FOLDER).unwrap();
        assert!(l.starts_with("::notice::sccache-stats:") && !l.contains('\n'), "{l}");
        assert!(
            l.contains("3 executed") && l.contains("changed first-party crates") && l.contains("fresh in the kept"),
            "{l}"
        );
        assert!(l.contains("101 non-cacheable"), "the probes are named so 0% is read right: {l}");
        // The label and the miss warning never both speak: the warning needs more than 20 misses, the label at most 20 executed.
        assert_eq!(miss_warning(KEPT_FOLDER, "CI/paddock-dev", "s1"), None);
    }

    #[test]
    fn a_cold_job_a_hitting_job_and_unreadable_counters_get_no_warm_label() {
        // Cold: 90 requests executed, none hit: the miss warning's case, never labelled warm.
        let cold = KEPT_FOLDER
            .replace("executed             3", "executed            90")
            .replace("misses                          3", "misses                         90");
        assert_eq!(warm_label(&cold), None, "90 executed is a cold job");
        let twenty = KEPT_FOLDER
            .replace("executed             3", "executed            20")
            .replace("misses                          3", "misses                         20");
        assert!(warm_label(&twenty).is_some(), "20 is not above the bound");
        let hit = KEPT_FOLDER.replace("Cache hits                            0", "Cache hits                            2");
        assert_eq!(warm_label(&hit), None, "a job that hit is warm by its rate");
        let nothing = KEPT_FOLDER.replace("executed             3", "executed             0");
        assert_eq!(warm_label(&nothing), None, "nothing executed says nothing");
        // Old counters without the executed line (COLD/WARM above) and garbage: not measured (ER-33).
        for text in [COLD, WARM, "", "Compile requests executed   x\nCache hits  0\nCache misses 3"] {
            assert_eq!(warm_label(text), None, "not measured: {text:?}");
        }
    }

    #[test]
    fn the_step_prints_the_warm_label_after_the_statistics_and_exits_0() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout(KEPT_FOLDER))
            .install()
            .unwrap();
        let (code, out, _) = go(d.path(), &[("SCCACHE_SERVER_PORT", "4501")]);
        assert_eq!(code, 0);
        assert_eq!(out.matches("::notice::sccache-stats: warm").count(), 1, "{out}");
        assert!(out.find("Compile requests").unwrap() < out.find("::notice::sccache-stats").unwrap());
        assert!(!out.contains("::warning::sccache-stats"), "{out}");
    }

    #[test]
    fn a_warm_job_a_small_job_and_unreadable_counters_get_no_warning() {
        assert_eq!(
            miss_warning(WARM, "CI/crate", "s1"),
            None,
            "L0 hit 277 times: warm, though L1 never hit"
        );
        let hit = COLD.replace("L1 (s3) hits                        0", "L1 (s3) hits                       30");
        assert_eq!(miss_warning(&hit, "CI/crate", "s1"), None, "an S3 hit");
        let few = COLD.replace("L1 (s3) misses                     90", "L1 (s3) misses                     20");
        assert_eq!(miss_warning(&few, "CI/crate", "s1"), None, "20 misses is not above 20");
        assert!(
            miss_warning(
                &few.replace("misses                     20", "misses                     21"),
                "CI/crate",
                "s1"
            )
            .is_some()
        );
        for text in ["", "Compile requests 4", "Cache hits   x\n  L1 (s3) hits  0\n  L1 (s3) misses  90"] {
            assert_eq!(miss_warning(text, "CI/crate", "s1"), None, "not measured: {text:?}");
        }
        assert_eq!(stat(COLD, "Cache hits"), Some(0), "the rate line is not the count");
    }

    #[test]
    fn the_step_prints_the_warning_after_the_statistics_and_still_ends_with_exit_0() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout(COLD))
            .install()
            .unwrap();
        let (code, out, _) = go(
            d.path(),
            &[
                ("GITHUB_WORKFLOW", "CI"),
                ("GITHUB_JOB", "paddock-dev"),
                ("PADDOCK_CI_SLOT", "s2"),
                ("SCCACHE_SERVER_PORT", "4501"),
            ],
        );
        assert_eq!(code, 0, "report-only: it never fails the step");
        assert_eq!(out.matches("::warning::sccache-stats").count(), 1, "{out}");
        assert!(out.find("Compile requests").unwrap() < out.find("::warning::sccache-stats").unwrap());
        assert!(out.contains("leg CI/paddock-dev (slot s2)"), "{out}");
        let d = crate::ci_host::canonical_tempdir();
        StandIn::new(d.path(), "sccache")
            .answers(
                Actions::new()
                    .appends_with(&d.path().join("calls").display().to_string(), CALL_LINE)
                    .stdout(WARM),
            )
            .install()
            .unwrap();
        let (code, out, _) = go(d.path(), &[("SCCACHE_SERVER_PORT", "4501")]);
        assert_eq!(code, 0);
        assert!(!out.contains("::warning::sccache-stats"), "{out}");
    }

    #[test]
    fn the_statistics_are_printed_and_only_the_jobs_own_server_is_stopped() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout("Compile requests 10"))
            .install()
            .unwrap();
        let (code, out, calls) = go(d.path(), &[("SCCACHE_SERVER_PORT", "4501")]);
        assert_eq!(code, 0);
        assert!(out.contains("Compile requests 10"), "{out}");
        assert_eq!(
            calls, "sccache --show-stats port=4501 token=\nsccache --stop-server port=4501 token=\n",
            "the token never reaches sccache"
        );
        // No port of the job's own: never a call, so never the numbers of a server on the default port (a developer's or another job's).
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout("stats"))
            .install()
            .unwrap();
        let (code, out, calls) = go(d.path(), &[]);
        assert_eq!(code, 0);
        assert_eq!(calls, "", "sccache is not called without the job's port");
        assert!(
            out.contains("not measured: the job's server did not start (setup-rust set no server port)"),
            "{out}"
        );
        assert!(!out.contains("stats\n"), "no numbers are printed: {out}");
    }

    #[test]
    fn the_off_state_setup_rust_wrote_is_not_measured_with_its_cause_and_calls_nothing() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(
                Actions::new()
                    .appends_with(&calls, CALL_LINE)
                    .stdout("Compile requests 0\nCache hits 0"),
            )
            .install()
            .unwrap();
        // Even a port in the environment (a stale or foreign one) is not asked once the job's server is known not to have started.
        let (code, out, calls) = go(
            d.path(),
            &[("PADDOCK_SCCACHE_STATE", "off:start-failed-twice"), ("SCCACHE_SERVER_PORT", "4501")],
        );
        assert_eq!(code, 0, "it never fails the job");
        assert!(
            out.contains("not measured: the job's server did not start (start-failed-twice)"),
            "{out}"
        );
        assert_eq!(
            calls, "",
            "no --show-stats and no --stop-server: there is no server of this job to ask"
        );
        assert!(
            !out.contains("Compile requests"),
            "empty stats are never printed as a result: {out}"
        );
    }

    #[test]
    fn an_unreachable_server_is_a_notice_and_a_missing_sccache_is_silence() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).exit(2))
            .install()
            .unwrap();
        let (code, out, _) = go(d.path(), &[("SCCACHE_SERVER_PORT", "4501")]);
        assert_eq!(code, 0);
        assert!(
            out.contains("::notice::sccache statistics are unavailable (the cache server was not reachable)."),
            "{out}"
        );
        let d = crate::ci_host::canonical_tempdir();
        let (code, out, calls) = go(d.path(), &[]);
        assert_eq!((code, out.as_str(), calls.as_str()), (0, "", ""));
    }

    #[test]
    fn a_non_empty_error_log_is_shown_and_an_empty_one_is_not() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout("stats"))
            .install()
            .unwrap();
        let log = d.path().join("sccache-error.log");
        let log_s = log.display().to_string();
        std::fs::write(&log, "").unwrap();
        let (_, out, _) = go(d.path(), &[("SCCACHE_ERROR_LOG", &log_s)]);
        assert!(!out.contains("error log"), "an empty log says nothing: {out}");
        std::fs::write(&log, "S3 write failed: timeout\n").unwrap();
        let (code, out, _) = go(d.path(), &[("SCCACHE_ERROR_LOG", &log_s)]);
        assert_eq!(code, 0, "it never fails the job");
        assert!(
            out.contains("::warning::sccache's error log") && out.contains("S3 write failed: timeout"),
            "{out}"
        );
        let (_, out, _) = go(
            d.path(),
            &[("SCCACHE_ERROR_LOG", &d.path().join("absent.log").display().to_string())],
        );
        assert!(!out.contains("error log"), "a missing log says nothing: {out}");
    }

    /// What the job's counters read when RustFS refused every PUT (runs 9381 and 9388): 0 L1 writes, 897 write failures.
    const REFUSED: &str = "Cache hits                            0
Cache misses                         897
Multi-level cache levels              2
  L1 (s3) hits                        0
  L1 (s3) misses                     897
  L1 (s3) writes                      0
  L1 (s3) write failures            897
";
    /// Two refused writes as the server logs them (opendal's error text, with a signed URL and a key in it) and one other line.
    const QUOTA_LOG: &str = "2026-11-28T10:00:01Z ERROR sccache::cache::multilevel: Failed to write: Unexpected (persistent) at write, context: { uri: http://172.16.1.220:15000/sccache-paddock/paddock-ci/Linux-x86_64/a/b/key?X-Amz-Signature=deadbeef&X-Amz-Credential=AKIAEXAMPLEKEY12345, response: Parts { status: 400 }, service: s3 } => S3Error { code: \"InvalidRequest\", message: \"Bucket quota exceeded. Current usage: 107374182274 bytes, limit: 107374182400 bytes\", resource: \"/x\" }
2026-11-28T10:00:02Z ERROR sccache::cache::multilevel: Failed to write: Unexpected (persistent) at write, context: { uri: http://172.16.1.220:15000/sccache-paddock/paddock-ci/Linux-x86_64/c/d/key2?X-Amz-Signature=cafe, response: Parts { status: 400 }, service: s3 } => S3Error { code: \"InvalidRequest\", message: \"Bucket quota exceeded. Current usage: 107374182274 bytes, limit: 107374182400 bytes\", resource: \"/y\" }
2026-11-28T10:00:03Z ERROR sccache: connection reset by peer
";

    #[test]
    fn a_quota_refused_error_log_is_counted_by_cause_with_no_signature_or_key() {
        let causes = error_causes(QUOTA_LOG);
        assert_eq!(causes.len(), 2, "{causes:?}");
        assert_eq!(causes[0].1, 2, "the most frequent cause first: {causes:?}");
        assert!(
            causes[0]
                .0
                .starts_with("Bucket quota exceeded. Current usage: 107374182274 bytes, limit: 107374182400 bytes"),
            "{causes:?}"
        );
        let all = format!("{causes:?}");
        assert!(
            !all.contains("deadbeef") && !all.contains("AKIA") && !all.contains("X-Amz-Signature"),
            "no secret: {all}"
        );
    }

    #[test]
    fn s3_writes_of_zero_with_failures_get_one_warning_naming_the_quota() {
        let w = write_refusal_warning(REFUSED, &error_causes(QUOTA_LOG), Some("/tmp/e.log")).unwrap();
        assert!(w.starts_with("::warning::") && !w.contains('\n'), "{w}");
        assert!(
            w.contains("897 S3 writes refused: Bucket quota exceeded") && w.contains("free space or raise the quota on the sccache bucket"),
            "{w}"
        );
    }

    #[test]
    fn without_an_error_log_the_cause_is_unknown_and_the_setting_is_named() {
        let w = write_refusal_warning(REFUSED, &[], None).unwrap();
        assert!(
            w.contains("897 S3 writes refused") && w.contains("cause unknown") && w.contains("SCCACHE_ERROR_LOG"),
            "{w}"
        );
        let w = write_refusal_warning(REFUSED, &[], Some("/tmp/e.log")).unwrap();
        assert!(
            w.contains("cause unknown") && w.contains("/tmp/e.log"),
            "a log with no cause line: {w}"
        );
    }

    /// The positive control (ER-33): with writes that succeeded, or no failures, or no counters, the warning stays silent.
    #[test]
    fn healthy_or_unmeasured_s3_writes_get_no_refusal_warning() {
        let ok = REFUSED
            .replace("writes                      0", "writes                    500")
            .replace("failures            897", "failures              0");
        assert_eq!(write_refusal_warning(&ok, &error_causes(QUOTA_LOG), Some("l")), None);
        assert_eq!(
            write_refusal_warning("Cache hits 0\n", &[], None),
            None,
            "no counters: not measured"
        );
        let some = REFUSED.replace("writes                      0", "writes                     12");
        assert_eq!(write_refusal_warning(&some, &[], None), None, "some writes went through");
    }

    #[test]
    fn the_step_prints_the_refusal_warning_from_a_quota_error_log() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout(REFUSED))
            .install()
            .unwrap();
        let log = d.path().join("sccache-error.log");
        std::fs::write(&log, QUOTA_LOG).unwrap();
        let (code, out, _) = go(
            d.path(),
            &[("SCCACHE_SERVER_PORT", "4999"), ("SCCACHE_ERROR_LOG", &log.display().to_string())],
        );
        assert_eq!(code, 0);
        assert!(out.contains("897 S3 writes refused: Bucket quota exceeded"), "{out}");
        assert!(out.contains("(x2)"), "the distinct causes are listed with counts: {out}");
        assert!(!out.contains("deadbeef") && !out.contains("AKIAEXAMPLE"), "{out}");
        let (_, out, _) = go(d.path(), &[("SCCACHE_SERVER_PORT", "4999")]);
        assert!(out.contains("cause unknown") && out.contains("SCCACHE_ERROR_LOG"), "{out}");
    }

    #[test]
    fn the_s3_guard_is_stopped_and_what_it_cut_is_shown() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout("stats"))
            .install()
            .unwrap();
        // A listener stands in for the guard: it must receive the stop request.
        let guard = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = guard.local_addr().unwrap().port().to_string();
        let log = d.path().join("s3-guard.log");
        std::fs::write(&log, "s3-guard: the store did not answer within 10 s\n").unwrap();
        let log_s = log.display().to_string();
        let (code, out, _) = go(d.path(), &[("PADDOCK_S3_GUARD_PORT", &port), ("PADDOCK_S3_GUARD_LOG", &log_s)]);
        assert_eq!(code, 0, "it never fails the job");
        assert!(
            out.contains("::warning::the S3 timeout guard cut") && out.contains("did not answer within 10 s"),
            "{out}"
        );
        let (mut c, _) = guard.accept().unwrap();
        let mut got = Vec::new();
        std::io::Read::read_to_end(&mut c, &mut got).unwrap();
        assert_eq!(got, paddock_net::stall_guard::STOP_MAGIC, "the guard gets the stop request");
        // Only idle connections closed: a notice with the count, never a warning.
        std::fs::write(&log, "s3-guard: a connection to h:1 was quiet for 10 s and is closed\ns3-guard: a connection to h:1 was quiet for 10 s and is closed\n").unwrap();
        let (_, out, _) = go(d.path(), &[("PADDOCK_S3_GUARD_PORT", &port), ("PADDOCK_S3_GUARD_LOG", &log_s)]);
        assert!(out.contains("::notice::the S3 timeout guard closed 2 idle connection(s)"), "{out}");
        assert!(
            !out.contains("::warning::the S3 timeout guard"),
            "routine closes are not a warning: {out}"
        );
        let _ = guard.accept();
        // No guard variables: nothing is sent and nothing is said.
        let (_, out, _) = go(d.path(), &[]);
        assert!(!out.contains("timeout guard"), "{out}");
    }
}
