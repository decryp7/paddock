//! The CPU taming engine: a pure state machine per (rule, app), fed one tick at a time (ADR 0004).
//!
//! An app that stays above a rule's threshold for `for_secs` gets the ladder's first step; each later step follows
//! `after_secs` after the previous one while the condition keeps holding. Steps add up (priority, then efficiency
//! mode, then a cap). Once the app stays below the threshold for `release_after_secs` — or the rule stops applying
//! (disabled, paused, other profile, context no longer true, the app came to the foreground) — every step is undone in
//! reverse order. Processes that join the app later get the steps already taken. Protected processes and the
//! foreground app are never touched. A manual undo snoozes the pair until the app drops below the threshold.

use super::model::{ActiveAction, Rule, RuleContext, RuleSet, RuleTarget, StepAction};
use crate::grouping::{file_name, AppGroup};
use crate::metrics::PowerSource;
use crate::process::{Pid, ProcessRef, ProcessSample};
use crate::protect::Protection;
use crate::time::{Timestamp, SECOND};
use std::collections::{BTreeMap, HashMap};

/// The situation the rules look at, besides CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Situation {
    /// Whether someone used the machine recently; `None` when unknown (then neither active nor idle holds).
    pub user_active: Option<bool>,
    /// Power source; `None` when unknown (desktops count as AC).
    pub power: Option<PowerSource>,
    /// The foreground app's process.
    pub foreground: Option<Pid>,
}

impl Situation {
    fn holds(&self, c: RuleContext) -> bool {
        match c {
            RuleContext::UserActive => self.user_active == Some(true),
            RuleContext::UserIdle => self.user_active == Some(false),
            RuleContext::OnBattery => self.power == Some(PowerSource::Battery),
            RuleContext::OnAc => self.power != Some(PowerSource::Battery),
        }
    }
}

/// What the engine asks the agent to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Carry out a step on a process.
    Apply {
        /// The rule.
        rule_id: String,
        /// The app.
        app: String,
        /// The process.
        target: ProcessRef,
        /// The step.
        step: StepAction,
    },
    /// Undo a step on a process.
    Undo {
        /// The rule.
        rule_id: String,
        /// The app.
        app: String,
        /// The process.
        target: ProcessRef,
        /// The step.
        step: StepAction,
    },
    /// Tell the user a rule acted.
    Notify {
        /// The rule's name.
        rule_name: String,
        /// The app.
        app: String,
        /// Its CPU.
        cpu: f32,
    },
}

#[derive(Debug, Clone)]
enum State {
    Watching {
        since: Timestamp,
    },
    Acting {
        app: String,
        root: ProcessRef,
        step: usize,
        step_at: Timestamp,
        since: Timestamp,
        below_since: Option<Timestamp>,
        /// How many ladder steps each member has had applied.
        applied: BTreeMap<ProcessRef, usize>,
    },
    Snoozed,
}

/// The engine's memory between ticks.
#[derive(Debug, Default)]
pub struct Engine {
    states: BTreeMap<(String, String), State>,
}

/// The input of one tick.
pub struct Tick<'a> {
    /// Now.
    pub now: Timestamp,
    /// Apps with their summed CPU.
    pub groups: &'a [AppGroup],
    /// Every process (to resolve the groups' pids).
    pub processes: &'a [ProcessSample],
    /// User and power context.
    pub situation: Situation,
}

fn matches_target(target: &RuleTarget, g: &AppGroup) -> bool {
    match target {
        RuleTarget::AnyApp => true,
        RuleTarget::Apps(apps) => {
            let exe = g.exe.as_deref().map(|e| e.replace('\\', "/").to_lowercase());
            let exe_name = g.exe.as_deref().map(|e| file_name(e).to_lowercase());
            let name = g.name.to_lowercase();
            apps.iter().any(|a| {
                let a = a.trim().replace('\\', "/").to_lowercase();
                a == name || Some(&a) == exe_name.as_ref() || Some(&a) == exe.as_ref()
            })
        }
    }
}

fn rule_active(rule: &Rule, set: &RuleSet, situation: &Situation) -> bool {
    rule.enabled
        && !set.paused
        && (rule.profiles.is_empty() || rule.profiles.contains(&set.profile))
        && rule.condition.when.iter().all(|c| situation.holds(*c))
}

