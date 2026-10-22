//! Tuning suggestions: each explained, previewed, applied only on the user's click through the helper, and
//! reversible (the inverse action is recorded when it is applied).

use crate::cleanup::Os;
use crate::facts::SystemFacts;
use crate::helper::{scheme_name, HelperAction, BALANCED_SCHEME};
use crate::metrics::{PowerSource, Reading};
use serde::{Deserialize, Serialize};

/// A suggestion as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningSuggestion {
    /// Id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Why, and what it costs.
    pub explanation: String,
    /// The setting now.
    pub current: String,
    /// The suggested setting.
    pub proposed: String,
    /// Needs the helper's admin prompt.
    pub needs_admin: bool,
    /// Already applied by Dyno (offers Revert).
    pub applied: bool,
}

/// A suggestion with the change that applies it and the change that reverts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The suggestion.
    pub suggestion: TuningSuggestion,
    /// Applies it.
    pub apply: HelperAction,
    /// Reverts it.
    pub revert: HelperAction,
}

/// Every suggestion that fits the machine now. `applied` are the ids Dyno applied before (shown with Revert even if
/// their trigger no longer holds, so the user can always undo).
pub fn plans(facts: &SystemFacts, os: Os, power: Option<PowerSource>, applied: &[String]) -> Vec<Plan> {
    let mut out = Vec::new();
    let mut push =
        |id: &str, title: &str, explanation: String, current: String, proposed: String, apply: HelperAction, revert: HelperAction| {
            out.push(Plan {
                suggestion: TuningSuggestion {
                    id: id.into(),
                    title: title.into(),
                    explanation,
                    current,
                    proposed,
                    needs_admin: apply.needs_admin(),
                    applied: applied.iter().any(|a| a == id),
                },
                apply,
                revert,
            });
        };
    let dev: Vec<String> = facts.dev_folders.iter().map(|d| d.path.clone()).collect();

    if os == Os::Windows {
        if let Reading::Available(plan) = &facts.power_plan {
            if plan.power_saver && power != Some(PowerSource::Battery) {
                push(
                    "powerPlan",
                    "Use the Balanced power plan while plugged in",
                    "Power saver keeps the CPU slow even on mains power. Balanced speeds up when you need it and still saves \
                     power when idle."
                        .into(),
                    plan.name.clone(),
                    "Balanced".into(),
                    HelperAction::SetPowerScheme(BALANCED_SCHEME.into()),
                    HelperAction::SetPowerScheme(plan.id.clone()),
                );
            }
        }
        if let Reading::Available(true) = facts.visual_effects_full {
            push(
                "visualEffects",
                "Turn off animations and transparency",
                "On older or busy machines, window animations, shadows and transparency cost GPU and CPU time. Windows \
                 looks plainer afterwards."
                    .into(),
                "Best appearance".into(),
                "Best performance".into(),
                HelperAction::SetVisualEffects(2),
                HelperAction::SetVisualEffects(0),
            );
        }
        if let Reading::Available(n) = facts.background_apps {
            if n >= 10 {
                push(
                    "backgroundApps",
                    "Stop apps from running in the background",
                    format!(
                        "{n} Store apps may run in the background to fetch updates and show notifications. Turning this off \
                         saves memory and battery; those apps update when you open them."
                    ),
                    format!("{n} apps allowed"),
                    "Off".into(),
                    HelperAction::SetBackgroundAppsDisabled(true),
                    HelperAction::SetBackgroundAppsDisabled(false),
                );
            }
        }
    }
    let indexed: Vec<String> = facts
        .dev_folders
        .iter()
        .filter(|d| d.indexed == Some(true))
        .map(|d| d.path.clone())
        .collect();
    if !indexed.is_empty() {
        push(
            "indexDevFolders",
            "Stop indexing your code folders",
            format!(
                "Search indexing re-reads build output every time you compile ({} folder{}). Excluding them keeps builds \
                 fast; you can still search them in your editor.",
                indexed.len(),
                if indexed.len() == 1 { "" } else { "s" }
            ),
            "Indexed".into(),
            "Not indexed".into(),
            HelperAction::SetIndexingExcluded {
                paths: indexed.clone(),
                excluded: true,
            },
            HelperAction::SetIndexingExcluded {
                paths: indexed,
                excluded: false,
            },
        );
    }
    if os == Os::Windows && !dev.is_empty() && facts.dev_folders.iter().any(|d| d.av_excluded != Some(true)) {
        let paths: Vec<String> = dev.iter().take(crate::helper::MAX_PATHS).cloned().collect();
        push(
            "defenderDevFolders",
            "Exclude your code folders from Defender scanning",
            "Defender scans every file a build writes, which can double build times. Excluding your own project folders \
             is a common developer setting — but files there are no longer scanned, so only do this for code you trust."
                .into(),
            "Scanned".into(),
            "Excluded".into(),
            HelperAction::SetDefenderExcluded {
                paths: paths.clone(),
                excluded: true,
            },
            HelperAction::SetDefenderExcluded { paths, excluded: false },
        );
    }
    if os == Os::Macos {
        if let Reading::Available(plan) = &facts.power_plan {
            if plan.power_saver && power != Some(PowerSource::Battery) {
                push(
                    "lowPowerMode",
                    "Turn off Low Power Mode while plugged in",
                    "Low Power Mode slows the processor to save battery, even on the charger.".into(),
                    "On".into(),
                    "Off".into(),
                    HelperAction::SetLowPowerMode(false),
                    HelperAction::SetLowPowerMode(true),
                );
            }
        }
    }
    out
}

