//! The CI adapter with its positive controls (ADR 0117 §8): each node kind and edge kind it emits, removed at its source line,
//! disappears.
#![allow(clippy::unwrap_used)]

use super::*;
use paddock_repo_graph::model::Reach;
use paddock_repo_graph::model::extract::{MemoryRead, build};

const CI: &str = r#"
name: ci
on: [push]
jobs:
  plan:
    runs-on: x
    steps:
      - uses: ./.gitea/actions/plan
  ui:
    if: ${{ needs.plan.outputs.ui == 'true' }}
    steps:
      - uses: ./.gitea/actions/cache
      - run: ./dev ci run ui specs
  app:
    if: ${{ needs.plan.outputs.any_app == 'true' }}
    steps:
      - uses: ./.gitea/actions/setup-rust
      - run: |
          grep -q conformance/ci/mentioned.sh .gitea/workflows/nightly.yml
          bash conformance/ci/kit.sh build
"#;

const RELEASE: &str = r#"
on: [push]
jobs:
  bundle:
    steps:
      - uses: ./.gitea/actions/cache
      - uses: ./.gitea/actions/setup-rust
      - uses: ./.gitea/actions/tauri-bundle
"#;

const CACHE: &str = r#"
runs:
  using: composite
  steps:
    - run: |
        r="${{ github.action_path }}/../../../conformance/ci/cache-report.sh"
        bash "$r" line
"#;

const PLAN: &str = r#"
runs:
  using: composite
  steps:
    - run: ./dev plan --ci
"#;

fn repo(with_uses: bool) -> MemoryRead {
    let ci = if with_uses {
        CI.to_string()
    } else {
        CI.replace("      - uses: ./.gitea/actions/cache\n", "")
    };
    MemoryRead::new()
        .with(".gitea/workflows/ci.yml", &ci)
        .with(".gitea/workflows/dyno-release.yml", RELEASE)
        .with(".gitea/actions/cache/action.yml", CACHE)
        .with(".gitea/actions/cache/README.md", "# cache")
        .with(".gitea/actions/plan/action.yml", PLAN)
        .with(".gitea/actions/setup-rust/action.yml", "runs:\n  using: composite\n  steps: []\n")
        .with(".gitea/actions/tauri-bundle/action.yml", "runs:\n  using: composite\n  steps: []\n")
        .with("conformance/ci/kit.sh", "#!/bin/sh\nbash conformance/ci/inner.sh\n")
        .with("conformance/ci/inner.sh", "echo hi\n")
        .with("conformance/ci/cache-report.sh", "echo report\n")
        .with("conformance/ci/mentioned.sh", "echo mentioned\n")
        .with(
            "apps/dyno/paddock.ci.json",
            r#"{"checks": ["node conformance/bin/paddock-conformance.mjs --only C13"]}"#,
        )
        .with("conformance/bin/paddock-conformance.mjs", "// cli\n")
}

fn adapter() -> CiExtractor {
    CiExtractor {
        apps: ["dyno".to_string(), "spotter".to_string()].into_iter().collect(),
        projects: ["ui", "conformance", "paddock-dev", "tools", "crates"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        decl: CiDecl {
            setup_only: ["setup-rust".to_string()].into_iter().collect(),
            release_actions: ["tauri-bundle".to_string()].into_iter().collect(),
        },
    }
}

fn model(m: &MemoryRead) -> paddock_repo_graph::model::nodes::Model {
    let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(adapter())];
    let b = build(&m.paths(), m, &adapters);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    b.model
}

/// The projects a change to `file` reaches through the model: each reached node's `runs` targets, with the kind.
fn projects(m: &paddock_repo_graph::model::nodes::Model, file: &str) -> BTreeMap<String, BTreeSet<Reach>> {
    let (hits, _) = m.reach(&[file.to_string()]);
    let mut out: BTreeMap<String, BTreeSet<Reach>> = BTreeMap::new();
    for (id, h) in hits {
        for p in m
            .targets(&id, EdgeKind::Runs)
            .into_iter()
            .filter(|p| !p.starts_with("script:") && !p.starts_with("action:"))
        {
            out.entry(p.to_string()).or_default().extend(h.kinds.iter().copied());
        }
    }
    out
}

fn set(k: &[Reach]) -> BTreeSet<Reach> {
    k.iter().copied().collect()
}

#[test]
fn every_ci_node_kind_is_emitted_and_owns_its_files() {
    let m = model(&repo(true));
    for (id, kind) in [
        ("wf:ci.yml", NodeKind::Workflow),
        ("job:ci.yml#ui", NodeKind::Job),
        ("step:ci.yml#ui/0", NodeKind::Step),
        ("action:cache", NodeKind::Action),
        ("script:conformance/ci/kit.sh", NodeKind::Script),
    ] {
        assert_eq!(m.node(id).map(|n| n.kind), Some(kind), "{id}");
    }
    assert_eq!(m.owner_of(".gitea/actions/cache/action.yml").unwrap().id, "action:cache");
    assert!(
        m.node("job:dyno-release.yml#bundle").unwrap().release,
        "a job using a release action is a release node"
    );
    assert!(m.node("action:setup-rust").unwrap().setup_only);
}

