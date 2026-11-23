//! What the plan selected for one project, applied to a phase (ADR 0117 §5, the wiring): the plan job writes one JSON object,
//! `selection` (project id to its narrowed selection; a project the plan runs in full is not in it), every job that runs
//! `./dev ci run` passes it as [`ENV`], and the executor
//!
//! - skips a phase the selection does not keep (a smoke set keeps its declared phases, an affected run the phases whose test
//!   runners have an affected file), with a notice that says it was skipped, never that it passed (ER-33; the run's declared scope
//!   says `smoke` or `affected`, so the verdict never counts it complete);
//! - gives a test runner line only the affected files of its runner: Playwright and Vitest file arguments, `ng test --include`,
//!   `node --test` with the affected files its globs match; a runner line with no affected file under its folder is skipped. Other
//!   lines (builds, lint) run as written.
//!
//! ```json
//! {"ui": {"phases": ["build"]},
//!  "app:dyno": {"phases": ["prepare", "ui"], "files": {"vitest": ["apps/dyno/app/src/a.spec.ts"]}}}
//! ```
//!
//! The planner computes the kept phases with the same [`narrow_lines`] the executor applies, so the two cannot disagree (ER-22).

use super::Line;
use paddock_repo_graph::model::glob;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The environment variable a job passes the plan job's `selection` output in.
pub const ENV: &str = "PADDOCK_PLAN_SELECTION";

/// One project's narrowed selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Narrow {
    /// The phases that run (`None`: every phase).
    pub phases: Option<BTreeSet<String>>,
    /// Runner (`vitest`, `playwright`, `node-test`) to the affected test files (repository paths); empty: no file narrowing.
    pub files: BTreeMap<String, Vec<String>>,
}

impl Narrow {
    /// As the `selection` output writes it.
    pub fn json(&self) -> Value {
        let mut v = serde_json::Map::new();
        if let Some(p) = &self.phases {
            v.insert("phases".into(), serde_json::json!(p));
        }
        if !self.files.is_empty() {
            v.insert("files".into(), serde_json::json!(self.files));
        }
        Value::Object(v)
    }
}

/// The project id of a `ci run` project name: a root project is itself, any other name is an app (`app:<name>`).
pub fn project_id(project: &str) -> String {
    if super::contract::ROOT_PROJECTS.contains(&project) {
        project.to_string()
    } else {
        format!("app:{project}")
    }
}

/// The selection of `project` (a project id) in the `selection` text: `None` when the text is empty or does not name it (the
/// project runs in full); `Err` for text that is not the planner's shape (the job then runs everything: never less on a bad value).
pub fn of(text: &str, project: &str) -> Result<Option<Narrow>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("{ENV} is not JSON ({e})"))?;
    let all = v.as_object().ok_or_else(|| format!("{ENV} is not an object"))?;
    let Some(p) = all.get(project) else { return Ok(None) };
    let strings = |v: &Value, what: &str| -> Result<Vec<String>, String> {
        v.as_array()
            .ok_or_else(|| format!("{ENV}: {project}.{what} is not a list"))?
            .iter()
            .map(|s| {
                s.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{ENV}: {project}.{what} holds a non-string"))
            })
            .collect()
    };
    let phases = match p.get("phases") {
        Some(v) => Some(strings(v, "phases")?.into_iter().collect()),
        None => None,
    };
    let mut files = BTreeMap::new();
    if let Some(f) = p.get("files") {
        for (runner, list) in f.as_object().ok_or_else(|| format!("{ENV}: {project}.files is not an object"))? {
            files.insert(runner.clone(), strings(list, &format!("files.{runner}"))?);
        }
    }
    Ok(Some(Narrow { phases, files }))
}

/// The runner a command line starts (after leading `NAME=value` assignments), and the index of its first argument.
fn runner_of(words: &[&str]) -> Option<(&'static str, usize)> {
    let start = words
        .iter()
        .position(|w| !(w.contains('=') && !w.starts_with('-') && w.chars().next().is_some_and(|c| c.is_ascii_uppercase())))?;
    let w = &words[start..];
    match w {
        ["npx", "playwright", "test", ..] => Some(("playwright", start + 3)),
        ["npx", "ng", "test", ..] => Some(("ng", start + 3)),
        ["npx", "vitest", ..] => Some(("vitest", start + 2)),
        ["node", "--test", ..] => Some(("node-test", start + 2)),
        _ => None,
    }
}

/// The affected files of `runner` below `cwd`, relative to it.
fn below<'a>(files: &'a BTreeMap<String, Vec<String>>, runner: &str, cwd: &str) -> Vec<&'a str> {
    files
        .get(runner)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            if cwd.is_empty() {
                Some(f.as_str())
            } else {
                f.strip_prefix(cwd).and_then(|r| r.strip_prefix('/'))
            }
        })
        .collect()
}

