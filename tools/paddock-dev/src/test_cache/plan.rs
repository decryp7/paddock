//! The plan side of the switch-on (ADR 0116 §5, §6): which packages each runner class can skip, and what that does to the legs of the
//! `crate` and `paddock-dev` jobs. `ci test-cache plan` writes a [`ReusePlan`] (one step, the only one that holds the key: C141);
//! `./dev plan --ci` reads it ([`ENV_FILE`]) and calls [`apply`] on each job's legs.
//!
//! What it changes, and what it must not:
//! - a leg that runs the whole workspace gets `test_cache_exclude`, a nextest filterset `not (package(=a) | ...)` that the job's test
//!   step ANDs with its own filter; a leg with a `-p` package list (hostTests) loses the reused packages from it;
//! - `test_filter` is never touched, so the plan's `whole` stays true (reuse narrows nothing the run declares: the reused targets are
//!   covered by their exact-key pass, named in the declared scope's `reused` map and in the verdict);
//! - a leg whose tests are narrowed by `test_filter` (a hostTests module filter or a member run's filter) is left alone: its run is
//!   not a whole-package run, so a pass of the whole package cannot stand in for it;
//! - the Linux leg of the `crate` job runs no tests there (the coverage job runs them whole, decision D4), so it is left alone.
//!
//! A job is never switched off in phase 1: every leg also runs clippy (and fmt), which are phase 2 targets (decision D2). The declared
//! scope's `reused` map ([`crate::run_scope::Declared::with_reused`]) holds the reused targets per leg, and `complete()` already counts
//! a job as covered when it ran or was reused, so the day a leg's only work is cached it can be dropped here without touching the reader.

use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The environment variable that names the file `ci test-cache plan` wrote (a path); absent or unreadable means no reuse (fail open).
pub const ENV_FILE: &str = "PLAN_TEST_CACHE_FILE";

/// The runner class of a `runs-on` label of the plan's legs (`ubuntu-latest`, `windows-latest`, `macos-latest`).
pub fn class_of_runner(os: &str) -> &'static str {
    match os {
        "windows-latest" => "Windows-X64",
        "macos-latest" => "macOS-ARM64",
        _ => "Linux-X64",
    }
}

/// The packages each runner class may skip, with the note that names the pass each rests on: class, then package, then note
/// (`rust:crates/a@Windows-X64 from run 8635`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReusePlan {
    /// By runner class, then package name.
    pub by_class: BTreeMap<String, BTreeMap<String, String>>,
}

impl ReusePlan {
    /// The plan as JSON.
    pub fn json(&self) -> Value {
        json!(self.by_class)
    }

    /// The plan out of the file's text; anything that is not a class-to-package-to-note map is an error, never "no reuse" read as a plan.
    pub fn parse(text: &str) -> Result<ReusePlan, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("the test-cache plan is not JSON: {e}"))?;
        let classes = v.as_object().ok_or("the test-cache plan is not an object")?;
        let mut by_class = BTreeMap::new();
        for (class, pk) in classes {
            let pk = pk
                .as_object()
                .ok_or_else(|| format!("the test-cache plan's {class} is not an object"))?;
            let mut m = BTreeMap::new();
            for (package, note) in pk {
                let note = note
                    .as_str()
                    .filter(|n| !n.is_empty())
                    .ok_or_else(|| format!("the test-cache plan's {class}/{package} has no note"))?;
                m.insert(package.clone(), note.to_string());
            }
            by_class.insert(class.clone(), m);
        }
        Ok(ReusePlan { by_class })
    }
}

/// What [`apply`] did: the legs, and the reused targets per leg (`crate@macos-latest` and its notes) for the declared scope.
pub struct Applied {
    /// The legs, edited.
    pub legs: Vec<Value>,
    /// Per leg (`<job>@<os>`), one note per reused package.
    pub reused: BTreeMap<String, Vec<String>>,
}

/// The nextest filterset that leaves `packages` out.
fn exclude(packages: &[&String]) -> String {
    format!(
        "not ({})",
        packages.iter().map(|p| format!("package(={p})")).collect::<Vec<_>>().join(" | ")
    )
}

/// Applies `plan` to the legs of `job` (`crate` or `paddock-dev`). `linux_tests`: whether the Linux leg runs this workspace's tests
/// itself (false for the `crate` job: the coverage job does).
pub fn apply(legs: Vec<Value>, job: &str, plan: &ReusePlan, linux_tests: bool) -> Applied {
    let mut reused: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let legs = legs
        .into_iter()
        .map(|mut leg| {
            let os = leg["os"].as_str().unwrap_or("").to_string();
            let class = class_of_runner(&os);
            let narrowed = !leg["test_filter"].as_str().unwrap_or("").is_empty() || leg["test_skip"].as_str() == Some("true");
            if narrowed || (class == "Linux-X64" && !linux_tests) {
                return leg;
            }
            let Some(skip) = plan.by_class.get(class).filter(|m| !m.is_empty()) else {
                return leg;
            };
            let packages: Vec<String> = leg["test_packages"]
                .as_str()
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let notes: Vec<String> = if packages.is_empty() {
                // The whole workspace: leave the reused packages out by a filterset the step ANDs with its own.
                let names: Vec<&String> = skip.keys().collect();
                leg["test_cache_exclude"] = json!(exclude(&names));
                skip.values().cloned().collect()
            } else {
                let (gone, kept): (Vec<&String>, Vec<&String>) = packages.iter().partition(|p| skip.contains_key(*p));
                if gone.is_empty() {
                    return leg;
                }
                leg["test_packages"] = json!(kept.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "));
                if kept.is_empty() {
                    leg["test_skip"] = json!("true");
                }
                gone.iter().filter_map(|p| skip.get(*p).cloned()).collect()
            };
            reused.insert(format!("{job}@{os}"), notes);
            leg
        })
        .collect();
    Applied { legs, reused }
}
