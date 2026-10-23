//! Backup health on the agent: runs the checks off the request thread (a tool may take a minute), keeps the last report,
//! and raises or clears one alarm per tool. Dyno only looks: no tool is ever asked to start, stop or change a backup.

use super::ExtrasCentre;
use crate::agent::{lock, now, Shared};
use dyno_core::backup::{observation, BackupReport, BackupSettings, BackupStatus, BackupTool};
use dyno_platform::extras::{backup_statuses, BackupEnv};
use dyno_platform::issues::ToolRunner;
use dyno_platform::toolpath::{PathResolver, PathRunner};
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
    fn run(&self, program: &str, args: &[String], timeout: std::time::Duration) -> Result<String, dyno_core::ports::PlatformError> {
        self.0.run(program, args, timeout)
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

/// The report: the last one, or (when `refresh` or nothing was ever checked) a new check is started in the background and
/// the report says `checking`. The caller polls `Request::Backup` until `checking` is false.
pub fn report(shared: &Arc<Shared>, refresh: bool) -> BackupReport {
    let x = &shared.extras;
    let start = {
        let mut slot = lock(&x.backup);
        let want = (refresh || slot.report.checked_at.is_none()) && !slot.checking;
        if want {
            slot.checking = true;
        }
        want
    };
    if start {
        let s = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("dyno-backup-check".into())
            .spawn(move || run_check(&s));
        if spawned.is_err() {
            lock(&x.backup).checking = false;
        }
    }
    snapshot(x)
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
    run_check(shared);
}

fn run_check(shared: &Arc<Shared>) {
    let x = &shared.extras;
    let (settings, off) = {
        let core = lock(&shared.core);
        (core.settings.backup.clone(), core.settings.alarms.utc_offset_minutes)
    };
    let resolver = lock(&x.resolver).clone();
    let t = now();
    // One tool failing or panicking must not stop the others, nor leave `checking` set.
    let statuses = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let env = Env {
            resolver: &resolver,
            settings: &settings,
            scripted: lock(&x.backup_tools).clone(),
        };
        backup_statuses(&env, x.os(), t, off, settings.stale_after_hours)
    }))
    .unwrap_or_default();
    if settings.alert_when_stale {
        raise_alarms(shared, &statuses, t);
    }
    let mut slot = lock(&x.backup);
    slot.report = BackupReport {
        statuses,
        checked_at: Some(t),
        checking: false,
        login_path_used: resolver.login_path_used(),
    };
    slot.checking = false;
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
