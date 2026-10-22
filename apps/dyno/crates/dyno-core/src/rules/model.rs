//! Rules, profiles and the user's rule set (ADR 0004).

use crate::process::{Priority, ProcessAction, ProcessRef};
use crate::protect;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// A usage profile: which rules are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ProfileId {
    /// Everyday use.
    #[default]
    Normal,
    /// A game in front: everything else gets out of the way.
    Gaming,
    /// A call: keep audio and video smooth.
    Meeting,
    /// Save power.
    OnBattery,
    /// Compilers get the machine; indexers and sync clients wait.
    BuildingCode,
}

/// A profile's description for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Id.
    pub id: ProfileId,
    /// Name.
    pub name: String,
    /// One sentence.
    pub description: String,
}

/// Every profile.
pub fn profiles() -> Vec<Profile> {
    let p = |id, name: &str, description: &str| Profile {
        id,
        name: name.into(),
        description: description.into(),
    };
    vec![
        p(
            ProfileId::Normal,
            "Normal",
            "Everyday use: only the rules you enabled for every profile.",
        ),
        p(
            ProfileId::Gaming,
            "Gaming",
            "Background apps get out of the way of the game in front.",
        ),
        p(
            ProfileId::Meeting,
            "Meeting",
            "Keeps calls smooth: busy background apps are slowed down.",
        ),
        p(
            ProfileId::OnBattery,
            "On battery",
            "Saves power: busy apps run in efficiency mode, hogs are capped.",
        ),
        p(
            ProfileId::BuildingCode,
            "Building code",
            "Compilers get the CPU; indexers, sync and update clients wait.",
        ),
    ]
}

/// When a rule applies, besides its CPU condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleContext {
    /// Someone used the keyboard or mouse in the last 2 minutes.
    UserActive,
    /// Nobody did.
    UserIdle,
    /// Running on battery.
    OnBattery,
    /// Plugged in.
    OnAc,
}

/// Which apps a rule watches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleTarget {
    /// Any app (never the one in the foreground, never a protected one).
    AnyApp,
    /// Apps whose name, executable name or path matches one of these (case-insensitive).
    Apps(Vec<String>),
}

/// The condition: an app uses more than `cpu_above` percent of the machine for `for_secs`, in the given contexts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCondition {
    /// Which apps.
    pub target: RuleTarget,
    /// Percent of the whole machine, 1–100.
    pub cpu_above: f32,
    /// Seconds it must hold, 5–86 400.
    pub for_secs: u32,
    /// All of these must hold (empty: always).
    pub when: Vec<RuleContext>,
}

/// One step of an escalation ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepAction {
    /// Lower the priority to Below normal.
    LowerPriority,
    /// Turn on efficiency mode (EcoQoS / background task policy).
    EfficiencyMode,
    /// Hard-cap CPU at this percentage of the machine (1–100).
    CpuCap(u8),
    /// Tell the user.
    Notify,
}

impl StepAction {
    /// The process action that carries out this step (`None` for [`StepAction::Notify`]).
    pub fn apply(self) -> Option<ProcessAction> {
        match self {
            StepAction::LowerPriority => Some(ProcessAction::SetPriority(Priority::BelowNormal)),
            StepAction::EfficiencyMode => Some(ProcessAction::SetEfficiencyMode(true)),
            StepAction::CpuCap(pct) => Some(ProcessAction::SetCpuCap(Some(pct))),
            StepAction::Notify => None,
        }
    }

    /// The process action that undoes this step; `previous` is the priority recorded before lowering it.
    pub fn undo(self, previous: Option<Priority>) -> Option<ProcessAction> {
        match self {
            StepAction::LowerPriority => Some(ProcessAction::SetPriority(previous.unwrap_or(Priority::Normal))),
            StepAction::EfficiencyMode => Some(ProcessAction::SetEfficiencyMode(false)),
            StepAction::CpuCap(_) => Some(ProcessAction::SetCpuCap(None)),
            StepAction::Notify => None,
        }
    }

