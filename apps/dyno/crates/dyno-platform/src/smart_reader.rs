//! The helper's `Smart` reader (ADR 0027 WP-8): `smartctl --scan-open -j`, then `smartctl -a -j [-d type] <device>` for devices
//! of that scan only, one at a time, each bounded by the runner's timeout. A disk that cannot be read is listed with the reason;
//! it never fails the other disks. Pure over a [`ToolRunner`] and a [`Clock`].

use crate::parse::smart::{parse_disk, parse_scan, SmartParseError};
use crate::runner::{tool, ToolRunner};
use dyno_core::helper_requests::{SmartQuery, MAX_SMART_DISKS};
use dyno_core::ports::PlatformError;
use dyno_core::smart::{SmartReader, SmartReply, SmartSkip};
use paddock_jobs::time::Clock;
use std::sync::{Arc, Mutex};

/// Reads SMART through a `smartctl` the helper chose (a protected path, never one a caller named).
pub struct SmartctlReader {
    smartctl: String,
    runner: Mutex<Box<dyn ToolRunner>>,
    clock: Arc<dyn Clock>,
}

impl SmartctlReader {
    /// A reader that runs `smartctl` (an absolute path in the helper's tools folder or `/usr/sbin`) through `runner`.
    pub fn new(smartctl: impl Into<String>, runner: Box<dyn ToolRunner>, clock: Arc<dyn Clock>) -> Self {
        SmartctlReader {
            smartctl: smartctl.into(),
            runner: Mutex::new(runner),
            clock,
        }
    }
}

impl SmartReader for SmartctlReader {
    fn read(&self, query: SmartQuery) -> Result<SmartReply, PlatformError> {
        let started = self.clock.monotonic();
        // One slot of the account holds this worker for every disk, so the runner stays locked for the whole read.
        let mut runner = self.runner.lock().unwrap_or_else(|p| p.into_inner());
        let scan_tool = tool(&self.smartctl, &["--scan-open", "-j"]);
        let scan = runner.run(&scan_tool).inspect_err(|e| {
            log::warn!(Tool = self.smartctl.as_str(), Error = e.to_string().as_str(); "smartctl scan did not run to completion");
        })?;
        let cap = usize::from(query.max_disks.min(MAX_SMART_DISKS));
        let found = parse_scan(&scan.stdout, usize::from(MAX_SMART_DISKS) + 1 + 4096);
        let (mut disks, mut skipped) = (Vec::new(), Vec::new());
        for dev in found.iter().take(cap) {
            let one = self.clock.monotonic();
            let mut args = vec!["-a", "-j"];
            if let Some(k) = dev.kind.as_deref() {
                args.extend(["-d", k]);
            }
            args.push(&dev.name);
            let outcome = match runner.run(&tool(&self.smartctl, &args)) {
                Ok(out) => parse_disk(&dev.name, &out.stdout, None).map_err(|e| match e {
                    SmartParseError::NotJson if !out.error.is_empty() => format!("smartctl said: {}", out.error),
                    other => other.to_string(),
                }),
                Err(e) => Err(format!("{e} Try again later.")),
            };
            let ms = self.clock.monotonic().saturating_sub(one).as_millis() as u64;
            match outcome {
                Ok(d) => {
                    log::info!(Ms = ms; "SMART read for one disk");
                    disks.push(d);
                }
                Err(reason) => {
                    log::warn!(Ms = ms, Reason = reason.as_str(); "SMART could not be read for one disk");
                    skipped.push(SmartSkip {
                        device: dev.name.chars().take(128).collect(),
                        reason,
                    });
                }
            }
        }
        let not_visited = found.len().saturating_sub(cap) as u32;
        log::info!(
            Disks = disks.len(),
            Skipped = skipped.len(),
            NotVisited = not_visited,
            Ms = self.clock.monotonic().saturating_sub(started).as_millis() as u64;
            "SMART read finished"
        );
        Ok(SmartReply {
            disks,
            skipped,
            not_visited,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Tool, ToolOutput};
    use paddock_jobs::time::FakeClock;
    use std::collections::VecDeque;
    use std::time::Duration;

    struct Script {
        answers: VecDeque<Result<ToolOutput, PlatformError>>,
        seen: Arc<Mutex<Vec<Tool>>>,
        clock: FakeClock,
    }
    impl ToolRunner for Script {
        fn run(&mut self, t: &Tool) -> Result<ToolOutput, PlatformError> {
            self.seen.lock().unwrap().push(t.clone());
            self.clock.advance(Duration::from_secs(2));
            self.answers.pop_front().expect("an unexpected extra run")
        }
    }

    fn out(stdout: &str, ok: bool) -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok,
            stdout: stdout.into(),
            error: String::new(),
        })
    }

    const GOOD: &str = r#"{"smartctl":{"exit_status":4},"model_name":"EX","serial_number":"SECRET1","smart_status":{"passed":true},"nvme_smart_health_information_log":{"percentage_used":1,"media_errors":0}}"#;

    fn reader(answers: Vec<Result<ToolOutput, PlatformError>>) -> (SmartctlReader, Arc<Mutex<Vec<Tool>>>, FakeClock) {
        let clock = FakeClock::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let script = Script {
            answers: answers.into(),
            seen: seen.clone(),
            clock: clock.clone(),
        };
        (
            SmartctlReader::new("/x/smartctl", Box::new(script), Arc::new(clock.clone())),
            seen,
            clock,
        )
    }

    #[test]
    fn reads_only_scanned_devices_with_fixed_arguments_and_survives_one_bad_disk() {
        let scan = r#"{"devices":[{"name":"/dev/nvme0","type":"nvme"},{"name":"/dev/sda","type":"sat"},{"name":"/dev/sdb"}]}"#;
        let (r, seen, _) = reader(vec![
            out(scan, true),
            out(GOOD, false),
            Err(PlatformError::Timeout("smartctl took longer than 15 s".into())),
            out(r#"{"smartctl":{"exit_status":2}}"#, false),
        ]);
        let reply = r.read(SmartQuery::default()).unwrap();
        assert_eq!(reply.disks.len(), 1);
        assert_eq!(reply.skipped.len(), 2);
        assert!(reply.skipped[0].reason.contains("took longer") && reply.skipped[0].reason.contains("Try again"));
        assert!(!format!("{reply:?}").contains("SECRET1"));
        let seen = seen.lock().unwrap();
        let cmds: Vec<_> = seen.iter().map(|t| t.args.join(" ")).collect();
        assert_eq!(
            cmds,
            [
                "--scan-open -j",
                "-a -j -d nvme /dev/nvme0",
                "-a -j -d sat /dev/sda",
                "-a -j /dev/sdb"
            ]
        );
    }

    #[test]
    fn the_query_caps_the_disks_visited_and_a_failed_scan_is_an_error() {
        let scan = r#"{"devices":[{"name":"/dev/a"},{"name":"/dev/b"},{"name":"/dev/c"}]}"#;
        let (r, seen, _) = reader(vec![out(scan, true), out(GOOD, false)]);
        let reply = r.read(SmartQuery { max_disks: 1 }).unwrap();
        assert_eq!((reply.disks.len(), reply.not_visited), (1, 2));
        assert_eq!(seen.lock().unwrap().len(), 2);
        let (r, _, _) = reader(vec![Err(PlatformError::NotFound("no smartctl".into()))]);
        assert!(matches!(r.read(SmartQuery::default()), Err(PlatformError::NotFound(_))));
    }
}
