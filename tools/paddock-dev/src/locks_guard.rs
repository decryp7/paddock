//! `locks guard`: a dependency added to a shared crate must not change what the other workspaces build or lock without that change
//! being in the same commit. Twice (`toml`, then `oxc` and `syn` in `crates/paddock-repo-graph`) a new dependency changed the
//! workspace hack (`cargo hakari generate --diff`) and the features Cargo resolves for `serde_json` (oxc_resolver forces
//! `preserve_order`), which changed every app's `Cargo.lock` and broke Clippy in paddock-mcp under `--all-features`. Each was found
//! only in a later CI run.
//!
//! The guard is [`crate::locks`]'s plan with one more kind of fact. It fails when
//! - (a) the workspace hack is not what `cargo hakari generate` writes (the hakari target of [`crate::locks::plan`], run through its
//!   [`Runner`]);
//! - (b) the features Cargo resolves for a watched crate (`cargo metadata`, per workspace) differ from the pins in
//!   `docs/shared-crate-features.toml`: the watch list and the pins are data, never code, and the pins are written by
//!   `./dev locks guard --write` (ER-40), so a wanted change is one reviewed diff of that file in the same commit;
//! - (c) a workspace's `Cargo.lock` would change (`cargo metadata --locked` refuses), so the lockfile the change needs is not in it.
//!
//! It starts report-only (ER-41): [`MODE`] makes every finding a `::warning::` and the exit 0. The first green run on the real tree is
//! recorded at [`MODE`], and flipping it to [`Mode::Gate`] is the same change that records it.
//!
//! Metadata comes through [`Metadata`], so the parsing and the comparison are tested on fixtures and the real cargo only on path-only
//! workspaces in a scratch folder (no registry).

use crate::locks::{Kind, Runner, Target, check_all, plan, program_for};
use paddock_cli::{CliError, Exit, Report};
use paddock_proc::{Limits, command, run_bounded};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Whether a finding fails the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Findings are `::warning::` lines and the exit is 0 (ER-41: until the first green run on the real tree).
    ReportOnly,
    /// Findings are `::error::` lines and the exit is 5.
    Gate,
}

/// The mode of the guard, report-only (ER-41) until its first green CI run on the real tree is recorded here, by run id, in the same
/// change that sets this to [`Mode::Gate`].
///
/// First run on the real tree: `./dev locks guard` on d04af7b46 (main), 21 workspaces resolved, 0 findings, hakari current, on the
/// developer's machine (2026-11-23). The CI run id of the `Shared-crate dependency guard` step is not recorded yet.
pub const MODE: Mode = Mode::ReportOnly;

/// The file that holds the watch list and the pinned features (data, read through [`parse_pins`]).
pub const PINS_FILE: &str = "docs/shared-crate-features.toml";

const LIMIT: Duration = Duration::from_secs(600);
const PARALLEL: usize = 4;

/// The watch list and the features pinned for each watched crate in each workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pins {
    /// Crates whose resolved features are pinned.
    pub watch: Vec<String>,
    /// crate -> workspace folder (`crates`, `apps/dyno`) -> features.
    pub features: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
}

/// Reads the pins file. A watched crate with no table is an empty pin set; a malformed file is an error that names the key.
pub fn parse_pins(text: &str) -> Result<Pins, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("{PINS_FILE} is not valid TOML: {e}"))?;
    let watch = match table.get("watch") {
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{PINS_FILE}: `watch` holds only crate names"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(format!("{PINS_FILE}: `watch` (the list of crate names) is missing")),
    };
    let mut features = BTreeMap::new();
    if let Some(toml::Value::Table(all)) = table.get("features") {
        for (krate, per_ws) in all {
            let toml::Value::Table(per_ws) = per_ws else {
                return Err(format!("{PINS_FILE}: `features.{krate}` is a table of workspace = [features]"));
            };
            let mut m = BTreeMap::new();
            for (ws, list) in per_ws {
                let toml::Value::Array(list) = list else {
                    return Err(format!("{PINS_FILE}: `features.{krate}.{ws}` is a list of feature names"));
                };
                let set = list
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| format!("{PINS_FILE}: `features.{krate}.{ws}` holds only names"))
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?;
                m.insert(ws.clone(), set);
            }
            features.insert(krate.clone(), m);
        }
    }
    Ok(Pins { watch, features })
}

