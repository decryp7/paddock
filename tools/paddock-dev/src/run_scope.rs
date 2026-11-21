//! The scope of a CI run, declared as data (ADR 0113). A plan job prints one line, `plan scope: {json}`, and `./dev ci verdict`
//! (the one reader, [`crate::ci_runs::verdict`]) decides "complete" by comparing what the run did (`ran`) with what a push to main
//! does for the same tree (`need`): complete means `ran` covers `need`, jobs, engines, apps and whole-workspace tests alike.
//! No workflow decides it any more, so a smaller scope cannot be written as "complete" by a shell line that forgot a dimension
//! (run 8335; the schedule-skipped and rerun-only runs counted green on 2026-11-12; the WebKit hole of the D4 reuse).
//!
//! Producers: `./dev plan --ci` for ci.yml ([`ci_scope`]), `./dev ci scope` for the app workflows ([`app_scope`]).
//! The reader: [`read`] and [`Declared::complete`].

use crate::log_text::plain;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The prefix of the scope line (after the colours and the timestamp).
pub const SCOPE_LINE: &str = "plan scope: ";

/// The version of the line's shape; a reader refuses another.
pub const VERSION: u64 = 1;

/// The runners the `crate` and `paddock-dev` jobs run on when the plan names none of its own (ci.yml's `PLAN_OS_LEGS`).
pub const DEFAULT_OS_LEGS: [&str; 3] = ["ubuntu-latest", "windows-latest", "macos-latest"];

/// The project flags of a plan that each decide a job of ci.yml.
const PROJECTS: [&str; 8] = ["crates", "ui", "eslint", "conformance", "c99", "tools", "icons", "paddockDev"];

/// What a run covers: jobs, the browser engines of its tests, apps (a leg is `app@os` with an e2e leg appended) and whether its
/// tests ran whole (not narrowed to packages).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// The jobs the plan switched on (`crates`, `crate@macos-latest`, ...).
    pub jobs: BTreeSet<String>,
    /// The browser engines of the tests (`chromium`, `webkit`).
    pub engines: BTreeSet<String>,
    /// The app legs (`dyno@ubuntu-latest`, `dyno@ubuntu-latest:e2e-webkit`).
    pub apps: BTreeSet<String>,
    /// True when no test run was narrowed to a package list.
    pub whole: bool,
}

fn set(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

impl Scope {
    /// The scope as JSON (sorted lists).
    pub fn json(&self) -> Value {
        json!({"jobs": self.jobs, "engines": self.engines, "apps": self.apps, "whole": self.whole})
    }

    fn parse(v: &Value, what: &str) -> Result<Scope, String> {
        for key in ["jobs", "engines", "apps"] {
            if !v[key].is_array() {
                return Err(format!("{what}.{key} is missing or not a list"));
            }
        }
        let whole = v["whole"]
            .as_bool()
            .ok_or_else(|| format!("{what}.whole is missing or not a boolean"))?;
        Ok(Scope {
            jobs: set(&v["jobs"]),
            engines: set(&v["engines"]),
            apps: set(&v["apps"]),
            whole,
        })
    }

    /// What `need` has that `self` lacks, one phrase per dimension; empty when `self` covers it.
    fn missing(&self, need: &Scope) -> Vec<String> {
        let mut out = Vec::new();
        for (name, have, want) in [
            ("jobs", &self.jobs, &need.jobs),
            ("engines", &self.engines, &need.engines),
            ("apps", &self.apps, &need.apps),
        ] {
            let gone: Vec<&String> = want.difference(have).collect();
            if !gone.is_empty() {
                out.push(format!("{name} {}", gone.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
            }
        }
        if need.whole && !self.whole {
            out.push("whole-workspace tests (a member run narrowed them to packages)".into());
        }
        out
    }
}

/// What one plan job declared: the workflow, whether it is a rerun of failures, what it ran and what a push to main needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The workflow file (`ci.yml`): a line of another workflow is not evidence.
    pub workflow: String,
    /// A rerun of an earlier run's failures: never complete, whatever it ran.
    pub rerun: bool,
    /// What the run did.
    pub ran: Scope,
    /// What a push to main does for this tree.
    pub need: Scope,
}

impl Declared {
    /// The scope line.
    pub fn line(&self) -> String {
        let v = json!({"v": VERSION, "workflow": self.workflow, "rerun": self.rerun, "ran": self.ran.json(), "need": self.need.json()});
        format!("{SCOPE_LINE}{v}")
    }

    /// `Ok` when the run is complete: not a rerun, `need` is not empty (an empty need proves nothing), and `ran` covers `need`.
    /// `Err` names what is missing.
    pub fn complete(&self) -> Result<(), String> {
        if self.rerun {
            return Err("it is a rerun of an earlier run's failures".into());
        }
        if self.need.jobs.is_empty() && self.need.apps.is_empty() {
            return Err("its declared need is empty (a scope that needs nothing proves nothing)".into());
        }
        let gone = self.ran.missing(&self.need);
        if gone.is_empty() {
            Ok(())
        } else {
            Err(format!("it did not cover what a push to main does: {}", gone.join("; ")))
        }
    }
}

/// The declaration a plan job's log holds: `None` when no line starts with [`SCOPE_LINE`], `Err` for a line that cannot be read
/// (not JSON, another version, a missing field) or a second line (ambiguous: only one plan job prints one).
pub fn read(log: &str) -> Option<Result<Declared, String>> {
    let lines: Vec<String> = log
        .lines()
        .map(plain)
        .filter_map(|l| l.trim().strip_prefix(SCOPE_LINE).map(str::to_string))
        .collect();
    let first = lines.first()?;
    if lines.len() > 1 {
        return Some(Err(format!("the plan job logged {} scope lines, not one", lines.len())));
    }
    Some(parse(first))
}

fn parse(text: &str) -> Result<Declared, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("the scope line is not JSON ({e})"))?;
    if v["v"].as_u64() != Some(VERSION) {
        return Err(format!("the scope line has version {}, this reader knows {VERSION}", v["v"]));
    }
    Ok(Declared {
        workflow: v["workflow"].as_str().ok_or("the scope line has no workflow")?.to_string(),
        rerun: v["rerun"].as_bool().ok_or("the scope line has no rerun flag")?,
        ran: Scope::parse(&v["ran"], "ran")?,
        need: Scope::parse(&v["need"], "need")?,
    })
}

