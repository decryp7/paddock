//! Backup health on the agent: runs the checks off the request thread (a tool may take a minute), keeps the last report,
//! and raises or clears one alarm per tool. Dyno only looks: no tool is ever asked to start, stop or change a backup.

use super::ExtrasCentre;
use crate::agent::{lock, now, Shared};
use dyno_core::backup::{observation, BackupReport, BackupSettings, BackupStatus, BackupTool};
use dyno_core::metrics::Reading;
use dyno_core::read_via::ReadVia;
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
    /// Where the helper stood ([`ExtrasCentre::helper_epoch`]) when the report was made: a report made while it was Off is read
    /// again once it changed (Windows VM check 22).
    helper_epoch: u64,
}

/// Whether the page's poll should start a check: nothing was ever checked, or the helper changed since the report was made
/// (and no check runs).
fn wants_check(x: &ExtrasCentre) -> bool {
    let epoch = x.helper_epoch();
    let slot = lock(&x.backup);
    !slot.checking && (slot.report.checked_at.is_none() || slot.helper_epoch != epoch)
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
        let want = wants_check(x);
        if want {
            let mut slot = lock(&x.backup);
            // Another poll may have started it between the two locks.
            if slot.checking {
                false
            } else {
                if slot.report.checked_at.is_some() {
                    log::info!(target: "dyno_agent::backup", "backup report is {} s old and the helper changed since (epoch {} to {}): checking again", slot.report.checked_at.map_or(0, |t| now().0.saturating_sub(t.0) / 1000), slot.helper_epoch, x.helper_epoch());
                }
                slot.checking = true;
                true
            }
        } else {
            false
        }
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
    let epoch = x.helper_epoch();
    let began = std::time::Instant::now();
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
    let mut via = ReadVia::Local;
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
            match windows_status_from_helper(x, &resolver, &settings, tool, t, settings.stale_after_hours, job.is_some()) {
                Some((status, read_via)) => {
                    via = read_via;
                    status
                }
                None => backup_status_of(&env, tool, t, zone.as_ref(), settings.stale_after_hours),
            }
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
    log::info!(
        target: "dyno_agent::backup",
        "backup check done: {} tools, read via {:?}, helper epoch {}, {} ms",
        statuses.len(), via, epoch, began.elapsed().as_millis()
    );
    let mut slot = lock(&x.backup);
    slot.helper_epoch = epoch;
    slot.report = BackupReport {
        statuses,
        checked_at: Some(t),
        checking: false,
        login_path_used: resolver.login_path_used(),
        // Only the Windows Backup versions can come from the helper; every other tool is read here.
        via,
    };
    slot.checking = false;
    x.backup_done.notify_all();
    Ok(())
}

