//! The plan job under the graph planner, end to end (ADR 0117 §5 and §6, the wiring): `./dev plan --ci` on the rule goldens'
//! fixture (its inputs file has no `[planner]` table, so the graph decides), one commit per kind of change, and what the job logs
//! (the planner line and the declared scope, ADR 0113 v2) and writes to `GITHUB_OUTPUT`, recorded in
//! `tests/golden/expected/cli-graph.json`. The controls of the brief are its cases: a plumbing change plans the smoke set, a
//! test-only change only the affected files, a docs change no app, an app's code that app in full, an unowned file everything, and
//! the switch on `rules` plans exactly as the rule tables do (no `selection` or `partial` output).
//!
//! Bless with `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test(plan_graph_ci::)'` (never in CI), review the diff, commit it.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn golden() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/expected/cli-graph.json")
}

/// The fixture with the app's declarations committed: a smoke set and its test files.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let (keep, root) = crate::repo_graph_net::rules_fixture();
    let path = root.join("apps/x/paddock.ci.json");
    let mut x: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    x["graph"] = json!({"tests": {"vitest": ["app/src/**/*.spec.ts"]}, "smoke": {"phases": ["checks"], "nextest": "package(=x-core)"}});
    // A test runner line `./dev ci run` can give files to (an unknown one runs the phase whole).
    x["ui"] = json!(["cd apps/x/app", "npx ng test --watch=false --coverage"]);
    std::fs::write(&path, serde_json::to_string_pretty(&x).unwrap()).unwrap();
    // The app job's gates use a setup-only action (`node-caches`, declared in the fixture's inputs file): a change to it is plumbing.
    std::fs::write(
        root.join(".gitea/actions/app-gates/action.yml"),
        "name: app-gates\nruns:\n  using: composite\n  steps:\n    - uses: ./.gitea/actions/node-caches\n    - run: echo app-gates\n      shell: bash\n",
    )
    .unwrap();
    // The app job runs the apps the plan names (its `if` reads `any_app`, as ci.yml's does).
    let ci = root.join(".gitea/workflows/ci.yml");
    let text = paddock_dev::gates::ci::workflow_text_raw(&root, "ci.yml").unwrap();
    let text = text.replace(
        "  app:\n    needs: [plan]\n",
        "  app:\n    needs: [plan]\n    if: ${{ needs.plan.outputs.any_app == 'true' }}\n",
    );
    assert!(text.contains("outputs.any_app"), "the fixture's app job");
    std::fs::write(&ci, text).unwrap();
    std::fs::create_dir_all(root.join("apps/x/app/src")).unwrap();
    std::fs::write(root.join("apps/x/app/src/main.spec.ts"), "it('runs', () => {});\n").unwrap();
    crate::golden::git(&root, &["add", "-A"]);
    crate::golden::git(&root, &["commit", "-q", "-m", "declarations"]);
    (keep, root)
}

/// What the plan job of a dispatch with the parent as its base logs and writes, after one commit that writes `edit`.
fn plan_job(edit: &[(&str, &str)]) -> Value {
    let (_keep, root) = fixture();
    for (path, text) in edit {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::write(root.join(path), text).unwrap();
    }
    crate::golden::git(&root, &["add", "-A"]);
    crate::golden::git(&root, &["commit", "-q", "-m", "change"]);
    let dir = tempfile::tempdir().unwrap();
    let mut program = crate::dev_bin::command();
    program.arg("plan").arg("--ci").current_dir(&root);
    for (k, _) in paddock_dev::process_env::vars() {
        if k.starts_with("GITHUB_") || k.starts_with("PLAN_") || k == "PADDOCK_PLAN_SELECTION" {
            program.env_remove(k);
        }
    }
    program
        .env("GITHUB_OUTPUT", dir.path().join("out"))
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_REF", "refs/heads/topic")
        .env("PLAN_BASE", "HEAD~1");
    let out = program.output().unwrap();
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let sha = regex::Regex::new(r"\b[0-9a-f]{40}\b").unwrap();
    let mask = |t: &str| sha.replace_all(t, "<sha>").to_string();
    let lines: Vec<String> = log
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("planner:") || l.starts_with("plan scope:") || l.starts_with("::warning::") || l.starts_with("::error::"))
        .map(mask)
        .collect();
    let outputs: serde_json::Map<String, Value> = std::fs::read_to_string(dir.path().join("out"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), json!(mask(v))))
        .collect();
    json!({"code": out.status.code(), "log": lines, "outputs": outputs})
}