/// A leg's identity in [`Scope::apps`]: `app@os`, with `:e2e-n` or `:e2e-webkit` for a derived e2e leg.
fn leg_id(leg: &Value) -> String {
    let e2e = leg["e2e_leg"].as_str().unwrap_or("");
    let base = format!("{}@{}", leg["app"].as_str().unwrap_or(""), leg["os"].as_str().unwrap_or(""));
    if e2e.is_empty() { base } else { format!("{base}:{e2e}") }
}

/// The scope of a ci.yml plan: the project jobs it switched on (and the crate and paddock-dev jobs on each runner), the app legs,
/// the engines (WebKit only when `webkit` and a UI is reached), and whether every test ran whole.
pub fn scope_of(plan: &Value, legs: &[Value], webkit: bool, os_legs: &[String], package_filters: &[&str]) -> Scope {
    let mut jobs: BTreeSet<String> = PROJECTS
        .iter()
        .filter(|p| plan[**p].as_bool() == Some(true))
        .map(|p| p.to_string())
        .collect();
    for (flag, job) in [("crates", "crate"), ("paddockDev", "paddock-dev")] {
        if plan[flag].as_bool() == Some(true) {
            jobs.extend(os_legs.iter().map(|os| format!("{job}@{os}")));
        }
    }
    let ui_leg = legs
        .iter()
        .any(|l| l["ui"].as_bool() == Some(true) || l["e2e_leg"].as_str().is_some_and(|e| e.starts_with("e2e")));
    let ui = plan["ui"].as_bool() == Some(true) || ui_leg;
    let mut engines = BTreeSet::new();
    if ui {
        engines.insert("chromium".to_string());
        if webkit {
            engines.insert("webkit".to_string());
        }
    }
    let whole = package_filters.iter().all(|f| f.is_empty()) && legs.iter().all(|l| l["filter"].as_str().unwrap_or("").is_empty());
    Scope {
        jobs,
        engines,
        apps: legs.iter().map(leg_id).collect(),
        whole,
    }
}

/// What a ci.yml plan job declares. `ran` is the scope the run's own plan switched on (empty for a reused run, which runs no job,
/// and for a rerun); `need` the same planner's answer for a push to main, with every engine on and every test whole. A rerun is
/// never complete, whatever it ran.
pub fn ci_scope(ran: Scope, rerun: bool, need: Scope) -> Declared {
    Declared {
        workflow: "ci.yml".into(),
        rerun,
        ran,
        need,
    }
}

