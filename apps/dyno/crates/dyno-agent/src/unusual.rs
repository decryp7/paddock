//! The agent's look at new and unusual programs (WP-07): every half minute it compares the process table with the
//! baseline kept in the store, checks the signature of programs it has never seen (a few at a time, off the sampler),
//! and writes one activity line for a program that just became unusual. Observe-only: it explains and never acts.

use crate::agent::{lock, now, Shared};
use dyno_core::activity::ActivityKind;
use dyno_core::os::Os;
use dyno_core::process::ProcessSample;
use dyno_core::protocol::{AgentError, ErrorCode, Message};
use dyno_core::startup::StartupState;
use dyno_core::unusual::baseline::{Baseline, Observed, StartupObs, UnusualOverview};
use dyno_core::unusual::{program_key, Signing};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How often the process table is compared with the baseline.
pub const LOOK_EVERY: Duration = Duration::from_secs(30);
/// How long the startup list is reused (reading it parses launch agents or the registry).
const STARTUP_TTL: Duration = Duration::from_secs(5 * 60);
/// Signatures checked per look (each can take a second or two).
const MAX_SIGNATURE_CHECKS: usize = 3;
/// Activity lines written per look (a burst of new programs must not flood the log).
const MAX_LINES_PER_LOOK: usize = 3;

/// Dyno's own executables: never listed as unusual.
const DYNO_OWN: &[&str] = &["dyno.exe", "dyno", "dyno-app.exe", "dyno-app", "dyno-agent.exe", "dyno-agent"];

/// The OS this build runs on, for the pure rules that depend on path conventions.
pub fn current_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
    }
}

struct State {
    baseline: Baseline,
    startup: Option<(Instant, Vec<StartupObs>)>,
}

/// The baseline and its schedule.
pub struct UnusualCentre {
    state: Mutex<State>,
}

impl UnusualCentre {
    /// Restores the baseline from the store (a damaged store starts a new one).
    pub fn load(store: &crate::store::Store) -> UnusualCentre {
        let baseline = match store.load_baseline() {
            Ok((since, entries, startup)) => Baseline::restore(since, entries, startup),
            Err(e) => {
                log::error!("dyno-agent: could not read the program baseline: {e}");
                Baseline::default()
            }
        };
        UnusualCentre {
            state: Mutex::new(State { baseline, startup: None }),
        }
    }

    /// One look at the process table.
    pub fn look(&self, shared: &Shared) {
        let settings = lock(&shared.core).settings.unusual;
        if !settings.enabled {
            return;
        }
        let t = now();
        let processes = lock(&shared.core).latest.processes.clone();
        if processes.is_empty() {
            return;
        }
        let seen = observed(&processes, &shared.network.connection_counts());
        let startup = self.startup_entries(shared);
        // Signatures of programs the baseline has not seen, outside the baseline's lock (a check can take seconds).
        let learning = lock(&self.state).baseline.learning(t, &settings).active;
        let mut signatures: HashMap<String, Signing> = HashMap::new();
        if !learning {
            let fresh: Vec<(String, &ProcessSample)> = {
                let st = lock(&self.state);
                let mut keys = std::collections::HashSet::new();
                processes
                    .iter()
                    .map(|p| (program_key(&p.name, p.exe.as_deref()), p))
                    .filter(|(k, _)| st.baseline.is_new(k) && keys.insert(k.clone()))
                    .take(MAX_SIGNATURE_CHECKS)
                    .collect()
            };
            for (key, p) in fresh {
                let sig = lock(&shared.ports.details)
                    .details(p.reference())
                    .map_or(Signing::Unknown, |d| Signing::from(&d.signature));
                signatures.insert(key, sig);
            }
        }
        let outcome = {
            let mut st = lock(&self.state);
            st.baseline.observe(t, &settings, current_os(), &seen, &startup, &signatures)
        };
        let mut core = lock(&shared.core);
        if let Err(e) = core.store.apply_baseline(&outcome.changes) {
            log::error!("dyno-agent: could not store the program baseline: {e}");
        }
        let overview = lock(&self.state).baseline.overview(t, &settings);
        for key in outcome.newly_unusual.iter().take(MAX_LINES_PER_LOOK) {
            let Some(item) = overview.items.iter().find(|i| &i.key == key) else {
                continue;
            };
            let entry = core.log(
                t,
                ActivityKind::System,
                format!("Unusual program: {}", item.name),
                format!(
                    "{} is new on this computer. Unusual because it {}.",
                    item.name,
                    item.reasons.join(", ")
                ),
            );
            shared.broadcast(&Message::Activity(entry));
        }
    }