#[test]
fn the_plan_job_log_and_outputs_under_the_graph_planner() {
    let inputs = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/rules/extra/docs/repo-graph.toml");
    let with_switch = format!(
        "{}\n[planner]\nmode = \"rules\"\nreason = \"a control\"\n",
        std::fs::read_to_string(inputs).unwrap()
    );
    let cases: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "plumbing-only-plans-the-smoke-set",
            vec![(
                ".gitea/actions/node-caches/action.yml",
                "name: node-caches\nruns:\n  using: composite\n  steps: []\n",
            )],
        ),
        (
            "test-only-passes-the-affected-spec",
            vec![("apps/x/app/src/main.spec.ts", "it('runs again', () => {});\n")],
        ),
        ("docs-only-plans-no-app", vec![("docs/readme.md", "# readme, edited\n")]),
        (
            "app-code-plans-the-app-in-full",
            vec![("apps/x/app/src/main.ts", "export const changed = 1;\n")],
        ),
        ("unowned-plans-everything", vec![("Mystery", "an unowned file, edited\n")]),
        (
            "the-rules-switch-plans-as-before",
            vec![("apps/x/app/src/main.spec.ts", "it('runs again', () => {});\n")],
        ),
    ];
    let mut got = serde_json::Map::new();
    for (name, edit) in &cases {
        let mut edit = edit.clone();
        // The rules case keeps the inputs file and only adds the switch.
        if *name == "the-rules-switch-plans-as-before" {
            edit.push(("docs/repo-graph.toml", with_switch.as_str()));
        }
        got.insert(name.to_string(), plan_job(&edit));
    }
    let got = Value::Object(got);
    // Checks that hold whatever is recorded (the golden cannot bless them away).
    let scope_of = |case: &str| -> paddock_dev::run_scope::Declared {
        let line = got[case]["log"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .find(|l| l.starts_with("plan scope:"))
            .unwrap()
            .to_string();
        paddock_dev::run_scope::read(&line).unwrap().unwrap()
    };
    let out = |case: &str, k: &str| got[case]["outputs"][k].as_str().map(str::to_string);
    for (case, _) in &cases {
        assert_eq!(got[case]["code"], 0, "{case}: {}", got[case]);
        let planner = if *case == "the-rules-switch-plans-as-before" {
            "planner: rules (a control)"
        } else {
            "planner: graph"
        };
        assert!(
            got[case]["log"].as_array().unwrap().iter().any(|l| l == planner),
            "{case}: {}",
            got[case]["log"]
        );
    }
    assert!(
        scope_of("plumbing-only-plans-the-smoke-set").ran.smoke.contains("app:x"),
        "{}",
        got["plumbing-only-plans-the-smoke-set"]
    );
    assert!(scope_of("test-only-passes-the-affected-spec").ran.affected.contains("app:x"));
    assert!(
        scope_of("test-only-passes-the-affected-spec").complete().is_err(),
        "an affected run is never complete"
    );
    assert_eq!(out("test-only-passes-the-affected-spec", "partial").as_deref(), Some("true"));
    assert!(out("docs-only-plans-no-app", "any_app").as_deref() == Some("false"));
    assert_eq!(out("app-code-plans-the-app-in-full", "partial").as_deref(), Some("false"));
    assert!(scope_of("unowned-plans-everything").ran.smoke.is_empty() && scope_of("unowned-plans-everything").ran.affected.is_empty());
    assert_eq!(
        out("the-rules-switch-plans-as-before", "selection"),
        None,
        "the rule tables add no output"
    );
    if crate::golden::bless() {
        crate::golden::record(&golden(), &got);
        return;
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(golden()).unwrap_or_else(|_| "null".into())).unwrap();
    assert!(
        want == got,
        "{} changed; review, then run `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test(plan_graph_ci::)'` and commit it:\n{}",
        golden().display(),
        serde_json::to_string_pretty(&got).unwrap()
    );
}