const HEADER: &str = "# The shared-crate dependency guard (`./dev locks guard`): the features Cargo resolves for each watched crate, per workspace.\n\
# Generated by `./dev locks guard --write` (never by hand, ER-40); CI and the push gate compare it with `cargo metadata`.\n\
# A dependency added to a shared crate (crates/*/Cargo.toml) can force a feature on a crate every app builds (oxc_resolver forces\n\
# serde_json's `preserve_order`), which changes every app's Cargo.lock. A change that moves a line here says so in review: it is\n\
# either unwanted (drop the dependency or its feature) or wanted (commit this file with it).\n\
# `watch` is the list of crates to pin: add a crate name there and run `./dev locks guard --write`.\n";

/// The text of the pins file for `pins` (what `--write` stores; the inverse of [`parse_pins`]).
pub fn render_pins(pins: &Pins) -> String {
    let mut out = String::from(HEADER);
    let names: Vec<String> = pins.watch.iter().map(|w| format!("{w:?}")).collect();
    out.push_str(&format!("\nwatch = [{}]\n", names.join(", ")));
    for krate in &pins.watch {
        let Some(per_ws) = pins.features.get(krate) else { continue };
        out.push_str(&format!("\n[features.{krate}]\n"));
        for (ws, set) in per_ws {
            let items: Vec<String> = set.iter().map(|f| format!("{f:?}")).collect();
            out.push_str(&format!("{ws:?} = [{}]\n", items.join(", ")));
        }
    }
    out
}

/// The features Cargo resolved for the package(s) named `name` in one `cargo metadata` document (the union over versions);
/// `None` when the workspace does not build it.
pub fn resolved_features(meta: &Value, name: &str) -> Option<BTreeSet<String>> {
    let ids: BTreeSet<&str> = meta["packages"]
        .as_array()?
        .iter()
        .filter(|p| p["name"].as_str() == Some(name))
        .filter_map(|p| p["id"].as_str())
        .collect();
    if ids.is_empty() {
        return None;
    }
    let mut out = BTreeSet::new();
    for node in meta["resolve"]["nodes"].as_array()? {
        if node["id"].as_str().is_some_and(|id| ids.contains(id)) {
            out.extend(
                node["features"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str().map(str::to_string)),
            );
        }
    }
    Some(out)
}

/// How a workspace's resolved features differ from its pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drift {
    /// The set differs: features now present that were not pinned, and pinned features that are gone.
    Changed {
        /// Features now resolved that the pin lacks.
        added: Vec<String>,
        /// Pinned features that are no longer resolved.
        removed: Vec<String>,
    },
    /// The workspace builds the crate and has no pin.
    Unpinned(Vec<String>),
    /// A pin for a workspace that no longer builds the crate (or no longer exists).
    Gone,
}

/// One difference between the pins and what Cargo resolves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    /// The watched crate.
    pub krate: String,
    /// The workspace folder.
    pub workspace: String,
    /// How it differs.
    pub drift: Drift,
}

/// What Cargo resolves: crate -> workspace -> features (a workspace that does not build the crate is absent).
pub type Actual = BTreeMap<String, BTreeMap<String, BTreeSet<String>>>;

/// Every difference between the pins and the resolved features, in crate then workspace order (pure).
pub fn compare(pins: &Pins, actual: &Actual) -> Vec<Difference> {
    let mut out = Vec::new();
    let empty = BTreeMap::new();
    for krate in &pins.watch {
        let pinned = pins.features.get(krate).unwrap_or(&empty);
        let now = actual.get(krate).unwrap_or(&empty);
        let workspaces: BTreeSet<&String> = pinned.keys().chain(now.keys()).collect();
        for ws in workspaces {
            let drift = match (pinned.get(ws), now.get(ws)) {
                (Some(p), Some(n)) if p == n => continue,
                (Some(p), Some(n)) => Drift::Changed {
                    added: n.difference(p).cloned().collect(),
                    removed: p.difference(n).cloned().collect(),
                },
                (None, Some(n)) => Drift::Unpinned(n.iter().cloned().collect()),
                (Some(_), None) => Drift::Gone,
                (None, None) => continue,
            };
            out.push(Difference {
                krate: krate.clone(),
                workspace: ws.clone(),
                drift,
            });
        }
    }
    out
}

/// Which of the three failures a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// (a) the workspace hack is stale.
    Hack,
    /// (b) a watched crate's resolved features moved.
    Features,
    /// (c) a lockfile would change, or the workspace does not resolve.
    Lock,
    /// The pins file is missing or unreadable: the guard could not measure.
    Pins,
}

/// One finding with the file it names and the next step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Which check.
    pub part: Part,
    /// The file the annotation names.
    pub file: String,
    /// What is wrong and the next step.
    pub message: String,
}

