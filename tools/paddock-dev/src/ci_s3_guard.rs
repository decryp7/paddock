//! `dev ci s3-guard`: the command line around `paddock_net::stall_guard`, the localhost proxy in front of CI's S3 compile cache that
//! cuts a request the store does not answer (user decision 2026-11-20; docs/research/2026-11-19-sccache.md section 8). The guard
//! itself, its rules and its tests are the shared crate's (ER-22); this file only starts it, tells the starter its port and writes
//! its log to a file.

use clap::{Args, Subcommand};
use paddock_net::stall_guard::{Guard, Limits, Log};
use std::io::Write;
use std::sync::Arc;

/// `dev ci s3-guard`.
#[derive(Subcommand)]
pub enum GuardCommand {
    /// Run the guard: listens on a free port of 127.0.0.1, prints `PORT <n>` on standard output once, then forwards until stopped.
    /// Started detached by `ci setup-rust`. Not for people.
    #[command(hide = true)]
    Serve(ServeArgs),
    /// Stop the guard that listens on this port (sent by `ci sccache-stats`); does nothing when none answers.
    Stop(StopArgs),
}

/// `serve`'s arguments.
#[derive(Args)]
pub struct ServeArgs {
    /// `host:port` of the store.
    #[arg(long)]
    upstream: String,
    /// A file the guard appends one line to for every refusal, stall and trip (never a key); `sccache-stats` prints it.
    #[arg(long)]
    log: std::path::PathBuf,
}

/// `stop`'s arguments.
#[derive(Args)]
pub struct StopArgs {
    /// The guard's port on 127.0.0.1.
    #[arg(long)]
    port: u16,
}

/// `ci s3-guard serve`: binds a free port, tells the starter on standard output, forwards until stopped. Returns the exit code.
pub fn run_serve(a: &ServeArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let guard = match Guard::bind(a.upstream.clone(), Limits::default()) {
        Ok(g) => g,
        Err(e) => {
            let _ = writeln!(err, "::error::s3-guard: {e}. Next: the job's sccache runs on its local disk alone.");
            return 1;
        }
    };
    let log_path = a.log.clone();
    let log: Log = Arc::new(move |line: &str| {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(f, "s3-guard: {line}");
        }
    });
    // The starter reads this one line and lets go of the pipe; the guard writes nothing more to it.
    let _ = writeln!(out, "PORT {}", guard.port());
    let _ = out.flush();
    guard.serve(log);
    0
}

/// Sends the stop request to the guard on `port`. Never fails; a missing guard is not an error.
pub fn stop(port: u16) {
    let _ = paddock_net::stall_guard::stop(port);
}

/// Ends what the S3 level left running for this job: the timeout guard (its port is `PADDOCK_S3_GUARD_PORT`, set by `ci setup-rust`
/// only when the S3 level is on) and the job's own sccache server, whose environment holds the S3 secret until it ends (review of
/// 534918839, P2: a server that outlives its job is readable by a later job of the same user on a host runner). Called by
/// `ci sccache-stats` and, as the net under it, by `ci isolation release`, the last step of every host job (`if: always()`), so a job
/// whose stats step never ran still stops it. Does nothing, and runs nothing, when the S3 level was not on for this job; never fails.
pub fn stop_job_cache(env: &std::collections::HashMap<String, String>, out: &mut dyn Write) {
    let Some(port) = env.get("PADDOCK_S3_GUARD_PORT").and_then(|p| p.parse::<u16>().ok()) else {
        return;
    };
    stop(port);
    let mut log = Vec::new();
    let _ = crate::ci_tool::ToolCall::new("sccache")
        .args(["--stop-server"])
        .pass(["SCCACHE_*"])
        .timeout(std::time::Duration::from_secs(30))
        .run(env, &mut log);
    let _ = out.write_all(&log);
}

/// The command over the real process.
pub fn run(cmd: &GuardCommand, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match cmd {
        GuardCommand::Serve(a) => run_serve(a, out, err),
        GuardCommand::Stop(a) => {
            stop(a.port);
            0
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use paddock_testkit::{Actions, StandIn};
    use std::collections::HashMap;
    use std::io::Read;

    /// `stop_job_cache` ends the guard and the sccache server of an S3 job, and runs nothing for a job without the S3 level.
    #[test]
    fn the_job_end_stops_the_guard_and_the_server_only_for_an_s3_job() {
        let d = crate::ci_host::canonical_tempdir();
        let calls = d.path().join("calls").display().to_string();
        StandIn::new(d.path(), "sccache")
            .answers(Actions::new().appends_with(&calls, "sccache {args} port={env:SCCACHE_SERVER_PORT|}\n"))
            .install()
            .unwrap();
        let guard = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = guard.local_addr().unwrap().port().to_string();
        let env: HashMap<String, String> = [
            ("PATH".to_string(), format!("{}:/usr/bin:/bin", d.path().display())),
            ("SCCACHE_SERVER_PORT".to_string(), "4501".to_string()),
            ("PADDOCK_S3_GUARD_PORT".to_string(), port),
        ]
        .into();
        let mut out = Vec::new();
        stop_job_cache(&env, &mut out);
        assert_eq!(
            std::fs::read_to_string(d.path().join("calls")).unwrap(),
            "sccache --stop-server port=4501\n"
        );
        let (mut c, _) = guard.accept().unwrap();
        let mut got = Vec::new();
        c.read_to_end(&mut got).unwrap();
        assert_eq!(got, paddock_net::stall_guard::STOP_MAGIC, "the guard got the stop request");
        // A job without the S3 level: nothing runs (a local-disk server of another job is never stopped from here).
        std::fs::remove_file(d.path().join("calls")).unwrap();
        let plain: HashMap<String, String> = env
            .iter()
            .filter(|(k, _)| *k != "PADDOCK_S3_GUARD_PORT")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        stop_job_cache(&plain, &mut out);
        assert!(!d.path().join("calls").exists(), "no sccache call without the S3 level");
    }
}
