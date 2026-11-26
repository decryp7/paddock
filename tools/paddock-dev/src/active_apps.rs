//! The active apps of train and branch CI: the one accessor of `docs/ci-active-apps.toml` (ER-42).
//!
//! The user decided on 2026-11-24 to run the foundation only (the shared crates, the ui packages, paddock-dev, conformance, icons,
//! eslint and the workflow and gates checks) and to bring the apps back one at a time: train and branch CI plan the foundation plus
//! the apps the set names, and no job of another app is dispatched. The nightly ignores the set and runs every app (the full check).
//!
//! Its readers: `./dev plan --ci` (an inactive app gets no leg, a `::notice::inactive: ...` line, a line in the job summary, and the
//! `inactive` list of the declared scope, [`crate::run_scope`]), `./dev plan --ci --app` (an app's own workflow: `false` and
//! `inactive=true` for an inactive app), `./dev ci scope` (an app workflow's scope line) and `./dev ci verdict --tag --app` (a tag rests
//! only on a run that covered the app, ER-15). An app made active is planned in full by the run whose base did not have it active
//! ([`resume`]), so activating an app is one line in the file plus that first full run. docs/ci-schedules.md, "Active apps", is the
//! how-to.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The file, relative to the repository root.
pub const FILE: &str = "docs/ci-active-apps.toml";

/// The set, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveApps {
    /// The apps train and branch CI plan beside the foundation.
    pub active: BTreeSet<String>,
    /// The day of the decision (`YYYY-MM-DD`).
    pub decided: String,
    /// Who decided (the user).
    pub by: String,
    /// Why.
    pub reason: String,
    /// The order in which the inactive apps are planned to come back (data for people; the planner reads only `active`).
    pub next: Vec<String>,
}

