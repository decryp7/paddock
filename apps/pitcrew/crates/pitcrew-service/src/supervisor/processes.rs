//! The supervisor's handling of a runner's processes: classifying them every probe tick, stopping due leftovers
//! while the runner is idle, and the requests of the Processes tab (ADR 0012).

use super::*;
use crate::process_watch::Stopped;
use pitcrew_core::process_policy::KeepFor;
use pitcrew_core::process_track::Refusal;

/// `3 h 02 min`, `12 min`, `45 s`.
fn age(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{} h {:02} min", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

impl Supervisor {
    /// Every tick: moves the stops in progress along; on a probe tick also starts the stops that are due.
    pub(super) fn tidy_processes(&mut self, name: &RunnerName, now: Timestamp, probe: bool) {
        let Some(r) = self.runners.get_mut(name) else { return };
        let done = {
            let probe_ref = &mut self.probe;
            r.watch
                .advance(now, &mut |pid, started| probe_ref.alive(pid, started), self.terminator.as_ref())
        };
        let mut failures: Vec<String> = vec![];
        if probe && r.process.is_some() && !r.jobs.running(now) {
            let policy = r.cfg.process_policy(&self.config.defaults);
            for t in r.watch.due(now, true) {
                let rule = process_track::rule_label(&t);
                r.log.supervisor(
                    now,
                    &format!(
                        "stopping leftover {} (process {}, running {}): its job ended and {rule} has passed",
                        t.meta.name,
                        t.meta.pid,
                        age(t.meta.started_ms.map_or(0, |s| now.0.saturating_sub(s)))
                    ),
                );
                if let Err(e) = r.watch.begin(&t, rule, StopReason::Cleanup, now, &policy, self.terminator.as_ref()) {
                    failures.push(format!("could not stop leftover {} (process {}): {e}", t.meta.name, t.meta.pid));
                }
            }
        }
        for f in failures {
            if let Some(r) = self.runners.get_mut(name) {
                r.log.supervisor(now, &f);
            }
        }
        for d in done {
            self.record_stop(name, &d, now);
        }
    }

    fn record_stop(&mut self, name: &RunnerName, d: &Stopped, now: Timestamp) {
        let who = match d.by {
            StopReason::Cleanup => "cleanup",
            StopReason::User => "you",
        };
        if let Some(r) = self.runners.get_mut(name) {
            r.log.supervisor(
                now,
                &format!(
                    "stopped {} (process {}, running {}) by {who}: {}{}",
                    d.name,
                    d.pid,
                    age(d.age_ms),
                    d.rule,
                    if d.forced { "; it ignored the request and was ended" } else { "" }
                ),
            );
        }
        let rule = if d.by == StopReason::User && !d.rule.contains("could not be ended") {
            "you".to_string()
        } else {
            d.rule.clone()
        };
        self.state.runners.entry(name.to_string()).or_default().history.push(
            now,
            HistoryKind::ProcessStopped {
                name: d.name.clone(),
                pid: d.pid,
                age_ms: d.age_ms,
                rule,
                by: d.by,
                forced: d.forced,
            },
        );
        self.dirty = true;
    }

    /// `Request::Processes`.
    pub(super) fn processes_response(&self, runner: &RunnerName, now: Timestamp) -> Response {
        let Some(r) = self.runners.get(runner) else {
            return unknown(runner);
        };
        let policy = r.cfg.process_policy(&self.config.defaults);
        let list = r.watch.list(!r.jobs.running(now), &policy).unwrap_or(ProcessList {
            runner_pid: None,
            idle: true,
            cleanup: policy.cleanup,
            keep: policy.keep,
            entries: vec![],
            truncated: false,
            taken_at: now,
        });
        Response::Processes { list }
    }

    /// `Request::StopProcess`.
    pub(super) fn stop_process(
        &mut self,
        runner: &RunnerName,
        pid: u32,
        started_at: Option<Timestamp>,
        allow_runner: bool,
        now: Timestamp,
    ) -> Response {
        let Some(r) = self.runners.get_mut(runner) else {
            return unknown(runner);
        };
        // Look at the process table again, so the check is against the tree as it is now.
        self.probe.refresh_processes();
        if let Some(root) = r.process.as_ref().map(|p| p.pid()) {
            let table = self.probe.process_table();
            let members = tree_members(root, &table);
            if !members.iter().any(|m| m.pid == pid) {
                return err(ErrorCode::BadRequest, Refusal::NotInTree.to_string());
            }
        }
        let target = match r.watch.check(pid, started_at.map(|t| t.0), std::process::id(), allow_runner) {
            Ok(t) => t.clone(),
            Err(refusal) => return err(ErrorCode::BadRequest, refusal.to_string()),
        };
        let policy = r.cfg.process_policy(&self.config.defaults);
        match r
            .watch
            .begin(&target, "you".into(), StopReason::User, now, &policy, self.terminator.as_ref())
        {
            Ok(()) => {
                r.log.supervisor(
                    now,
                    &format!("asked {} (process {}) to stop at your request", target.meta.name, target.meta.pid),
                );
                Response::Ok
            }
            Err(e) => err(ErrorCode::Busy, e),
        }
    }

    /// `Request::KeepProcess`.
    pub(super) fn keep_process(&mut self, runner: &RunnerName, pid: u32, started_at: Timestamp, extra: KeepFor) -> Response {
        let Some(r) = self.runners.get_mut(runner) else {
            return unknown(runner);
        };
        if let Err(e) = extra.validate() {
            return err(ErrorCode::BadRequest, e);
        }
        match r.watch.keep(pid, started_at.0, extra) {
            Ok(()) => {
                self.dirty = true;
                Response::Ok
            }
            Err(e) => err(ErrorCode::BadRequest, e),
        }
    }
}
