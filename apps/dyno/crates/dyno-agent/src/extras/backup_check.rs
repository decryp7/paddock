//! Backup health on the agent: runs the checks off the request thread (a tool may take a minute), keeps the last report,
//! and raises or clears one alarm per tool. Dyno only looks: no tool is ever asked to start, stop or change a backup.

use super::ExtrasCentre;
use crate::agent::{lock, now, Shared};
use dyno_core::backup::{observation, BackupReport, BackupSettings, BackupStatus, BackupTool};
use dyno_platform::extras::{backup_status_of, backup_tools_present, BackupEnv};
use dyno_platform::runner::ToolRunner;
use dyno_platform::toolpath::{PathResolver, PathRunner};
use paddock_jobs::{JobContext, JobError, Phase};
use std::sync::Arc;

/// The last report and whether a check is running.
#[derive(Default)]
pub struct Slot {
    report: BackupReport,
    checking: bool,
}

/// Finds backup tools through the login-shell `PATH` and the user's per-tool folders.
struct Env<'a> {
    resolver: &'a PathResolver,
    settings: &'a BackupSettings,
    scripted: Option<Arc<dyn ToolRunner>>,
}

/// Lets a scripted runner stand in for every tool (tests) while the real ones go through the resolver.
struct Shared2(Arc<dyn ToolRunner>);

impl ToolRunner for Shared2 {
    fn run_tool(
        &self,
        tool: &dyno_platform::runner::Tool,
        timeout: std::time::Duration,
    ) -> Result<dyno_platform::runner::ToolOutput, dyno_core::ports::PlatformError> {
        self.0.run_tool(tool, timeout)
    }
}

impl BackupEnv for Env<'_> {
    fn available(&self, tool: BackupTool) -> bool {
        if self.scripted.is_some() {
            return true;
        }
        match tool.binary() {
            None => true,
            Some(b) => self.resolver.find(b, self.settings.dirs_for(tool)).is_some(),
        }
    }

    fn runner(&self, tool: BackupTool) -> Box<dyn ToolRunner + '_> {
        match &self.scripted {
            Some(s) => Box::new(Shared2(s.clone())),
            None => Box::new(PathRunner::new(self.resolver, self.settings.dirs_for(tool))),
        }
    }
}

/// The report: the last one, or (when nothing was ever checked) a new check is started in the background and the report
/// says `checking`. The caller polls `Request::Backup` until `checking` is false. A check the person asks for is the job
/// [`refresh`].
pub fn report(shared: &Arc<Shared>) -> BackupReport {
    let x = &shared.extras;
    let start = {
        let mut slot = lock(&x.backup);
        let want = slot.report.checked_at.is_none() && !slot.checking;
        if want {
            slot.checking = true;
        }
        want
    };
    if start {
        let spawned = first_check(shared.clone(), |s| {
            let _ = run_check(&s, None);
        });
        if !spawned {
            end_check(x);
        }
    }
    snapshot(x)
}

/// Runs the first check on its own thread as live work (rule 52): the Backups page polls until it fills. Returns whether
/// the thread started.
fn first_check<T: Send + 'static>(state: T, check: impl FnOnce(T) + Send + 'static) -> bool {
    std::thread::Builder::new()
        .name("dyno-backup-check".into())
        .spawn(move || {
            dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
            check(state);
        })
        .is_ok()
}

/// The report and whether a check runs, without starting one.
pub fn peek(x: &ExtrasCentre) -> BackupReport {
    snapshot(x)
}

/// The last report as it is, without starting a check (the popup's backup widget reads it on open).
pub fn last_report(shared: &Shared) -> BackupReport {
    snapshot(&shared.extras)
}

fn snapshot(x: &ExtrasCentre) -> BackupReport {
    let slot = lock(&x.backup);
    BackupReport {
        checking: slot.checking,
        ..slot.report.clone()
    }
}

/// Runs a check on this thread (the background loop). Skips when one is already running.
pub fn check_now(shared: &Arc<Shared>) {
    {
        let mut slot = lock(&shared.extras.backup);
        if slot.checking {
            return;
        }
        slot.checking = true;
    }
    let _ = run_check(shared, None);
}