fn is_day(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter()
            .enumerate()
            .all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

fn names(t: &toml::Table, key: &str) -> Result<Vec<String>, String> {
    let list = t
        .get(key)
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{FILE}: `{key}` is missing or not a list of app names"))?;
    let mut out = Vec::new();
    for v in list {
        let name = v
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{FILE}: `{key}` holds {v:?}, not an app name"))?;
        if out.iter().any(|n| n == name) {
            return Err(format!("{FILE}: `{key}` names {name} twice"));
        }
        out.push(name.to_string());
    }
    Ok(out)
}

impl ActiveApps {
    /// The set from the file's text; an unknown key, a missing field or a malformed name is an error, never "no app" (ER-33).
    pub fn parse(text: &str) -> Result<ActiveApps, String> {
        let t: toml::Table = text.parse().map_err(|e| format!("{FILE} is not TOML: {e}"))?;
        if let Some(k) = t
            .keys()
            .find(|k| !["active", "decided", "by", "reason", "next"].contains(&k.as_str()))
        {
            return Err(format!("{FILE}: unknown key {k:?} (keys: active, decided, by, reason, next)"));
        }
        let text_of = |k: &str| -> Result<String, String> {
            t.get(k)
                .and_then(toml::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("{FILE}: `{k}` is missing or empty"))
        };
        let decided = text_of("decided")?;
        if !is_day(&decided) {
            return Err(format!("{FILE}: `decided` {decided:?} is not a day (YYYY-MM-DD)"));
        }
        Ok(ActiveApps {
            active: names(&t, "active")?.into_iter().collect(),
            decided,
            by: text_of("by")?,
            reason: text_of("reason")?,
            next: names(&t, "next")?,
        })
    }

    /// The set of the checkout at `root`.
    pub fn load(root: &Path) -> Result<ActiveApps, String> {
        let text = std::fs::read_to_string(root.join(FILE)).map_err(|e| format!("{FILE} cannot be read ({e})"))?;
        ActiveApps::parse(&text)
    }

    /// `Err` when `active` or `next` names an app the tree does not have (`apps/<app>/paddock.ci.json`): a typo must not turn an
    /// app off or claim one on.
    pub fn check_known(&self, apps: &BTreeSet<String>) -> Result<(), String> {
        match self.active.iter().chain(&self.next).find(|a| !apps.contains(*a)) {
            Some(a) => Err(format!("{FILE} names {a}, which is not an app (no apps/{a}/paddock.ci.json)")),
            None => Ok(()),
        }
    }
}

/// The apps active at a revision, read through `file_at`: without the file every app (before the set existed every app ran); with a
/// file that cannot be read none (fail closed: [`resume`] then plans the head's active apps in full).
pub fn active_at(apps: &BTreeSet<String>, file_at: &dyn Fn(&str) -> Result<Option<String>, String>) -> Result<BTreeSet<String>, String> {
    Ok(match file_at(FILE)? {
        None => apps.clone(),
        Some(text) => ActiveApps::parse(&text).map(|a| a.active).unwrap_or_default(),
    })
}

/// Whether this run plans every app whatever the set says: the nightly (a `schedule` event, or the nightly's `os_full` input, which
/// also marks its manual dispatch and a manual ask for the full check).
pub fn every_app(env: &dyn Fn(&str) -> Option<String>) -> bool {
    env("GITHUB_EVENT_NAME").as_deref() == Some("schedule") || env("PLAN_FULL_OS").is_some_and(|v| v == "true" || v == "1")
}

/// The inactive apps of a run: none when it plans every app, else the apps the set does not name, less the ones the run asks for by
/// name (`also`: an app's own gates, `PLAN_APP` or a tag of it).
pub fn inactive(set: &ActiveApps, apps: &BTreeSet<String>, every: bool, also: &[&str]) -> BTreeSet<String> {
    if every {
        return BTreeSet::new();
    }
    apps.iter()
        .filter(|a| !set.active.contains(*a) && !also.contains(&a.as_str()))
        .cloned()
        .collect()
}

/// Takes the inactive apps out of a plan (`apps`, `kinds`, the graph's selection and `cargoOnly`) and lists them as the plan's
/// `inactive`, so nothing downstream (the matrix, the declared scope, the escape monitor) plans them and the output names them. A plan
/// with none is left as it is.
pub fn apply(plan: &mut Value, inactive: &BTreeSet<String>) {
    if inactive.is_empty() {
        return;
    }
    let off = |a: &str| inactive.contains(a);
    if let Some(apps) = plan["apps"].as_array_mut() {
        apps.retain(|a| !a.as_str().is_some_and(off));
    }
    if let Some(kinds) = plan["kinds"].as_object_mut() {
        kinds.retain(|a, _| !off(a));
    }
    if let Some(graph) = plan["graph"].as_object_mut() {
        graph.retain(|id, _| !id.strip_prefix("app:").is_some_and(off));
    }
    if let Some(only) = plan["cargoOnly"].as_array_mut() {
        only.retain(|p| !p.as_str().and_then(|p| p.strip_prefix("app:")).is_some_and(off));
    }
    plan["inactive"] = json!(inactive);
}

/// The apps active here and not at the run's base (made active by the change): each is planned in full, every leg and both halves,
/// whatever the change reached, so the activating commit's run is the app's first full run. Returns them.
pub fn resume(plan: &mut Value, base_active: &BTreeSet<String>, head_inactive: &BTreeSet<String>, apps: &BTreeSet<String>) -> Vec<String> {
    let resumed: Vec<String> = apps
        .iter()
        .filter(|a| !base_active.contains(*a) && !head_inactive.contains(*a))
        .cloned()
        .collect();
    for app in &resumed {
        if !plan["apps"].is_array() {
            plan["apps"] = json!([]);
        }
        if let Some(a) = plan["apps"].as_array_mut()
            && !a.iter().any(|x| x == app.as_str())
        {
            a.push(json!(app));
        }
        if !plan["kinds"].is_object() {
            plan["kinds"] = json!({});
        }
        plan["kinds"][app.as_str()] = json!(["rust", "ui"]);
        if let Some(graph) = plan["graph"].as_object_mut() {
            graph.remove(&format!("app:{app}"));
        }
        if let Some(only) = plan["cargoOnly"].as_array_mut() {
            only.retain(|p| p.as_str() != Some(&format!("app:{app}")));
        }
    }
    if !resumed.is_empty()
        && let Some(a) = plan["apps"].as_array_mut()
    {
        let mut names: Vec<String> = a.iter().filter_map(Value::as_str).map(str::to_string).collect();
        names.sort();
        names.dedup();
        *a = names.into_iter().map(Value::String).collect();
    }
    resumed
}

/// The line a plan prints for its inactive apps (log, notice and job summary); `None` when every app is planned.
pub fn line(inactive: &BTreeSet<String>) -> Option<String> {
    (!inactive.is_empty()).then(|| {
        format!(
            "inactive: {} ({FILE} does not name them: no job of theirs runs here, nothing of theirs is green; the nightly runs them)",
            inactive.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    })
}

/// What a red nightly means (the green-main rule of 2026-11-24): failures in the foundation or an active app stop the next train;
/// failures in an inactive app are that app's fix items (they block its tag, never a train).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NightlyRed {
    /// The failed jobs that stop the next train, as `workflow: job`.
    pub blocking: Vec<String>,
    /// The failed jobs of each inactive app, as `workflow: job`: its fix items.
    pub fix_items: BTreeMap<String, Vec<String>>,
}

/// The app a failed job of a nightly belongs to, `None` for the foundation: an app's own workflow (`debrief.yml`, `<app>-*.yml`),
/// a job whose entry in the run's scope line (`expected`, also under a caller's `<job> / ` prefix) has an `app` leg, or a nightly
/// installers job (`installers (<App>, ...)`, a name the planner does not write: registered in docs/known-copies.toml).
pub fn app_of_job(workflow: &str, job: &str, apps: &BTreeSet<String>, expected: &crate::run_scope::ExpectedMap) -> Option<String> {
    if let Some(a) = crate::plan::app_of_workflow(&format!(".gitea/workflows/{workflow}"), apps) {
        return Some(a.to_string());
    }
    if let Some(a) = crate::run_scope::lookup_entry(expected, job)
        .and_then(|e| e.leg.get("app"))
        .filter(|a| apps.contains(*a))
    {
        return Some(a.clone());
    }
    let lower = crate::run_scope::strip_caller(job).to_ascii_lowercase();
    apps.iter()
        .find(|a| lower.starts_with(&format!("installers ({}", a.to_ascii_lowercase())))
        .cloned()
}

/// Sorts a nightly's failed jobs (`(workflow file, job name)`) by [`NightlyRed`] under the active set of the nightly's commit.
pub fn classify_nightly(
    failed: &[(String, String)],
    active: &BTreeSet<String>,
    apps: &BTreeSet<String>,
    expected: &crate::run_scope::ExpectedMap,
) -> NightlyRed {
    let mut red = NightlyRed::default();
    for (workflow, job) in failed {
        let what = format!("{workflow}: {job}");
        match app_of_job(workflow, job, apps, expected) {
            Some(app) if !active.contains(&app) => red.fix_items.entry(app).or_default().push(what),
            _ => red.blocking.push(what),
        }
    }
    red
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str =
        "decided = \"2026-11-24\"\nby = \"user\"\nreason = \"foundation first\"\nactive = []\nnext = [\"dyno\", \"pitwall\"]\n";

    fn apps() -> BTreeSet<String> {
        ["debrief", "dyno", "pitwall", "shell-demo"].map(String::from).into()
    }

    #[test]
    fn the_set_is_read_strictly_and_names_only_known_apps() {
        let set = ActiveApps::parse(TEXT).unwrap();
        assert!(set.active.is_empty());
        assert_eq!(set.next, ["dyno", "pitwall"]);
        assert_eq!(set.check_known(&apps()), Ok(()));
        for bad in [
            TEXT.replace("active = []", "active = [\"dyno\", \"dyno\"]"),
            TEXT.replace("active = []", "active = \"dyno\""),
            TEXT.replace("active = []\n", ""),
            TEXT.replace("2026-11-24", "24.11.2026"),
            format!("{TEXT}actve = []\n"),
        ] {
            assert!(ActiveApps::parse(&bad).is_err(), "{bad}");
        }
        let typo = ActiveApps::parse(&TEXT.replace("active = []", "active = [\"dyon\"]")).unwrap();
        assert!(typo.check_known(&apps()).unwrap_err().contains("dyon"));
    }

    #[test]
    fn a_run_plans_the_active_apps_the_nightly_every_app_and_an_apps_own_gates_that_app() {
        let mut set = ActiveApps::parse(TEXT).unwrap();
        assert_eq!(inactive(&set, &apps(), false, &[]), apps());
        assert!(inactive(&set, &apps(), true, &[]).is_empty(), "the nightly ignores the set");
        assert!(
            !inactive(&set, &apps(), false, &["dyno"]).contains("dyno"),
            "an app's own gates plan it"
        );
        set.active.insert("dyno".into());
        assert!(!inactive(&set, &apps(), false, &[]).contains("dyno"));
        let env = |k: &str| (k == "GITHUB_EVENT_NAME").then(|| "schedule".to_string());
        assert!(every_app(&env));
        assert!(!every_app(&|k: &str| (k == "GITHUB_EVENT_NAME").then(|| "push".to_string())));
        assert!(every_app(&|k: &str| (k == "PLAN_FULL_OS").then(|| "true".to_string())));
    }

    #[test]
    fn apply_takes_the_inactive_apps_out_and_lists_them() {
        let mut plan = json!({"apps": ["a", "b"], "kinds": {"a": ["rust"], "b": ["ui"]}, "graph": {"app:a": {}, "crates": {}},
                              "cargoOnly": ["app:a", "crates"]});
        apply(&mut plan, &["a".to_string()].into());
        assert_eq!(plan["apps"], json!(["b"]));
        assert!(plan["kinds"].get("a").is_none() && plan["graph"].get("app:a").is_none());
        assert_eq!(plan["cargoOnly"], json!(["crates"]));
        assert_eq!(plan["inactive"], json!(["a"]));
        // Positive control: with none inactive the plan is untouched.
        let before = json!({"apps": ["a"]});
        let mut same = before.clone();
        apply(&mut same, &BTreeSet::new());
        assert_eq!(same, before);
    }

    #[test]
    fn an_app_made_active_since_the_base_is_planned_in_full() {
        let apps: BTreeSet<String> = ["a", "b"].map(String::from).into();
        let mut plan = json!({"apps": [], "kinds": {}, "graph": {"app:a": {"modes": ["smoke"]}}, "cargoOnly": ["app:a"]});
        // `a` is active here (not inactive) and was not at the base; `b` is inactive here.
        assert_eq!(resume(&mut plan, &BTreeSet::new(), &["b".to_string()].into(), &apps), ["a"]);
        assert_eq!(plan["apps"], json!(["a"]));
        assert_eq!(plan["kinds"]["a"], json!(["rust", "ui"]));
        assert!(plan["graph"].get("app:a").is_none() && plan["cargoOnly"] == json!([]));
        // Positive control: an app active at the base too is left to the change's own plan.
        let mut same = json!({"apps": []});
        assert!(resume(&mut same, &apps, &["b".to_string()].into(), &apps).is_empty());
        assert_eq!(same["apps"], json!([]));
    }

    #[test]
    fn the_set_at_a_base_without_the_file_is_every_app_and_an_unreadable_one_is_none() {
        assert_eq!(active_at(&apps(), &|_| Ok(None)).unwrap(), apps());
        assert!(active_at(&apps(), &|_| Ok(Some("not toml [".into()))).unwrap().is_empty());
        let one = TEXT.replace("active = []", "active = [\"dyno\"]");
        assert_eq!(active_at(&apps(), &|_| Ok(Some(one.clone()))).unwrap(), ["dyno".to_string()].into());
    }

    #[test]
    fn a_nightly_red_in_an_inactive_app_is_its_fix_item_and_a_red_elsewhere_stops_trains() {
        let active: BTreeSet<String> = ["dyno".to_string()].into();
        let failed = [
            (
                "nightly.yml".to_string(),
                "everything / app pitwall (ubuntu-latest, e2e-1)".to_string(),
            ),
            ("nightly.yml".to_string(), "installers (Pitwall, no publish)".to_string()),
            ("debrief.yml".to_string(), "e2e (shard 2)".to_string()),
            ("nightly.yml".to_string(), "everything / app dyno (macos-latest)".to_string()),
            ("nightly.yml".to_string(), "everything / crate (windows-latest)".to_string()),
        ];
        let app = |a: &str| crate::run_scope::Expected {
            job: "app".into(),
            leg: [("app".to_string(), a.to_string())].into(),
            ..Default::default()
        };
        let expected = crate::run_scope::ExpectedMap::from([
            ("app pitwall (ubuntu-latest, e2e-1)".to_string(), app("pitwall")),
            ("app dyno (macos-latest)".to_string(), app("dyno")),
        ]);
        let red = classify_nightly(&failed, &active, &apps(), &expected);
        assert_eq!(red.fix_items["pitwall"].len(), 2);
        assert_eq!(red.fix_items["debrief"], ["debrief.yml: e2e (shard 2)"]);
        assert_eq!(
            red.blocking,
            [
                "nightly.yml: everything / app dyno (macos-latest)",
                "nightly.yml: everything / crate (windows-latest)"
            ]
        );
        // Positive control: with pitwall active its failures stop trains too.
        let all = classify_nightly(&failed, &apps(), &apps(), &expected);
        // Without the run's line only the workflow's own app and the installers name are known: the app job counts as shared.
        let bare = classify_nightly(&failed, &active, &apps(), &crate::run_scope::ExpectedMap::new());
        assert_eq!(bare.fix_items["pitwall"].len(), 1, "{bare:?}");
        assert!(all.fix_items.is_empty() && all.blocking.len() == failed.len());
    }
}