/// What an app's own workflow declares (`./dev ci scope`): one gates job for the app. `full` is the workflow's own full mode
/// (a push, a tag, the nightly, a dispatch with its full input), `run` whether its plan ran its jobs at all (a schedule skipped
/// for a train, an unchanged main, a tagged commit ran none), `since_run` a rerun of failures. `full_engines` are the engines a
/// full run tests; a run that is not full tests the first one only.
pub fn app_scope(workflow: &str, app: &str, full: bool, run: bool, since_run: bool, full_engines: &[String]) -> Declared {
    let need = Scope {
        jobs: ["gates".to_string()].into(),
        engines: full_engines.iter().cloned().collect(),
        apps: [app.to_string()].into(),
        whole: true,
    };
    let ran = if run && !since_run {
        // A run that is not full ran no gates job (its nightly or installer jobs are not the gates) and one engine.
        Scope {
            jobs: if full { need.jobs.clone() } else { BTreeSet::new() },
            engines: if full {
                need.engines.clone()
            } else {
                full_engines.iter().take(1).cloned().collect()
            },
            ..need.clone()
        }
    } else {
        Scope::default()
    };
    Declared {
        workflow: workflow.into(),
        rerun: since_run,
        ran,
        need,
    }
}

/// A complete ci.yml scope line, for the tests of the readers (`verdict`, `reuse`): a run that did what a push to main does.
#[cfg(test)]
pub const TEST_COMPLETE: &str = "plan scope: {\"v\":1,\"workflow\":\"ci.yml\",\"rerun\":false,\"ran\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true},\"need\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true}}\n";

