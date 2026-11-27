//! The sinks built now: the run log and the stall report. (Output capture and Pitcrew are later sinks of the same stream.)

use crate::event::{RunEvent, Sink};
use paddock_throttle::capture::MAX_LOG_BYTES;
use paddock_throttle::clock::Clock;
use paddock_throttle::log::Logger;
use paddock_throttle::timefmt::{day_text, rfc3339_micros};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The folder of the shim's run log under throttle's runs folder: `<runs>/shim/<day>.log`, one line per run.
pub const AGENT: &str = "shim";

/// The record of one event: `<time> shim tool=cargo project=paddock start=<time> duration_ms=1234 exit=0`. A command line is never
/// in it (ER-27: arguments can carry secrets, and the tool and the project say what ran). `None` for an event the run log does not keep.
pub fn run_log_line(e: &RunEvent<'_>, now_us: u64) -> Option<String> {
    match e {
        RunEvent::Started { .. } => None,
        RunEvent::Ended {
            tool,
            project,
            start_us,
            duration,
            code,
        } => Some(format!(
            "{} shim tool={tool} project={project} start={} duration_ms={} exit={code}",
            rfc3339_micros(now_us),
            rfc3339_micros(*start_us),
            duration.as_millis()
        )),
        RunEvent::Stalled {
            tool,
            project,
            quiet,
            pid,
            ends,
        } => Some(format!(
            "{} shim-stall tool={tool} project={project} quiet_s={} pid={pid} ends={ends}",
            rfc3339_micros(now_us),
            quiet.as_secs()
        )),
    }
}

/// Appends [`run_log_line`]s to `<runs>/shim/<day>.log` and mirrors them into throttle's troubleshooting log.
pub struct RunLogSink {
    runs: PathBuf,
    clock: Arc<dyn Clock>,
    log: Arc<Logger>,
}

impl RunLogSink {
    /// A sink writing under `runs` (throttle's runs folder).
    pub fn new(runs: &Path, clock: Arc<dyn Clock>, log: Arc<Logger>) -> RunLogSink {
        RunLogSink {
            runs: runs.to_path_buf(),
            clock,
            log,
        }
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        let dir = self.runs.join(AGENT);
        std::fs::create_dir_all(&dir)?;
        let file = dir.join(format!("{}.log", day_text(self.clock.now_secs())));
        if std::fs::metadata(&file).is_ok_and(|m| m.len() >= MAX_LOG_BYTES) {
            return Ok(());
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)?;
        writeln!(f, "{line}")
    }
}

impl Sink for RunLogSink {
    fn event(&mut self, e: &RunEvent<'_>) {
        let Some(line) = run_log_line(e, self.clock.now_micros()) else {
            return;
        };
        // The run is never failed by its record: a write error is logged and the run goes on.
        if let Err(err) = self.append(&line) {
            self.log.warn(
                "throttle.shim",
                "could not write the run log line",
                &[
                    ("Error", json!(err.to_string())),
                    ("Folder", json!(self.runs.join(AGENT).to_string_lossy())),
                ],
            );
        }
        self.log.info("throttle.shim", &line, &[]);
    }
}

/// Says a stall on the terminal (standard error, where the user is) and logs it. Nothing else is said by the shim.
pub struct StallReport {
    say: Box<dyn Fn(&str)>,
    log: Arc<Logger>,
}

impl StallReport {
    /// A report that speaks through `say`.
    pub fn new(say: Box<dyn Fn(&str)>, log: Arc<Logger>) -> StallReport {
        StallReport { say, log }
    }
}

impl Sink for StallReport {
    fn event(&mut self, e: &RunEvent<'_>) {
        let RunEvent::Stalled {
            tool,
            project,
            quiet,
            pid,
            ends,
        } = e
        else {
            return;
        };
        let what = if *ends {
            "ending it"
        } else {
            "not ending it (PADDOCK_SHIM_STALL_ACTION=end would)"
        };
        let msg = format!(
            "{tool} ({project}, pid {pid}) used no CPU for {} s and may be stuck; {what}",
            quiet.as_secs()
        );
        (self.say)(&msg);
        self.log.warn(
            "throttle.shim",
            &msg,
            &[
                ("QuietSeconds", json!(quiet.as_secs())),
                ("Pid", json!(pid)),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_ended_run_is_one_line_with_the_tool_project_start_duration_and_exit() {
        let e = RunEvent::Ended {
            tool: "cargo",
            project: "paddock",
            start_us: 1_000_000,
            duration: Duration::from_millis(1234),
            code: 101,
        };
        let l = run_log_line(&e, 3_000_000).unwrap();
        assert!(l.contains("shim tool=cargo project=paddock "), "{l}");
        assert!(l.contains("duration_ms=1234 exit=101"), "{l}");
        assert!(l.starts_with("1970-01-01T00:00:03"), "{l}");
        assert!(!l.contains('\n'));
    }

    #[test]
    fn a_stall_is_a_line_and_a_start_is_none() {
        let s = RunEvent::Stalled {
            tool: "npm",
            project: "p",
            quiet: Duration::from_secs(600),
            pid: 7,
            ends: false,
        };
        assert!(
            run_log_line(&s, 0)
                .unwrap()
                .contains("shim-stall tool=npm project=p quiet_s=600 pid=7 ends=false")
        );
        assert!(
            run_log_line(
                &RunEvent::Started {
                    tool: "npm",
                    project: "p",
                    start_us: 0
                },
                0
            )
            .is_none()
        );
    }
}
