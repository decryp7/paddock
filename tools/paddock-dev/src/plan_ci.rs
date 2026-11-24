//! The CI half of the planner: the matrix legs of the affected apps, the base of a run, the `--ci` outputs and summary, the
//! commit lint's range and the command line. The port of the rest of the retired `scripts/affected.mjs` (`matrix`, `onlyApp`, `wantsWebkit`,
//! `chooseBase`, `ciOutputs`, `summary`, `commitLintRange`, `parseArgs` and `ci`); the rules that decide which projects a change
//! reaches are in [`crate::plan`].

use crate::host_tests::{HostTests, and_filters};
use crate::plan::RepoGraph;
use paddock_repo_graph::join_normalized;
use serde_json::{Map, Value, json};

/// The runner that carries an app's UI half, prepare steps and checks (the others run only the Rust gates).
pub const LINUX: &str = "ubuntu-latest";

/// What `matrix` was asked.
#[derive(Default, Clone, Copy)]
pub struct MatrixOpts {
    /// A member run (a pull request, or a dispatch with a `base`, see [`member_run`]): narrow nextest to the reached crates of
    /// the app's workspace.
    pub narrow: bool,
    /// The run wants the WebKit tests.
    pub webkit: bool,
    /// The run tests every package on every OS, whatever `hostTests` says (the nightly, a tag's or an app's release gates: see
    /// [`full_os`]).
    pub full_os: bool,
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// The Cargo workspace folder of an app's CI contract (`dir` joined with `cargo`), empty for an app without Rust: the one reading of it,
/// for the app's package filter here and for the grading of a FLAKY test ([`crate::ci_runs::green::Touched`]).
pub fn app_cargo_dir(app: &Value) -> String {
    if truthy(app.get("cargo")) {
        join_normalized(&text(app, "dir"), &text(app, "cargo")).unwrap_or_default()
    } else {
        String::new()
    }
}

fn flag(b: bool) -> Value {
    json!(if b { "true" } else { "false" })
}

fn has_len(v: Option<&Value>) -> bool {
    v.and_then(Value::as_array).is_some_and(|a| !a.is_empty())
}

fn kinds_of(plan: &Value, app: &str) -> Vec<String> {
    plan["kinds"][app]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Whether a run is a member's or a fix round's (not the final head's): a manual dispatch with a `base` (a branch run: the
/// diff from the base is what it tests) and no `since_run`. A push to main, a tag, the nightly, a manual run with no base and an
/// app's own gates (`PLAN_APP`) are full runs and are never narrowed (ER-8: the full run on the final head catches what the
/// narrowed ones missed). `since_run` reruns narrow themselves by failed tests ([`crate::plan_since`]).
pub fn member_run(event: &str, input_base: &str, since_run: &str, plan_app: Option<&str>) -> bool {
    event == "workflow_dispatch" && !input_base.trim().is_empty() && since_run.trim().is_empty() && plan_app.is_none()
}

/// The nextest filterset (`package(=a) | package(=b)`) of the packages of one Cargo workspace (`cargo_dir`, owned by `project`:
/// `crates`, `paddock-dev` or `app:<name>`) that a change reaches, when `narrow`; empty (every test) when not narrowing, when the
/// plan says everything, when it names no package, when it names them all, or when the project has a reason beyond Cargo.
pub fn package_filter(plan: &Value, graph: &RepoGraph, cargo_dir: &str, project: &str, narrow: bool) -> String {
    let members = graph.cargo.iter().filter(|(_, n)| n.workspace == cargo_dir).count();
    let hit: Vec<String> = plan["packages"][cargo_dir]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let all = plan["all"].as_bool().unwrap_or(false);
    // Only when every reason the plan gave for the project is a Cargo reach (`cargoOnly`): a UI file, an installer file, a lockfile
    // or a script reaches tests that no package list names, so the project then runs whole. A plan without the list narrows nothing.
    let cargo_only = plan["cargoOnly"].as_array().is_some_and(|a| a.iter().any(|p| p == project));
    if narrow && !all && cargo_only && !hit.is_empty() && hit.len() < members {
        hit.iter().map(|n| format!("package(={n})")).collect::<Vec<_>>().join(" | ")
    } else {
        String::new()
    }
}

/// The package filters of the two workspaces that are not apps, as the plan's `filters` object (`crates`, `paddockDev`; empty: every
/// test): [`ci_outputs`] writes them as `crates_filter` and `paddock_dev_filter`, read by the host legs of the `crate` job and the
/// tests of the `paddock-dev` job. The Linux crate tests run whole under coverage in the `crates` job: a coverage figure of a
/// narrowed run would not be the workspace's.
pub fn package_filters(plan: &Value, graph: &RepoGraph, narrow: bool) -> Value {
    json!({
        "crates": package_filter(plan, graph, "crates", "crates", narrow),
        "paddockDev": package_filter(plan, graph, "tools/paddock-dev", "paddock-dev", narrow),
    })
}

/// Whether a run tests every package on every OS (no `hostTests` scope): the nightly (a `schedule` event, or the nightly's own
/// `os_full` input, which also covers its manual dispatch), a tag run, a release workflow's gates (`release`) and an app's own gates
/// (`PLAN_APP`). Every other run (a pull request, a member or fix-round dispatch, a push to main, a manual run) tests the OS-specific
/// code on each OS and the rest on Linux (the ADR 0080 amendment, docs/adr/amendments/0080-os-scoped-placement-2026-11-22.md on docs/adr-audit-2026-11-22: the nightly and the release gates are the full runs).
pub fn full_os(env: &dyn Fn(&str) -> Option<String>) -> bool {
    let set = |k: &str| env(k).is_some_and(|v| v == "true" || v == "1");
    set("PLAN_FULL_OS")
        || set("PLAN_RELEASE")
        || env("PLAN_APP").is_some_and(|a| !a.is_empty())
        || env("GITHUB_EVENT_NAME").as_deref() == Some("schedule")
        || env("GITHUB_REF").is_some_and(|r| r.starts_with("refs/tags/"))
}

/// The legs of one of the two jobs that are not apps (`crate`, `paddock-dev`): one object per runner of `os_legs` with the packages
/// (`test_packages`, space-separated, for `-p`; empty: the workspace's) and the nextest filterset (`test_filter`) its tests run. A
/// Windows or macOS leg tests only what the workspace's `hostTests` names (OS-scoped tests); every leg also keeps the member run's
/// package filter, so a member run on a host leg tests the reached packages that a host tests. A workspace with no `hostTests` has
/// empty packages and the member filter alone on every leg: every test everywhere, as before.
pub fn workspace_legs(os_legs: &[String], host: Option<&HostTests>, member_filter: &str, reached: Option<&[String]>) -> Vec<Value> {
    os_legs
        .iter()
        .map(|os| {
            let scope = host.map(|h| h.scope(os)).unwrap_or_default();
            // A member run builds only the host packages it reaches (`reached`: the packages the plan names, given when the run is
            // narrowed); a run that reaches none of them has nothing to test on this host, said by `test_skip` (the step ends with a
            // notice), never by an empty list (which would mean the whole workspace).
            let (packages, skip) = match (reached, scope.packages.is_empty()) {
                (Some(r), false) => {
                    let hit: Vec<String> = scope.packages.iter().filter(|p| r.contains(p)).cloned().collect();
                    let none = hit.is_empty();
                    (hit, none)
                }
                _ => (scope.packages.clone(), false),
            };
            json!({
                "os": os,
                "test_packages": packages.join(" "),
                "test_filter": and_filters(member_filter, &scope.filter),
                "test_skip": if skip { "true" } else { "" },
            })
        })
        .collect()
}

/// The matrix legs of the affected apps: one object per app and operating system (paddock.ci.json `os`, default Linux only)
/// with the app's contract, the halves to run and the nextest filter (only when `narrow`: a pull request whose change reaches
/// part of the app's Cargo workspace; '' runs it all). The Linux leg runs everything the change reaches; a Windows or macOS leg
/// runs only the Rust gates, and only when the change reaches the Rust half.
pub fn matrix(plan: &Value, graph: &RepoGraph, opts: MatrixOpts) -> Result<Vec<Value>, String> {
    let mut legs_out: Vec<Value> = Vec::new();
    let apps: Vec<&str> = plan["apps"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for name in apps {
        let app = graph.config(name);
        // An app with a workflow of its own is not a leg; an app with nothing to run (docs only) is no leg either.
        let runs = !truthy(app.get("workflow"))
            && (truthy(app.get("cargo")) || has_len(app.get("prepare")) || has_len(app.get("checks")) || has_len(app.get("ui")));
        if !runs {
            continue;
        }
        let mut oses = strings(app, "os");
        if oses.is_empty() {
            oses = vec![LINUX.to_string()];
        }
        if !oses.iter().any(|o| o == LINUX) {
            return Err(format!(
                "apps/{name}/paddock.ci.json: \"os\" must include {LINUX} (it carries the UI half and the checks)"
            ));
        }
        let dir = text(app, "dir");
        let cargo_dir = app_cargo_dir(app);
        let filter = if cargo_dir.is_empty() {
            String::new()
        } else {
            package_filter(plan, graph, &cargo_dir, &format!("app:{name}"), opts.narrow)
        };
        // hostTests (OS-scoped tests): what each Windows and macOS leg of this app tests; none declared, every test on every OS.
        let host = crate::host_tests::read(
            app,
            &format!("apps/{name}/paddock.ci.json"),
            &graph.derived(&format!("apps/{name}")),
        )?;
        let kinds = kinds_of(plan, name);
        let rust = kinds.iter().any(|k| k == "rust");
        let ui = kinds.iter().any(|k| k == "ui");
        let join = |key: &str, sep: &str| strings(app, key).join(sep);
        let mut legs: Vec<Value> = Vec::new();
        for os in oses.iter().filter(|os| *os == LINUX || (rust && truthy(app.get("cargo")))) {
            let linux = os == LINUX;
            let tb = app.get("testBrowser").cloned().unwrap_or(Value::Null);
            let mut leg = Map::new();
            leg.insert("app".into(), json!(name));
            leg.insert("os".into(), json!(os));
            leg.insert("dir".into(), json!(dir));
            leg.insert("cargo".into(), json!(cargo_dir));
            leg.insert("nextest_config".into(), json!(text(app, "nextestConfig")));
            leg.insert("prepare".into(), json!(if linux { join("prepare", "\n") } else { String::new() }));
            leg.insert("checks".into(), json!(if linux { join("checks", "\n") } else { String::new() }));
            leg.insert(
                "rust_checks".into(),
                json!(if linux { join("rustChecks", "\n") } else { String::new() }),
            );
            leg.insert(
                "host_rust_checks".into(),
                json!(if linux { String::new() } else { join("hostRustChecks", "\n") }),
            );
            leg.insert("ui_checks".into(), json!(if linux { join("ui", "\n") } else { String::new() }));
            leg.insert(
                "angular_dir".into(),
                json!(if linux && truthy(app.get("angularDir")) {
                    join_normalized(&dir, &text(app, "angularDir")).unwrap_or_default()
                } else {
                    String::new()
                }),
            );
            leg.insert("last_packages".into(), json!(join("lastPackages", " ")));
            leg.insert("windows_clippy".into(), flag(truthy(app.get("windowsClippy"))));
            leg.insert("notices".into(), flag(linux && truthy(app.get("notices")) && (rust || ui)));
            leg.insert("process_patterns".into(), json!(join("processPatterns", "\n")));
            leg.insert("test_browser_prefix".into(), json!(text(&tb, "envPrefix")));
            leg.insert("test_browser_catalogue".into(), json!(text(&tb, "catalogue")));
            leg.insert("test_browser_no_sandbox".into(), flag(linux && truthy(tb.get("noSandboxOnLinux"))));
            leg.insert("display".into(), flag(linux && truthy(app.get("display"))));
            leg.insert("rust".into(), json!(rust));
            leg.insert("ui".into(), json!(linux && ui));
            leg.insert("filter".into(), json!(filter));
            // The packages (`-p`) and the nextest filterset this host leg tests, empty on Linux and for an app with no `hostTests`.
            let scope = host.as_ref().filter(|_| !opts.full_os).map(|h| h.scope(os)).unwrap_or_default();
            leg.insert("host_packages".into(), json!(scope.packages.join(" ")));
            leg.insert("host_filter".into(), json!(scope.filter));
            leg.insert("e2e_checks".into(), json!(""));
            leg.insert("e2e_shard".into(), json!(""));
            leg.insert("e2e_leg".into(), json!(""));
            leg.insert(
                "e2e_artifacts".into(),
                json!(if linux && ui { text(app, "e2eArtifacts") } else { String::new() }),
            );
            legs.push(Value::Object(leg));
        }
        // `e2e` (commands) with `e2eShards` (default 1): the end-to-end tests are legs of their own on Linux, one per shard `i/N`
        // (the command reads $E2E_SHARD and $E2E_INDEX), running at the same time; the main leg keeps the other UI lines.
        let shards = if ui && has_len(app.get("e2e")) {
            app.get("e2eShards").and_then(Value::as_u64).unwrap_or(1).max(1)
        } else {
            0
        };
        let base = legs.iter().find(|l| l["os"] == LINUX).cloned().unwrap_or(Value::Null);
        let derived = |checks: String, shard: String, leg: String| -> Value {
            let mut l = base.clone();
            for (k, v) in [
                ("prepare", json!("")),
                ("checks", json!("")),
                ("ui_checks", json!("")),
                ("rust", json!(false)),
                ("ui", json!(false)),
                ("display", json!("false")),
                ("notices", json!("false")), // the main leg checks them once
                ("e2e_checks", json!(checks)),
                ("e2e_shard", json!(shard)),
                ("e2e_leg", json!(leg)),
            ] {
                l[k] = v;
            }
            l
        };
        for i in 1..=shards {
            legs.push(derived(join("e2e", "\n"), format!("{i}/{shards}"), format!("e2e-{i}")));
        }
        // `e2eWebkit` (commands): the WebKit end-to-end tests, a leg of their own when the run asks for WebKit.
        if ui && opts.webkit && has_len(app.get("e2eWebkit")) {
            legs.push(derived(join("e2eWebkit", "\n"), String::new(), "e2e-webkit".to_string()));
        }
        legs_out.extend(legs);
    }
    Ok(legs_out)
}

/// Whether this run wants the WebKit tests: a push to main or a tag, the nightly, or a manual run that asked for them
/// (`PLAN_WEBKIT`, from ci.yml's `webkit` input). Never a plain pull request.
pub fn wants_webkit(env: &dyn Fn(&str) -> Option<String>) -> bool {
    if env("PLAN_WEBKIT").is_some_and(|v| !v.is_empty()) {
        return true;
    }
    let reference = env("GITHUB_REF").unwrap_or_default();
    let event = env("GITHUB_EVENT_NAME").unwrap_or_default();
    event == "schedule" || (event != "pull_request" && (reference == "refs/heads/main" || reference.starts_with("refs/tags/")))
}

/// The plan of one app only (`--ci` with PLAN_APP, from ci.yml's `app` input): that app's legs with both halves and no other
/// project. A release run gates a tag with exactly the legs the app's own changes would run.
pub fn only_app(plan: &Value, app: &str) -> Value {
    let mut out = plan.clone();
    for k in [
        "all",
        "crates",
        "ui",
        "eslint",
        "conformance",
        "c99",
        "tools",
        "icons",
        "ciImage",
        "paddockDev",
        "docs",
    ] {
        out[k] = json!(false);
    }
    out["apps"] = json!([app]);
    out["kinds"] = json!({ app: ["rust", "ui"] });
    out["packages"] = json!({});
    out["cargoOnly"] = json!([]);
    out
}

// ------------------------------------------------------------------------------------------------------------------ base

/// The inputs of [`choose_base`].
pub struct BaseInput<'a> {
    /// `GITHUB_EVENT_NAME`.
    pub event: &'a str,
    /// `GITHUB_REF`.
    pub reference: &'a str,
    /// The `base` input of a manual run.
    pub input_base: &'a str,
    /// The `landing_base` input of a train's landing run (ADR 0117 WP-8): main's last complete commit, which the run's diff starts at.
    /// Unlike `input_base` it does not make the run a member run.
    pub landing_base: &'a str,
    /// `GITHUB_BASE_REF`.
    pub base_ref: &'a str,
    /// The base of a push to main from the one definition of green (`ci_runs::green::find_base`: the last first-parent commit whose
    /// outcome is complete and green, with or without fix items): the commit and the run that decided it, `Ok(None)` when none is
    /// found, `Err` when the runs could not be read. `None`: not asked (every other event).
    pub green: Option<&'a Result<Option<(String, u64)>, String>>,
    /// A pull request's merge base with a branch.
    pub merge_base: &'a dyn Fn(&str) -> Option<String>,
}

/// The base of a CI run (`None`: everything) and why.
pub fn choose_base(i: &BaseInput) -> (Option<String>, String) {
    let main = "refs/heads/main";
    if i.event == "pull_request" || i.event == "pull_request_target" {
        return match (i.merge_base)(i.base_ref) {
            Some(b) => (Some(b), format!("merge base with {}", i.base_ref)),
            None => (None, format!("no merge base with {}: everything", i.base_ref)),
        };
    }
    if i.event == "workflow_dispatch" {
        return if !i.input_base.is_empty() {
            (Some(i.input_base.to_string()), format!("the base input {}", i.input_base))
        } else if !i.landing_base.is_empty() {
            (
                Some(i.landing_base.to_string()),
                format!("a landing run: main's last complete green commit {}", i.landing_base),
            )
        } else {
            (None, "a manual run without a base: everything".to_string())
        };
    }
    if i.event == "push" && i.reference == main {
        return match i.green {
            None => (None, "the CI runs could not be read: everything".to_string()),
            Some(Err(why)) => (None, format!("the CI runs could not be read ({why}): everything")),
            Some(Ok(Some((sha, id)))) => (
                Some(sha.clone()),
                format!(
                    "the last complete green ci run on main ({}, run {id})",
                    sha.chars().take(10).collect::<String>()
                ),
            ),
            Some(Ok(None)) => (None, "no complete green ci run among main's newest commits: everything".to_string()),
        };
    }
    (None, format!("{} {}: everything", i.event, i.reference))
}

/// `/^ci\.ya?ml@/`.
pub fn is_ci_yml(path: &str) -> bool {
    path.strip_prefix("ci.y")
        .is_some_and(|r| r.strip_prefix("ml@").or_else(|| r.strip_prefix("aml@")).is_some())
}

// --------------------------------------------------------------------------------------------------------------- outputs

/// The ci.yml job outputs that say whether a project runs: (output name, project id, plan key). The one table: [`ci_outputs`] writes
/// them, and the repository model reads a job's `if:` through it to know which project the job runs for (ADR 0117 WP-2).
pub const PROJECT_OUTPUTS: [(&str, &str, &str); 8] = [
    ("crates", "crates", "crates"),
    ("ui", "ui", "ui"),
    ("eslint", "eslint", "eslint"),
    ("conformance", "conformance", "conformance"),
    ("c99", "c99", "c99"),
    ("tools", "tools", "tools"),
    ("icons", "icons", "icons"),
    ("paddock_dev", "paddock-dev", "paddockDev"),
];

/// The ci.yml job outputs of a plan (the plan job declares and wires exactly these names), as `(name, value)` lines.
pub fn ci_outputs(plan: &Value, legs: &[Value], base: Option<&str>) -> Vec<(String, String)> {
    let b = |k: &str| plan[k].as_bool().unwrap_or(false).to_string();
    let mut out: Vec<(String, String)> = PROJECT_OUTPUTS.iter().map(|(name, _, key)| (name.to_string(), b(key))).collect();
    out.extend(vec![
        // Whether the WebKit tests run (plan_ci::wants_webkit): the `e2e-webkit` job and the declared scope's engines read this one value.
        ("webkit".into(), plan["webkit"].as_bool().unwrap_or(false).to_string()),
        ("crates_filter".into(), plan["filters"]["crates"].as_str().unwrap_or("").to_string()),
        (
            "paddock_dev_filter".into(),
            plan["filters"]["paddockDev"].as_str().unwrap_or("").to_string(),
        ),
        ("apps".into(), serde_json::to_string(legs).unwrap_or_default()),
        ("any_app".into(), (!legs.is_empty()).to_string()),
        // The commit lint's range start (empty: no base, only the head commit is linted).
        ("base".into(), base.unwrap_or("").to_string()),
    ]);
    out
}

/// A short Markdown summary of a plan for the job summary.
pub fn summary(plan: &Value, reason: &str, legs: &[Value]) -> String {
    let yes = |b: bool| if b { "yes" } else { "no" };
    let mut lines: Vec<String> = vec![
        "### Affected".into(),
        String::new(),
        format!("Base: {reason}"),
        String::new(),
        "| Project | Runs |".into(),
        "|---|---|".into(),
    ];
    for p in ["crates", "ui", "eslint", "conformance", "c99", "tools", "paddockDev"] {
        lines.push(format!("| {p} | {} |", yes(plan[p].as_bool().unwrap_or(false))));
    }
    for l in legs {
        let os = l["os"].as_str().unwrap_or("");
        let leg = l["e2e_leg"].as_str().unwrap_or("");
        let shard = l["e2e_shard"].as_str().unwrap_or("");
        let runs = if !leg.is_empty() {
            format!(
                "{}{}",
                leg.replacen('-', " ", 1),
                if shard.is_empty() { String::new() } else { format!(" {shard}") }
            )
        } else {
            let halves: Vec<&str> = [
                ("rust", l["rust"].as_bool().unwrap_or(false)),
                ("ui", l["ui"].as_bool().unwrap_or(false)),
            ]
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(n, _)| n)
            .collect();
            if halves.is_empty() {
                "checks only".to_string()
            } else {
                halves.join(" + ")
            }
        };
        let filter = l["filter"].as_str().unwrap_or("");
        lines.push(format!(
            "| app {}{} | {runs}{} |",
            l["app"].as_str().unwrap_or(""),
            if os == LINUX { String::new() } else { format!(" ({os})") },
            if filter.is_empty() {
                String::new()
            } else {
                format!(" (`{filter}`)")
            }
        ));
    }
    if plan["docs"].as_bool().unwrap_or(false)
        && legs.is_empty()
        && !plan["crates"].as_bool().unwrap_or(false)
        && !plan["ui"].as_bool().unwrap_or(false)
    {
        lines.push(String::new());
        lines.push("Docs only: nothing to build.".into());
    }
    format!("{}\n", lines.join("\n"))
}

// ---------------------------------------------------------------------------------------------------------- commit lint

/// The inputs of [`commit_lint_range`].
#[derive(Default)]
pub struct LintInput<'a> {
    /// The event name.
    pub event: &'a str,
    /// The ref.
    pub reference: &'a str,
    /// The commit before a push.
    pub before: &'a str,
    /// The pushed commit; `None` (the variable is not set) means `HEAD`.
    pub sha: Option<&'a str>,
    /// The plan's base of a manual run.
    pub base: &'a str,
    /// The pull request's target branch.
    pub base_ref: &'a str,
    /// The `app` input of a tag's gates.
    pub app: &'a str,
}

/// What the commit lint is asked: `git log` options and one revision range, kept apart so no value is ever split into arguments.
#[derive(Debug, PartialEq)]
pub struct LintRange {
    /// Zero or one fixed option (`--first-parent`).
    pub flags: Vec<String>,
    /// The revision range.
    pub range: String,
}

fn lint_range(flags: &[&str], range: String) -> Option<LintRange> {
    Some(LintRange {
        flags: flags.iter().map(|f| f.to_string()).collect(),
        range,
    })
}

/// A commit id of the event: `HEAD` or 7 to 40 lower-case hex digits (it ends up as a git argument).
fn commit_id(value: &str, what: &str) -> Result<(), String> {
    let hex = (7..=40).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if value != "HEAD" && !hex {
        return Err(format!(
            "commit range: {what} \"{value}\" is not a commit id (7 to 40 lower-case hex digits)"
        ));
    }
    Ok(())
}

/// A branch name of the event: the same shape as `/^[A-Za-z0-9_][A-Za-z0-9._/-]*$/`.
fn branch_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-'))
}