/// One line given only the affected files: `Some(line)` to run (narrowed when it is a test runner), `None` to skip it (a runner
/// line with no affected file of its runner below its folder).
pub fn narrow_line(text: &str, cwd: &str, files: &BTreeMap<String, Vec<String>>) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let Some((runner, args)) = runner_of(&words) else {
        return Some(text.to_string());
    };
    // A coverage figure of a subset is not the project's: an affected run measures none.
    let keep = |w: &&str| *w != "--coverage" && !w.starts_with("--coverage=");
    match runner {
        "playwright" => {
            let hit = below(files, "playwright", cwd);
            (!hit.is_empty()).then(|| {
                let mut out: Vec<&str> = words.iter().copied().filter(keep).collect();
                out.push("--pass-with-no-tests");
                out.extend(hit);
                out.join(" ")
            })
        }
        "ng" | "vitest" => {
            let hit = below(files, "vitest", cwd);
            (!hit.is_empty()).then(|| {
                let mut out: Vec<String> = words.iter().copied().filter(keep).map(str::to_string).collect();
                for f in hit {
                    if runner == "ng" {
                        out.push("--include".into());
                    }
                    out.push(f.to_string());
                }
                out.join(" ")
            })
        }
        _ => {
            // `node --test <globs>`: the affected files the line's own globs match, in their place.
            let (head, rest) = words.split_at(args);
            let (flags, globs): (Vec<&str>, Vec<&str>) = rest.iter().partition(|w| w.starts_with('-'));
            let hit: Vec<&str> = below(files, "node-test", cwd)
                .into_iter()
                .filter(|f| globs.iter().any(|g| glob::matches(&format!("/{g}"), f)))
                .collect();
            (!hit.is_empty()).then(|| head.iter().chain(&flags).chain(&hit).copied().collect::<Vec<_>>().join(" "))
        }
    }
}

/// A phase's lines given only the affected files ([`narrow_line`] on each command, `cd`s kept), and how many runner lines kept an
/// affected file (zero: the phase has nothing of the change to test).
pub fn narrow_lines(lines: &[Line], files: &BTreeMap<String, Vec<String>>) -> (Vec<Line>, usize) {
    let mut kept = 0;
    let mut out = Vec::new();
    for l in lines {
        match l {
            Line::Cd(_) => out.push(l.clone()),
            Line::Cmd { text, cwd } => {
                let is_runner = runner_of(&text.split_whitespace().collect::<Vec<_>>()).is_some();
                if let Some(t) = narrow_line(text, cwd, files) {
                    kept += usize::from(is_runner);
                    out.push(Line::Cmd { text: t, cwd: cwd.clone() });
                }
            }
        }
    }
    (out, kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(runner: &str, list: &[&str]) -> BTreeMap<String, Vec<String>> {
        BTreeMap::from([(runner.to_string(), list.iter().map(|s| s.to_string()).collect())])
    }

    #[test]
    fn each_runner_is_given_only_its_affected_files_below_its_folder() {
        let v = files("vitest", &["apps/a/app/src/x.spec.ts"]);
        assert_eq!(
            narrow_line("npx ng test --watch=false --coverage", "apps/a/app", &v).as_deref(),
            Some("npx ng test --watch=false --include src/x.spec.ts")
        );
        assert_eq!(
            narrow_line("TZ=Pacific/Auckland npx ng test --watch=false", "apps/a/app", &v).as_deref(),
            Some("TZ=Pacific/Auckland npx ng test --watch=false --include src/x.spec.ts")
        );
        let p = files("playwright", &["ui/e2e/a.spec.ts"]);
        assert_eq!(
            narrow_line("npx playwright test --shard=$E2E_SHARD", "ui", &p).as_deref(),
            Some("npx playwright test --shard=$E2E_SHARD --pass-with-no-tests e2e/a.spec.ts")
        );
        let n = files("node-test", &["apps/a/scripts/b.test.mjs", "apps/a/docs/c.test.mjs"]);
        assert_eq!(
            narrow_line("node --test scripts/b.test.mjs", "apps/a", &n).as_deref(),
            Some("node --test scripts/b.test.mjs")
        );
        assert_eq!(
            narrow_line("node --test scripts/z.test.mjs", "apps/a", &n),
            None,
            "its globs match no affected file"
        );
        // A runner line with no affected file of its runner is skipped; any other line runs as written.
        assert_eq!(narrow_line("npx playwright test", "ui", &v), None);
        assert_eq!(narrow_line("npm run build -w ui", "", &v).as_deref(), Some("npm run build -w ui"));
        assert_eq!(narrow_line("npx ng test", "apps/b/app", &v), None, "a file of another folder");
    }

    #[test]
    fn the_selection_names_a_project_or_says_nothing_and_a_bad_value_is_an_error() {
        let text = r#"{"ui": {"phases": ["build"]}, "app:x": {"files": {"vitest": ["apps/x/a.spec.ts"]}}}"#;
        let ui = of(text, "ui").unwrap().unwrap();
        assert_eq!(ui.phases, Some(BTreeSet::from(["build".to_string()])));
        assert!(of(text, "app:x").unwrap().unwrap().phases.is_none());
        assert_eq!(of(text, "crates").unwrap(), None, "not named: in full");
        assert_eq!(of("", "ui").unwrap(), None, "no selection: in full");
        assert!(of("[1]", "ui").is_err() && of(r#"{"ui": {"phases": "build"}}"#, "ui").is_err());
        assert_eq!((project_id("ui"), project_id("dyno")), ("ui".to_string(), "app:dyno".to_string()));
        assert_eq!(
            of(&serde_json::json!({"ui": ui.json()}).to_string(), "ui"),
            Ok(Some(ui)),
            "the planner's own shape reads back"
        );
    }

    #[test]
    fn a_phase_counts_the_runner_lines_that_kept_a_file() {
        let lines = super::super::expand(&[
            "npm run build -w ui".into(),
            "cd apps/a/app".into(),
            "npm run lint".into(),
            "npx ng test --watch=false".into(),
        ])
        .unwrap();
        let (out, kept) = narrow_lines(&lines, &files("vitest", &["apps/a/app/src/x.spec.ts"]));
        assert_eq!((out.len(), kept), (4, 1));
        let (out, kept) = narrow_lines(&lines, &files("vitest", &["apps/b/app/src/x.spec.ts"]));
        assert_eq!((out.len(), kept), (3, 0), "the runner line is skipped, the others stay");
    }
}