    /// Wording for the activity log.
    pub fn describe(self) -> String {
        match self {
            StepAction::LowerPriority => "lowered its priority".into(),
            StepAction::EfficiencyMode => "turned on efficiency mode".into(),
            StepAction::CpuCap(p) => format!("capped it at {p}% CPU"),
            StepAction::Notify => "notified you".into(),
        }
    }
}

/// One step and when it is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleStep {
    /// What to do.
    pub action: StepAction,
    /// Seconds after the previous step (the first: after the condition started holding for `for_secs`).
    pub after_secs: u32,
}

/// A CPU taming rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Stable id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Off by default: Dyno acts only on rules the user enabled.
    pub enabled: bool,
    /// When it fires.
    pub condition: RuleCondition,
    /// What it does, step by step, while the condition keeps holding.
    pub ladder: Vec<RuleStep>,
    /// Seconds below the threshold before everything is undone.
    pub release_after_secs: u32,
    /// Profiles it is active in (empty: all).
    pub profiles: Vec<ProfileId>,
}

/// The user's rules, protected list, profile and pause switch (persisted by the agent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSet {
    /// Rules.
    pub rules: Vec<Rule>,
    /// The user's protected executables.
    pub protected: Vec<String>,
    /// The built-in protected list (filled in for the UI; ignored when saving).
    pub built_in_protected: Vec<String>,
    /// The active profile.
    pub profile: ProfileId,
    /// Every profile (filled in for the UI; ignored when saving).
    pub profiles: Vec<Profile>,
    /// All rules paused.
    pub paused: bool,
}

impl Default for RuleSet {
    fn default() -> Self {
        RuleSet {
            rules: default_rules(),
            protected: Vec::new(),
            built_in_protected: protect::built_in(),
            profile: ProfileId::Normal,
            profiles: profiles(),
            paused: false,
        }
    }
}

/// The most rules a set may hold.
pub const MAX_RULES: usize = 100;
/// The most steps a ladder may hold.
pub const MAX_STEPS: usize = 8;

impl RuleSet {
    /// Fills in the display-only fields.
    pub fn for_display(mut self) -> RuleSet {
        self.built_in_protected = protect::built_in();
        self.profiles = profiles();
        self
    }

    /// Checks a rule set from the UI; on success returns it normalised (display fields emptied, entries trimmed).
    pub fn validate(mut self) -> Result<RuleSet, String> {
        if self.rules.len() > MAX_RULES {
            return Err(format!("At most {MAX_RULES} rules."));
        }
        if self.protected.len() > protect::MAX_USER_ENTRIES {
            return Err(format!("At most {} protected entries.", protect::MAX_USER_ENTRIES));
        }
        let mut ids = std::collections::HashSet::new();
        for r in &self.rules {
            validate_rule(r)?;
            if !ids.insert(r.id.as_str()) {
                return Err(format!("Two rules share the id {}.", r.id));
            }
        }
        self.protected = self.protected.iter().map(|e| protect::parse_entry(e)).collect::<Result<_, _>>()?;
        self.built_in_protected.clear();
        self.profiles.clear();
        Ok(self)
    }
}

/// Checks one rule.
pub fn validate_rule(r: &Rule) -> Result<(), String> {
    let name = if r.name.trim().is_empty() { "A rule" } else { r.name.as_str() };
    if r.id.trim().is_empty() || r.id.len() > 64 {
        return Err(format!("{name} needs an id of 1–64 characters."));
    }
    if r.name.trim().is_empty() || r.name.chars().count() > 100 {
        return Err("Give the rule a name (at most 100 characters).".into());
    }
    let c = &r.condition;
    if !(1.0..=100.0).contains(&c.cpu_above) {
        return Err(format!("{name}: the CPU threshold is between 1% and 100%."));
    }
    if !(5..=86_400).contains(&c.for_secs) {
        return Err(format!("{name}: \"for\" is between 5 seconds and 24 hours."));
    }
    if let RuleTarget::Apps(apps) = &c.target {
        if apps.is_empty() || apps.len() > 100 || apps.iter().any(|a| a.trim().is_empty() || a.len() > 260) {
            return Err(format!("{name}: list 1–100 apps (names or paths)."));
        }
    }
    let has = |x| c.when.contains(&x);
    if (has(RuleContext::UserActive) && has(RuleContext::UserIdle)) || (has(RuleContext::OnBattery) && has(RuleContext::OnAc)) {
        return Err(format!("{name}: those conditions can never all hold."));
    }
    if r.ladder.is_empty() || r.ladder.len() > MAX_STEPS {
        return Err(format!("{name}: add 1–{MAX_STEPS} steps."));
    }
    for s in &r.ladder {
        if let StepAction::CpuCap(p) = s.action {
            if p == 0 || p > 100 {
                return Err(format!("{name}: a CPU cap is between 1% and 100%."));
            }
        }
        if s.after_secs > 86_400 {
            return Err(format!("{name}: a step waits at most 24 hours."));
        }
    }
    if !(5..=3_600).contains(&r.release_after_secs) {
        return Err(format!("{name}: release after 5 seconds to 1 hour."));
    }
    Ok(())
}