#[test]
fn g6_a_shared_action_reaches_its_jobs_projects_in_full_unless_setup_only_and_release_where_bundled() {
    let m = model(&repo(true));
    let r = projects(&m, ".gitea/actions/cache/action.yml");
    // Not declared setup-only: an input of its jobs in full (the coordinator's decision of 2026-11-23), release in a bundle job.
    assert_eq!(r["ui"], set(&[Reach::Code]));
    assert_eq!(r["app:dyno"], set(&[Reach::Release]), "dyno-release.yml's bundle job uses it");
    // A setup-only action is plumbing even there.
    let s = projects(&m, ".gitea/actions/setup-rust/action.yml");
    assert_eq!(s["app:dyno"], set(&[Reach::Plumbing]));
    assert_eq!(s["app:spotter"], set(&[Reach::Plumbing]), "any_app: every app");
    // Positive control (uses): without the `uses:` line the ui job is not reached.
    let c = projects(&model(&repo(false)), ".gitea/actions/cache/action.yml");
    assert!(!c.contains_key("ui"), "{c:?}");
}

#[test]
fn runs_and_reads_script_edges_come_from_invoked_programs_and_g4_a_mention_is_no_use() {
    let m = model(&repo(true));
    assert!(
        m.targets("step:ci.yml#app/1", EdgeKind::Runs)
            .contains(&"script:conformance/ci/kit.sh")
    );
    assert!(
        m.targets("script:conformance/ci/kit.sh", EdgeKind::ReadsScript)
            .contains(&"script:conformance/ci/inner.sh")
    );
    // A script two hops away is an input of the job in full (only declared setup-only actions are plumbing).
    assert!(projects(&m, "conformance/ci/inner.sh")["app:dyno"].contains(&Reach::Code));
    // G4: grep's pattern names mentioned.sh and nightly.yml; neither is an edge.
    assert!(projects(&m, "conformance/ci/mentioned.sh").is_empty());
    assert!(m.edges().all(|e| e.to != "wf:nightly.yml"));
    // A path held in a variable (`r="..."; bash "$r"`) inside an action is a use of the script.
    assert!(
        m.targets("action:cache", EdgeKind::Runs)
            .contains(&"script:conformance/ci/cache-report.sh")
    );
    // Positive control (runs): drop the bash line and kit.sh no longer reaches the app.
    let mut r = repo(true);
    r = r.with(
        ".gitea/workflows/ci.yml",
        &CI.replace("          bash conformance/ci/kit.sh build\n", ""),
    );
    assert!(!projects(&model(&r), "conformance/ci/kit.sh").contains_key("app:dyno"));
    // Positive control (reads-script): drop kit.sh's call and inner.sh reaches nothing.
    let r2 = repo(true).with("conformance/ci/kit.sh", "#!/bin/sh\necho no call\n");
    assert!(projects(&model(&r2), "conformance/ci/inner.sh").is_empty());
}

#[test]
fn dev_ci_run_and_dev_plan_run_their_projects_and_a_contract_check_line_runs_the_conformance_cli() {
    let m = model(&repo(true));
    assert!(m.targets("job:ci.yml#ui", EdgeKind::Runs).contains(&"ui"));
    assert!(
        m.targets("action:plan", EdgeKind::Runs).contains(&"paddock-dev"),
        "G5: the plan job runs the planner"
    );
    // The contract's check line is a step of the app (its home is the app's folder): the CLI it runs selects that phase, not
    // the app's tests (difference 4 of §8).
    let (hits, _) = m.reach(&["conformance/bin/paddock-conformance.mjs".to_string()]);
    assert!(hits.contains_key("step:apps/dyno/paddock.ci.json#checks"), "{hits:?}");
    assert_eq!(
        m.node("step:apps/dyno/paddock.ci.json#checks").and_then(|n| n.home.as_deref()),
        Some("apps/dyno")
    );
    assert!(
        !projects(&m, "conformance/bin/paddock-conformance.mjs").contains_key("app:dyno"),
        "no app suite"
    );
}

#[test]
fn the_readme_of_an_action_is_owned_by_its_action_folder_only_in_the_model() {
    // The model's folder entry would own it; the project level gives prose to the doc owner first (G7, crate test).
    let m = model(&repo(true));
    assert_eq!(
        m.owner_of(".gitea/actions/cache/README.md").map(|n| n.id.as_str()),
        Some("action:cache")
    );
}