fn drift_message(d: &Difference) -> String {
    let what = match &d.drift {
        Drift::Changed { added, removed } => format!(
            "resolved features changed:{}{}",
            if added.is_empty() {
                String::new()
            } else {
                format!(" +{}", added.join(" +"))
            },
            if removed.is_empty() {
                String::new()
            } else {
                format!(" -{}", removed.join(" -"))
            },
        ),
        Drift::Unpinned(f) => format!("is built here with [{}] and has no pin", f.join(", ")),
        Drift::Gone => "is no longer built here but its pin remains".to_string(),
    };
    format!(
        "{} in {}: {what}. A dependency added to a shared crate forces features on the crates every app builds (oxc_resolver forces serde_json's `preserve_order`) and changes every app's Cargo.lock. Drop the dependency or its feature; if the change is wanted, run `./dev locks guard --write` and commit {PINS_FILE} with it.",
        d.krate, d.workspace
    )
}

/// Cargo's view of one workspace, faked in tests.
pub trait Metadata: Sync {
    /// `cargo metadata --locked --format-version 1` in `dir`, parsed; `Err` with the cause when cargo refuses (a stale lock, a
    /// workspace that does not resolve) or cannot be started.
    fn metadata(&self, dir: &Path) -> Result<Value, String>;
}

/// The real cargo, bounded, through `paddock-proc`.
pub struct RealMetadata;

impl Metadata for RealMetadata {
    fn metadata(&self, dir: &Path) -> Result<Value, String> {
        crate::test_cost::charge(crate::test_cost::Class::Graph, "locks_guard::RealMetadata")?;
        let mut cmd = command(program_for(
            "cargo",
            &std::env::var("PATH").unwrap_or_default(),
            std::env::var("PATHEXT").ok().as_deref(),
            cfg!(windows),
        ));
        // A probe never installs a toolchain (ER-45).
        cmd.current_dir(dir)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .args(["metadata", "--locked", "--format-version", "1"]);
        let out = run_bounded(cmd, Limits::new(LIMIT).max_output(1 << 27)).map_err(|e| format!("cargo could not run: {e}"))?;
        if !out.success() {
            let err = out.stderr_text();
            let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
            return Err(tail[tail.len().saturating_sub(3)..].join(" | "));
        }
        serde_json::from_str(&out.stdout_text()).map_err(|e| format!("cargo metadata printed no JSON: {e}"))
    }
}

/// The result of a run.
#[derive(Debug)]
pub struct Outcome {
    /// Every finding.
    pub findings: Vec<Finding>,
    /// What cargo resolved, for `--write`.
    pub actual: Actual,
    /// Workspaces measured.
    pub workspaces: usize,
}

fn ws_label(dir: &Path) -> String {
    if dir.as_os_str().is_empty() {
        ".".to_string()
    } else {
        dir.to_string_lossy().replace('\\', "/")
    }
}

/// Runs the three checks over the tracked files. `pins` is the parsed pins file, or the reason it cannot be read.
pub fn measure(runner: &dyn Runner, meta: &dyn Metadata, root: &Path, tracked: &[String], pins: &Result<Pins, String>) -> Outcome {
    let targets = plan(tracked);
    let mut findings = Vec::new();
    // (a) the workspace hack.
    let hack: Vec<Target> = targets.iter().filter(|t| t.kind == Kind::Hakari).cloned().collect();
    for s in check_all(runner, root, &hack) {
        findings.push(Finding {
            part: Part::Hack,
            file: s.target.file.clone(),
            message: format!(
                "the workspace hack is not what cargo-hakari generates: `{}` failed ({}). A dependency added to a shared crate changes it. Run `{}` and commit the result with the change.",
                s.command,
                s.tail,
                s.target.fix_text()
            ),
        });
    }
    // (c) and the input of (b): cargo's resolution of every workspace, a few at a time.
    let cargo: Vec<&Target> = targets.iter().filter(|t| t.kind == Kind::Cargo).collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Result<Value, String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..PARALLEL.min(cargo.len().max(1)) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(t) = cargo.get(i) else { break };
                    let r = meta.metadata(&root.join(&t.dir));
                    results.lock().unwrap_or_else(|p| p.into_inner()).push((i, r));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(|p| p.into_inner());
    results.sort_by_key(|(i, _)| *i);
    let mut docs: Vec<(String, Value)> = Vec::new();
    for (i, r) in results {
        let t = cargo[i];
        match r {
            Ok(v) => docs.push((ws_label(&t.dir), v)),
            Err(why) => findings.push(Finding {
                part: Part::Lock,
                file: t.file.clone(),
                message: format!(
                    "{} would change or the workspace does not resolve (`cargo metadata --locked`: {why}). Run `{}` and commit the lockfile with the change that needs it.",
                    t.file,
                    t.fix_text()
                ),
            }),
        }
    }
    // (b) the pinned features.
    let mut actual: Actual = BTreeMap::new();
    match pins {
        Ok(pins) => {
            for krate in &pins.watch {
                let per = actual.entry(krate.clone()).or_default();
                for (ws, doc) in &docs {
                    if let Some(set) = resolved_features(doc, krate) {
                        per.insert(ws.clone(), set);
                    }
                }
            }
            // A workspace whose metadata failed is reported as (c); its pin is not also reported as gone.
            let failed: BTreeSet<String> = cargo
                .iter()
                .map(|t| ws_label(&t.dir))
                .filter(|l| !docs.iter().any(|(w, _)| w == l))
                .collect();
            for d in compare(pins, &actual) {
                if d.drift == Drift::Gone && failed.contains(&d.workspace) {
                    continue;
                }
                findings.push(Finding {
                    part: Part::Features,
                    file: if d.workspace == "." {
                        "Cargo.toml".into()
                    } else {
                        format!("{}/Cargo.toml", d.workspace)
                    },
                    message: drift_message(&d),
                });
            }
        }
        Err(why) => findings.push(Finding {
            part: Part::Pins,
            file: PINS_FILE.into(),
            message: format!("not measured: {why}. Run `./dev locks guard --write` and commit the file."),
        }),
    }
    Outcome {
        findings,
        actual,
        workspaces: docs.len(),
    }
}