/// The built-in rules (all off until the user enables them).
pub fn default_rules() -> Vec<Rule> {
    let step = |action, after_secs| RuleStep { action, after_secs };
    let rule = |id: &str, name: &str, target, cpu_above, for_secs, when: Vec<RuleContext>, ladder, profiles| Rule {
        id: id.into(),
        name: name.into(),
        enabled: false,
        condition: RuleCondition {
            target,
            cpu_above,
            for_secs,
            when,
        },
        ladder,
        release_after_secs: 30,
        profiles,
    };
    vec![
        rule(
            "background-hogs",
            "Tame background hogs",
            RuleTarget::AnyApp,
            40.0,
            120,
            vec![RuleContext::UserActive],
            vec![
                step(StepAction::LowerPriority, 0),
                step(StepAction::EfficiencyMode, 60),
                step(StepAction::Notify, 0),
            ],
            vec![],
        ),
        rule(
            "gaming",
            "Gaming: keep the game smooth",
            RuleTarget::AnyApp,
            15.0,
            30,
            vec![],
            vec![
                step(StepAction::LowerPriority, 0),
                step(StepAction::EfficiencyMode, 30),
                step(StepAction::CpuCap(20), 60),
            ],
            vec![ProfileId::Gaming],
        ),
        rule(
            "meeting",
            "Meeting: protect the call",
            RuleTarget::AnyApp,
            25.0,
            30,
            vec![],
            vec![step(StepAction::LowerPriority, 0), step(StepAction::EfficiencyMode, 30)],
            vec![ProfileId::Meeting],
        ),
        rule(
            "battery",
            "On battery: save power",
            RuleTarget::AnyApp,
            20.0,
            60,
            vec![RuleContext::OnBattery],
            vec![step(StepAction::EfficiencyMode, 0), step(StepAction::CpuCap(30), 120)],
            vec![ProfileId::OnBattery],
        ),
        rule(
            "build",
            "Building code: indexers and sync wait",
            RuleTarget::Apps(
                [
                    "SearchIndexer.exe",
                    "SearchProtocolHost.exe",
                    "OneDrive.exe",
                    "Dropbox.exe",
                    "GoogleDriveFS.exe",
                    "mds_stores",
                    "mdworker_shared",
                    "bird",
                    "TiWorker.exe",
                ]
                .map(String::from)
                .to_vec(),
            ),
            5.0,
            15,
            vec![],
            vec![step(StepAction::LowerPriority, 0), step(StepAction::EfficiencyMode, 15)],
            vec![ProfileId::BuildingCode],
        ),
    ]
}

