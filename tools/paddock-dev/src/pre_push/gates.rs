//! The gates of one push, as data: which of ER-9's fast gates the change reaches (the planner's answer) and the exact command of each.
//! Pure: no process is started and no file is read here, so every verdict is tested with a plan and a list of files.
//!
//! Extension point for the per-commit gate (`./dev check --commits`, ADR 0038's 2026-11-08 amendment, X-3 in
//! docs/plans/ci-in-rust.md; it needs the user's yes and is not built): it calls [`plan_gates`] once per commit with that commit's
//! files and no ranges, in a worktree of its own. Nothing in this file depends on the push as a whole.

use crate::cargo_target;
use serde_json::Value;
use std::collections::BTreeSet;

/// What a gate needs on the machine before it can say anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// Nothing beyond the program itself (checked when it starts).
    Nothing,
    /// A program on PATH (`cargo-xwin`, `cargo-deny`): without it the gate is "not measured", never a pass (ER-33).
    Tool(String),
}

/// One command of the push gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    /// The row of the table: `fmt crates`, `clippy-windows apps/dyno`, `conformance`.
    pub id: String,
    /// The folder it runs in, relative to the repository root (empty: the root).
    pub cwd: String,
    /// The program and its arguments, exactly as CI runs them.
    pub program: String,
    /// The arguments.
    pub args: Vec<String>,
    /// Variables this gate alone sets.
    pub env: Vec<(String, String)>,
    /// Runs through `heavy` (ER-24) with a core cap: compiles and lints.
    pub heavy: bool,
    /// What must exist before it can run.
    pub need: Need,
    /// Cheap gates first (0), compiles later: the common failures surface in seconds.
    pub rank: u8,
    /// What to do when it fails, in one clause.
    pub next: String,
    /// The Cargo workspace folders it lints (the host Clippy), for the steps that name them; empty for the other gates.
    pub workspaces: Vec<String>,
    /// One line the push prints when the gate checks less than its full command (the deny row without advisories); `None` otherwise.
    pub notice: Option<String>,
    /// How its output is read for progress ([`super::progress`]): cargo's JSON messages for the host and xwin Clippy
    /// (the executor adds the flag to the cargo command), every output line for the rest.
    pub progress: super::progress::Progress,
    /// The cargo build folder it uses, relative to the repository root or absolute (`None`: not a cargo build, or not known): a step
    /// whose folder holds no build yet is COLD, and the history keeps cold and warm apart.
    pub build_dir: Option<String>,
    /// The row runs CI's own tests (the member gate's `tests <ws>`): its child gets CI's job environment
    /// ([`crate::ci_run::job_env`]) less the secrets, not the [`super::exec::PASS`] list of the other rows.
    pub ci_env: bool,
}

impl Gate {
    /// The command line a person re-runs, with its folder.
    pub fn rerun(&self) -> String {
        let line = std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ");
        if self.cwd.is_empty() {
            line
        } else {
            format!("cd {} && {line}", self.cwd)
        }
    }
}