#[test]
fn changed_jobs_names_the_jobs_whose_text_changed_and_none_for_a_header_change() {
    let after = CI.replace("./dev ci run ui specs", "./dev ci run ui lint");
    assert_eq!(changed_jobs(Some(CI), Some(&after)), Some(vec!["ui".to_string()]));
    // Moving a job down a line is no change to it.
    assert_eq!(
        changed_jobs(Some(CI), Some(&CI.replace("name: ci\n", "name: ci\n\n"))),
        Some(vec![])
    );
    assert_eq!(
        changed_jobs(Some(CI), Some(&CI.replace("on: [push]", "on: [push, pull_request]"))),
        None
    );
    assert_eq!(changed_jobs(Some(CI), Some("jobs: [")), None, "unreadable: fail closed");
}

#[test]
fn words_honour_quotes_and_stop_at_an_operator() {
    assert_eq!(words(r#"bash "a b.sh" x | tee y"#), ["bash", "a b.sh", "x"]);
    assert_eq!(words("A=1 ./x.sh"), ["A=1", "./x.sh"]);
}

#[test]
fn a_dispatcher_reaches_the_scripts_it_runs_through_its_own_folder_and_dynamic_names() {
    let tracked: BTreeSet<String> = [
        "conformance/ci/kit.sh",
        "conformance/ci/build-folder.sh",
        "conformance/ci/host/a.sh",
        "conformance/ci/host/b.sh",
        "dev",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let kit = r#"here=$(cd "conformance/ci" && pwd)
root=$(cd "$here/../.." && pwd)
case "$1" in
  build-folder) exec bash "$here/build-folder.sh" "$@" ;;
  host-tools) exec bash -e -o pipefail "$here/host/$step.sh" ;;
esac
exec bash "$root/dev" ci run ui specs
"#;
    let inv = invoked(kit, None, &tracked, &|w| (w == "ui").then(|| "ui".to_string()));
    let want: BTreeSet<String> = [
        "conformance/ci/build-folder.sh",
        "conformance/ci/host/a.sh",
        "conformance/ci/host/b.sh",
        "dev",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(inv.scripts, want);
    assert!(inv.projects.contains("ui"));
    // Control: a variable nobody assigned resolves to nothing (no guess), and a lone `$x` matches no file.
    let none = invoked("bash \"$nowhere/build-folder.sh\"\n$x\n", None, &tracked, &|_| None);
    assert!(none.scripts.iter().all(|s| s == "conformance/ci/build-folder.sh"), "{none:?}");
}

#[test]
fn on_the_real_tree_kit_sh_dispatches_to_build_folder_sh_and_a_shared_action_reaches_every_app() {
    // From --compare: conformance/ci/build-folder.sh (run by kit.sh's `build-folder)` branch) reached no app.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files: Vec<String> =
        paddock_repo_graph::History::tracked_files(&paddock_repo_graph::GitHistory::new(&root, std::time::Duration::from_secs(60)))
            .unwrap();
    let read = crate::repo_model::GitBulk::new(&root, None);
    let mut a = adapter();
    a.apps = crate::plan::apps_of(&files);
    let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(a)];
    let b = build(&files, &read, &adapters);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let kit = b.model.targets("script:conformance/ci/kit.sh", EdgeKind::ReadsScript);
    assert!(kit.contains(&"script:conformance/ci/build-folder.sh"), "{kit:?}");
    let r = projects(&b.model, "conformance/ci/build-folder.sh");
    // An input of an app's gates in full, release where a release job runs it (build-folder is not declared setup-only).
    assert!(r.get("app:debrief").is_some_and(|k| k.contains(&Reach::Code)), "{r:?}");
    assert!(
        r.get("app:dyno")
            .is_some_and(|k| k.contains(&Reach::Release) && k.contains(&Reach::Code)),
        "{r:?}"
    );
}

/// Review of 59e5f7ec3, P1-4: the shell forms a step runs a script by, the data a script reads, and unresolved paths.
const P14: &str = r#"
on: [push]
defaults:
  run:
    working-directory: apps/dyno
env:
  KIT: conformance/ci/kit.sh
jobs:
  app:
    if: ${{ needs.plan.outputs.any_app == 'true' }}
    env:
      HELPER: ../../conformance/ci/inner.sh
    steps:
      - run: bash scripts/local.sh
      - working-directory: .
        run: bash "$KIT" x
      - run: source "$HELPER"
      - working-directory: .
        run: port=$(bash conformance/ci/ports.sh webkit)
      - working-directory: .
        env:
          GONE: conformance/ci/deleted.sh
        run: bash "$GONE"
      - working-directory: .
        run: |
          d=$(mktemp -d)
          "$d/downloaded-tool" --version
          cat <<'EOF'
          <key>not/a/command</key>
          EOF
"#;

fn p14_repo(ci: &str) -> MemoryRead {
    MemoryRead::new()
        .with(".gitea/workflows/ci.yml", ci)
        .with("apps/dyno/scripts/local.sh", "echo local\n")
        .with("conformance/ci/kit.sh", "echo kit\n")
        .with("conformance/ci/inner.sh", "echo inner\n")
        .with("conformance/ci/ports.sh", "cat conformance/ci/ports.txt\n")
        .with("conformance/ci/ports.txt", "4359\n")
        .with(
            "apps/dyno/paddock.ci.json",
            r#"{"ui": ["cd apps/dyno/app", "node ../../../conformance/ci/check-index.mjs dist/index.html"]}"#,
        )
        .with("conformance/ci/check-index.mjs", "// check\n")
}

fn p14_model(ci: &str) -> paddock_repo_graph::model::nodes::Model {
    let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(adapter())];
    let m = p14_repo(ci);
    build(&m.paths(), &m, &adapters).model
}