/// What Dyno is doing to an app right now because of a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAction {
    /// The app's root process.
    pub target: ProcessRef,
    /// The app.
    pub app: String,
    /// The rule.
    pub rule_id: String,
    /// The rule's name.
    pub rule_name: String,
    /// The last step taken (index into the ladder).
    pub step: usize,
    /// What has been done, e.g. "lowered its priority, turned on efficiency mode".
    pub description: String,
    /// Since when.
    pub since: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_off() {
        let set = RuleSet::default();
        assert_eq!(set.profiles.len(), 5);
        assert!(set.rules.iter().all(|r| !r.enabled));
        assert!(!set.built_in_protected.is_empty());
        let v = set.clone().validate().unwrap();
        assert!(v.built_in_protected.is_empty() && v.profiles.is_empty());
        assert_eq!(v.for_display().profiles.len(), 5);
    }

    #[test]
    fn steps_map_to_actions_and_back() {
        assert_eq!(
            StepAction::LowerPriority.apply(),
            Some(ProcessAction::SetPriority(Priority::BelowNormal))
        );
        assert_eq!(
            StepAction::LowerPriority.undo(Some(Priority::High)),
            Some(ProcessAction::SetPriority(Priority::High))
        );
        assert_eq!(
            StepAction::LowerPriority.undo(None),
            Some(ProcessAction::SetPriority(Priority::Normal))
        );
        assert_eq!(StepAction::EfficiencyMode.apply(), Some(ProcessAction::SetEfficiencyMode(true)));
        assert_eq!(StepAction::EfficiencyMode.undo(None), Some(ProcessAction::SetEfficiencyMode(false)));
        assert_eq!(StepAction::CpuCap(20).apply(), Some(ProcessAction::SetCpuCap(Some(20))));
        assert_eq!(StepAction::CpuCap(20).undo(None), Some(ProcessAction::SetCpuCap(None)));
        assert_eq!(StepAction::Notify.apply(), None);
        assert_eq!(StepAction::Notify.undo(None), None);
        for s in [
            StepAction::LowerPriority,
            StepAction::EfficiencyMode,
            StepAction::CpuCap(5),
            StepAction::Notify,
        ] {
            assert!(!s.describe().is_empty());
        }
    }

    fn bad(f: impl FnOnce(&mut Rule)) -> String {
        let mut r = default_rules().remove(0);
        f(&mut r);
        validate_rule(&r).unwrap_err()
    }

    #[test]
    fn rules_are_validated() {
        assert!(bad(|r| r.id = String::new()).contains("id"));
        assert!(bad(|r| r.name = " ".into()).contains("name"));
        assert!(bad(|r| r.condition.cpu_above = 0.0).contains("threshold"));
        assert!(bad(|r| r.condition.for_secs = 1).contains("for"));
        assert!(bad(|r| r.condition.target = RuleTarget::Apps(vec![])).contains("apps"));
        assert!(bad(|r| r.condition.target = RuleTarget::Apps(vec![" ".into()])).contains("apps"));
        assert!(bad(|r| r.condition.when = vec![RuleContext::UserActive, RuleContext::UserIdle]).contains("never"));
        assert!(bad(|r| r.condition.when = vec![RuleContext::OnBattery, RuleContext::OnAc]).contains("never"));
        assert!(bad(|r| r.ladder.clear()).contains("steps"));
        assert!(bad(|r| r.ladder[0].action = StepAction::CpuCap(0)).contains("cap"));
        assert!(bad(|r| r.ladder[0].after_secs = 100_000).contains("24 hours"));
        assert!(bad(|r| r.release_after_secs = 1).contains("release"));
    }

    #[test]
    fn sets_are_validated() {
        let mut s = RuleSet::default();
        s.rules.push(s.rules[0].clone());
        assert!(s.validate().unwrap_err().contains("share"));
        let mut s = RuleSet::default();
        s.rules = (0..=MAX_RULES).map(|_| default_rules().remove(0)).collect();
        assert!(s.validate().is_err());
        let mut s = RuleSet::default();
        s.protected = vec![String::new()];
        assert!(s.validate().is_err());
        let mut s = RuleSet::default();
        s.protected = vec!["x".into(); protect::MAX_USER_ENTRIES + 1];
        assert!(s.validate().is_err());
        let mut s = RuleSet::default();
        s.rules[0].condition.cpu_above = 500.0;
        assert!(s.validate().is_err());
        let mut s = RuleSet::default();
        s.protected = vec!["  game.exe ".into()];
        assert_eq!(s.validate().unwrap().protected, vec!["game.exe".to_string()]);
    }
}