/// `word` as one shell word: unchanged when it holds only safe characters, else in single quotes, so a printed command can be
/// pasted.
pub fn shell_quote(word: &str) -> String {
    let safe = !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=+@%,".contains(c));
    if safe {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// A gate the change reaches, or the reason it does not, or a check that cannot be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planned {
    /// Run it.
    Run(Gate),
    /// Not reached by this change, with the reason (printed in the table).
    Skip {
        /// The row.
        id: String,
        /// Why.
        reason: String,
    },
    /// The environment is not ready (a missing or stale install): the check did not look, which is not a finding in the change and
    /// not a broken check (ADR 0089 amendment A2): exit 6, never a pass.
    NotMeasured {
        /// The row.
        id: String,
        /// What is not ready.
        reason: String,
        /// What to do.
        next: String,
    },
    /// The check itself is broken (its input cannot be read): the push does not pass (ER-33).
    Broken {
        /// The row.
        id: String,
        /// What is wrong.
        cause: String,
        /// What to do.
        next: String,
    },
}

/// What the plan alone cannot say; read from the repository once by the caller.
#[derive(Debug, Clone)]
pub struct Facts {
    /// The conformance command's arguments after `node`, read from conformance/paddock.ci.json, or why that failed.
    pub conformance: Option<Result<Vec<String>, String>>,
    /// Why conformance's npm dependencies are not installed (the environment is not ready), or None when they are.
    pub conformance_env: Option<String>,
    /// Workspace folders whose CI lints Windows too (`apps/<app>`, from `windowsClippy`); `crates` always does.
    pub windows_clippy: BTreeSet<String>,
    /// Pitwall's script tests (`scripts/ci/*.test.mjs`, relative to `apps/pitwall`): no shell expands a glob here.
    pub pitwall_tests: Vec<String>,
    /// This program, for the checks `./dev` runs itself ([`crate::tool_cache::self_exe`]), or why it cannot be started again (a test
    /// program is never started as paddock-dev: its harness would run its tests again).
    pub dev_exe: Result<String, String>,
    /// The run is the member gate of `./dev merge check --gates` (`--member-gate`): the repository-wide audits run too.
    pub member_gate: bool,
    /// A kept build folder per workspace (`<it>/<workspace>`), used when the caller sets no `CARGO_TARGET_DIR`: the throw-away
    /// worktree of a pushed commit must not hold the builds (they would start cold every run).
    pub kept_targets: Option<String>,
    /// The caller's `CARGO_TARGET_DIR`: the Windows lint gets `<it>/xwin` (ADR 0021 §3).
    pub target_dir: Option<String>,
    /// The apps whose own build checks this run leaves out ([`crate::active_apps`]: the apps `docs/ci-active-apps.toml` does not
    /// name; none with `--all-apps`), or why the set cannot be read (then every row runs and the `active-apps` row is broken).
    pub inactive: Result<BTreeSet<String>, String>,
    /// The nextest filterset of the packages each workspace's change reaches, as CI's member run narrows it
    /// ([`crate::plan_ci::package_filter`], the one copy); a workspace with none (or an empty one) runs every test.
    pub test_filters: std::collections::BTreeMap<String, String>,
    /// The nextest configuration file an app's contract names (`nextestConfig`, repository-relative), by workspace.
    pub nextest_config: std::collections::BTreeMap<String, String>,
    /// The packages each workspace's change reaches, where [`Facts::test_filters`] narrows to them (absent: every package).
    pub reached: std::collections::BTreeMap<String, BTreeSet<String>>,
}

/// The reason of every row an inactive app's set leaves out: a skip with this reason is never a pass ([`super::render_timed`]
/// counts it apart).
pub fn inactive_reason(app: &str) -> String {
    format!(
        "{}: {app} is not an active app; its own build checks run in the nightly (and in `./dev check --pre-push --all-apps`)",
        inactive_label()
    )
}

/// The first words of [`inactive_reason`], `inactive (docs/ci-active-apps.toml)`: how the table and its tests tell an inactive app's
/// row from another skip. The file's name comes from its one accessor ([`crate::active_apps::FILE`]).
pub fn inactive_label() -> String {
    format!("inactive ({})", crate::active_apps::FILE)
}

/// The app a Cargo workspace belongs to (`apps/<app>` and every workspace below it), `None` for the foundation (`crates`,
/// `tools/...`).
pub fn app_of_workspace(ws: &str) -> Option<&str> {
    ws.strip_prefix("apps/").and_then(|r| r.split('/').next()).filter(|a| !a.is_empty())
}

/// The inactive app `ws` belongs to, when this run leaves that app out.
pub fn inactive_app(facts: &Facts, ws: &str) -> Option<String> {
    let app = app_of_workspace(ws)?;
    facts.inactive.as_ref().ok().filter(|s| s.contains(app)).map(|_| app.to_string())
}

/// Whether an app's own rows are left out of this run.
fn app_inactive(facts: &Facts, app: &str) -> bool {
    facts.inactive.as_ref().is_ok_and(|s| s.contains(app))
}

/// The push: its changed files and the commits to lint.
#[derive(Debug, Clone, Default)]
pub struct Change {
    /// Repository-relative files: committed since the merge base, modified and untracked.
    pub files: Vec<String>,
    /// The commit ranges to lint (`<merge base>..<pushed commit>`, one per pushed ref); none when there is no commit of its own.
    pub ranges: Vec<String>,
    /// The commit the files were measured from. `None`: unknown, and the checks that judge only what the push adds run in full.
    pub base: Option<String>,
}

/// The rows that do not depend on a workspace, as the pass record stores them (`known::write_record`): a record made by a tool with
/// another set is refused, so a row added here is never skipped through an older pass. A test checks it against the planner.
pub const ROW_SET: &[&str] = &[
    "commit-lint",
    "conformance",
    "conformance-tests",
    "dyno-wire-fixtures",
    "gates-register",
    "locks-guard",
    "member-audits",
    "pitwall-deny",
    "pitwall-scripts",
];

/// The id prefixes of the rows planned per workspace : they
/// are not in [`ROW_SET`], and the planner's test tells the fixed rows from these.
pub const PER_WORKSPACE_PREFIXES: &[&str] = &["fmt ", "lockfile ", "clippy ", "clippy-windows ", "tests "];

/// Workspaces whose lints are not a push gate: they have a CI job of their own that is not part of a branch run.
const OWN_JOB: &[&str] = &["/fuzz", "/templates/"];

/// `g` run by this program ([`Facts::dev_exe`]), or not measured when it cannot be started again: never a guessed `./dev`, never
/// a test program (whose harness would take the arguments as test filters and run its tests again).
fn dev_run(facts: &Facts, g: Gate) -> Planned {
    match &facts.dev_exe {
        Ok(program) => Planned::Run(Gate {
            program: program.clone(),
            ..g
        }),
        Err(why) => Planned::NotMeasured {
            id: g.id,
            reason: format!("this program cannot be started again: {why}"),
            next: "run the push gate through ./dev (the paddock-dev binary); this is not a finding in your change".into(),
        },
    }
}

fn gate(id: String, cwd: &str, program: &str, args: &[&str], rank: u8, next: String) -> Gate {
    Gate {
        id,
        cwd: cwd.to_string(),
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Vec::new(),
        heavy: false,
        need: Need::Nothing,
        rank,
        next,
        workspaces: Vec::new(),
        notice: None,
        progress: super::progress::Progress::Lines,
        build_dir: None,
        ci_env: false,
    }
}

/// A workspace folder as one folder name: `apps/dyno` is `apps-dyno`.
fn slug(ws: &str) -> String {
    ws.replace('/', "-")
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The affected Cargo workspaces, in the planner's order.
pub fn workspaces(plan: &Value) -> Vec<String> {
    plan["packages"]
        .as_object()
        .map(|o| o.keys().filter(|k| !OWN_JOB.iter().any(|s| k.contains(s))).cloned().collect())
        .unwrap_or_default()
}

/// The workspaces that hold a changed file themselves (the longest workspace folder that is a prefix of the file): the ones whose
/// own Clippy and format the change can break. The others that the plan reaches are dependents, which CI lints.
pub fn own_workspaces(plan: &Value, files: &[String]) -> std::collections::BTreeSet<String> {
    let all = workspaces(plan);
    files
        .iter()
        .filter_map(|f| {
            all.iter()
                .filter(|w| f.starts_with(&format!("{w}/")))
                .max_by_key(|w| w.len())
                .cloned()
        })
        .collect()
}

/// The developer tools whose Clippy also runs for Windows here, through xwin on the Mac like `crates` (user, 2026-11-26): four Windows-only
/// CI failures in `tools/paddock-dev` in one day, the latest an import used only by `cfg(unix)` tests (dead code on Windows), found
/// only by CI's Windows leg. Selected by the plan like every other row: a tool the change reaches.
pub const WINDOWS_LINTED_TOOLS: &[&str] = &["tools/paddock-dev", "tools/throttle"];

/// Why a workspace gets no Windows lint here. An app opts in with `windowsClippy` in its `paddock.ci.json`; a workspace with no
/// such contract (a tool other than [`WINDOWS_LINTED_TOOLS`]) is linted by CI's own job on every OS leg, so on Windows natively.
pub(super) fn windows_skip_reason(ws: &str) -> String {
    if ws.starts_with("apps/") {
        "CI lints this workspace on the host OSes only (no windowsClippy in its paddock.ci.json)".into()
    } else {
        format!("{ws} has no paddock.ci.json contract: CI's own job runs cargo clippy on its Windows leg natively, so no xwin lint here")
    }
}

/// The cargo build folder a workspace's host (or, with `suffix`, xwin) row builds in, as the rows are told it: the caller's
/// `CARGO_TARGET_DIR`, else the kept folder of the workspace, else cargo's own `<workspace>/target`.
fn build_dir(facts: &Facts, ws: &str, suffix: Option<&str>) -> String {
    let base = match (&facts.target_dir, &facts.kept_targets) {
        (Some(dir), _) => dir.trim_end_matches('/').to_string(),
        (None, Some(k)) => format!("{}/{}", k.trim_end_matches('/'), slug(ws)),
        (None, None) => format!("{ws}/target"),
    };
    match suffix {
        Some(s) => format!("{base}/{s}"),
        None => base,
    }
}

/// The row that says the doctests are off: `cargo test --doc` runs nowhere, here, in CI or in the nightly (user 2026-11-24: the APIs
/// are unstable; every library target says `doctest = false` and doc examples are text blocks, crates/README.md "Doctests"). Named in
/// every run that reaches a Cargo workspace, so the table says what is not checked; a skip, never a pass.
pub const DOCTESTS_OFF: &str = "doctests: off (user 2026-11-24; crates/README.md, \"Doctests\"): every library target has `doctest = false` and doc examples are text, so no `cargo test --doc` runs here, in CI or in the nightly";

/// The gates `change` reaches under `plan`, in table order. Each is CI's own command (the one source is named in `next` or the
/// doc of the check that reads it).
pub fn plan_gates(plan: &Value, change: &Change, facts: &Facts) -> Vec<Planned> {
    plan_rows(plan, change, facts)
}

fn plan_rows(plan: &Value, change: &Change, facts: &Facts) -> Vec<Planned> {
    let mut out = Vec::new();
    let own = own_workspaces(plan, &change.files);
    let mut dependents = Vec::new();
    for ws in workspaces(plan) {
        let all_features = ws == "crates";
        out.push(Planned::Run(gate(
            format!("lockfile {ws}"),
            &ws,
            "cargo",
            &["metadata", "--locked", "--format-version", "1"],
            1,
            format!("Cargo.lock in {ws} is out of date: run `cargo metadata` there without --locked, review the lock change and commit it"),
        )));
        if !own.contains(&ws) {
            dependents.push(ws);
            continue;
        }
        // An inactive app's own build checks (format, Clippy, the Windows lint) are left out and named; its lockfile row
        // above still runs: Cargo.lock is a generated file and stays current (ER-40), in seconds.
        if let Some(app) = inactive_app(facts, &ws) {
            for kind in ["fmt", "clippy", "clippy-windows"] {
                out.push(Planned::Skip {
                    id: format!("{kind} {ws}"),
                    reason: inactive_reason(&app),
                });
            }
            continue;
        }
        out.push(Planned::Run(gate(
            format!("fmt {ws}"),
            &ws,
            "cargo",
            &["fmt", "--all", "--check"],
            0,
            "format your own files (`rustfmt --edition 2024 <file>`; never `cargo fmt --all` while others edit), then re-run `./dev check --pre-push`".to_string(),
        )));
        let mut clippy: Vec<&str> = vec!["clippy", "--workspace", "--all-targets"];
        if all_features {
            clippy.push("--all-features");
        }
        clippy.extend(["--locked", "--", "-D", "warnings"]);
        let mut g = gate(
            format!("clippy {ws}"),
            &ws,
            "cargo",
            &clippy,
            2,
            "fix the lints it printed (a Clippy that needs `--fix` runs on your own files only)".into(),
        );
        g.heavy = true;
        // The build folder goes in as cargo configuration, never as `CARGO_TARGET_DIR` (sccache hashes it: crate::cargo_target).
        match (&facts.target_dir, &facts.kept_targets) {
            (Some(dir), _) => g.args = cargo_target::with_target_dir(&g.args, dir.trim_end_matches('/')),
            (None, Some(k)) => g.args = cargo_target::with_target_dir(&g.args, &format!("{}/{}", k.trim_end_matches('/'), slug(&ws))),
            (None, None) => {}
        }
        g.progress = super::progress::Progress::CargoJson;
        g.build_dir = Some(build_dir(facts, &ws, None));
        out.push(Planned::Run(g));
        if all_features || facts.windows_clippy.contains(&ws) || WINDOWS_LINTED_TOOLS.contains(&ws.as_str()) {
            let mut args: Vec<&str> = vec![
                "xwin",
                "clippy",
                "--target",
                "x86_64-pc-windows-msvc",
                "--workspace",
                "--all-targets",
            ];
            if all_features {
                args.push("--all-features");
            }
            args.extend(["--locked", "--", "-D", "warnings"]);
            let mut g = gate(
                format!("clippy-windows {ws}"),
                &ws,
                "cargo",
                &args,
                3,
                "an import or helper used only by a unix-gated test is dead code on Windows (lessons.md, \"Windows-only assumptions in tests\"): gate it with cfg too. If rustup says the target is not installed: `rustup target add x86_64-pc-windows-msvc --toolchain <the channel in rust-toolchain.toml>` (the pinned toolchain has no extra targets until you add one)".into(),
            );
            g.heavy = true;
            g.need = Need::Tool("cargo-xwin".into());
            match (&facts.target_dir, &facts.kept_targets) {
                (Some(dir), _) => g.args = cargo_target::with_target_dir(&g.args, &format!("{}/xwin", dir.trim_end_matches('/'))),
                (None, Some(k)) => {
                    g.args = cargo_target::with_target_dir(&g.args, &format!("{}/{}/xwin", k.trim_end_matches('/'), slug(&ws)));
                }
                (None, None) => {}
            }
            g.progress = super::progress::Progress::CargoJson;
            g.build_dir = Some(build_dir(facts, &ws, Some("xwin")));
            out.push(Planned::Run(g));
        } else {
            out.push(Planned::Skip {
                id: format!("clippy-windows {ws}"),
                reason: windows_skip_reason(&ws),
            });
        }
    }
    if workspaces(plan).is_empty() {
        out.push(Planned::Skip {
            id: "fmt, clippy, lockfiles".into(),
            reason: "the change reaches no Cargo workspace".into(),
        });
    } else {
        out.push(Planned::Skip {
            id: "doctests".into(),
            reason: DOCTESTS_OFF.into(),
        });
    }
    if !dependents.is_empty() {
        out.push(Planned::Skip {
            id: "clippy dependents".into(),
            reason: format!(
                "{} workspace(s) reached through dependencies, none with a changed file of its own ({}): CI covers their Clippy (ER-9 overlap)",
                dependents.len(),
                dependents.join(", ")
            ),
        });
    }
    out.extend(active_apps_row(facts));
    out.push(conformance(facts));
    out.push(conformance_tests(plan, facts));
    out.extend(commit_lint(change));
    out.push(member_audits(facts));
    out.extend(member_tests(plan, change, facts));
    let reaches_pitwall = plan["apps"].as_array().is_some_and(|a| a.iter().any(|x| x == "pitwall"));
    if reaches_pitwall && app_inactive(facts, "pitwall") {
        out.push(Planned::Skip {
            id: "pitwall-scripts, pitwall-deny".into(),
            reason: inactive_reason("pitwall"),
        });
    } else if reaches_pitwall {
        let mut args = vec!["--test".to_string()];
        args.extend(facts.pitwall_tests.iter().cloned());
        let mut g = gate(
            "pitwall-scripts".into(),
            "apps/pitwall",
            "node",
            &[],
            0,
            "run `cd apps/pitwall && node --test scripts/ci/*.test.mjs` and fix the test it names".into(),
        );
        g.args = args;
        out.push(if facts.pitwall_tests.is_empty() {
            Planned::Broken {
                id: "pitwall-scripts".into(),
                cause: "apps/pitwall/scripts/ci has no *.test.mjs file".into(),
                next: "restore the script tests or remove this gate from pre_push/gates.rs".into(),
            }
        } else {
            Planned::Run(g)
        });
        let mut d = gate("pitwall-deny".into(), "apps/pitwall", "cargo", &[], 1, String::new());
        let scope = super::deny::scope(change, "apps/pitwall");
        d.id = scope.label(&d.id);
        d.next = format!(
            "`cargo {}` in apps/pitwall names the finding; fix it or record it in deny.toml",
            scope.args().join(" ")
        );
        d.args = scope.args();
        d.notice = scope.notice("apps/pitwall");
        d.need = Need::Tool("cargo-deny".into());
        out.push(Planned::Run(d));
    } else {
        out.push(Planned::Skip {
            id: "pitwall-scripts, pitwall-deny".into(),
            reason: "the change does not reach Pitwall".into(),
        });
    }
    out.push(locks_guard(change, facts));
    // C100 (ADR 0060) on every push, like the conformance row (ER-41; ER-42: a hand list of its inputs missed apps/*/paddock.ci.json,
    // so that is not kept). Checks landed unwired on 2026-11-12 (four) and 2026-11-17 (C138). `./dev gates check` is CI's own command;
    // 5 to 8 s, in parallel with the conformance tests (docs/benchmarks.md, "C100 in the push gate"). It blocks from its first run:
    // C100 is long established (0 findings on the real tree at 97176067c, `./dev gates check`, 2026-11-17) and the user asked for the
    // refusal (2026-11-17).
    out.push(dev_run(
        facts,
        Gate {
            args: strs(&["gates", "check"]),
            ..gate(
                "gates-register".into(),
                "",
                "",
                &[],
                0,
                "fix the C100 finding printed above: its ::error line names the check and the fix (ADR 0060)".into(),
            )
        },
    ));
    // Dyno's wire fixtures (the UI contract) are regenerated from the Rust types when dyno-core's sources changed; a stale one
    // fails the push with the files named (apps/dyno/scripts/regen-wire-fixtures.sh runs cargo test). A generator: it runs whether
    // Dyno is active or not, as generated files stay current (ER-40).
    if change
        .files
        .iter()
        .any(|f| f.starts_with("apps/dyno/crates/dyno-core/src/") && f.ends_with(".rs"))
    {
        out.push(Planned::Run(Gate {
            heavy: true,
            ..gate(
                "dyno-wire-fixtures".into(),
                "",
                "sh",
                &["apps/dyno/scripts/regen-wire-fixtures.sh"],
                1,
                "update the *.contract.spec.ts the script names and commit the regenerated *.wire.json with it".into(),
            )
        }));
    } else {
        out.push(Planned::Skip {
            id: "dyno-wire-fixtures".into(),
            reason: "no file under apps/dyno/crates/dyno-core/src changed".into(),
        });
    }
    out
}

/// The workspaces the member gate tests: every Cargo workspace with a changed file of its own (documentation aside), and
/// `crates` when only `.cargo/` (which every build of it reads) changed. Pure.
pub fn touched_workspaces(plan: &Value, change: &Change) -> BTreeSet<String> {
    let code: Vec<String> = change.files.iter().filter(|f| !docs_only(f)).cloned().collect();
    let mut out = own_workspaces(plan, &code);
    if workspaces(plan).iter().any(|w| w == "crates") && code.iter().any(|f| f.starts_with(".cargo/")) {
        out.insert("crates".into());
    }
    out
}

/// The active set cannot be read (or names an app the tree does not have): every app's rows run (nothing is left out on a guess),
/// and this row is broken, so the push does not pass on it (ER-33). `None` when the set was read.
fn active_apps_row(facts: &Facts) -> Option<Planned> {
    facts.inactive.as_ref().err().map(|why| Planned::Broken {
        id: "active-apps".into(),
        cause: why.clone(),
        next: format!(
            "fix {} (`./dev plan --ci` reads the same file and stops on it too); every app's rows ran meanwhile",
            crate::active_apps::FILE
        ),
    })
}

/// Conformance's npm install is not usable: the row did not look (exit 6), which is not a finding in the change.
fn conformance_not_ready(id: &str, facts: &Facts) -> Option<Planned> {
    facts.conformance_env.as_ref().map(|cause| Planned::NotMeasured {
        id: id.to_string(),
        reason: format!("environment not ready: {cause}"),
        next: "run `npm ci --ignore-scripts -w conformance` in the main checkout, then re-run `./dev check --pre-push`; this is not a finding in your change".into(),
    })
}

/// The conformance row runs on every push, whatever changed and whatever the plan says (ER-41, user 2026-11-14): its checks read
/// app code, specs and workflows, and a check skipped on the change that brings in its class never sees it. The planner marks
/// conformance on every plan too (`conformance always runs`); this gate does not depend on that.
fn conformance(facts: &Facts) -> Planned {
    let id = "conformance".to_string();
    if let Some(p) = conformance_not_ready(&id, facts) {
        return p;
    }
    match &facts.conformance {
        // The contract was read and is well-formed; the gate runs it through the executor, as CI does (`./dev ci run conformance paddock`),
        // so a flag or field added to the contract reaches the push gate with no change here.
        Some(Ok(_)) => dev_run(facts, Gate {
            args: strs(&["ci", "run", "conformance", "paddock"]),
            ..gate(id, "", "", &[], 0, "fix the finding it printed; the list of checks is the `conformance` field of conformance/paddock.ci.json (CI's \"Paddock itself\" step runs the same)".into())
        }),
        Some(Err(e)) => Planned::Broken { id, cause: e.clone(), next: "fix the `conformance` object in conformance/paddock.ci.json (app, root, only, strict)".into() },
        None => Planned::Broken { id, cause: "the conformance contract was not read".into(), next: "report this: the caller of plan_gates must fill Facts::conformance".into() },
    }
}

/// The conformance package's own tests (`cd conformance && npm test`), run through the executor as CI's conformance job does
/// (`./dev ci run conformance tests`, the `tests` list of conformance/paddock.ci.json). In a developer's push only when the change
/// reaches the conformance project for a reason of its own (the planner's reach, [`crate::plan::reaches_conformance`]: conformance/**,
/// a check's declared input, a workflow); the member gate runs them whatever changed, because a test there reads the whole tree
/// (lib-usage.test.mjs failed on fix/spotter-release-plan-verdict, 2026-11-14, while its touched-file tests passed). User,
/// 2026-11-24: the row was 154 of the push gate's ~575 row-minutes a day.
///
/// Blocking since its first green run on the real tree (ER-41): `./dev check --pre-push --refs-from-stdin --remote origin` on
/// ci/conformance-every-change, 2026-11-14 (6002697d3: 71.1 s; the hook's run on the push of c84694951: 70.6 s), recorded in
/// docs/benchmarks.md, "Conformance tests in the push gate". A local gate has no CI run id; the benchmark section is its record.
///
/// [`CONFORMANCE_TEST_CONCURRENCY`] caps the test processes (conformance/test/run-tests.mjs reads it; CI leaves it unset).
/// The conformance tests' share of the machine in the push gate (ER-24): uncapped, `node --test` starts one process per core
/// (about 13 on the 14-core Mac mini) beside the Clippy rows; the gate runs at most three rows at a time.
const CONFORMANCE_TEST_CONCURRENCY: &str = "4";

fn conformance_tests(plan: &Value, facts: &Facts) -> Planned {
    let id = "conformance-tests";
    if !facts.member_gate && !crate::plan::reaches_conformance(plan) {
        return Planned::Skip {
            id: id.into(),
            reason: "the change does not reach the conformance project (the member gate runs them whatever changed)".into(),
        };
    }
    // The phase also compiles paddock-dev's contracts tests: the executor puts this build folder on that line as cargo configuration
    // (never a CARGO_* variable) and the gate runs through heavy (ER-24), compiling through the local sccache.
    let mut env: Vec<(String, String)> = vec![("PADDOCK_TEST_CONCURRENCY".into(), CONFORMANCE_TEST_CONCURRENCY.into())];
    match (&facts.target_dir, &facts.kept_targets) {
        (Some(d), _) => env.push((crate::ci_run::GATE_TARGET_DIR.into(), d.trim_end_matches('/').to_string())),
        (None, Some(k)) => env.push((
            crate::ci_run::GATE_TARGET_DIR.into(),
            format!("{}/tools-paddock-dev", k.trim_end_matches('/')),
        )),
        (None, None) => {}
    }
    conformance_not_ready(id, facts).unwrap_or_else(|| {
        dev_run(
            facts,
            Gate {
                args: strs(&["ci", "run", "conformance", "tests"]),
                heavy: true,
                env,
                ..gate(
                    id.into(),
                    "",
                    "",
                    &[],
                    0,
                    "fix the test it names (`cd conformance && npm test`; CI's conformance job runs the same)".into(),
                )
            },
        )
    })
}

/// The nextest profile that selects the repository-wide audits (`tools/paddock-dev/.config/nextest.toml`, its `default-filter`): the
/// one place that lists them (ER-42), so a new audit is one line there and the gate and its tests read the same list.
pub const AUDIT_PROFILE: &str = "member-audits";

/// The two parts of the audit profile's `default-filter`: the leading `(binary(it) & ( ... ))` group (the audits of `tests/it`) and the
/// rest (clauses that select tests of the tool's own modules by name: the source audits `no_source_file_*`). Pure text in, text out;
/// `None` when the filter does not start with such a group (the guards of both parts read the filter through this one function, ER-42).
pub fn split_audit_filter(filter: &str) -> Option<(String, String)> {
    let f = filter.trim();
    if !f.starts_with("(binary(it)") {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in f.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((f[..=i].to_string(), f[i + 1..].trim().to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

/// The audits of `tools/paddock-dev/tests/it`: tests that read the whole tree (the allow-list parity, the cargo-target lint, the
/// parity corpus, the plan against the tree, the golden checks). A member's own touched-file tests never see a clash between two
/// members' changes in them; on the merged tree they do (train 13: local-sccache's audits against golden-checked-when-made's code, the
/// allow-list parity against C135, the parity corpus against test-env-builder; each first showed in the train-wide nextest run, ER-8).
///
/// Runs only when the push gate is the member gate (`./dev merge check --gates` passes `--member-gate`), whatever changed (ER-41); a
/// developer's push skips it with a reason, as the user wants no extra minutes there (2026-11-17). Blocking from the start: the audits are established tests, not a new check (user, 2026-11-17). `--no-tests=fail`
/// makes a filter that selects nothing a failure, never a pass (ER-33); `RUSTUP_AUTO_INSTALL=0` stops a probe from installing a
/// toolchain (ER-45). Measured in docs/benchmarks.md, "Member audits in the member gate".
fn member_audits(facts: &Facts) -> Planned {
    if !facts.member_gate {
        return Planned::Skip {
            id: "member-audits".into(),
            reason: "they run in the member gate of `./dev merge check --gates` (`--member-gate`), not in a developer's push".into(),
        };
    }
    let mut g = Gate {
        heavy: true,
        need: Need::Tool("cargo-nextest".into()),
        env: vec![("RUSTUP_AUTO_INSTALL".into(), "0".into())],
        ..gate(
            "member-audits".into(),
            "tools/paddock-dev",
            "cargo",
            &["nextest", "run", "--profile", AUDIT_PROFILE, "--locked", "--no-tests=fail"],
            4,
            format!(
                "fix the audit it names, or the change that clashes with it; re-run `cd tools/paddock-dev && cargo nextest run --profile {AUDIT_PROFILE}` (the list is the profile's default-filter in tools/paddock-dev/.config/nextest.toml)"
            ),
        )
    };
    // The build folder goes in as cargo configuration, never as `CARGO_TARGET_DIR` (sccache hashes it: crate::cargo_target). `cargo nextest`
    // is not a builtin cargo subcommand, so `with_target_dir` would not place it; nextest takes `--config` itself, after `run`.
    let dir = match (&facts.target_dir, &facts.kept_targets) {
        (Some(d), _) => Some(d.trim_end_matches('/').to_string()),
        (None, Some(k)) => Some(format!("{}/tools-paddock-dev", k.trim_end_matches('/'))),
        (None, None) => None,
    };
    if let Some(dir) = dir {
        g.args.splice(2..2, ["--config".to_string(), cargo_target::config_value(&dir)]);
    }
    Planned::Run(g)
}

/// The member gate's test row: this program's `ci test-cache run` (ADR 0116, ER-49), which runs CI's `ci` profile nextest command ([`crate::test_cache::cmd`], `--no-tests=fail` unless reuse left nothing to run, every feature in `crates`, an app's own nextest configuration) with each package
/// whose inputs did not change since a green run left out and its pass stored when it passes whole. Reuse is on only when
/// `docs/test-cache.toml` has recorded two clean shadow runs; until then every package runs and the passes are recorded (shadow). The build
/// folder goes in as cargo configuration (`--build-dir`), never as `CARGO_TARGET_DIR` (sccache hashes it).
fn test_cache_args(ws: &str, facts: &Facts, filter: &str) -> Vec<String> {
    let mut args = strs(&["ci", "test-cache", "run", "--workspace", ws]);
    if facts.target_dir.is_some() || facts.kept_targets.is_some() {
        args.extend(["--build-dir".to_string(), build_dir(facts, ws, None)]);
    }
    if let Some(cfg) = facts.nextest_config.get(ws) {
        let up = "../".repeat(ws.split('/').filter(|p| !p.is_empty()).count());
        args.extend(["--nextest-config".to_string(), format!("{up}{cfg}")]);
    }
    if !filter.trim().is_empty() {
        args.extend(["--filter".to_string(), filter.to_string()]);
    }
    args
}

/// The tests of `ws` its host run selects: the plan's reached packages as CI's member run narrows them ([`Facts::test_filters`]).
fn reached_filter<'a>(ws: &str, facts: &'a Facts) -> &'a str {
    facts.test_filters.get(ws).map(String::as_str).unwrap_or("")
}

/// The member gate's tests (user, 2026-11-25): the full nextest `ci` profile of every Cargo workspace the member touches (a workspace
/// with a changed file of its own, documentation aside), on this host (`tests <ws>`). The Linux tests, with the orphan guardian's
/// acting tests as uid 65534, are CI's Linux jobs on the Linux runner: no VM or container starts on this Mac (user, 2026-11-26;
/// docs/rules.md rule 101), so there is no `tests-linux` row. Only in the member gate (`--member-gate`): a developer's push
/// skips them with the reason. An inactive app's rows are skipped and named. A missing nextest is not measured, never a pass.
/// Blocking from the start: the tests are CI's own established tests, not a new check (user, 2026-11-25). Measured in
/// docs/benchmarks.md, "Member gate: CI tests of the touched workspaces".
fn member_tests(plan: &Value, change: &Change, facts: &Facts) -> Vec<Planned> {
    if !facts.member_gate {
        return vec![Planned::Skip {
            id: "tests".into(),
            reason: "they run in the member gate of `./dev merge check --gates` (`--member-gate`), not in a developer's push".into(),
        }];
    }
    let (candidates, off): (BTreeSet<String>, BTreeSet<String>) = touched_workspaces(plan, change)
        .into_iter()
        .partition(|ws| inactive_app(facts, ws).is_none());
    let mut out: Vec<Planned> = Vec::new();
    for ws in &off {
        out.push(Planned::Skip {
            id: format!("tests {ws}"),
            reason: inactive_reason(app_of_workspace(ws).unwrap_or(ws)),
        });
    }
    if candidates.is_empty() {
        if off.is_empty() {
            out.push(Planned::Skip {
                id: "tests".into(),
                reason: "nothing but documentation, or no Cargo workspace of its own, changed".into(),
            });
        }
        return out;
    }
    for ws in &candidates {
        let mut g = Gate {
            heavy: true,
            need: Need::Tool("cargo-nextest".into()),
            env: vec![("RUSTUP_AUTO_INSTALL".into(), "0".into())],
            ci_env: true,
            ..gate(
                format!("tests {ws}"),
                "",
                "",
                &[],
                5,
                "fix the test it names (its failed tests and first panic are above), then re-run the member gate".into(),
            )
        };
        g.args = test_cache_args(ws, facts, reached_filter(ws, facts));
        g.build_dir = Some(build_dir(facts, ws, None));
        out.push(dev_run(facts, g));
    }
    out
}

/// The shared-crate dependency guard (`./dev locks guard`): the workspace hack, the resolved features of the watched crates and every
/// lockfile agree with the manifests. A dependency added to a shared crate changed all three twice (`toml`, then `oxc` and `syn` in
/// paddock-repo-graph), found only in CI each time. Runs when a manifest, a lockfile, the hack config or its pins file changed (the
/// files that can bring in its class, ER-41); a push of other files skips it with the reason. Report-only until its first green run on
/// the real tree (`locks_guard::MODE`, where its first green run is recorded): the guard prints warnings and exits 0, so this row passes.
fn locks_guard(change: &Change, facts: &Facts) -> Planned {
    let reaches = |f: &str| {
        let name = f.rsplit('/').next().unwrap_or(f);
        name == "Cargo.toml" || name == "Cargo.lock" || name == "hakari.toml" || f == crate::locks_guard::PINS_FILE
    };
    if !change.files.iter().any(|f| reaches(f)) {
        return Planned::Skip {
            id: "locks-guard".into(),
            reason: "no Cargo.toml, Cargo.lock, hakari.toml or docs/shared-crate-features.toml changed".into(),
        };
    }
    dev_run(
        facts,
        Gate {
            args: strs(&["locks", "guard"]),
            ..gate(
                "locks-guard".into(),
                "",
                "",
                &[],
                1,
                "read the warning it printed: a dependency added to a shared crate changed the workspace hack, a watched crate's features or a lockfile; regenerate with `./dev locks sync` and `./dev locks guard --write` and commit them with the change".into(),
            )
        },
    )
}

fn commit_lint(change: &Change) -> Vec<Planned> {
    if change.ranges.is_empty() {
        return vec![Planned::Skip {
            id: "commit-lint".into(),
            reason: "no commit of its own since the base".into(),
        }];
    }
    change
        .ranges
        .iter()
        .map(|range| {
            let id = if change.ranges.len() == 1 {
                "commit-lint".to_string()
            } else {
                format!("commit-lint {range}")
            };
            Planned::Run(gate(
                id,
                "",
                "node",
                &["conformance/bin/paddock-conformance.mjs", "commit-lint", "--range", range],
                0,
                "reword the commit it names (a Conventional Commit subject of at most 100 characters; no trailers the rules forbid)".into(),
            ))
        })
        .collect()
}

/// A file that cannot change what Clippy says: documentation next to the code.
fn docs_only(path: &str) -> bool {
    [".md", ".txt", ".adoc"].iter().any(|e| path.ends_with(e))
}

/// What CI runs on a branch that the push gate does not, with the reason; printed with every run (ADR 0038 §3: the gate prints
/// the difference). Each line is a gap to close by a work package or a decision, not a promise that CI will not find it.
pub const NOT_RUN: &[(&str, &str)] = &[
    (
        "cargo hakari verify (workspace hack)",
        "needs the CI image's pinned cargo-hakari; WP-3 moves it into the contract. The `locks-guard` row runs `cargo hakari generate --diff` when a manifest changed (report-only)",
    ),
    (
        "cargo machete (unused dependencies)",
        "needs the CI image's pinned cargo-machete; WP-3",
    ),
    (
        "cargo deny outside Pitwall",
        "each workspace's deny.toml is checked by CI only; WP-3",
    ),
    (
        "lint and type-check of touched TypeScript (eslint, tsc)",
        "per-project commands live in YAML until WP-3's contracts",
    ),
    (
        "Linux Clippy and Linux tests, the guardian's acting tests as uid 65534",
        "CI-owned: the Linux jobs on the Linux runner; no VM or container starts on this Mac (user, 2026-11-26)",
    ),
    (
        "Clippy and the Windows lint of workspaces reached only through dependencies",
        "CI covers them (ER-9 overlap); only workspaces with a changed file of their own are linted here",
    ),
    (
        "tests (nextest, vitest, Playwright) in a developer's push, other than the member audits; Windows tests anywhere locally",
        "slower local checks run beside CI after dispatch (ER-9); the member gate (`--member-gate`) runs the nextest `ci` profile of every touched Cargo workspace on this host (`tests`) and the repository-wide audits (`member-audits`); Windows stays CI's. Doctests run nowhere (the `doctests` row)",
    ),
];
