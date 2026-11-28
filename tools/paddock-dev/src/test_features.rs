//! C170 (rule 99): no released binary is built with a test feature. Test-only hooks (injected clocks, gates, fakes, test
//! setters) compile only under `#[cfg(any(test, feature = "test-support"))]`, and the feature is enabled from `[dev-dependencies]`
//! only. This check proves the second half for what the installers carry: for every released binary it asks cargo which
//! features a build of that binary turns on (`cargo tree -e normal`, so dev-dependency features stay out, as in
//! `tauri build` and a sidecar's `cargo build`), and reports every crate in that graph that has a feature of
//! [`TEST_FEATURES`]. That covers the workspace hack too: a feature that `paddock-workspace-hack` unified into the graph
//! shows on the crate it was unified onto.
//!
//! The released binaries are derived, not kept by hand ([`shipped`]): per app with a Cargo workspace (its `paddock.ci.json`
//! `cargo`), the package of its Tauri configuration's folder plus the package of every `bundle.externalBin` of every
//! `tauri*.conf.json` there (the sidecars: services, helpers, workers), and the binaries no Tauri configuration names
//! ([`EXTRA_SHIPPED`]: `paddock-setup.exe`, which per-machine installers embed).
//!
//! Report-only until its first green run on the real tree ([`REPORT_ONLY`], ER-41): findings are warnings and the exit is 0.
//! A probe that cannot see (cargo fails, or a binary's own package is missing from its tree) is `check broken`, never a pass
//! (ER-33). The positive control is `tests/it/test_features.rs`: a release crate that enables `testing` turns it red.
//!
//! ER-42: `cargo metadata` is read through its JSON. `cargo tree` has no JSON output; the check asks for its own line format
//! (`{p}<TAB>{f}`, no prefix), whose separator cargo never prints inside a package id, and parses only that.

use crate::ci_run::contract;
use paddock_cli::{CliError, Exit, Report};
use paddock_proc::{Limits, command as proc_command, run_bounded};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The check's id.
pub const ID: &str = "C170";

/// ER-41: findings are warnings until the first green run on the real tree (the conformance job's `./dev check test-features`
/// line in conformance/paddock.ci.json) is recorded here; then set this to false and the `CHECKS` row's `report_only` follows.
pub const REPORT_ONLY: bool = true;

/// Feature names that turn on test-only code. `test-support` is rule 99's (and ADR 0073's) name; `testing` the name
/// some crates used before (a stray one is still caught); `test-hooks` and `test-util` the same idea elsewhere (Pacenotes' branch, third-party crates
/// such as tokio); `mock-runtime` builds a crate on Tauri's mock runtime (paddock-shell, paddock-dialogs, paddock-panels).
pub const TEST_FEATURES: &[&str] = &["testing", "test-support", "test-hooks", "test-util", "mock-runtime"];

/// Released binaries no Tauri configuration names: (Cargo workspace folder, package, why it ships).
pub const EXTRA_SHIPPED: &[(&str, &str, &str)] = &[(
    "crates",
    "paddock-setup",
    "paddock-setup.exe, embedded by the per-machine installers (scripts/stage-paddock-setup.mjs, ADR 0053)",
)];

/// A hang detector for one cargo call (ER-12: not a performance limit, set with a wide margin). Measured 2026-11-25 on the Mac
/// mini with a warm index: the whole run, 18 binaries in 8 workspaces, took about 6 s; a cold sparse index adds the download.
const CARGO_LIMIT: Duration = Duration::from_secs(900);
/// The most one cargo call's output is read into.
const MAX_OUTPUT: usize = 64 << 20;

/// One released binary: the package cargo builds for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Shipped {
    /// The app that ships it (`family` for [`EXTRA_SHIPPED`]).
    pub app: String,
    /// Its Cargo workspace, relative to the repository root (`/`-separated).
    pub workspace: String,
    /// The package.
    pub package: String,
    /// Why it ships (the Tauri app, or the `externalBin` that names it).
    pub why: String,
}

