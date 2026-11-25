//! `dev ci sccache-stats`: prints `sccache --show-stats` (the hit rate) in the job log and stops the job's own server (the port
//! of the `sccache-stats` action; the last step of a job, with `if: always()`). Does nothing when sccache is absent. A server on
//! the default port is never stopped: only CI's own server, whose port setup-rust sets (`SCCACHE_SERVER_PORT`).
//!
//! sccache gets `SCCACHE_*` only (the server's port and settings), never the job's tokens ([`ToolCall`]). Never fails the job.

use super::{Step, on_path};
use crate::ci_tool::ToolCall;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

const STATS_TIMEOUT: Duration = Duration::from_secs(120);
/// How many lines of the server's error log the job log shows.
const ERROR_LOG_LINES: usize = 20;

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, out: &mut dyn Write) -> i32 {
    run_with(env, out, crate::sccache_server::PROBE_LIMIT)
}

/// [`run`] with the probe limit named (a test passes a short one).
fn run_with(env: &HashMap<String, String>, out: &mut dyn Write, probe_limit: Duration) -> i32 {
    let mut step = Step {
        env,
        out,
        windows: cfg!(windows),
        name: "sccache-stats",
    };
    if !on_path(env, "sccache") {
        return 0;
    }
    // The probe setup-rust and the agents' local server use (crate::sccache_server): a hung server takes the connection and never
    // answers, so the statistics are asked for within the probe limit, not the step's.
    let stats = ToolCall::new("sccache").args(["--show-stats"]).pass(["SCCACHE_*"]);
    match crate::sccache_server::probe(&stats, env, probe_limit) {
        Ok(out) => step.say(out.trim_end()),
        // The words of the shell step (conformance/ci/host/sccache-stats.sh), which the parity test holds this to.
        Err(_) => step.say("::notice::sccache statistics are unavailable (the cache server was not reachable)."),
    }
    // What the server logged as an error (setup-rust puts it in the job's temp folder when the S3 level is on): a failed S3 write
    // never fails a build, so this is where it shows (ER-27).
    if let Some(log) = step.var("SCCACHE_ERROR_LOG")
        && let Ok(text) = std::fs::read_to_string(log)
        && !text.trim().is_empty()
    {
        let lines: Vec<&str> = text.trim_end().lines().collect();
        let from = lines.len().saturating_sub(ERROR_LOG_LINES);
        step.say(format!(
            "::warning::sccache's error log {log} is not empty ({} lines); the last {}:\n{}",
            lines.len(),
            lines.len() - from,
            lines[from..].join("\n")
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
    if step.var("SCCACHE_SERVER_PORT").is_some() {
        let stop = ToolCall::new("sccache")
            .args(["--stop-server"])
            .pass(["SCCACHE_*"])
            .timeout(STATS_TIMEOUT);
        let mut log = Vec::new();
        let _ = stop.run(env, &mut log);
        step.out.write_all(&log).ok();
    }
    0
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
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).stdout("stats"))
            .install()
            .unwrap();
        let (_, _, calls) = go(d.path(), &[]);
        assert_eq!(
            calls, "sccache --show-stats port= token=\n",
            "a server on the default port is left running"
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
        let (code, out, _) = go(d.path(), &[]);
        assert_eq!(code, 0);
        assert!(
            out.contains("::notice::sccache statistics are unavailable (the cache server was not reachable)."),
            "{out}"
        );
        let d = crate::ci_host::canonical_tempdir();
        let (code, out, calls) = go(d.path(), &[]);
        assert_eq!((code, out.as_str(), calls.as_str()), (0, "", ""));
    }

    /// A hung server (its statistics never come) holds the step only for the probe limit, then the notice; the positive control is
    /// the first test above, where the statistics come.
    #[test]
    fn a_hung_server_holds_the_step_only_for_the_probe_limit() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, CALL_LINE).sleeps(Duration::from_secs(30)))
            .install()
            .unwrap();
        let env: HashMap<String, String> = [("PATH".to_string(), format!("{}:/usr/bin:/bin", d.path().display()))].into();
        let mut out = Vec::new();
        assert_eq!(run_with(&env, &mut out, Duration::from_millis(500)), 0);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("statistics are unavailable"), "{out}");
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