/// The plan with this id, if its trigger holds now (an applied change whose trigger is gone is listed with
/// [`applied_entry`] and reverted from the recorded action).
pub fn find<'a>(plans: &'a [Plan], id: &str) -> Option<&'a Plan> {
    plans.iter().find(|p| p.suggestion.id == id)
}

/// A suggestion entry for a change Dyno applied whose trigger no longer shows (so it can still be reverted).
pub fn applied_entry(id: &str, revert: &HelperAction) -> TuningSuggestion {
    TuningSuggestion {
        id: id.into(),
        title: match id {
            "powerPlan" => "Balanced power plan (applied by Dyno)".into(),
            "visualEffects" => "Animations and transparency off (applied by Dyno)".into(),
            "backgroundApps" => "Background apps off (applied by Dyno)".into(),
            "indexDevFolders" => "Code folders not indexed (applied by Dyno)".into(),
            "defenderDevFolders" => "Code folders excluded from Defender (applied by Dyno)".into(),
            "lowPowerMode" => "Low Power Mode off (applied by Dyno)".into(),
            other => format!("{other} (applied by Dyno)"),
        },
        explanation: "Dyno applied this change. Revert puts the previous setting back.".into(),
        current: "Applied".into(),
        proposed: match revert {
            HelperAction::SetPowerScheme(g) => scheme_name(g),
            other => other.preview().join("; "),
        },
        needs_admin: revert.needs_admin(),
        applied: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{DevFolder, PowerPlan};
    use crate::helper::POWER_SAVER_SCHEME;

    fn facts() -> SystemFacts {
        let mut f = SystemFacts::unknown("Windows 11");
        f.power_plan = Reading::Available(PowerPlan {
            id: POWER_SAVER_SCHEME.into(),
            name: "Power saver".into(),
            power_saver: true,
        });
        f.visual_effects_full = Reading::Available(true);
        f.background_apps = Reading::Available(14);
        f.dev_folders = vec![
            DevFolder {
                path: "C:\\Users\\me\\src\\dyno".into(),
                indexed: Some(true),
                av_excluded: None,
            },
            DevFolder {
                path: "C:\\Users\\me\\src\\web".into(),
                indexed: Some(false),
                av_excluded: Some(true),
            },
        ];
        f
    }

    #[test]
    fn windows_suggestions() {
        let p = plans(&facts(), Os::Windows, Some(PowerSource::Ac), &["visualEffects".into()]);
        let ids: Vec<&str> = p.iter().map(|p| p.suggestion.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "powerPlan",
                "visualEffects",
                "backgroundApps",
                "indexDevFolders",
                "defenderDevFolders"
            ]
        );
        assert!(p[1].suggestion.applied);
        assert!(!p[0].suggestion.applied);
        assert_eq!(p[0].revert, HelperAction::SetPowerScheme(POWER_SAVER_SCHEME.into()));
        assert!(p[4].suggestion.needs_admin);
        assert!(p[3].suggestion.explanation.contains("1 folder)"));
        assert!(p.iter().all(|p| p.apply.validate().is_ok() && p.revert.validate().is_ok()));
        assert!(find(&p, "backgroundApps").is_some());
        assert!(find(&p, "nope").is_none());
    }

    #[test]
    fn nothing_to_suggest_on_battery_or_a_tuned_machine() {
        let p = plans(&facts(), Os::Windows, Some(PowerSource::Battery), &[]);
        assert!(p.iter().all(|p| p.suggestion.id != "powerPlan"));
        let mut f = SystemFacts::unknown("Windows 11");
        f.background_apps = Reading::Available(3);
        assert!(plans(&f, Os::Windows, None, &[]).is_empty());
    }

    #[test]
    fn mac_suggestions() {
        let mut f = facts();
        f.dev_folders[1].indexed = Some(true);
        let p = plans(&f, Os::Macos, None, &[]);
        let ids: Vec<&str> = p.iter().map(|p| p.suggestion.id.as_str()).collect();
        assert_eq!(ids, ["indexDevFolders", "lowPowerMode"]);
        assert!(p[0].suggestion.explanation.contains("2 folders"));
    }

    #[test]
    fn applied_entries_can_be_reverted() {
        for id in [
            "powerPlan",
            "visualEffects",
            "backgroundApps",
            "indexDevFolders",
            "defenderDevFolders",
            "lowPowerMode",
            "other",
        ] {
            let e = applied_entry(id, &HelperAction::SetVisualEffects(0));
            assert!(e.applied && e.title.contains("applied by Dyno"));
        }
        assert_eq!(
            applied_entry("powerPlan", &HelperAction::SetPowerScheme(POWER_SAVER_SCHEME.into())).proposed,
            "Power saver"
        );
    }
}