/// A crate built with a test feature in a released binary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// The binary.
    pub shipped: Shipped,
    /// The crate that has the feature.
    pub krate: String,
    /// The feature.
    pub feature: String,
}

/// Runs cargo in a workspace folder; `Err` is the whole explanation. A port, so the derivation is tested without cargo.
pub trait Cargo {
    /// `cargo <args>` in `dir`: its standard output.
    fn run(&self, dir: &Path, args: &[&str]) -> Result<String, String>;
}

/// The real cargo, through paddock-proc's bounded runner.
pub struct RealCargo;

impl Cargo for RealCargo {
    fn run(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let mut cmd = proc_command("cargo");
        cmd.args(args).current_dir(dir);
        let out =
            run_bounded(cmd, Limits::new(CARGO_LIMIT).max_output(MAX_OUTPUT)).map_err(|e| format!("cargo {}: {e}", args.join(" ")))?;
        if !out.success() {
            let err = out.stderr_text();
            let tail: Vec<&str> = err.trim().lines().rev().take(5).collect();
            return Err(format!(
                "`cargo {}` in {} failed: {}",
                args.join(" "),
                dir.display(),
                tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
            ));
        }
        Ok(out.stdout_text())
    }
}

/// The apps under `root/apps` that have a contract, sorted.
pub fn apps(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("apps");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| format!("{} cannot be read: {e}", dir.display()))? {
        let e = e.map_err(|e| format!("{} cannot be read: {e}", dir.display()))?;
        if e.path().join(contract::FILE).is_file() {
            out.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    Ok(out)
}

/// The folder of an app's Tauri configuration (`app/src-tauri` or `src-tauri`), if it has one.
fn tauri_dir(app_dir: &Path) -> Option<PathBuf> {
    ["app/src-tauri", "src-tauri"]
        .iter()
        .map(|d| app_dir.join(d))
        .find(|d| d.join("tauri.conf.json").is_file())
}

/// The file names of every `bundle.externalBin` in the `tauri*.conf.json` files of `dir` (the platform and sidecar
/// configurations merge over the main one, so every file counts), without their folder.
pub fn external_bins(dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{} cannot be read: {e}", dir.display()))?;
    for e in entries {
        let e = e.map_err(|e| format!("{} cannot be read: {e}", dir.display()))?;
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("tauri") && name.ends_with(".conf.json")) {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).map_err(|err| format!("{} cannot be read: {err}", e.path().display()))?;
        let conf: Value = serde_json::from_str(&text).map_err(|err| format!("{} is not JSON: {err}", e.path().display()))?;
        let Some(bins) = conf.pointer("/bundle/externalBin") else {
            continue;
        };
        let bins = bins
            .as_array()
            .ok_or_else(|| format!("{}: bundle.externalBin is not an array", e.path().display()))?;
        for b in bins {
            let b = b
                .as_str()
                .ok_or_else(|| format!("{}: an externalBin entry is not a string", e.path().display()))?;
            out.insert(b.rsplit('/').next().unwrap_or(b).to_string());
        }
    }
    Ok(out)
}