/// A runner that runs nothing and says it passed: `--write` pins what Cargo resolves and leaves the hack to CI's lockfile checks.
struct NoRunner;

impl Runner for NoRunner {
    fn run(&self, _dir: &Path, _argv: &[String]) -> crate::locks::Ran {
        crate::locks::Ran {
            ok: true,
            tail: String::new(),
        }
    }
}

fn read_pins(root: &Path) -> Result<Pins, String> {
    let text = std::fs::read_to_string(root.join(PINS_FILE)).map_err(|e| format!("{PINS_FILE} cannot be read ({e})"))?;
    parse_pins(&text)
}

fn tracked(root: &Path) -> Result<Vec<String>, CliError> {
    crate::locks::tracked(root)
}

/// The annotation for a finding in `mode`.
pub fn annotation(f: &Finding, mode: Mode) -> String {
    let level = if mode == Mode::Gate { "error" } else { "warning" };
    format!("::{level} file={}::{}", f.file, f.message)
}

/// `locks guard [--write]`.
pub fn command_guard(root: Option<PathBuf>, write: bool) -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(root.as_deref())?;
    run(&crate::locks::Real, &RealMetadata, &root, write, MODE)
}

/// [`command_guard`] with the tools and the mode given.
pub fn run(runner: &dyn Runner, meta: &dyn Metadata, root: &Path, write: bool, mode: Mode) -> Result<Report, CliError> {
    let files = tracked(root)?;
    let started = std::time::Instant::now();
    if write {
        return write_pins(meta, root, &files);
    }
    let pins = read_pins(root);
    let out = measure(runner, meta, root, &files, &pins);
    let mut text: String = out.findings.iter().map(|f| annotation(f, mode) + "\n").collect();
    text.push_str(&format!(
        "locks guard: {} workspace(s) resolved, {} finding(s), {} ({} ms)\n",
        out.workspaces,
        out.findings.len(),
        if mode == Mode::Gate {
            "blocking"
        } else {
            "report-only: not a failure yet (ER-41)"
        },
        started.elapsed().as_millis()
    ));
    let data = json!({
        "mode": if mode == Mode::Gate { "gate" } else { "report-only" },
        "workspaces": out.workspaces,
        "findings": out.findings.iter().map(|f| json!({"part": format!("{:?}", f.part), "file": f.file, "message": f.message})).collect::<Vec<Value>>(),
    });
    let report = Report::new(data, text);
    Ok(if out.findings.is_empty() || mode == Mode::ReportOnly {
        report
    } else {
        report.with_exit(Exit::Findings)
    })
}