impl Engine {
    /// A fresh engine.
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Evaluates one tick and returns what to do, in order.
    pub fn tick(&mut self, set: &RuleSet, protection: &Protection<'_>, t: &Tick<'_>) -> Vec<Command> {
        let by_pid: HashMap<Pid, &ProcessSample> = t.processes.iter().map(|p| (p.pid, p)).collect();
        let groups: HashMap<&str, &AppGroup> = t.groups.iter().map(|g| (g.id.as_str(), g)).collect();
        let mut out = Vec::new();

        // Pairs whose rule or app went away, or that may no longer act: undo and forget.
        let keys: Vec<(String, String)> = self.states.keys().cloned().collect();
        for key in keys {
            let rule = set.rules.iter().find(|r| r.id == key.0);
            let group = groups.get(key.1.as_str()).copied();
            let keep = match (rule, group) {
                (Some(r), Some(g)) => rule_active(r, set, &t.situation) && eligible(g, &by_pid, protection, &t.situation),
                _ => false,
            };
            if !keep {
                if let (Some(state), Some(rule)) = (self.states.remove(&key), rule) {
                    undo_state(&state, rule, &key.0, &by_pid, &mut out);
                } else {
                    self.states.remove(&key);
                }
            }
        }

        for rule in set.rules.iter().filter(|r| rule_active(r, set, &t.situation)) {
            for g in t.groups.iter().filter(|g| matches_target(&rule.condition.target, g)) {
                if !eligible(g, &by_pid, protection, &t.situation) {
                    continue;
                }
                let key = (rule.id.clone(), g.id.clone());
                let above = g.cpu > rule.condition.cpu_above;
                let state = self.states.remove(&key);
                let next = step_state(state, rule, g, above, t.now, &by_pid, protection, &mut out);
                if let Some(s) = next {
                    self.states.insert(key, s);
                }
            }
        }
        out
    }