/// The packages of a workspace from `cargo metadata --no-deps` output: (package name, manifest path, binary target names).
fn packages(metadata: &str) -> Result<Vec<(String, PathBuf, Vec<String>)>, String> {
    let v: Value = serde_json::from_str(metadata).map_err(|e| format!("`cargo metadata` is not JSON: {e}"))?;
    let pkgs = v
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("`cargo metadata` has no packages array")?;
    let mut out = Vec::new();
    for p in pkgs {
        let name = p.get("name").and_then(Value::as_str).ok_or("a package without a name")?;
        let manifest = p
            .get("manifest_path")
            .and_then(Value::as_str)
            .ok_or("a package without a manifest_path")?;
        let bins = p
            .get("targets")
            .and_then(Value::as_array)
            .map(|ts| {
                ts.iter()
                    .filter(|t| {
                        t.get("kind")
                            .and_then(Value::as_array)
                            .is_some_and(|k| k.iter().any(|k| k == "bin"))
                    })
                    .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push((name.to_string(), PathBuf::from(manifest), bins));
    }
    Ok(out)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (paddock_env::winpath::real(a), paddock_env::winpath::real(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The released binaries of `apps` (and [`EXTRA_SHIPPED`]-like `extras`), derived from each app's contract and Tauri
/// configuration. An app without a `cargo` workspace ships no binary yet (Pacenotes, Sighter); an app with one but no Tauri
/// configuration, or an `externalBin` no package builds, is an error (fail closed: the check would not know what ships).
pub fn shipped(root: &Path, apps: &[String], extras: &[(&str, &str, &str)], cargo: &dyn Cargo) -> Result<Vec<Shipped>, String> {
    let mut out = Vec::new();
    for app in apps {
        let c = contract::load(root, app)?;
        let Some(ws) = c.get("cargo") else {
            continue;
        };
        let ws = ws
            .as_str()
            .ok_or_else(|| format!("{}: `cargo` is not a string", contract::repo_path_of(app)))?;
        let app_dir = root.join("apps").join(app);
        let ws_dir = app_dir.join(ws);
        let workspace = format!("apps/{app}/{}", ws.trim_start_matches("./"))
            .trim_end_matches("/.")
            .trim_end_matches('/')
            .to_string();
        let tauri = tauri_dir(&app_dir).ok_or_else(|| {
            format!(
                "apps/{app} has a Cargo workspace but no tauri.conf.json in app/src-tauri or src-tauri, so C170 cannot tell what it \
                 ships; add its released packages to EXTRA_SHIPPED in tools/paddock-dev/src/test_features.rs"
            )
        })?;
        let meta = cargo.run(&ws_dir, &["metadata", "--no-deps", "--format-version", "1", "--locked"])?;
        let pkgs = packages(&meta)?;
        let manifest = tauri.join("Cargo.toml");
        let app_pkg = pkgs
            .iter()
            .find(|(_, m, _)| same_file(m, &manifest))
            .ok_or_else(|| format!("no package of {workspace} has the manifest {}", manifest.display()))?;
        out.push(Shipped {
            app: app.clone(),
            workspace: workspace.clone(),
            package: app_pkg.0.clone(),
            why: "the Tauri app".into(),
        });
        for bin in external_bins(&tauri)? {
            let pkg = pkgs
                .iter()
                .find(|(_, _, bins)| bins.contains(&bin))
                .ok_or_else(|| format!("apps/{app}: bundle.externalBin names {bin}, which no package of {workspace} builds"))?;
            let s = Shipped {
                app: app.clone(),
                workspace: workspace.clone(),
                package: pkg.0.clone(),
                why: format!("bundle.externalBin {bin}"),
            };
            if !out.iter().any(|o: &Shipped| o.workspace == s.workspace && o.package == s.package) {
                out.push(s);
            }
        }
    }
    for (ws, package, why) in extras {
        out.push(Shipped {
            app: "family".into(),
            workspace: (*ws).to_string(),
            package: (*package).to_string(),
            why: (*why).to_string(),
        });
    }
    Ok(out)
}

/// The arguments of the probe: the normal-edge feature graph of `package` for every target, one `{p}<TAB>{f}` line per crate.
pub fn tree_args(package: &str) -> Vec<String> {
    [
        "tree",
        "--locked",
        "-p",
        package,
        "-e",
        "normal",
        "--target",
        "all",
        "--prefix",
        "none",
        "--charset",
        "ascii",
        "-f",
        "{p}\t{f}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The `(crate, feature)` pairs of `tree` (the probe's output) whose feature is a test feature, and whether the root
/// `package` itself was in it (the probe's precondition: without it the output is not this binary's graph).
pub fn test_features_in(tree: &str, package: &str) -> (BTreeSet<(String, String)>, bool) {
    let mut found = BTreeSet::new();
    let mut saw_root = false;
    for line in tree.lines() {
        let Some((id, feats)) = line.split_once('\t') else {
            continue;
        };
        let name = id.split(' ').next().unwrap_or_default();
        if name == package {
            saw_root = true;
        }
        let feats = feats.trim().trim_end_matches("(*)").trim();
        for f in feats.split(',').map(str::trim) {
            if TEST_FEATURES.contains(&f) {
                found.insert((name.to_string(), f.to_string()));
            }
        }
    }
    (found, saw_root)
}

/// What a run read and found.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The binaries probed.
    pub shipped: Vec<Shipped>,
    /// What they enable.
    pub findings: Vec<Finding>,
}

/// Probes every binary of `shipped`. `Err` is `check broken`: cargo failed or the probe did not see the binary's package.
pub fn probe(root: &Path, shipped: Vec<Shipped>, cargo: &dyn Cargo) -> Result<Outcome, String> {
    let mut findings = Vec::new();
    // per-item: features are resolved per binary (one `tauri build` or sidecar `cargo build` each); a tree of several `-p`
    // unifies their features and would report what no single binary has.
    for s in &shipped {
        let args = tree_args(&s.package);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = cargo.run(&root.join(&s.workspace), &args)?;
        let (found, saw_root) = test_features_in(&out, &s.package);
        if !saw_root {
            return Err(format!(
                "the probe of {} ({}) did not list {} itself, so it cannot see that binary's features",
                s.package, s.workspace, s.package
            ));
        }
        for (krate, feature) in found {
            findings.push(Finding {
                shipped: s.clone(),
                krate,
                feature,
            });
        }
    }
    Ok(Outcome { shipped, findings })
}

/// The whole check: `apps` (all of them when `None`) and, for a whole run, [`EXTRA_SHIPPED`].
pub fn check(root: &Path, app: Option<&str>, cargo: &dyn Cargo) -> Result<Outcome, String> {
    let (apps, extras): (Vec<String>, &[(&str, &str, &str)]) = match app {
        Some(a) => (vec![a.to_string()], &[]),
        None => (apps(root)?, EXTRA_SHIPPED),
    };
    let shipped = shipped(root, &apps, extras, cargo)?;
    if shipped.is_empty() {
        return Err("no released binary was found, so nothing was probed".into());
    }
    probe(root, shipped, cargo)
}

/// The line of one finding.
pub fn finding_line(f: &Finding) -> String {
    format!(
        "{ID} {}: the release build of {} ({}, {}) enables feature \"{}\" on {}. Enable it from [dev-dependencies] only (rule 99); \
         `cargo tree -p {} -e normal,features -i {}` in {} shows who turns it on.",
        f.shipped.app,
        f.shipped.package,
        f.shipped.why,
        f.shipped.workspace,
        f.feature,
        f.krate,
        f.shipped.package,
        f.krate,
        f.shipped.workspace
    )
}

/// `./dev check test-features [--app <app>] [--root <dir>]`.
pub fn command(root: Option<PathBuf>, app: Option<String>) -> Result<Report, CliError> {
    let root = crate::repo_root::repo_root(root.as_deref())?;
    let outcome = check(&root, app.as_deref(), &RealCargo).map_err(|e| {
        CliError::new(Exit::CheckBroken, format!("::error::{ID} check broken: {e}"))
            .with_hint("Fix the cause named above (a lockfile out of date, a tauri.conf.json, cargo on PATH) and run `./dev check test-features` again.")
    })?;
    let lines: Vec<String> = outcome.findings.iter().map(finding_line).collect();
    if !lines.is_empty() && !REPORT_ONLY {
        let text: Vec<String> = lines.iter().map(|l| format!("::error::{l}")).collect();
        return Err(CliError::new(Exit::Findings, text.join("\n")).with_hint("Each line names its next step."));
    }
    let mut text: String = lines.iter().map(|l| format!("::warning::{l}\n")).collect();
    let workspaces: BTreeSet<&str> = outcome.shipped.iter().map(|s| s.workspace.as_str()).collect();
    text.push_str(&format!(
        "{ID}: {} released binar{} in {} workspace{} probed, {} finding{}{}.",
        outcome.shipped.len(),
        if outcome.shipped.len() == 1 { "y" } else { "ies" },
        workspaces.len(),
        if workspaces.len() == 1 { "" } else { "s" },
        outcome.findings.len(),
        if outcome.findings.len() == 1 { "" } else { "s" },
        if REPORT_ONLY { " (report-only, ER-41)" } else { "" }
    ));
    let shipped: Vec<Value> = outcome
        .shipped
        .iter()
        .map(|s| json!({"app": s.app, "workspace": s.workspace, "package": s.package, "why": s.why}))
        .collect();
    let findings: Vec<Value> = outcome
        .findings
        .iter()
        .map(|f| json!({"app": f.shipped.app, "package": f.shipped.package, "crate": f.krate, "feature": f.feature}))
        .collect();
    Ok(Report::new(
        json!({"shipped": shipped, "findings": findings, "reportOnly": REPORT_ONLY}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn a_test_feature_is_found_on_any_crate_and_repeats_count_once() {
        let tree = "app v1.0.0 (/x/app)\tdefault,tray\nhooked v0.1.0 (/x/hooked)\ttesting,ts\nhooked v0.1.0 (/x/hooked)\ttesting,ts (*)\n\
                    tokio v1.0.0\tmacros,test-util\nclean v0.1.0 (/x/clean)\t\nlook-alike v0.1.0\ttestings,my-testing\n";
        let (found, saw_root) = test_features_in(tree, "app");
        assert!(saw_root);
        let found: Vec<(&str, &str)> = found.iter().map(|(c, f)| (c.as_str(), f.as_str())).collect();
        assert_eq!(found, [("hooked", "testing"), ("tokio", "test-util")]);
    }

    #[test]
    fn a_tree_without_the_binary_itself_is_not_its_graph() {
        let (_, saw_root) = test_features_in("other v1.0.0\ttesting\n", "app");
        assert!(!saw_root);
        let (_, saw_root) = test_features_in("cargo said nothing useful\n", "app");
        assert!(!saw_root);
    }

    /// A cargo that answers `metadata` with `meta` and every `tree` with `tree`, recording the folders it ran in.
    struct Fake {
        meta: String,
        tree: String,
        ran: RefCell<Vec<(PathBuf, String)>>,
    }

    impl Cargo for Fake {
        fn run(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
            self.ran.borrow_mut().push((dir.to_path_buf(), args.join(" ")));
            Ok(if args[0] == "metadata" {
                self.meta.clone()
            } else {
                self.tree.clone()
            })
        }
    }

    fn app_tree(conf: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("apps/demo");
        std::fs::create_dir_all(app.join("app/src-tauri")).unwrap();
        std::fs::write(app.join(contract::FILE), r#"{"cargo": "."}"#).unwrap();
        std::fs::write(app.join("app/src-tauri/tauri.conf.json"), conf).unwrap();
        std::fs::write(app.join("app/src-tauri/Cargo.toml"), "").unwrap();
        std::fs::write(
            app.join("app/src-tauri/tauri.sidecars.conf.json"),
            r#"{"bundle": {"externalBin": ["binaries/demo-worker"]}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(d.path().join("apps/docs-only")).unwrap();
        std::fs::write(d.path().join("apps/docs-only").join(contract::FILE), "{}").unwrap();
        d
    }

    fn meta(root: &Path) -> String {
        let m = |p: &str| root.join("apps/demo").join(p).join("Cargo.toml").to_string_lossy().into_owned();
        json!({"packages": [
            {"name": "demo-app", "manifest_path": m("app/src-tauri"), "targets": [{"name": "demo-app", "kind": ["bin"]}, {"name": "demo_app", "kind": ["lib"]}]},
            {"name": "demo-side", "manifest_path": m("crates/side"), "targets": [{"name": "demo-side", "kind": ["bin"]}]},
            {"name": "demo-worker", "manifest_path": m("crates/worker"), "targets": [{"name": "demo-worker", "kind": ["bin"]}]},
            {"name": "demo-lib", "manifest_path": m("crates/lib"), "targets": [{"name": "demo_lib", "kind": ["lib"]}]}
        ]})
        .to_string()
    }

    #[test]
    fn the_shipped_set_is_the_app_and_every_external_bin_of_every_tauri_configuration() {
        let d = app_tree(r#"{"bundle": {"externalBin": ["binaries/demo-side"]}}"#);
        let fake = Fake {
            meta: meta(d.path()),
            tree: String::new(),
            ran: RefCell::default(),
        };
        let apps = apps(d.path()).unwrap();
        assert_eq!(apps, ["demo", "docs-only"]);
        let s = shipped(d.path(), &apps, &[("crates", "setup", "embedded")], &fake).unwrap();
        let got: Vec<(&str, &str, &str)> = s
            .iter()
            .map(|s| (s.app.as_str(), s.workspace.as_str(), s.package.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("demo", "apps/demo", "demo-app"),
                ("demo", "apps/demo", "demo-side"),
                ("demo", "apps/demo", "demo-worker"),
                ("family", "crates", "setup")
            ]
        );
        assert_eq!(fake.ran.borrow()[0].0, d.path().join("apps/demo/."));
    }

    #[test]
    fn an_external_bin_no_package_builds_or_a_workspace_without_tauri_fails_closed() {
        let d = app_tree(r#"{"bundle": {"externalBin": ["binaries/gone"]}}"#);
        let fake = Fake {
            meta: meta(d.path()),
            tree: String::new(),
            ran: RefCell::default(),
        };
        let e = shipped(d.path(), &["demo".into()], &[], &fake).unwrap_err();
        assert!(e.contains("externalBin names gone"), "{e}");
        std::fs::remove_file(d.path().join("apps/demo/app/src-tauri/tauri.conf.json")).unwrap();
        let e = shipped(d.path(), &["demo".into()], &[], &fake).unwrap_err();
        assert!(e.contains("no tauri.conf.json"), "{e}");
    }

    #[test]
    fn a_probe_that_does_not_list_the_binary_is_check_broken_and_findings_name_the_binary() {
        let d = app_tree(r#"{"bundle": {}}"#);
        let blind = Fake {
            meta: meta(d.path()),
            tree: "something else v1.0.0\t\n".into(),
            ran: RefCell::default(),
        };
        let e = check(d.path(), Some("demo"), &blind).unwrap_err();
        assert!(e.contains("did not list demo-app itself"), "{e}");
        let red = Fake {
            meta: meta(d.path()),
            tree: "demo-app v1.0.0 (/x)\tdefault\ndemo-worker v1.0.0 (/x)\t\ndemo-lib v0.1.0 (/y)\ttesting\n".into(),
            ran: RefCell::default(),
        };
        let o = check(d.path(), Some("demo"), &red).unwrap();
        let got: Vec<(&str, &str, &str)> = o
            .findings
            .iter()
            .map(|f| (f.shipped.package.as_str(), f.krate.as_str(), f.feature.as_str()))
            .collect();
        assert_eq!(got, [("demo-app", "demo-lib", "testing"), ("demo-worker", "demo-lib", "testing")]);
        let line = finding_line(&o.findings[0]);
        assert!(line.contains("demo-app") && line.contains("\"testing\" on demo-lib"), "{line}");
    }

    #[test]
    fn the_probe_asks_for_normal_edges_on_every_target() {
        let a = tree_args("x");
        assert!(a.windows(2).any(|w| w == ["-e", "normal"]));
        assert!(a.windows(2).any(|w| w == ["--target", "all"]));
        assert!(a.contains(&"--locked".to_string()));
    }
}
