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
        }
        _ => step.say("::notice::sccache statistics are unavailable (the cache server was not reachable)."),
    }
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
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn sccache(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("sccache");
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\necho \"sccache $* port=${{SCCACHE_SERVER_PORT:-}} token=${{GITHUB_TOKEN:-}}\" >> '{}'\n{body}\n",
                dir.join("calls").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

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
        let d = tempfile::tempdir().unwrap();
        sccache(d.path(), "echo 'Compile requests 10'");
        let (code, out, calls) = go(d.path(), &[("SCCACHE_SERVER_PORT", "4501")]);
        assert_eq!(code, 0);
        assert!(out.contains("Compile requests 10"), "{out}");
        assert_eq!(
            calls, "sccache --show-stats port=4501 token=\nsccache --stop-server port=4501 token=\n",
            "the token never reaches sccache"
        );
        let d = tempfile::tempdir().unwrap();
        sccache(d.path(), "echo stats");
        let (_, _, calls) = go(d.path(), &[]);
        assert_eq!(
            calls, "sccache --show-stats port= token=\n",
            "a server on the default port is left running"
        );
    }

    #[test]
    fn an_unreachable_server_is_a_notice_and_a_missing_sccache_is_silence() {
        let d = tempfile::tempdir().unwrap();
        sccache(d.path(), "exit 2");
        let (code, out, _) = go(d.path(), &[]);
        assert_eq!(code, 0);
        assert!(
            out.contains("::notice::sccache statistics are unavailable (the cache server was not reachable)."),
            "{out}"
        );
        let d = tempfile::tempdir().unwrap();
        let (code, out, calls) = go(d.path(), &[]);
        assert_eq!((code, out.as_str(), calls.as_str()), (0, "", ""));
    }
}