/// The commits the commit lint checks, as flags and a git revision range, or `None` to skip it. (From branch
/// `ci/commit-lint-first-parent`, inside `debrief/1.2-batch1`; compatible with the script on main for every valid input.)
/// Only what a change adds is linted, so an old bad commit never keeps main red: a pull request lints since its target branch, a
/// push the commits it added (`before..sha`; a new branch, an all-zero `before`, lints the head commit only), a manual run from the
/// plan's base (the head commit alone without one). On main a push lints only the first-parent commits (`--first-parent`: what main
/// itself adds, the landing merge included; the commits a merge brought in from a branch were linted when the branch landed).
/// `known(commit)` says whether a commit is in the clone. Tags and a call with an `app` (an app's tag gates, ER-15) and every
/// other event skip it: a release must not fail on an old commit's subject. `before`, `sha` and `base` must be commit ids and
/// `base_ref` a branch name: they come from the event and end up as git arguments.
pub fn commit_lint_range(i: &LintInput, known: &dyn Fn(&str) -> bool) -> Result<Option<LintRange>, String> {
    if !i.app.is_empty() || i.reference.starts_with("refs/tags/") {
        return Ok(None);
    }
    let tip = i.sha.unwrap_or("HEAD");
    commit_id(tip, "sha")?;
    let head = || lint_range(&[], format!("{tip}^!"));
    match i.event {
        "pull_request" => {
            if i.base_ref.is_empty() {
                return Ok(None);
            }
            if !branch_name(i.base_ref) {
                return Err(format!("commit range: base ref \"{}\" is not a branch name", i.base_ref));
            }
            Ok(lint_range(&[], format!("origin/{}..HEAD", i.base_ref)))
        }
        // A `before` missing from the history (after a force push) lints the head commit only.
        "push" => {
            if i.before.is_empty() || i.before.chars().all(|c| c == '0') {
                return Ok(head());
            }
            commit_id(i.before, "before")?;
            if !known(i.before) {
                return Ok(head());
            }
            let flags: &[&str] = if i.reference == "refs/heads/main" {
                &["--first-parent"]
            } else {
                &[]
            };
            Ok(lint_range(flags, format!("{}..{tip}", i.before)))
        }
        "workflow_dispatch" => {
            if i.base.is_empty() {
                return Ok(head());
            }
            commit_id(i.base, "base")?;
            Ok(lint_range(&[], format!("{}..{tip}", i.base)))
        }
        _ => Ok(None),
    }
}