/// A ci.yml scope line of a run that missed one engine (WebKit): not complete.
#[cfg(test)]
pub const TEST_PARTIAL: &str = "plan scope: {\"v\":1,\"workflow\":\"ci.yml\",\"rerun\":false,\"ran\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\"],\"apps\":[],\"whole\":true},\"need\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true}}\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn leg(app: &str, os: &str, e2e: &str, ui: bool) -> Value {
        json!({"app": app, "os": os, "e2e_leg": e2e, "ui": ui, "filter": ""})
    }

    fn plan() -> Value {
        json!({"crates": true, "ui": true, "conformance": true, "paddockDev": false})
    }

    fn legs(webkit: bool) -> Vec<Value> {
        let mut l = vec![
            leg("dyno", "ubuntu-latest", "", true),
            leg("dyno", "macos-latest", "", false),
            leg("spotter", "ubuntu-latest", "", true),
        ];
        if webkit {
            l.push(leg("dyno", "ubuntu-latest", "e2e-webkit", true));
        }
        l
    }

    fn os() -> Vec<String> {
        strings(&DEFAULT_OS_LEGS)
    }

    fn declared(ran_webkit: bool, filters: &[&str]) -> Declared {
        let need = scope_of(&plan(), &legs(true), true, &os(), &[]);
        let ran = scope_of(&plan(), &legs(ran_webkit), ran_webkit, &os(), filters);
        ci_scope(ran, false, need)
    }

    #[test]
    fn a_run_that_covers_every_job_engine_app_and_test_is_complete() {
        let d = declared(true, &["", ""]);
        assert_eq!(d.complete(), Ok(()), "{}", d.line());
        // The scope has what it should: the crate jobs on each runner, both engines, the e2e WebKit leg.
        assert!(d.need.jobs.contains("crate@macos-latest") && d.need.jobs.contains("ui"));
        assert_eq!(d.need.engines, ["chromium", "webkit"].map(String::from).into());
        assert!(d.need.apps.contains("dyno@ubuntu-latest:e2e-webkit"));
    }

    #[test]
    fn a_run_missing_one_engine_is_not_complete_and_the_positive_control_with_it_is() {
        // The D4 hole: a full run without WebKit. Without the engine the same run, in the same scope function, is incomplete.
        let without = declared(false, &[]);
        let why = without.complete().unwrap_err();
        assert!(why.contains("engines webkit"), "{why}");
        assert!(why.contains("dyno@ubuntu-latest:e2e-webkit"), "the missing leg is named too: {why}");
        assert_eq!(declared(true, &[]).complete(), Ok(()));
    }

    #[test]
    fn a_run_missing_one_app_or_one_runner_is_not_complete() {
        let mut d = declared(true, &[]);
        d.ran.apps.remove("spotter@ubuntu-latest");
        assert!(d.complete().unwrap_err().contains("apps spotter@ubuntu-latest"));
        let mut d = declared(true, &[]);
        d.ran.jobs.remove("crate@windows-latest");
        assert!(d.complete().unwrap_err().contains("jobs crate@windows-latest"));
        // A wider run is complete: extra jobs or apps do no harm.
        let mut d = declared(true, &[]);
        d.ran.apps.insert("pitwall@ubuntu-latest".into());
        assert_eq!(d.complete(), Ok(()));
    }

    #[test]
    fn a_member_run_narrowed_to_packages_is_not_whole() {
        let d = declared(true, &["package(=fx-b)", ""]);
        assert!(d.complete().unwrap_err().contains("whole-workspace tests"), "{:?}", d.complete());
        let narrowed_leg = {
            let mut l = legs(true);
            l[0]["filter"] = json!("package(=x)");
            l
        };
        let ran = scope_of(&plan(), &narrowed_leg, true, &os(), &[]);
        assert!(!ran.whole);
    }

    #[test]
    fn a_rerun_and_a_run_that_ran_nothing_are_never_complete() {
        let need = scope_of(&plan(), &legs(true), true, &os(), &[]);
        let rerun = ci_scope(Scope::default(), true, need.clone());
        assert!(rerun.complete().unwrap_err().contains("rerun"), "{:?}", rerun.complete());
        // Even a rerun that claims to have run everything is a rerun.
        let mut claims = declared(true, &[]);
        claims.rerun = true;
        assert!(claims.complete().is_err());
        // A skipped or reused run declares an empty `ran`: it covers nothing.
        let skipped = Declared {
            ran: Scope::default(),
            ..declared(true, &[])
        };
        assert!(skipped.complete().is_err());
        // An empty need proves nothing (ER-33): a run that ran nothing against a need of nothing is not complete.
        let nothing = Declared {
            workflow: "ci.yml".into(),
            rerun: false,
            ran: Scope {
                whole: true,
                ..Scope::default()
            },
            need: Scope {
                whole: true,
                ..Scope::default()
            },
        };
        assert!(nothing.complete().unwrap_err().contains("empty"));
    }

    #[test]
    fn the_test_lines_of_the_readers_are_what_they_say() {
        // The constants the verdict and reuse tests log: one complete, one missing an engine (a positive control for both).
        assert_eq!(read(TEST_COMPLETE).unwrap().unwrap().complete(), Ok(()));
        assert!(
            read(TEST_PARTIAL)
                .unwrap()
                .unwrap()
                .complete()
                .unwrap_err()
                .contains("engines webkit")
        );
    }

    #[test]
    fn webkit_is_an_engine_only_when_a_ui_is_reached() {
        let no_ui = json!({"crates": true, "ui": false});
        let s = scope_of(&no_ui, &[], true, &os(), &[]);
        assert!(s.engines.is_empty(), "a plan with no UI has no engine to miss: {:?}", s.engines);
    }

    #[test]
    fn the_line_round_trips_and_only_one_line_with_the_prefix_counts() {
        let d = declared(true, &[]);
        let line = d.line();
        assert!(line.starts_with(SCOPE_LINE) && !line.contains('\n'));
        let log = format!("2026-11-21T00:00:00.0000000Z \u{1b}[32m{line}\u{1b}[0m\nother\n");
        assert_eq!(read(&log), Some(Ok(d.clone())));
        // No line, a second line, an unreadable one, another version and another shape.
        assert_eq!(read("plan mode: full=true\n"), None);
        assert!(matches!(read(&format!("{line}\n{line}\n")), Some(Err(e)) if e.contains("2 scope lines")));
        assert!(matches!(read("plan scope: nope\n"), Some(Err(_))));
        assert!(matches!(read(&line.replace("\"v\":1", "\"v\":2").to_string()), Some(Err(e)) if e.contains("version")));
        assert!(matches!(
            read("plan scope: {\"v\":1,\"workflow\":\"ci.yml\",\"rerun\":false}\n"),
            Some(Err(_))
        ));
        // The prefix counts only at the start of a line: a mention inside a message is not a declaration.
        assert_eq!(read(&format!("the {line}\n")), None);
    }

    #[test]
    fn an_app_workflow_is_complete_only_when_full_run_and_not_a_rerun() {
        let engines = strings(&["chromium", "webkit"]);
        let at = |full, run, since| app_scope("debrief.yml", "debrief", full, run, since, &engines);
        assert_eq!(at(true, true, false).complete(), Ok(()));
        // Positive controls, one dimension each: not full (no WebKit), skipped (no run), a rerun.
        assert!(at(false, true, false).complete().unwrap_err().contains("engines webkit"));
        assert!(at(true, false, false).complete().is_err());
        assert!(at(true, true, true).complete().unwrap_err().contains("rerun"));
        // One engine only: the engine cannot tell a full run from a partial one, the gates job does.
        let one = strings(&["chromium"]);
        assert_eq!(app_scope("dyno-release.yml", "dyno", true, true, false, &one).complete(), Ok(()));
        assert!(
            app_scope("dyno-release.yml", "dyno", false, true, false, &one)
                .complete()
                .unwrap_err()
                .contains("jobs gates")
        );
    }
}