#[test]
fn p1_4_defaults_env_source_substitutions_and_every_contract_phase_are_uses() {
    let m = p14_model(P14);
    let (hits, _) = m.reach(&["conformance/ci/check-index.mjs".to_string()]);
    assert!(
        hits.contains_key("step:apps/dyno/paddock.ci.json#ui"),
        "every contract phase is a step: {hits:?}"
    );
    for script in [
        "apps/dyno/scripts/local.sh",
        "conformance/ci/kit.sh",
        "conformance/ci/inner.sh",
        "conformance/ci/ports.sh",
    ] {
        assert!(
            projects(&m, script).contains_key("app:dyno"),
            "{script} reaches no app: {:?}",
            projects(&m, script)
        );
    }
    // Data a script reads reaches the script's jobs.
    assert!(projects(&m, "conformance/ci/ports.txt").contains_key("app:dyno"));
    // Controls: without the defaults the relative script is a path of nothing (unresolved), and without the ui phase line the
    // check script reaches no app.
    let no_defaults = p14_model(&P14.replace("defaults:\n  run:\n    working-directory: apps/dyno\n", ""));
    assert!(no_defaults.unresolved().iter().any(|u| u.reference == "scripts/local.sh"));
    let mut r = p14_repo(P14);
    r = r.with("apps/dyno/paddock.ci.json", r#"{"ui": ["cd apps/dyno/app"]}"#);
    let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(adapter())];
    let (hits, _) = build(&r.paths(), &r, &adapters)
        .model
        .reach(&["conformance/ci/check-index.mjs".to_string()]);
    assert!(!hits.contains_key("step:apps/dyno/paddock.ci.json#ui"));
}

#[test]
fn p1_4_a_deleted_script_is_unresolved_and_held_by_its_step_and_runner_paths_are_not() {
    let m = p14_model(P14);
    let refs: Vec<(&str, Option<&str>)> = m.unresolved().iter().map(|u| (u.reference.as_str(), u.node.as_deref())).collect();
    assert_eq!(refs, [("conformance/ci/deleted.sh", Some("step:ci.yml#app/4"))], "{refs:?}");
}

#[test]
fn a_caller_of_a_reusable_workflow_runs_its_app_only_when_the_callee_releases() {
    // dyno-release.yml's gates job calls ci.yml: a change ci.yml's conformance job runs reaches conformance, not dyno's suite.
    let caller =
        "on: [push]\njobs:\n  gates:\n    uses: ./.gitea/workflows/ci.yml\n  installers:\n    uses: ./.gitea/workflows/app-release.yml\n";
    let ci = "on: [workflow_call]\njobs:\n  conformance:\n    if: ${{ needs.plan.outputs.conformance == 'true' }}\n    steps:\n      - run: bash conformance/ci/kit.sh\n";
    let release = "on: [workflow_call]\njobs:\n  bundle:\n    steps:\n      - uses: ./.gitea/actions/tauri-bundle\n";
    let r = MemoryRead::new()
        .with(".gitea/workflows/dyno-release.yml", caller)
        .with(".gitea/workflows/ci.yml", ci)
        .with(".gitea/workflows/app-release.yml", release)
        .with(".gitea/actions/tauri-bundle/action.yml", "runs:\n  using: composite\n  steps: []\n")
        .with("conformance/ci/kit.sh", "echo kit\n");
    let m = model(&r);
    assert!(
        m.targets("job:dyno-release.yml#gates", EdgeKind::Runs).is_empty(),
        "the non-releasing callee runs for its own jobs"
    );
    assert_eq!(m.targets("job:dyno-release.yml#installers", EdgeKind::Runs), ["app:dyno"]);
    assert!(m.node("job:dyno-release.yml#installers").unwrap().release);
    let p = projects(&m, "conformance/ci/kit.sh");
    assert!(p.contains_key("conformance") && !p.contains_key("app:dyno"), "{p:?}");
}