// ------------------------------------------------------------------------------------------------------ command line

/// The usage line printed under an argument error.
pub const USAGE: &str = "usage: dev plan <base> [<head>] | --files a,b | --all | --ci [--app <app>]  [--explain] [--compare] | --commit-range | --commit-flags | --since-run <run>";

/// A parsed command line of `dev plan`.
#[derive(Debug, PartialEq)]
pub struct PlanArgs {
    /// `--files a,b`.
    pub files: Option<Vec<String>>,
    /// The base revision.
    pub base: Option<String>,
    /// The head revision (default `HEAD`).
    pub head: String,
    /// `--all`.
    pub all: bool,
    /// `--ci`.
    pub ci: bool,
    /// `--app <app>` (with `--ci`).
    pub app: Option<String>,
    /// `--explain`.
    pub explain: bool,
    /// `--since-run <run>`: rerun only what failed in that CI run ([`crate::plan_since`]).
    pub since_run: Option<u64>,
    /// `--compare`: the repository model's answer beside the rule tables' (ADR 0117 §8; [`crate::repo_model::compare`]).
    pub compare: bool,
}

/// The command line: `--ci [--app x]`, `--files a,b`, `--all`, `--explain` and up to two positional commits, base then head.
/// Errors (the caller prints the usage and the reason) for anything incomplete, unknown or contradictory, so a typo never plans
/// nothing.
pub fn parse_args(args: &[String]) -> Result<PlanArgs, String> {
    let known = ["--files", "--all", "--ci", "--app", "--explain", "--since-run", "--compare"];
    let value_of = |name: &str| -> Result<(Option<usize>, Option<String>), String> {
        let Some(i) = args.iter().position(|a| a == name) else {
            return Ok((None, None));
        };
        match args.get(i + 1) {
            Some(v) if !v.is_empty() && !v.starts_with("--") => Ok((Some(i), Some(v.clone()))),
            _ => Err(format!("{name} needs a value")),
        }
    };
    for a in args {
        if a.starts_with("--") && !known.contains(&a.as_str()) {
            return Err(format!("unknown option {a}"));
        }
    }
    for k in known {
        if args.iter().filter(|a| *a == k).count() > 1 {
            return Err(format!("{k} given more than once"));
        }
    }
    let (fi, files_arg) = value_of("--files")?;
    let (ai, app) = value_of("--app")?;
    let (si, since) = value_of("--since-run")?;
    let ci = args.iter().any(|a| a == "--ci");
    let all = args.iter().any(|a| a == "--all");
    if app.is_some() && !ci {
        return Err("--app is only valid with --ci".into());
    }
    let value_idx: Vec<usize> = [fi, ai, si].into_iter().flatten().map(|i| i + 1).collect();
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !value_idx.contains(i))
        .map(|(_, a)| a)
        .collect();
    let base = positional.first().map(|s| s.to_string());
    let head = positional.get(1).map(|s| s.to_string()).unwrap_or_else(|| "HEAD".into());
    let since_run = match since {
        Some(v) => Some(
            v.parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("--since-run needs a run number, not {v:?}"))?,
        ),
        None => None,
    };
    if since_run.is_some() && (ci || files_arg.is_some() || all || !positional.is_empty()) {
        return Err("--since-run takes no commits, --files, --all or --ci (in CI, PLAN_SINCE_RUN with --ci)".into());
    }
    if ci && (files_arg.is_some() || all || !positional.is_empty()) {
        return Err("--ci takes no commits, --files or --all".into());
    }
    if positional.len() > 2 {
        return Err(format!(
            "too many commits: {}",
            positional.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
        ));
    }
    if files_arg.is_some() && (base.is_some() || all) {
        return Err("--files cannot be combined with a base or --all".into());
    }
    if all && base.is_some() {
        return Err("--all cannot be combined with a base".into());
    }
    let files = files_arg.map(|f| f.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>());
    if files.as_ref().is_some_and(|f| f.is_empty()) {
        return Err("--files needs at least one file".into());
    }
    Ok(PlanArgs {
        files,
        base,
        head,
        all,
        ci,
        app,
        explain: args.iter().any(|a| a == "--explain"),
        since_run,
        compare: args.iter().any(|a| a == "--compare"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_repo_graph::{Graph, Node};
    use std::collections::BTreeMap;

    fn graph_with(apps: Vec<(&str, Value)>, cargo: Graph) -> RepoGraph {
        RepoGraph {
            cargo,
            npm: Graph::new(),
            apps: apps.iter().map(|(n, _)| n.to_string()).collect(),
            configs: apps
                .into_iter()
                .map(|(n, mut v)| {
                    v["dir"] = json!(format!("apps/{n}"));
                    (n.to_string(), v)
                })
                .collect::<BTreeMap<_, _>>(),
            ..Default::default()
        }
    }

    fn plan(apps: &[&str], kinds: Value, packages: Value) -> Value {
        // Every project reached only through Cargo, as a plan of source changes in crates is.
        let cargo_only: Vec<String> = ["crates".to_string(), "paddock-dev".to_string()]
            .into_iter()
            .chain(apps.iter().map(|a| format!("app:{a}")))
            .collect();
        json!({"all": false, "apps": apps, "kinds": kinds, "packages": packages, "cargoOnly": cargo_only})
    }

    fn row(l: &Value, keys: &[&str]) -> Vec<Value> {
        keys.iter().map(|k| l[*k].clone()).collect()
    }

    #[test]
    fn only_a_dispatch_with_a_base_and_no_rerun_is_a_member_run() {
        // The member run (and a fix round that is not a failed-test rerun) narrows; every full run does not.
        assert!(member_run("workflow_dispatch", "origin/main", "", None));
        // The final head's runs and the gates of a tag stay whole: a push to main, a tag, the nightly, a manual run with no base,
        // an app's own gates, and a since_run rerun (which narrows by failed tests itself).
        for (event, base, since, app) in [
            ("push", "origin/main", "", None),
            ("schedule", "", "", None),
            ("workflow_dispatch", "", "", None),
            ("workflow_dispatch", "  ", "", None),
            ("workflow_dispatch", "origin/main", "8001", None),
            ("workflow_dispatch", "origin/main", "", Some("dyno")),
        ] {
            assert!(!member_run(event, base, since, app), "{event} {base:?} {since:?} {app:?}");
        }
    }

    #[test]
    fn the_package_filter_names_the_reached_packages_of_one_workspace_and_never_all_or_none() {
        let mut cargo = Graph::new();
        for n in ["a", "b", "c"] {
            cargo.insert(
                &format!("crates/{n}"),
                Node {
                    name: format!("p-{n}"),
                    workspace: "crates".into(),
                    deps: vec![],
                },
            );
        }
        let g = graph_with(vec![], cargo);
        let some = plan(&[], json!({}), json!({"crates": ["p-a", "p-c"]}));
        assert_eq!(package_filter(&some, &g, "crates", "crates", true), "package(=p-a) | package(=p-c)");
        // Positive control: the same plan not narrowing, a plan that says everything, none reached, all reached, another workspace.
        assert_eq!(package_filter(&some, &g, "crates", "crates", false), "");
        // A reason beyond Cargo for the project (an installer file, a UI file, a lockfile): it runs whole. The positive control is the
        // same plan with the project in `cargoOnly`, which narrows.
        let mut mixed = some.clone();
        mixed["cargoOnly"] = json!(["paddock-dev"]);
        assert_eq!(package_filter(&mixed, &g, "crates", "crates", true), "");
        let mut bare = some.clone();
        bare.as_object_mut().unwrap().remove("cargoOnly");
        assert_eq!(package_filter(&bare, &g, "crates", "crates", true), "");
        let mut everything = some.clone();
        everything["all"] = json!(true);
        assert_eq!(package_filter(&everything, &g, "crates", "crates", true), "");
        assert_eq!(package_filter(&plan(&[], json!({}), json!({})), &g, "crates", "crates", true), "");
        assert_eq!(
            package_filter(
                &plan(&[], json!({}), json!({"crates": ["p-a", "p-b", "p-c"]})),
                &g,
                "crates",
                "crates",
                true
            ),
            ""
        );
        assert_eq!(package_filter(&some, &g, "tools/paddock-dev", "paddock-dev", true), "");
        let mut with = some.clone();
        with["filters"] = package_filters(&some, &g, true);
        let out: std::collections::BTreeMap<String, String> = ci_outputs(&with, &[], None).into_iter().collect();
        assert_eq!(out["crates_filter"], "package(=p-a) | package(=p-c)");
        assert_eq!(out["paddock_dev_filter"], "");
        // Without a `filters` object (a rerun, an app's gates) both outputs are still written, empty.
        let bare: std::collections::BTreeMap<String, String> = ci_outputs(&some, &[], None).into_iter().collect();
        assert_eq!((bare["crates_filter"].as_str(), bare["paddock_dev_filter"].as_str()), ("", ""));
    }

    #[test]
    fn one_leg_per_affected_app_with_its_contract_and_a_pull_request_narrows_nextest() {
        let mut cargo = Graph::new();
        for n in ["a", "b", "c"] {
            cargo.insert(
                &format!("apps/x/src-tauri/{n}"),
                Node {
                    name: format!("x-{n}"),
                    workspace: "apps/x/src-tauri".into(),
                    deps: vec![],
                },
            );
        }
        let g = graph_with(
            vec![(
                "x",
                json!({"cargo": "src-tauri", "nextestConfig": "n.toml", "prepare": ["p1", "p2"], "checks": ["c1"], "lastPackages": ["l1", "l2"], "windowsClippy": true, "notices": {}}),
            )],
            cargo,
        );
        let p = plan(&["x"], json!({"x": ["rust"]}), json!({"apps/x/src-tauri": ["x-a"]}));
        let legs = matrix(&p, &g, MatrixOpts::default()).unwrap();
        assert_eq!(legs.len(), 1);
        assert_eq!(
            row(
                &legs[0],
                &[
                    "app",
                    "cargo",
                    "nextest_config",
                    "prepare",
                    "checks",
                    "last_packages",
                    "windows_clippy",
                    "notices",
                    "rust",
                    "ui",
                    "filter"
                ]
            ),
            vec![
                json!("x"),
                json!("apps/x/src-tauri"),
                json!("n.toml"),
                json!("p1\np2"),
                json!("c1"),
                json!("l1 l2"),
                json!("true"),
                json!("true"),
                json!(true),
                json!(false),
                json!("")
            ]
        );
        let narrowed = matrix(
            &p,
            &g,
            MatrixOpts {
                narrow: true,
                webkit: false,
                ..MatrixOpts::default()
            },
        )
        .unwrap();
        assert_eq!(narrowed[0]["filter"], "package(=x-a)");
        let all = json!({"all": true, "apps": ["x"], "kinds": {"x": ["rust", "ui"]}, "packages": {"apps/x/src-tauri": ["x-a"]}});
        assert_eq!(
            matrix(
                &all,
                &g,
                MatrixOpts {
                    narrow: true,
                    webkit: false,
                    ..MatrixOpts::default()
                }
            )
            .unwrap()[0]["filter"],
            ""
        );
        let whole = plan(&["x"], json!({"x": ["rust"]}), json!({"apps/x/src-tauri": ["x-a", "x-b", "x-c"]}));
        assert_eq!(
            matrix(
                &whole,
                &g,
                MatrixOpts {
                    narrow: true,
                    webkit: false,
                    ..MatrixOpts::default()
                }
            )
            .unwrap()[0]["filter"],
            ""
        );
    }

    #[test]
    fn an_os_list_adds_host_legs_with_the_rust_gates_only_and_must_include_linux() {
        let g = graph_with(
            vec![(
                "x",
                json!({"cargo": ".", "os": [LINUX, "windows-latest", "macos-latest"], "prepare": ["p"], "ui": ["ng build"], "angularDir": "app", "display": true, "testBrowser": {"envPrefix": "DEMO", "catalogue": "c.toml", "noSandboxOnLinux": true}}),
            )],
            Graph::new(),
        );
        let rust = matrix(&plan(&["x"], json!({"x": ["rust"]}), json!({})), &g, MatrixOpts::default()).unwrap();
        assert_eq!(
            rust.iter()
                .map(|l| row(
                    l,
                    &[
                        "os",
                        "rust",
                        "ui",
                        "test_browser_prefix",
                        "test_browser_no_sandbox",
                        "display",
                        "prepare",
                        "ui_checks",
                        "angular_dir"
                    ]
                ))
                .collect::<Vec<_>>(),
            vec![
                vec![
                    json!(LINUX),
                    json!(true),
                    json!(false),
                    json!("DEMO"),
                    json!("true"),
                    json!("true"),
                    json!("p"),
                    json!("ng build"),
                    json!("apps/x/app")
                ],
                vec![
                    json!("windows-latest"),
                    json!(true),
                    json!(false),
                    json!("DEMO"),
                    json!("false"),
                    json!("false"),
                    json!(""),
                    json!(""),
                    json!("")
                ],
                vec![
                    json!("macos-latest"),
                    json!(true),
                    json!(false),
                    json!("DEMO"),
                    json!("false"),
                    json!("false"),
                    json!(""),
                    json!(""),
                    json!("")
                ],
            ]
        );
        let ui = matrix(&plan(&["x"], json!({"x": ["ui"]}), json!({})), &g, MatrixOpts::default()).unwrap();
        assert_eq!(ui.len(), 1, "a UI-only change adds no host leg");
        let bad = graph_with(vec![("x", json!({"cargo": ".", "os": ["macos-latest"]}))], Graph::new());
        let err = matrix(&plan(&["x"], json!({"x": ["rust"]}), json!({})), &bad, MatrixOpts::default()).unwrap_err();
        assert!(err.contains("must include ubuntu-latest"), "{err}");
    }

    #[test]
    fn host_legs_of_an_app_with_host_tests_name_its_packages_and_linux_and_an_app_without_them_name_none() {
        let os = json!([LINUX, "windows-latest", "macos-latest"]);
        let mut with = graph_with(
            vec![(
                "x",
                json!({"cargo": ".", "os": os, "hostTests": {
                    "skip": [{"name": "x-a", "modules": ["pure::"], "reason": "pure logic only"}],
                    "only": [{"os": "macos-latest", "name": "x-m", "reason": "a tool of the dev Mac"}]
                }}),
            )],
            Graph::new(),
        );
        // The placement the code gives (two packages with OS marks); the contract holds only the exceptions.
        with.host.insert(
            "apps/x".into(),
            crate::host_tests::Derived {
                packages: ["x-b".to_string(), "x-a".to_string()].into(),
                known_packages: ["x-a".to_string(), "x-b".to_string(), "x-m".to_string(), "x-l".to_string()].into(),
                ..Default::default()
            },
        );
        let legs = matrix(&plan(&["x"], json!({"x": ["rust"]}), json!({})), &with, MatrixOpts::default()).unwrap();
        let rows: Vec<Vec<Value>> = legs.iter().map(|l| row(l, &["os", "host_packages", "host_filter"])).collect();
        assert_eq!(
            rows,
            vec![
                vec![json!(LINUX), json!(""), json!("")],
                vec![
                    json!("windows-latest"),
                    json!("x-a x-b"),
                    json!("not ((package(=x-a) & test(/^(?:pure::)/)))")
                ],
                vec![
                    json!("macos-latest"),
                    json!("x-a x-b x-m"),
                    json!("not ((package(=x-a) & test(/^(?:pure::)/)))")
                ],
            ],
            "Linux tests everything; a host leg tests the derived packages with the exceptions applied (and its own additions)"
        );
        // Positive control: with nothing derived the same exceptions name no host package, and nothing but the macOS-only one is left.
        let mut none = graph_with(
            vec![(
                "x",
                json!({"cargo": ".", "os": os, "hostTests": {"only": [{"os": "macos-latest", "name": "x-m", "reason": "a tool of the dev Mac"}]}}),
            )],
            Graph::new(),
        );
        none.host.insert("apps/x".into(), crate::host_tests::Derived::default());
        let legs = matrix(&plan(&["x"], json!({"x": ["rust"]}), json!({})), &none, MatrixOpts::default()).unwrap();
        let rows: Vec<Vec<Value>> = legs.iter().map(|l| row(l, &["os", "host_packages"])).collect();
        assert_eq!(
            rows[1],
            vec![json!("windows-latest"), json!("")],
            "no derived package: nothing is scoped on Windows"
        );
        assert_eq!(rows[2], vec![json!("macos-latest"), json!("x-m")]);
        // Positive control: the same app without the field has no scope on any leg.
        let without = graph_with(vec![("x", json!({"cargo": ".", "os": os}))], Graph::new());
        let legs = matrix(&plan(&["x"], json!({"x": ["rust"]}), json!({})), &without, MatrixOpts::default()).unwrap();
        assert!(legs.iter().all(|l| l["host_packages"] == "" && l["host_filter"] == ""));
        // A malformed field fails the plan, never turns the scope off.
        let bad = graph_with(
            vec![("x", json!({"cargo": ".", "os": os, "hostTests": {"ad": ["a"]}}))],
            Graph::new(),
        );
        let err = matrix(&plan(&["x"], json!({"x": ["rust"]}), json!({})), &bad, MatrixOpts::default()).unwrap_err();
        assert!(
            err.contains("apps/x/paddock.ci.json") && err.contains("unknown `hostTests.ad`"),
            "{err}"
        );
    }

    #[test]
    fn a_member_run_builds_only_the_host_packages_it_reaches_and_says_so_when_it_reaches_none() {
        let os: Vec<String> = [LINUX, "windows-latest"].map(String::from).to_vec();
        let host = HostTests::parse(&json!({"hostTests": {"packages": ["h-a", "h-b"]}}), "t.json")
            .unwrap()
            .unwrap();
        let reached = |r: &[&str]| -> Vec<String> { r.iter().map(|s| s.to_string()).collect() };
        let rows = |r: &[&str]| -> Vec<(String, String)> {
            workspace_legs(&os, Some(&host), "package(=h-a) | package(=l-x)", Some(&reached(r)))
                .iter()
                .map(|l| {
                    (
                        l["test_packages"].as_str().unwrap().to_string(),
                        l["test_skip"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        };
        // Linux is never scoped: it keeps no package list and never skips.
        assert_eq!(rows(&["h-a", "l-x"]), vec![("".into(), "".into()), ("h-a".into(), "".into())]);
        // The run reaches only a Linux-only package: the Windows leg has nothing to test (a skip, not an empty list that means everything).
        assert_eq!(rows(&["l-x"]), vec![("".into(), "".into()), ("".into(), "true".into())]);
        // Positive control: not narrowed (None) keeps every host package and never skips.
        let whole = workspace_legs(&os, Some(&host), "", None);
        assert_eq!(
            (whole[1]["test_packages"].as_str(), whole[1]["test_skip"].as_str()),
            (Some("h-a h-b"), Some(""))
        );
    }

    #[test]
    fn the_crate_and_paddock_dev_legs_carry_the_host_scope_and_a_member_runs_package_filter_together() {
        let os: Vec<String> = [LINUX, "windows-latest", "macos-latest"].map(String::from).to_vec();
        let host = HostTests::parse(
            &json!({"hostTests": {"modules": ["a::"], "only": {"macos-latest": ["b::"]}}}),
            "t.json",
        )
        .unwrap()
        .unwrap();
        let legs = workspace_legs(&os, Some(&host), "package(=paddock-dev)", None);
        let rows: Vec<(String, String, String)> = legs
            .iter()
            .map(|l| (l["os"].to_string(), l["test_packages"].to_string(), l["test_filter"].to_string()))
            .collect();
        assert_eq!(
            rows,
            vec![
                (r#""ubuntu-latest""#.into(), r#""""#.into(), r#""package(=paddock-dev)""#.into()),
                (
                    r#""windows-latest""#.into(),
                    r#""""#.into(),
                    r#""(package(=paddock-dev)) & (test(/^(?:a::)/))""#.into()
                ),
                (
                    r#""macos-latest""#.into(),
                    r#""""#.into(),
                    r#""(package(=paddock-dev)) & (test(/^(?:a::|b::)/))""#.into()
                ),
            ]
        );
        // Positive control: no `hostTests` and no member filter is the old behaviour, every test on every OS.
        let plain = workspace_legs(&os, None, "", None);
        assert!(plain.iter().all(|l| l["test_packages"] == "" && l["test_filter"] == ""));
        assert_eq!(plain.len(), 3);
    }

    #[test]
    fn the_notices_check_runs_on_exactly_one_linux_leg_of_an_app_that_has_the_block() {
        // Cases of the retired affected.test.mjs: the block, the leg that runs Rust or UI on Linux, never a host leg, an e2e shard or the WebKit leg.
        let with = json!({"cargo": ".", "os": [LINUX, "windows-latest"], "ui": ["npm test"], "e2e": ["pw"], "e2eShards": 2,
            "e2eWebkit": ["pw --project=webkit"], "notices": {"name": "X", "manifests": []}});
        let mut without = with.clone();
        without.as_object_mut().unwrap().remove("notices");
        let p = plan(&["x"], json!({"x": ["rust", "ui"]}), json!({}));
        let opts = MatrixOpts {
            narrow: false,
            webkit: true,
            ..MatrixOpts::default()
        };
        let legs = matrix(&p, &graph_with(vec![("x", with)], Graph::new()), opts).unwrap();
        assert!(legs.len() > 3, "the main leg, a host leg, the shards and WebKit: {}", legs.len());
        let checked: Vec<&Value> = legs.iter().filter(|l| l["notices"] == "true").collect();
        assert_eq!(checked.len(), 1, "exactly one leg checks the notices");
        assert!(checked[0]["os"] == LINUX && checked[0]["e2e_leg"] == "", "the Linux main leg");
        let none = matrix(&p, &graph_with(vec![("x", without)], Graph::new()), opts).unwrap();
        assert!(none.iter().all(|l| l["notices"] == "false"), "no block, no check");
    }

    #[test]
    fn e2e_shards_and_the_webkit_leg_are_legs_of_their_own() {
        let g = graph_with(
            vec![(
                "x",
                json!({"cargo": ".", "ui": ["npm test"], "e2e": ["pw"], "e2eShards": 2, "e2eWebkit": ["pw --project=webkit"], "e2eArtifacts": "res"}),
            )],
            Graph::new(),
        );
        let p = plan(&["x"], json!({"x": ["ui"]}), json!({}));
        let legs = matrix(
            &p,
            &g,
            MatrixOpts {
                narrow: false,
                webkit: true,
                ..MatrixOpts::default()
            },
        )
        .unwrap();
        assert_eq!(
            legs.iter()
                .map(|l| row(l, &["e2e_leg", "e2e_shard", "e2e_checks", "ui_checks", "e2e_artifacts"]))
                .collect::<Vec<_>>(),
            vec![
                vec![json!(""), json!(""), json!(""), json!("npm test"), json!("res")],
                vec![json!("e2e-1"), json!("1/2"), json!("pw"), json!(""), json!("res")],
                vec![json!("e2e-2"), json!("2/2"), json!("pw"), json!(""), json!("res")],
                vec![
                    json!("e2e-webkit"),
                    json!(""),
                    json!("pw --project=webkit"),
                    json!(""),
                    json!("res")
                ],
            ]
        );
        assert_eq!(
            matrix(&p, &g, MatrixOpts::default()).unwrap().len(),
            3,
            "no WebKit leg unless asked"
        );
        let rust_only = plan(&["x"], json!({"x": ["rust"]}), json!({}));
        assert_eq!(
            matrix(
                &rust_only,
                &g,
                MatrixOpts {
                    narrow: false,
                    webkit: true,
                    ..MatrixOpts::default()
                }
            )
            .unwrap()
            .len(),
            1
        );
        let one = graph_with(vec![("x", json!({"cargo": ".", "e2e": ["pw"]}))], Graph::new());
        let shards: Vec<Value> = matrix(&p, &one, MatrixOpts::default())
            .unwrap()
            .iter()
            .map(|l| l["e2e_shard"].clone())
            .collect();
        assert_eq!(shards, vec![json!(""), json!("1/1")]);
    }

    #[test]
    fn apps_with_a_workflow_or_nothing_to_run_are_no_legs() {
        let g = graph_with(
            vec![
                ("w", json!({"cargo": ".", "workflow": "w.yml"})),
                ("docs", json!({})),
                ("c", json!({"checks": ["x"]})),
            ],
            Graph::new(),
        );
        let legs = matrix(&plan(&["c", "docs", "w"], json!({}), json!({})), &g, MatrixOpts::default()).unwrap();
        assert_eq!(legs.iter().map(|l| l["app"].clone()).collect::<Vec<_>>(), vec![json!("c")]);
    }

    #[test]
    fn webkit_is_wanted_on_main_tags_the_nightly_and_a_manual_ask_never_on_a_plain_pull_request() {
        let env =
            |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert!(!wants_webkit(&env(&[
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_REF", "refs/pull/1/merge")
        ])));
        assert!(!wants_webkit(&env(&[
            ("GITHUB_EVENT_NAME", "workflow_dispatch"),
            ("GITHUB_REF", "refs/heads/topic")
        ])));
        assert!(wants_webkit(&env(&[
            ("GITHUB_EVENT_NAME", "workflow_dispatch"),
            ("GITHUB_REF", "refs/heads/topic"),
            ("PLAN_WEBKIT", "1")
        ])));
        assert!(wants_webkit(&env(&[
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_REF", "refs/heads/main")
        ])));
        assert!(wants_webkit(&env(&[
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_REF", "refs/tags/spotter-v1.0.0")
        ])));
        assert!(wants_webkit(&env(&[
            ("GITHUB_EVENT_NAME", "schedule"),
            ("GITHUB_REF", "refs/heads/main")
        ])));
        assert!(!wants_webkit(&env(&[
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_REF", "refs/heads/topic")
        ])));
    }

    #[test]
    fn only_app_keeps_one_app_both_halves_and_no_other_project() {
        let full = json!({"all": true, "crates": true, "ui": true, "eslint": true, "conformance": true, "c99": true, "tools": true, "icons": true, "ciImage": true, "paddockDev": true, "docs": true, "apps": ["a", "b"], "kinds": {"a": ["rust"]}, "packages": {"p": ["x"]}});
        let only = only_app(&full, "b");
        assert_eq!(only["apps"], json!(["b"]));
        assert_eq!(only["kinds"], json!({"b": ["rust", "ui"]}));
        for k in [
            "all",
            "crates",
            "ui",
            "eslint",
            "conformance",
            "c99",
            "tools",
            "icons",
            "ciImage",
            "paddockDev",
            "docs",
        ] {
            assert_eq!(only[k], false, "{k}");
        }
        assert_eq!(only["packages"], json!({}));
    }

    fn base_of(
        event: &str,
        reference: &str,
        input_base: &str,
        base_ref: &str,
        green: Option<&Result<Option<(String, u64)>, String>>,
    ) -> (Option<String>, String) {
        choose_base(&BaseInput {
            event,
            reference,
            input_base,
            landing_base: "",
            base_ref,
            green,
            merge_base: &|b| (b == "main").then(|| "mb123".to_string()),
        })
    }

    #[test]
    fn a_landing_base_makes_a_dispatch_a_diff_run_and_only_when_no_member_base_is_given() {
        let with = |input_base: &str, landing_base: &str, event: &str| {
            choose_base(&BaseInput {
                event,
                reference: "refs/heads/train/x",
                input_base,
                landing_base,
                base_ref: "",
                green: None,
                merge_base: &|_| None,
            })
        };
        let (b, why) = with("", "land1", "workflow_dispatch");
        assert_eq!(b.as_deref(), Some("land1"));
        assert!(why.contains("landing"), "{why}");
        // The member base wins (a member run is not a landing run), and with neither the run is everything (fail closed).
        assert_eq!(with("mem1", "land1", "workflow_dispatch").0.as_deref(), Some("mem1"));
        assert_eq!(with("", "", "workflow_dispatch").0, None);
        // Positive control: another event never reads it (a push to a branch, a schedule).
        assert_eq!(with("", "land1", "schedule").0, None);
    }

    #[test]
    fn the_base_of_a_run_follows_its_event() {
        assert_eq!(base_of("pull_request", "", "", "main", None).0.as_deref(), Some("mb123"));
        assert_eq!(base_of("pull_request", "", "", "gone", None).0, None);
        // A push to main takes the base the one definition of green found (ci_runs::green::find_base, tested there).
        let found = Ok(Some(("green1".to_string(), 7)));
        let (b, why) = base_of("push", "refs/heads/main", "", "", Some(&found));
        assert_eq!(b.as_deref(), Some("green1"));
        assert!(why.contains("run 7"), "{why}");
        assert_eq!(base_of("push", "refs/heads/main", "", "", Some(&Ok(None))).0, None);
        let unread = Err("HTTP 500".to_string());
        let (b, why) = base_of("push", "refs/heads/main", "", "", Some(&unread));
        assert!(b.is_none() && why.contains("HTTP 500"), "{why}");
        assert_eq!(base_of("push", "refs/heads/main", "", "", None).0, None);
        assert_eq!(base_of("workflow_dispatch", "", "abc", "", None).0.as_deref(), Some("abc"));
        assert_eq!(base_of("workflow_dispatch", "", "", "", None).0, None);
        assert_eq!(base_of("push", "refs/tags/v0.4.0", "", "", None).0, None);
        assert_eq!(base_of("schedule", "refs/heads/main", "", "", None).0, None);
        assert!(is_ci_yml("ci.yml@refs/heads/main") && is_ci_yml("ci.yaml@x") && !is_ci_yml("canary.yml@x"));
    }

    fn lint(i: LintInput, known: bool) -> Result<Option<(Vec<String>, String)>, String> {
        commit_lint_range(&i, &move |_| known).map(|r| r.map(|r| (r.flags, r.range)))
    }

    fn event<'a>(event: &'a str, reference: &'a str, before: &'a str, sha: &'a str) -> LintInput<'a> {
        LintInput {
            event,
            reference,
            before,
            sha: Some(sha),
            ..LintInput::default()
        }
    }

    fn just(range: &str) -> Result<Option<(Vec<String>, String)>, String> {
        Ok(Some((vec![], range.to_string())))
    }

    #[test]
    fn the_commit_lint_range_covers_only_what_a_change_adds_and_skips_tags_and_app_gates() {
        let zero = "0".repeat(40);
        let (a, b, c) = ("aaaaaaa", "bbbbbbb", "ccccccc");
        let pr = LintInput {
            event: "pull_request",
            base_ref: "main",
            ..LintInput::default()
        };
        assert_eq!(lint(pr, true), just("origin/main..HEAD"));
        let no_target = LintInput {
            event: "pull_request",
            ..LintInput::default()
        };
        assert_eq!(lint(no_target, true), Ok(None));
        assert_eq!(
            lint(event("push", "refs/heads/main", a, b), true),
            Ok(Some((vec!["--first-parent".to_string()], format!("{a}..{b}")))),
            "main: what main itself adds"
        );
        assert_eq!(
            lint(event("push", "refs/heads/x", a, b), true),
            just(&format!("{a}..{b}")),
            "another branch: every commit it adds"
        );
        assert_eq!(
            lint(event("push", "refs/heads/x", &zero, b), true),
            just(&format!("{b}^!")),
            "a new branch: the head commit"
        );
        assert_eq!(lint(event("push", "refs/heads/x", "", b), true), just(&format!("{b}^!")));
        assert_eq!(
            lint(event("push", "refs/heads/main", "abcdef0", b), false),
            just(&format!("{b}^!")),
            "a force push: before is not in the clone"
        );
        let manual = LintInput {
            event: "workflow_dispatch",
            base: c,
            sha: Some(b),
            ..LintInput::default()
        };
        assert_eq!(lint(manual, true), just(&format!("{c}..{b}")));
        let manual_no_base = LintInput {
            event: "workflow_dispatch",
            sha: Some(b),
            ..LintInput::default()
        };
        assert_eq!(lint(manual_no_base, true), just(&format!("{b}^!")));
        assert_eq!(lint(event("push", "refs/tags/v1.0.0", &zero, b), true), Ok(None), "a tag");
        assert_eq!(lint(event("push", "refs/tags/dyno-v1.0.0", a, b), true), Ok(None));
        let gates = LintInput {
            app: "dyno",
            ..event("push", "refs/heads/main", a, b)
        };
        assert_eq!(lint(gates, true), Ok(None), "an app tag call");
        assert_eq!(lint(event("schedule", "", "", b), true), Ok(None));
        assert_eq!(
            lint(
                LintInput {
                    event: "push",
                    reference: "refs/heads/main",
                    before: a,
                    sha: None,
                    ..LintInput::default()
                },
                true
            ),
            Ok(Some((vec!["--first-parent".to_string()], format!("{a}..HEAD")))),
            "an unset SHA is HEAD"
        );
    }

    #[test]
    fn values_that_are_not_commits_or_branch_names_are_refused_because_they_become_git_arguments() {
        let b = "bbbbbbb";
        for bad in ["HEAD --output=/tmp/x", "--output=/tmp/x", "abc", "zzzzzzz", "a..b"] {
            let manual = LintInput {
                event: "workflow_dispatch",
                base: bad,
                sha: Some(b),
                ..LintInput::default()
            };
            assert!(lint(manual, true).unwrap_err().contains("is not a commit id"), "{bad}");
            assert!(
                lint(event("push", "refs/heads/main", bad, b), true)
                    .unwrap_err()
                    .contains("is not a commit id"),
                "{bad}"
            );
            assert!(
                lint(event("push", "refs/heads/main", b, bad), true)
                    .unwrap_err()
                    .contains("is not a commit id"),
                "{bad}"
            );
        }
        for bad in ["--output=/tmp/x", "a b"] {
            let pr = LintInput {
                event: "pull_request",
                base_ref: bad,
                ..LintInput::default()
            };
            assert!(lint(pr, true).unwrap_err().contains("is not a branch name"), "{bad}");
        }
        let full = "a".repeat(40);
        let manual = LintInput {
            event: "workflow_dispatch",
            base: &full,
            sha: Some("HEAD"),
            ..LintInput::default()
        };
        assert_eq!(lint(manual, true), just(&format!("{full}..HEAD")));
    }

    #[test]
    fn the_job_outputs_and_summary_name_every_leg() {
        let plan = json!({"crates": true, "ui": false, "eslint": false, "conformance": true, "tools": false, "icons": false, "paddockDev": true, "docs": false});
        let legs = vec![
            json!({"app": "x", "os": LINUX, "rust": true, "ui": true, "filter": "package(=a)", "e2e_leg": "", "e2e_shard": ""}),
            json!({"app": "x", "os": "macos-latest", "rust": true, "ui": false, "filter": "", "e2e_leg": "", "e2e_shard": ""}),
            json!({"app": "y", "os": LINUX, "rust": false, "ui": false, "filter": "", "e2e_leg": "e2e-webkit", "e2e_shard": ""}),
            json!({"app": "y", "os": LINUX, "rust": false, "ui": false, "filter": "", "e2e_leg": "e2e-1", "e2e_shard": "1/2"}),
            json!({"app": "z", "os": LINUX, "rust": false, "ui": false, "filter": "", "e2e_leg": "", "e2e_shard": ""}),
        ];
        let out = ci_outputs(&plan, &legs, Some("abc"));
        let get = |k: &str| out.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(
            (
                get("crates").as_str(),
                get("ui").as_str(),
                get("paddock_dev").as_str(),
                get("any_app").as_str(),
                get("base").as_str()
            ),
            ("true", "false", "true", "true", "abc")
        );
        assert!(get("apps").starts_with(r#"[{"app":"x","os":"ubuntu-latest""#));
        assert_eq!(
            ci_outputs(&json!({}), &[], None).iter().find(|(n, _)| n == "any_app").unwrap().1,
            "false"
        );
        let md = summary(&plan, "the reason", &legs);
        assert!(
            md.starts_with("### Affected\n\nBase: the reason\n\n| Project | Runs |\n|---|---|\n| crates | yes |\n| ui | no |"),
            "{md}"
        );
        for needle in [
            "| app x | rust + ui (`package(=a)`) |",
            "| app x (macos-latest) | rust |",
            "| app y | e2e webkit |",
            "| app y | e2e 1 1/2 |",
            "| app z | checks only |",
        ] {
            assert!(md.contains(needle), "{needle}\n{md}");
        }
        let docs = summary(&json!({"docs": true}), "r", &[]);
        assert!(docs.ends_with("\nDocs only: nothing to build.\n"), "{docs}");
    }

    fn args(a: &[&str]) -> Result<PlanArgs, String> {
        parse_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn every_documented_command_line_parses_and_the_flag_values_are_not_positional() {
        let a = args(&["origin/main"]).unwrap();
        assert_eq!((a.base.as_deref(), a.head.as_str()), (Some("origin/main"), "HEAD"));
        let a = args(&["a", "b", "--explain"]).unwrap();
        assert_eq!((a.base.as_deref(), a.head.as_str(), a.explain), (Some("a"), "b", true));
        let a = args(&["--files", "x,y", "--explain"]).unwrap();
        assert_eq!((a.files, a.base), (Some(vec!["x".into(), "y".into()]), None));
        assert!(args(&["--all"]).unwrap().all);
        let a = args(&["--ci", "--app", "debrief"]).unwrap();
        assert_eq!((a.ci, a.app.as_deref(), a.base), (true, Some("debrief"), None));
        assert!(args(&["--ci"]).unwrap().ci);
        let a = args(&["--since-run", "7821"]).unwrap();
        assert_eq!((a.since_run, a.base, a.ci), (Some(7821), None, false));
        assert_eq!(args(&["a"]).unwrap().since_run, None);
    }

    #[test]
    fn incomplete_unknown_or_contradictory_arguments_are_refused_never_planned_as_nothing() {
        for (a, why) in [
            (vec!["--files"], "--files needs a value"),
            (vec!["--files", ""], "--files needs a value"),
            (vec!["--files", "--all"], "--files needs a value"),
            (vec!["--files", ","], "--files needs at least one file"),
            (vec!["--app", "x"], "--app is only valid with --ci"),
            (vec!["--ci", "--app"], "--app needs a value"),
            (vec!["--nope"], "unknown option --nope"),
            (vec!["--all", "--all"], "--all given more than once"),
            (vec!["--ci", "a"], "--ci takes no commits, --files or --all"),
            (vec!["--ci", "--all"], "--ci takes no commits, --files or --all"),
            (vec!["a", "b", "c"], "too many commits: a b c"),
            (vec!["--files", "x", "a"], "--files cannot be combined with a base or --all"),
            (vec!["--files", "x", "--all"], "--files cannot be combined with a base or --all"),
            (vec!["--all", "a"], "--all cannot be combined with a base"),
            (vec!["--since-run"], "--since-run needs a value"),
            (vec!["--since-run", "x"], "--since-run needs a run number, not \"x\""),
            (vec!["--since-run", "0"], "--since-run needs a run number, not \"0\""),
            (
                vec!["--since-run", "7", "a"],
                "--since-run takes no commits, --files, --all or --ci (in CI, PLAN_SINCE_RUN with --ci)",
            ),
            (
                vec!["--since-run", "7", "--ci"],
                "--since-run takes no commits, --files, --all or --ci (in CI, PLAN_SINCE_RUN with --ci)",
            ),
        ] {
            assert_eq!(args(&a).unwrap_err(), why, "{a:?}");
        }
    }
}