/// The Windows Backup status read helper first (event log and `wbadmin` with this account's rights when the helper cannot be
/// asked), with the path that answered. None for any other tool, and when the agent was composed without a helper.
fn windows_status_from_helper(
    x: &ExtrasCentre,
    resolver: &Arc<PathResolver>,
    settings: &BackupSettings,
    tool: BackupTool,
    now: dyno_core::time::Timestamp,
    stale: u32,
    asked_by_person: bool,
) -> Option<(BackupStatus, ReadVia)> {
    if tool != BackupTool::WindowsBackup {
        return None;
    }
    let mut helper = lock(&x.helper);
    let h = helper.as_mut()?;
    let (resolver, dirs, scripted) = (resolver.clone(), settings.dirs_for(tool).to_vec(), lock(&x.backup_tools).clone());
    h.backup.reading_mut().set_local(Box::new(move || {
        let runner: Box<dyn ToolRunner> = match &scripted {
            Some(s) => Box::new(Shared2(s.clone())),
            None => Box::new(PathRunner::new(&resolver, &dirs)),
        };
        dyno_platform::extras::windows_backup_facts(runner.as_ref())
    }));
    if asked_by_person {
        // "Check now": a helper left alone after a hang is asked again at once.
        h.backup.retry_now();
    }
    let got = h.backup.read();
    let status = match got.value {
        Reading::Available(facts) => facts.status(now, stale),
        Reading::Unavailable(why) => dyno_core::backup::cannot_check(tool, &why),
    };
    Some((status, got.via))
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
    use super::*;

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

    use crate::helper_first::testkit::{centre, Net};
    use dyno_core::backup::BackupState;
    use dyno_core::helper_extras::{BackupVersion, ExtrasAnswer};
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::read_via::ReadVia;
    use dyno_core::time::Timestamp;
    use paddock_jobs::time::FakeClock;

    /// Every Windows tool refuses, as for a standard account.
    struct Refusing;
    impl ToolRunner for Refusing {
        fn run_tool(
            &self,
            _: &dyno_platform::runner::Tool,
            _: std::time::Duration,
        ) -> Result<dyno_platform::runner::ToolOutput, dyno_core::ports::PlatformError> {
            Err(dyno_core::ports::PlatformError::AccessDenied("Access is denied.".into()))
        }
    }

    fn status(up: bool, answers: Vec<Result<HelperReply, crate::helper_link::TransportError>>) -> Option<(BackupStatus, ReadVia)> {
        let x = ExtrasCentre::new(false);
        x.use_backup_tools(Arc::new(Refusing));
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        x.use_helper(Arc::new(crate::helper_first::CentrePort(c)));
        let resolver = Arc::new(PathResolver::system());
        windows_status_from_helper(
            &x,
            &resolver,
            &BackupSettings::default(),
            BackupTool::WindowsBackup,
            Timestamp(0),
            48,
            false,
        )
    }

    #[test]
    fn windows_backup_comes_from_the_helper_when_wbadmin_is_refused() {
        let newest = BackupVersion {
            time: "x".into(),
            location: "D:".into(),
            identifier: "01/01/1970-00:00".into(),
            recovers: "Volume(s)".into(),
        };
        let answer = ExtrasAnswer::BackupVersions {
            count: 1,
            items: Some(vec![newest]),
        };
        let (status, via) = status(true, vec![Ok(HelperReply::Extras(answer))]).expect("the helper reading");
        assert_ne!(status.state, BackupState::CannotCheck, "{status:?}");
        assert_eq!(via, ReadVia::Helper);
    }

    /// Windows VM check 22 (2026-11-04): the report was made while the helper was Off ("Checked 7 min ago", "Turn on Administrator
    /// readings"), the helper was then turned on, and the page kept showing the old report: no chip, no versions.
    #[test]
    fn a_report_made_before_the_helper_changed_is_checked_again() {
        let x = ExtrasCentre::new(false);
        x.use_backup_tools(Arc::new(Refusing));
        let c = centre(&Net::new(true, vec![]), &FakeClock::new());
        x.use_helper(Arc::new(crate::helper_first::CentrePort(c.clone())));
        assert!(wants_check(&x), "nothing was ever checked");
        {
            let mut slot = lock(&x.backup);
            slot.report.checked_at = Some(Timestamp(1));
            slot.helper_epoch = x.helper_epoch();
        }
        assert!(!wants_check(&x), "the same helper standing is not read again");
        c.forget_ready();
        assert!(wants_check(&x), "the helper was turned on or off since the report");
        lock(&x.backup).checking = true;
        assert!(!wants_check(&x), "a check that runs is waited for");
    }

    #[test]
    fn windows_backup_without_the_helper_keeps_the_refusal_and_asks_for_the_helper() {
        let (status, via) = status(false, vec![]).expect("the helper reading");
        assert_eq!(status.state, BackupState::CannotCheck);
        assert!(
            status.summary.contains("administrator list Windows Backup versions"),
            "{}",
            status.summary
        );
        assert!(matches!(via, ReadVia::NeedsHelper { .. }), "{via:?}");
    }

    #[test]
    fn other_backup_tools_never_go_to_the_helper() {
        let x = ExtrasCentre::new(false);
        x.use_helper(Arc::new(crate::helper_first::OffPort));
        let resolver = Arc::new(PathResolver::system());
        let got = windows_status_from_helper(
            &x,
            &resolver,
            &BackupSettings::default(),
            BackupTool::TimeMachine,
            Timestamp(0),
            48,
            false,
        );
        assert!(got.is_none());
    }
}