    fn startup_entries(&self, shared: &Shared) -> Vec<StartupObs> {
        if let Some((at, list)) = &lock(&self.state).startup {
            if at.elapsed() < STARTUP_TTL {
                return list.clone();
            }
        }
        let list: Vec<StartupObs> = lock(&shared.ports.startup)
            .list()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.state != StartupState::Disabled)
            .map(|e| StartupObs {
                id: e.id,
                name: e.name,
                exe: e.exe,
            })
            .collect();
        lock(&self.state).startup = Some((Instant::now(), list.clone()));
        list
    }

    /// The page's data.
    pub fn overview(&self, shared: &Shared) -> UnusualOverview {
        let settings = lock(&shared.core).settings.unusual;
        lock(&self.state).baseline.overview(now(), &settings)
    }

    /// Forgets the baseline (learning starts again at the next look).
    pub fn forget(&self, shared: &Shared) -> Result<UnusualOverview, AgentError> {
        let core = lock(&shared.core);
        core.store
            .clear_baseline()
            .map_err(|e| AgentError::new(ErrorCode::Internal, e.to_string()))?;
        lock(&self.state).baseline.forget();
        let settings = core.settings.unusual;
        drop(core);
        Ok(lock(&self.state).baseline.overview(now(), &settings))
    }
}

/// The programs in a process table: one per program, with the most children one process has and the connections of all
/// its processes.
pub fn observed(processes: &[ProcessSample], connections: &HashMap<u32, u32>) -> Vec<Observed> {
    let mut children: HashMap<u32, u32> = HashMap::new();
    for p in processes {
        if let Some(parent) = p.parent {
            *children.entry(parent).or_default() += 1;
        }
    }
    let mut by_key: HashMap<String, Observed> = HashMap::new();
    for p in processes {
        if DYNO_OWN.contains(&p.name.to_lowercase().as_str()) || p.name.is_empty() {
            continue;
        }
        let key = program_key(&p.name, p.exe.as_deref());
        let o = by_key.entry(key).or_insert_with(|| Observed {
            name: p.name.clone(),
            exe: p.exe.clone(),
            children: 0,
            connections: 0,
        });
        o.children = o.children.max(children.get(&p.pid).copied().unwrap_or(0));
        o.connections += connections.get(&p.pid).copied().unwrap_or(0);
    }
    let mut out: Vec<Observed> = by_key.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The background look: every [`LOOK_EVERY`] until the agent stops.
pub fn run(shared: &Shared) {
    let wait = |d: Duration| {
        let mut waited = Duration::ZERO;
        while waited < d && !shared.stopped() {
            std::thread::sleep(Duration::from_millis(250));
            waited += Duration::from_millis(250);
        }
    };
    // The sampler needs a moment for its first process table.
    wait(Duration::from_secs(10));
    while !shared.stopped() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.unusual.look(shared)));
        if r.is_err() {
            log::error!("dyno-agent: a look at new programs failed unexpectedly");
        }
        wait(LOOK_EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::process::RunState;
    use dyno_core::process::{Measured, Owner, UnavailableReason};
    use dyno_core::time::Timestamp;

    fn p(pid: u32, parent: Option<u32>, name: &str, exe: &str) -> ProcessSample {
        ProcessSample {
            pid,
            parent,
            name: name.into(),
            exe: Some(exe.into()),
            user: Measured::unavailable(UnavailableReason::NotPermitted),
            cpu: Measured::Value(0.0),
            memory: Measured::Value(0),
            disk_read_bps: Measured::Value(0.0),
            disk_write_bps: Measured::Value(0.0),
            started: Measured::Value(Timestamp(0)),
            owner: Owner::Own,
            state: RunState::Running,
        }
    }

    #[test]
    fn programs_are_grouped_with_children_and_connections() {
        let table = vec![
            p(1, None, "launcher", "/a/launcher"),
            p(2, Some(1), "worker", "/a/worker"),
            p(3, Some(1), "worker", "/a/worker"),
            p(4, Some(2), "leaf", "/a/leaf"),
            p(5, None, "dyno-agent", "/d/dyno-agent"),
            p(6, None, "", "/x"),
        ];
        let conns = HashMap::from([(2, 3), (3, 4), (9, 1)]);
        let seen = observed(&table, &conns);
        let names: Vec<&str> = seen.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["launcher", "leaf", "worker"],
            "Dyno's own processes and nameless ones are left out"
        );
        let launcher = seen.iter().find(|o| o.name == "launcher").unwrap();
        assert_eq!(launcher.children, 2);
        let worker = seen.iter().find(|o| o.name == "worker").unwrap();
        assert_eq!((worker.children, worker.connections), (1, 7));
    }

    #[test]
    fn the_os_matches_this_build() {
        let os = current_os();
        assert_eq!(os == Os::Macos, cfg!(target_os = "macos"));
        assert_eq!(os == Os::Windows, cfg!(windows));
    }
}