/// The check the person asked for, as the job `job`: one step per backup tool, Cancel between tools, and the report is
/// replaced only when every tool was read. A check that is already running (the scheduled one) is waited for, not repeated.
pub fn refresh(shared: &Arc<Shared>, job: &JobContext) -> Result<BackupReport, JobError> {
    let x = &shared.extras;
    if lock(&x.backup).checking {
        job.phase(Phase::discovering("waiting", "Waiting for the backup check that is running"));
        // The waker goes in BEFORE the lock is taken: a token that is already cancelled runs it on the spot, and it takes this
        // same lock. The end of that check wakes the wait, and so does Cancel: no polling.
        let (state, done) = (x.backup.clone(), x.backup_done.clone());
        let _wake = job.cancel_token().on_cancel(move || {
            let _held = lock(&state);
            done.notify_all();
        });
        let mut slot = lock(&x.backup);
        while slot.checking {
            job.check()?;
            slot = x.backup_done.wait(slot).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        job.check()?;
    }
    {
        let mut slot = lock(&x.backup);
        if slot.checking {
            // Another check started in between: wait for it as well rather than run two.
            drop(slot);
            return refresh(shared, job);
        }
        slot.checking = true;
    }
    run_check(shared, Some(job))?;
    Ok(snapshot(x))
}

/// Marks the check as over without a new report (it failed to start or was cancelled) and wakes whoever waits for it.
fn end_check(x: &ExtrasCentre) {
    lock(&x.backup).checking = false;
    x.backup_done.notify_all();
}

/// Reads every tool and, when that completed, swaps the report in. `checking` must be set by the caller; it is cleared
/// on every way out.
fn run_check(shared: &Arc<Shared>, job: Option<&JobContext>) -> Result<(), JobError> {
    let x = &shared.extras;
    let (settings, zone) = {
        let core = lock(&shared.core);
        (core.settings.backup.clone(), core.zone.clone())
    };
    let resolver = lock(&x.resolver).clone();
    let t = now();
    let env = Env {
        resolver: &resolver,
        settings: &settings,
        scripted: lock(&x.backup_tools).clone(),
    };
    let tools = backup_tools_present(&env, x.os());
    if let Some(j) = job {
        j.phase(Phase::items("tools", "Checking backup tools", tools.len() as u64));
    }
    let mut statuses = Vec::with_capacity(tools.len());
    for tool in tools {
        if let Some(j) = job {
            if let Err(e) = j.check() {
                end_check(x);
                return Err(e);
            }
        }
        // One tool failing or panicking must not stop the others, nor leave `checking` set. One tool run cannot be
        // interrupted; it ends at its own timeout.
        let _step = job.map(|j| j.uncancellable("A backup tool is being asked; Cancel takes effect when it has answered."));
        if let Ok(s) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            backup_status_of(&env, tool, t, zone.as_ref(), settings.stale_after_hours)
        })) {
            statuses.push(s);
        }
        if let Some(j) = job {
            j.add(1);
        }
    }
    if let Some(j) = job {
        if let Err(e) = j.check() {
            end_check(x);
            return Err(e);
        }
    }
    if settings.alert_when_stale {
        raise_alarms(shared, &statuses, t);
    }
    let mut slot = lock(&x.backup);
    slot.report = BackupReport {
        statuses,
        checked_at: Some(t),
        checking: false,
        login_path_used: resolver.login_path_used(),
        // The Windows Backup versions come from the helper when WP-EXT wires them; today every tool is read here.
        via: dyno_core::read_via::ReadVia::Local,
    };
    slot.checking = false;
    x.backup_done.notify_all();
    Ok(())
}

fn raise_alarms(shared: &Shared, statuses: &[BackupStatus], t: dyno_core::time::Timestamp) {
    for s in statuses {
        let Some(o) = observation(s) else { continue };
        let changes = {
            let core = &mut *lock(&shared.core);
            core.alarms.observe(&core.store, o, t)
        };
        crate::sampler::push_alarm_changes(shared, changes);
    }
}

#[cfg(test)]
mod tests {
    /// Rule 52 (coordinator re-review, 2026-10-30): the first check fills the screen the Backups page polls for, so it runs
    /// as live work.
    #[test]
    fn the_first_check_runs_as_live_work() {
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(super::first_check(tx, |tx| {
            let _ = tx.send(dyno_platform::priority::current_role());
        }));
        let role = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the check ran (a hang guard)");
        assert_eq!(role, Some(dyno_platform::priority::ThreadRole::Live));
    }
}