    /// What Dyno is doing right now.
    pub fn active(&self, set: &RuleSet) -> Vec<ActiveAction> {
        self.states
            .iter()
            .filter_map(|((rule_id, _), s)| match s {
                State::Acting {
                    app, root, step, since, ..
                } => {
                    let rule = set.rules.iter().find(|r| &r.id == rule_id)?;
                    let description = rule.ladder[..=*step]
                        .iter()
                        .map(|s| s.action.describe())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Some(ActiveAction {
                        target: *root,
                        app: app.clone(),
                        rule_id: rule_id.clone(),
                        rule_name: rule.name.clone(),
                        step: *step,
                        description,
                        since: *since,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// The user undid what a rule did to an app: undo the rest and leave the app alone until it calms down.
    pub fn snooze(&mut self, set: &RuleSet, rule_id: &str, group_id: &str, processes: &[ProcessSample]) -> Vec<Command> {
        let by_pid: HashMap<Pid, &ProcessSample> = processes.iter().map(|p| (p.pid, p)).collect();
        let key = (rule_id.to_string(), group_id.to_string());
        let mut out = Vec::new();
        if let (Some(state), Some(rule)) = (self.states.remove(&key), set.rules.iter().find(|r| r.id == rule_id)) {
            undo_state(&state, rule, rule_id, &by_pid, &mut out);
        }
        self.states.insert(key, State::Snoozed);
        out
    }

    /// Undoes everything (Dyno is stopping, or the user paused all rules).
    pub fn release_all(&mut self, set: &RuleSet, processes: &[ProcessSample]) -> Vec<Command> {
        let by_pid: HashMap<Pid, &ProcessSample> = processes.iter().map(|p| (p.pid, p)).collect();
        let mut out = Vec::new();
        for ((rule_id, _), state) in std::mem::take(&mut self.states) {
            if let Some(rule) = set.rules.iter().find(|r| r.id == rule_id) {
                undo_state(&state, rule, &rule_id, &by_pid, &mut out);
            }
        }
        out
    }
}

/// Whether an app may be acted on at all: not in the foreground, and its root process not protected.
fn eligible(g: &AppGroup, by_pid: &HashMap<Pid, &ProcessSample>, protection: &Protection<'_>, s: &Situation) -> bool {
    if s.foreground.is_some_and(|f| g.pids.contains(&f)) {
        return false;
    }
    match g.pids.first().and_then(|p| by_pid.get(p)) {
        Some(root) => protection.allows(root),
        None => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn step_state(
    state: Option<State>,
    rule: &Rule,
    g: &AppGroup,
    above: bool,
    now: Timestamp,
    by_pid: &HashMap<Pid, &ProcessSample>,
    protection: &Protection<'_>,
    out: &mut Vec<Command>,
) -> Option<State> {
    match state {
        None | Some(State::Watching { .. }) if !above => None,
        Some(State::Snoozed) if !above => None,
        Some(State::Snoozed) => Some(State::Snoozed),
        None => Some(maybe_start(State::Watching { since: now }, rule, g, now, by_pid, protection, out)),
        Some(w @ State::Watching { .. }) => Some(maybe_start(w, rule, g, now, by_pid, protection, out)),
        Some(State::Acting {
            app,
            root,
            mut step,
            mut step_at,
            since,
            below_since,
            mut applied,
        }) => {
            if !above {
                let below = below_since.unwrap_or(now);
                if now.since(below) >= i64::from(rule.release_after_secs) * SECOND {
                    let s = State::Acting {
                        app,
                        root,
                        step,
                        step_at,
                        since,
                        below_since: Some(below),
                        applied,
                    };
                    undo_state(&s, rule, &rule.id, by_pid, out);
                    return None;
                }
                return Some(State::Acting {
                    app,
                    root,
                    step,
                    step_at,
                    since,
                    below_since: Some(below),
                    applied,
                });
            }
            if let Some(next) = rule.ladder.get(step + 1) {
                if now.since(step_at) >= i64::from(next.after_secs) * SECOND {
                    step += 1;
                    step_at = now;
                    if next.action == StepAction::Notify {
                        out.push(Command::Notify {
                            rule_name: rule.name.clone(),
                            app: g.name.clone(),
                            cpu: g.cpu,
                        });
                    }
                }
            }
            apply_members(rule, g, step, &mut applied, by_pid, protection, out);
            Some(State::Acting {
                app,
                root,
                step,
                step_at,
                since,
                below_since: None,
                applied,
            })
        }
    }
}

/// From watching: start acting once the condition held long enough.
fn maybe_start(
    state: State,
    rule: &Rule,
    g: &AppGroup,
    now: Timestamp,
    by_pid: &HashMap<Pid, &ProcessSample>,
    protection: &Protection<'_>,
    out: &mut Vec<Command>,
) -> State {
    let since = match state {
        State::Watching { since } => since,
        other => return other,
    };
    if now.since(since) < i64::from(rule.condition.for_secs) * SECOND {
        return State::Watching { since };
    }
    let root = match g.pids.first().and_then(|p| by_pid.get(p)) {
        Some(p) => p.reference(),
        None => return State::Watching { since },
    };
    if rule.ladder[0].action == StepAction::Notify {
        out.push(Command::Notify {
            rule_name: rule.name.clone(),
            app: g.name.clone(),
            cpu: g.cpu,
        });
    }
    let mut applied = BTreeMap::new();
    apply_members(rule, g, 0, &mut applied, by_pid, protection, out);
    State::Acting {
        app: g.name.clone(),
        root,
        step: 0,
        step_at: now,
        since: now,
        below_since: None,
        applied,
    }
}

/// Brings every live, unprotected member of the app up to `step` (inclusive).
fn apply_members(
    rule: &Rule,
    g: &AppGroup,
    step: usize,
    applied: &mut BTreeMap<ProcessRef, usize>,
    by_pid: &HashMap<Pid, &ProcessSample>,
    protection: &Protection<'_>,
    out: &mut Vec<Command>,
) {
    for p in g.pids.iter().filter_map(|pid| by_pid.get(pid)) {
        if !protection.allows(p) {
            continue;
        }
        let r = p.reference();
        let done = applied.entry(r).or_insert(0);
        while *done <= step {
            let s = rule.ladder[*done].action;
            if s != StepAction::Notify {
                out.push(Command::Apply {
                    rule_id: rule.id.clone(),
                    app: g.name.clone(),
                    target: r,
                    step: s,
                });
            }
            *done += 1;
        }
    }
}

/// Undoes every step applied to every still-running member, newest first.
fn undo_state(state: &State, rule: &Rule, rule_id: &str, by_pid: &HashMap<Pid, &ProcessSample>, out: &mut Vec<Command>) {
    if let State::Acting { app, applied, .. } = state {
        for (r, count) in applied {
            let alive = by_pid.get(&r.pid).is_some_and(|p| p.started == r.started);
            if !alive {
                continue;
            }
            for i in (0..*count).rev() {
                let s = rule.ladder.get(i).map(|s| s.action);
                if let Some(s) = s.filter(|s| *s != StepAction::Notify) {
                    out.push(Command::Undo {
                        rule_id: rule_id.to_string(),
                        app: app.clone(),
                        target: *r,
                        step: s,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grouping::group;
    use crate::process::fixtures::proc;
    use crate::rules::model::{RuleCondition, RuleStep};

    fn rule(cpu: f32, for_secs: u32, ladder: Vec<(StepAction, u32)>) -> Rule {
        Rule {
            id: "r".into(),
            name: "Rule".into(),
            enabled: true,
            condition: RuleCondition {
                target: RuleTarget::AnyApp,
                cpu_above: cpu,
                for_secs,
                when: vec![],
            },
            ladder: ladder
                .into_iter()
                .map(|(action, after_secs)| RuleStep { action, after_secs })
                .collect(),
            release_after_secs: 10,
            profiles: vec![],
        }
    }

    fn set(rules: Vec<Rule>) -> RuleSet {
        RuleSet {
            rules,
            ..RuleSet::default()
        }
    }

    struct World {
        procs: Vec<ProcessSample>,
        engine: Engine,
        set: RuleSet,
        situation: Situation,
    }

    impl World {
        fn new(rules: Vec<Rule>) -> World {
            let mut a = proc(100, None, "hog.exe", 0.0, 10);
            a.exe = Some("C:\\Hog\\hog.exe".into());
            let mut b = proc(101, Some(100), "hog.exe", 0.0, 10);
            b.exe = a.exe.clone();
            World {
                procs: vec![a, b, proc(200, None, "quiet.exe", 1.0, 10)],
                engine: Engine::new(),
                set: set(rules),
                situation: Situation::default(),
            }
        }
        fn hog(&mut self, cpu: f32) {
            for p in self.procs.iter_mut().filter(|p| p.name == "hog.exe") {
                p.cpu = cpu / 2.0;
            }
        }
        fn at(&mut self, secs: i64) -> Vec<Command> {
            let groups = group(&self.procs);
            let user = vec![];
            let prot = Protection::new(&user, &[]);
            self.engine.tick(
                &self.set,
                &prot,
                &Tick {
                    now: Timestamp(secs * 1000),
                    groups: &groups,
                    processes: &self.procs,
                    situation: self.situation,
                },
            )
        }
    }

    fn applies(cmds: &[Command]) -> Vec<(u32, StepAction)> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::Apply { target, step, .. } => Some((target.pid, *step)),
                _ => None,
            })
            .collect()
    }

    fn undos(cmds: &[Command]) -> Vec<(u32, StepAction)> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::Undo { target, step, .. } => Some((target.pid, *step)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn escalates_step_by_step_then_releases_in_reverse() {
        let mut w = World::new(vec![rule(
            50.0,
            60,
            vec![
                (StepAction::LowerPriority, 0),
                (StepAction::EfficiencyMode, 30),
                (StepAction::CpuCap(20), 30),
                (StepAction::Notify, 0),
            ],
        )]);
        w.hog(80.0);
        assert!(w.at(0).is_empty());
        assert!(w.at(59).is_empty(), "not long enough yet");
        let c = w.at(60);
        assert_eq!(
            applies(&c),
            vec![(100, StepAction::LowerPriority), (101, StepAction::LowerPriority)]
        );
        assert_eq!(w.engine.active(&w.set).len(), 1);
        assert!(w.at(70).is_empty());
        assert_eq!(
            applies(&w.at(90)),
            vec![(100, StepAction::EfficiencyMode), (101, StepAction::EfficiencyMode)]
        );
        assert_eq!(applies(&w.at(120)).len(), 2);
        let notify = w.at(121);
        assert!(matches!(&notify[0], Command::Notify { app, .. } if app == "Hog"));
        let a = &w.engine.active(&w.set)[0];
        assert_eq!(a.step, 3);
        assert!(a.description.contains("capped it at 20% CPU"));
        assert_eq!(a.target.pid, 100);
        assert!(w.at(500).is_empty(), "nothing more at the top of the ladder");

        w.hog(10.0);
        assert!(w.at(501).is_empty(), "release waits");
        let c = w.at(511);
        assert_eq!(
            undos(&c),
            vec![
                (100, StepAction::CpuCap(20)),
                (100, StepAction::EfficiencyMode),
                (100, StepAction::LowerPriority),
                (101, StepAction::CpuCap(20)),
                (101, StepAction::EfficiencyMode),
                (101, StepAction::LowerPriority),
            ]
        );
        assert!(w.engine.active(&w.set).is_empty());
    }

    #[test]
    fn a_dip_resets_the_wait_and_short_dips_keep_acting() {
        let mut w = World::new(vec![rule(50.0, 30, vec![(StepAction::LowerPriority, 0)])]);
        w.hog(80.0);
        w.at(0);
        w.hog(10.0);
        w.at(20);
        w.hog(80.0);
        w.at(25);
        assert!(w.at(50).is_empty(), "the wait restarted at 25");
        assert_eq!(applies(&w.at(55)).len(), 2);
        w.hog(10.0);
        assert!(w.at(56).is_empty());
        w.hog(80.0);
        assert!(w.at(60).is_empty(), "back above: no re-apply, no release");
        w.hog(10.0);
        w.at(61);
        assert!(w.at(70).is_empty());
        assert_eq!(undos(&w.at(71)).len(), 2);
    }

    #[test]
    fn notify_first_and_new_members_catch_up() {
        let mut w = World::new(vec![rule(50.0, 5, vec![(StepAction::Notify, 0), (StepAction::LowerPriority, 10)])]);
        w.hog(90.0);
        w.at(0);
        let c = w.at(5);
        assert!(matches!(c[0], Command::Notify { .. }));
        assert!(applies(&c).is_empty());
        assert_eq!(applies(&w.at(15)).len(), 2);
        let mut late = proc(102, Some(100), "hog.exe", 10.0, 1);
        late.exe = Some("C:\\Hog\\hog.exe".into());
        late.started = Timestamp(200_000);
        w.procs.push(late);
        assert_eq!(applies(&w.at(16)), vec![(102, StepAction::LowerPriority)]);
    }

    #[test]
    fn rules_that_stop_applying_release_at_once() {
        let mut w = World::new(vec![rule(50.0, 5, vec![(StepAction::EfficiencyMode, 0)])]);
        w.hog(90.0);
        w.at(0);
        assert_eq!(applies(&w.at(5)).len(), 2);
        w.set.paused = true;
        assert_eq!(undos(&w.at(6)).len(), 2);
        w.set.paused = false;
        w.at(7);
        assert_eq!(applies(&w.at(12)).len(), 2);
        w.set.rules[0].profiles = vec![crate::rules::ProfileId::Gaming];
        assert_eq!(undos(&w.at(13)).len(), 2);
        w.set.profile = crate::rules::ProfileId::Gaming;
        w.at(14);
        assert_eq!(applies(&w.at(19)).len(), 2);
        w.situation.foreground = Some(101);
        assert_eq!(undos(&w.at(20)).len(), 2, "the app came to the front");
        assert!(w.at(30).is_empty());
        w.situation.foreground = None;
        w.at(31);
        assert_eq!(applies(&w.at(36)).len(), 2);
        w.set.rules.clear();
        assert!(undos(&w.at(37)).is_empty(), "rule deleted: forgotten (its steps can't be named)");
        assert!(w.engine.states.is_empty());
    }

    #[test]
    fn contexts_gate_rules() {
        let mut r = rule(50.0, 5, vec![(StepAction::LowerPriority, 0)]);
        r.condition.when = vec![RuleContext::UserActive, RuleContext::OnBattery];
        let mut w = World::new(vec![r]);
        w.hog(90.0);
        w.at(0);
        assert!(w.at(10).is_empty(), "unknown user and power: no");
        w.situation.user_active = Some(true);
        w.situation.power = Some(PowerSource::Battery);
        w.at(11);
        assert_eq!(applies(&w.at(16)).len(), 2);
        w.situation.power = Some(PowerSource::Ac);
        assert_eq!(undos(&w.at(17)).len(), 2);
        let s = Situation {
            user_active: Some(false),
            power: None,
            foreground: None,
        };
        assert!(s.holds(RuleContext::UserIdle));
        assert!(s.holds(RuleContext::OnAc));
    }

    #[test]
    fn app_lists_protection_and_vanishing_apps() {
        let mut r = rule(50.0, 5, vec![(StepAction::LowerPriority, 0)]);
        r.condition.target = RuleTarget::Apps(vec!["HOG.EXE".into()]);
        let mut w = World::new(vec![r]);
        w.hog(90.0);
        for p in w.procs.iter_mut().filter(|p| p.name == "quiet.exe") {
            p.cpu = 90.0;
        }
        w.at(0);
        assert_eq!(applies(&w.at(5)).len(), 2, "only the listed app");
        // The app exits: its state is dropped without undo commands for dead processes.
        w.procs.retain(|p| p.name != "hog.exe");
        assert!(w.at(6).is_empty());
        assert!(w.engine.states.is_empty());

        let mut w = World::new(vec![rule(50.0, 5, vec![(StepAction::LowerPriority, 0)])]);
        w.procs = vec![proc(300, None, "csrss.exe", 90.0, 1)];
        w.at(0);
        assert!(w.at(10).is_empty(), "protected");

        let g = AppGroup {
            id: "exe:x".into(),
            name: "My App".into(),
            exe: Some("C:\\X\\x.exe".into()),
            pids: vec![],
            cpu: 0.0,
            memory: 0,
            disk_bps: 0.0,
        };
        assert!(matches_target(&RuleTarget::Apps(vec!["my app".into()]), &g));
        assert!(matches_target(&RuleTarget::Apps(vec!["c:/x/x.exe".into()]), &g));
        assert!(!matches_target(&RuleTarget::Apps(vec!["y".into()]), &g));
        let user = vec![];
        let prot = Protection::new(&user, &[]);
        assert!(!eligible(&g, &HashMap::new(), &prot, &Situation::default()), "no live root");
    }

    #[test]
    fn protected_members_are_skipped() {
        let mut w = World::new(vec![rule(50.0, 5, vec![(StepAction::LowerPriority, 0)])]);
        w.hog(90.0);
        let user = vec!["C:/Hog/hog.exe".to_string()];
        let prot = Protection::new(&user, &[]);
        let groups = group(&w.procs);
        let tick = |now| Tick {
            now: Timestamp(now),
            groups: &groups,
            processes: &w.procs,
            situation: Situation::default(),
        };
        w.engine.tick(&w.set, &prot, &tick(0));
        assert!(w.engine.tick(&w.set, &prot, &tick(10_000)).is_empty());
        let mut map = BTreeMap::new();
        let by_pid: HashMap<Pid, &ProcessSample> = w.procs.iter().map(|p| (p.pid, p)).collect();
        let mut out = vec![];
        apply_members(&w.set.rules[0], &groups[0], 0, &mut map, &by_pid, &prot, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn snooze_and_release_all() {
        let mut w = World::new(vec![rule(
            50.0,
            5,
            vec![(StepAction::LowerPriority, 0), (StepAction::EfficiencyMode, 5)],
        )]);
        w.hog(90.0);
        w.at(0);
        w.at(5);
        w.at(10);
        let gid = group(&w.procs)[0].id.clone();
        let procs = w.procs.clone();
        let c = w.engine.snooze(&w.set, "r", &gid, &procs);
        assert_eq!(undos(&c).len(), 4);
        assert!(w.at(30).is_empty(), "snoozed while still busy");
        w.hog(10.0);
        w.at(31);
        w.hog(90.0);
        w.at(32);
        assert_eq!(applies(&w.at(37)).len(), 2, "acts again after calming down");
        let procs = w.procs.clone();
        assert_eq!(undos(&w.engine.release_all(&w.set, &procs)).len(), 2);
        assert!(w.engine.active(&w.set).is_empty());
        // Snoozing a pair that isn't acting just snoozes it; release_all of a missing rule is silent.
        assert!(w.engine.snooze(&w.set, "r", "nothing", &procs).is_empty());
        let empty = set(vec![]);
        assert!(w.engine.release_all(&empty, &procs).is_empty());
        assert!(w.engine.active(&w.set).is_empty());
    }

    #[test]
    fn processes_that_died_are_not_undone() {
        let mut w = World::new(vec![rule(50.0, 5, vec![(StepAction::LowerPriority, 0)])]);
        w.hog(90.0);
        w.at(0);
        w.at(5);
        w.procs.retain(|p| p.pid != 101);
        w.hog(0.0);
        w.at(6);
        assert_eq!(undos(&w.at(20)), vec![(100, StepAction::LowerPriority)]);
        // A state that is not acting undoes nothing; a watching state survives an odd start.
        let mut out = vec![];
        undo_state(&State::Snoozed, &w.set.rules[0], "r", &HashMap::new(), &mut out);
        assert!(out.is_empty());
        let g = &group(&w.procs)[0];
        let by_pid = HashMap::new();
        let user = vec![];
        let prot = Protection::new(&user, &[]);
        let s = maybe_start(State::Snoozed, &w.set.rules[0], g, Timestamp(0), &by_pid, &prot, &mut out);
        assert!(matches!(s, State::Snoozed));
        let s = maybe_start(
            State::Watching { since: Timestamp(0) },
            &w.set.rules[0],
            g,
            Timestamp(99_000),
            &by_pid,
            &prot,
            &mut out,
        );
        assert!(matches!(s, State::Watching { .. }), "no live root process");
    }
}