/// Rewrites the pins file from what Cargo resolves now (the watch list is kept).
fn write_pins(meta: &dyn Metadata, root: &Path, files: &[String]) -> Result<Report, CliError> {
    let current = read_pins(root).map_err(|why| {
        CliError::failure(why).with_hint(format!(
            "Start {PINS_FILE} with `watch = [\"serde_json\"]`, then run `./dev locks guard --write` again."
        ))
    })?;
    let out = measure(&NoRunner, meta, root, files, &Ok(current.clone()));
    let lock_problems: Vec<&Finding> = out.findings.iter().filter(|f| f.part == Part::Lock).collect();
    if let Some(f) = lock_problems.first() {
        return Err(
            CliError::failure(format!("cannot pin while a workspace does not resolve: {}", f.message))
                .with_hint("Run `./dev locks sync` first."),
        );
    }
    let pins = Pins {
        watch: current.watch,
        features: out.actual,
    };
    std::fs::write(root.join(PINS_FILE), render_pins(&pins))
        .map_err(|e| CliError::failure(format!("{PINS_FILE} cannot be written: {e}")))?;
    Ok(Report::new(
        json!({"written": PINS_FILE}),
        format!(
            "locks guard: wrote {PINS_FILE} for {} workspace(s). Review the diff and commit it.\n",
            out.workspaces
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn pins_round_trip_through_the_rendered_file() {
        let mut features = BTreeMap::new();
        features.insert(
            "serde_json".to_string(),
            BTreeMap::from([
                ("crates".to_string(), set(&["default", "std"])),
                ("apps/dyno".to_string(), set(&["std"])),
            ]),
        );
        let pins = Pins {
            watch: vec!["serde_json".into()],
            features,
        };
        assert_eq!(parse_pins(&render_pins(&pins)).unwrap(), pins);
    }

    #[test]
    fn a_pins_file_without_a_watch_list_is_refused_with_the_key_named() {
        assert!(parse_pins("[features.x]\n").unwrap_err().contains("`watch`"));
        assert!(parse_pins("watch = [1]\n").unwrap_err().contains("only crate names"));
        assert!(parse_pins("watch = [").unwrap_err().contains("not valid TOML"));
    }

    fn doc() -> Value {
        json!({
            "packages": [
                {"name": "serde_json", "id": "serde_json 1.0.1"},
                {"name": "serde_json", "id": "serde_json 0.9.0"},
                {"name": "other", "id": "other 1.0.0"},
            ],
            "resolve": {"nodes": [
                {"id": "serde_json 1.0.1", "features": ["std", "preserve_order"]},
                {"id": "serde_json 0.9.0", "features": ["std"]},
                {"id": "other 1.0.0", "features": ["x"]},
            ]},
        })
    }

    #[test]
    fn resolved_features_are_the_union_over_the_versions_of_the_named_crate_only() {
        assert_eq!(resolved_features(&doc(), "serde_json"), Some(set(&["preserve_order", "std"])));
        assert_eq!(resolved_features(&doc(), "other"), Some(set(&["x"])));
        assert_eq!(
            resolved_features(&doc(), "absent"),
            None,
            "a workspace that does not build the crate"
        );
        assert_eq!(resolved_features(&json!({}), "serde_json"), None);
    }

    #[test]
    fn compare_names_added_removed_unpinned_and_gone() {
        let pins = Pins {
            watch: vec!["serde_json".into()],
            features: BTreeMap::from([(
                "serde_json".to_string(),
                BTreeMap::from([
                    ("a".to_string(), set(&["std"])),
                    ("b".to_string(), set(&["std", "x"])),
                    ("c".to_string(), set(&["std"])),
                    ("d".to_string(), set(&["std"])),
                ]),
            )]),
        };
        let actual: Actual = BTreeMap::from([(
            "serde_json".to_string(),
            BTreeMap::from([
                ("a".to_string(), set(&["std"])),
                ("b".to_string(), set(&["std", "preserve_order"])),
                ("c".to_string(), set(&["std"])),
                ("e".to_string(), set(&["std"])),
            ]),
        )]);
        let d = compare(&pins, &actual);
        let got: Vec<(&str, &Drift)> = d.iter().map(|x| (x.workspace.as_str(), &x.drift)).collect();
        assert_eq!(
            got,
            vec![
                (
                    "b",
                    &Drift::Changed {
                        added: vec!["preserve_order".into()],
                        removed: vec!["x".into()]
                    }
                ),
                ("d", &Drift::Gone),
                ("e", &Drift::Unpinned(vec!["std".into()])),
            ]
        );
    }

    #[test]
    fn an_annotation_is_a_warning_while_report_only_and_an_error_as_a_gate() {
        let f = Finding {
            part: Part::Lock,
            file: "a/Cargo.lock".into(),
            message: "m".into(),
        };
        assert_eq!(annotation(&f, Mode::ReportOnly), "::warning file=a/Cargo.lock::m");
        assert_eq!(annotation(&f, Mode::Gate), "::error file=a/Cargo.lock::m");
    }
}
