use paddock_ci_config::jobs::{
    APPS_PROJECT, Declared, JobModel, Leg, Legs, PROJECT_OUTPUTS, Problem, model, named_jobs,
    render, render_name,
};
use std::collections::{BTreeMap, BTreeSet};

/// A small workflow with the shapes of ci.yml: a plan job, a project job with step conditions, planned and static matrices.
const WF: &str = r#"
name: ci
on: [push]
jobs:
  plan:
    name: plan (affected projects)
    runs-on: ubuntu-latest
    outputs:
      ui: ${{ steps.plan.outputs.ui }}
    steps:
      - run: ./dev plan --ci
  ui:
    needs: [plan]
    if: ${{ contains(fromJSON(needs.plan.outputs.rerun_jobs || '["ui"]'), 'ui') }}
    name: ui (tokens)
    runs-on: ubuntu-latest
    steps:
      - uses: ./.gitea/actions/setup-rust
      - if: needs.plan.outputs.ui == 'true' || needs.plan.outputs.eslint == 'true'
        run: npm ci
      - if: needs.plan.outputs.icons == 'true'
        run: ./dev ci run icons tests
      - if: always()
        run: echo done
  e2e:
    needs: [plan]
    name: reference apps (shard ${{ matrix.shard }}/2)
    if: ${{ needs.plan.outputs.ui == 'true' && (needs.plan.outputs.webkit == 'true' || contains(fromJSON(needs.plan.outputs.rerun_jobs || '[]'), 'e2e')) && contains(fromJSON(needs.plan.outputs.rerun_jobs || '["e2e"]'), 'e2e') }}
    strategy:
      matrix:
        shard: [1, 2]
    runs-on: ubuntu-latest
    steps:
      - run: ./dev ci run ui e2e
  crate:
    needs: [plan]
    name: crate (${{ matrix.os }}${{ matrix.part_label }})
    if: ${{ needs.plan.outputs.crates == 'true' && contains(fromJSON(needs.plan.outputs.rerun_jobs || '["crate"]'), 'crate') }}
    strategy:
      matrix:
        include: ${{ fromJSON(needs.plan.outputs.crate_legs) }}
    runs-on: ${{ matrix.os }}
    steps:
      - run: cargo nextest run
  app:
    needs: [plan]
    name: app ${{ matrix.app }} (${{ matrix.os }}${{ matrix.e2e_leg != '' && format(', {0}', matrix.e2e_leg) || '' }})
    if: ${{ needs.plan.outputs.any_app == 'true' && contains(fromJSON(needs.plan.outputs.rerun_jobs || '["app"]'), 'app') }}
    strategy:
      matrix:
        include: ${{ fromJSON(needs.plan.outputs.apps) }}
    runs-on: ${{ matrix.os }}
    steps:
      - run: echo ${{ needs.plan.outputs.selection }}
  gitleaks:
    needs: [plan]
    name: gitleaks (secret scan)
    if: ${{ contains(fromJSON(needs.plan.outputs.rerun_jobs || '["gitleaks"]'), 'gitleaks') }}
    runs-on: ubuntu-latest
    steps:
      - run: gitleaks detect
"#;

fn declared() -> Declared {
    Declared {
        projects: BTreeMap::from([
            ("plan".to_string(), vec!["everything".to_string()]),
            ("gitleaks".to_string(), vec!["conformance".to_string()]),
        ]),
        tool_cache_actions: BTreeSet::from(["setup-rust".to_string()]),
    }
}

fn built() -> JobModel {
    model(WF, &declared()).unwrap_or_else(|p| panic!("the fixture models: {p:?}"))
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn problems(text: &str, declared: &Declared) -> Vec<Problem> {
    model(text, declared).expect_err("a problem was expected")
}

fn leg(pairs: &[(&str, &str)]) -> Leg {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn the_project_output_table_is_the_planners_eight() {
    let names: Vec<&str> = PROJECT_OUTPUTS.iter().map(|(n, _, _)| *n).collect();
    assert_eq!(
        names,
        [
            "crates",
            "ui",
            "eslint",
            "conformance",
            "c99",
            "tools",
            "icons",
            "paddock_dev"
        ]
    );
    assert_eq!(
        PROJECT_OUTPUTS[7],
        ("paddock_dev", "paddock-dev", "paddockDev")
    );
}

#[test]
fn jobs_come_in_file_order_with_their_raw_name_and_needs() {
    let m = built();
    let ids: Vec<&str> = m.jobs.iter().map(|j| j.id.as_str()).collect();
    assert_eq!(ids, ["plan", "ui", "e2e", "crate", "app", "gitleaks"]);
    assert_eq!(m.job("ui").unwrap().name, "ui (tokens)");
    assert_eq!(m.job("ui").unwrap().needs, ["plan"]);
    assert!(m.job("plan").unwrap().needs.is_empty());
}

#[test]
fn a_job_without_a_name_is_named_by_its_id() {
    let m = model(
        "jobs:\n  plan:\n    runs-on: x\n    steps:\n      - run: a\n",
        &Declared {
            projects: BTreeMap::from([("plan".to_string(), vec!["everything".to_string()])]),
            ..Declared::default()
        },
    )
    .unwrap();
    assert_eq!(m.job("plan").unwrap().name, "plan");
}

#[test]
fn projects_are_the_plan_outputs_the_job_and_its_step_conditions_read() {
    let m = built();
    assert_eq!(
        m.job("ui").unwrap().projects,
        set(&["ui", "eslint", "icons"])
    );
    assert_eq!(m.job("e2e").unwrap().projects, set(&["ui"]));
    assert_eq!(m.job("crate").unwrap().projects, set(&["crates"]));
    assert_eq!(m.job("app").unwrap().projects, set(&[APPS_PROJECT]));
}

#[test]
fn the_planners_paddock_dev_output_is_the_project_paddock_dev() {
    let wf = "jobs:\n  p:\n    if: ${{ needs.plan.outputs.paddock_dev == 'true' }}\n    steps:\n      - run: a\n";
    assert_eq!(
        model(wf, &Declared::default())
            .unwrap()
            .job("p")
            .unwrap()
            .projects,
        set(&["paddock-dev"])
    );
}

#[test]
fn declared_projects_are_added_to_the_derived_ones() {
    let m = built();
    assert_eq!(m.job("plan").unwrap().projects, set(&["everything"]));
    assert_eq!(m.job("gitleaks").unwrap().projects, set(&["conformance"]));
    let mut d = declared();
    d.projects
        .insert("crate".into(), vec!["conformance".into()]);
    assert_eq!(
        model(WF, &d).unwrap().job("crate").unwrap().projects,
        set(&["crates", "conformance"])
    );
}

#[test]
fn outputs_that_shape_legs_or_filters_add_no_project() {
    // rerun_jobs, webkit, selection and the *_legs outputs are read by these jobs and name no project.
    let m = built();
    assert!(!m.job("e2e").unwrap().projects.contains("webkit"));
    assert!(!m.job("app").unwrap().projects.contains("selection"));
}

#[test]
fn a_job_left_with_no_project_is_a_problem_never_always() {
    let mut d = declared();
    d.projects.remove("gitleaks");
    let p = problems(WF, &d);
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(p[0].job.as_deref(), Some("gitleaks"));
    assert!(p[0].message.contains("no project"), "{}", p[0].message);
    assert!(p[0].line > 0);
}

#[test]
fn an_unknown_plan_output_is_a_problem() {
    let wf = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.mystery == 'true' }}\n    steps:\n      - run: a\n";
    let p = problems(wf, &Declared::default());
    assert!(p.iter().any(|p| p.message.contains("mystery")), "{p:?}");
}

#[test]
fn a_condition_the_evaluator_cannot_read_is_a_problem_when_it_names_a_plan_output() {
    let wf = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' && ?? }}\n    steps:\n      - run: a\n";
    let p = problems(wf, &Declared::default());
    assert!(p.iter().any(|p| p.message.contains("cannot read")), "{p:?}");
}

#[test]
fn a_declared_job_the_workflow_lacks_is_a_problem() {
    let mut d = declared();
    d.projects.insert("ghost".into(), vec!["ui".into()]);
    let p = problems(WF, &d);
    assert!(p.iter().any(|p| p.message.contains("ghost")), "{p:?}");
}

#[test]
fn a_job_asks_rerun_jobs_when_its_condition_runs_it_for_its_own_name_and_not_for_another() {
    let m = built();
    for id in ["ui", "e2e", "crate", "app", "gitleaks"] {
        assert!(m.job(id).unwrap().asks_rerun_jobs, "{id}");
    }
    assert!(!m.job("plan").unwrap().asks_rerun_jobs);
    let wf = WF.replace(
        "    if: ${{ contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"gitleaks\"]'), 'gitleaks') }}\n",
        "    if: ${{ needs.plan.outputs.ui == 'true' }}\n",
    );
    assert!(
        !model(&wf, &declared())
            .unwrap()
            .job("gitleaks")
            .unwrap()
            .asks_rerun_jobs
    );
}

#[test]
fn the_tool_cache_must_come_before_the_first_dev_call() {
    let m = built();
    // `ui`: the setup-rust action (declared to restore the tool first) comes before its first ./dev step.
    assert!(m.job("ui").unwrap().tool_cache_before_first_dev);
    // `e2e` runs ./dev with nothing before it.
    assert!(!m.job("e2e").unwrap().tool_cache_before_first_dev);
    // A job that never runs ./dev has nothing to order.
    assert!(m.job("gitleaks").unwrap().tool_cache_before_first_dev);
    let late = "jobs:\n  j:\n    steps:\n      - run: ./dev ci run ui tests\n      - uses: ./.gitea/actions/setup-rust\n";
    let d = Declared {
        tool_cache_actions: set(&["setup-rust"]),
        ..Declared::default()
    };
    let mut d2 = d.clone();
    d2.projects.insert("j".into(), vec!["ui".into()]);
    assert!(
        !model(late, &d2)
            .unwrap()
            .job("j")
            .unwrap()
            .tool_cache_before_first_dev
    );
}

#[test]
fn legs_are_static_axes_planned_outputs_or_none() {
    let m = built();
    assert_eq!(m.job("ui").unwrap().legs, Legs::None);
    assert_eq!(
        m.job("crate").unwrap().legs,
        Legs::Planned("crate_legs".into())
    );
    assert_eq!(m.job("app").unwrap().legs, Legs::Planned("apps".into()));
    assert_eq!(
        m.job("e2e").unwrap().legs,
        Legs::Static(vec![leg(&[("shard", "1")]), leg(&[("shard", "2")])])
    );
}

#[test]
fn two_axes_make_every_combination_and_an_include_list_is_its_own_legs() {
    let two = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    strategy:\n      matrix:\n        os: [a, b]\n        n: [1, 2]\n    steps:\n      - run: x\n";
    let Legs::Static(legs) = model(two, &Declared::default())
        .unwrap()
        .job("j")
        .unwrap()
        .legs
        .clone()
    else {
        panic!("static")
    };
    assert_eq!(legs.len(), 4);
    assert!(legs.contains(&leg(&[("n", "2"), ("os", "a")])));
    let inc = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    strategy:\n      matrix:\n        include:\n          - os: a\n            x: 1\n          - os: b\n    steps:\n      - run: x\n";
    let Legs::Static(legs) = model(inc, &Declared::default())
        .unwrap()
        .job("j")
        .unwrap()
        .legs
        .clone()
    else {
        panic!("static")
    };
    assert_eq!(
        legs,
        vec![leg(&[("os", "a"), ("x", "1")]), leg(&[("os", "b")])]
    );
}

#[test]
fn a_matrix_the_model_cannot_expand_is_a_problem() {
    let base = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    strategy:\n      matrix:\n";
    for (matrix, word) in [
        ("        include: ${{ toJSON(x) }}\n", "include"),
        ("        os: ${{ x }}\n", "os"),
        (
            "        os: [a]\n        include:\n          - os: b\n",
            "mixes",
        ),
        (
            "        os: [a]\n        exclude:\n          - os: a\n",
            "exclude",
        ),
    ] {
        let p = problems(
            &format!("{base}{matrix}    steps:\n      - run: x\n"),
            &Declared::default(),
        );
        assert!(p.iter().any(|p| p.message.contains(word)), "{word}: {p:?}");
    }
}

#[test]
fn render_evaluates_the_name_with_the_legs_values() {
    let m = built();
    let krate = m.job("crate").unwrap();
    assert_eq!(
        render(
            krate,
            &leg(&[("os", "ubuntu-latest"), ("part_label", ", 2/3")])
        ),
        "crate (ubuntu-latest, 2/3)"
    );
    assert_eq!(
        render(krate, &leg(&[("os", "macos-latest")])),
        "crate (macos-latest)",
        "an absent value is empty"
    );
    let app = m.job("app").unwrap();
    assert_eq!(
        render(app, &leg(&[("app", "dyno"), ("os", "ubuntu-latest")])),
        "app dyno (ubuntu-latest)"
    );
    assert_eq!(
        render(
            app,
            &leg(&[
                ("app", "dyno"),
                ("os", "ubuntu-latest"),
                ("e2e_leg", "e2e-webkit")
            ])
        ),
        "app dyno (ubuntu-latest, e2e-webkit)"
    );
    assert_eq!(
        render(m.job("e2e").unwrap(), &leg(&[("shard", "2")])),
        "reference apps (shard 2/2)"
    );
    assert_eq!(render(m.job("ui").unwrap(), &Leg::new()), "ui (tokens)");
}

#[test]
fn a_workflow_the_reader_cannot_follow_is_a_problem_not_silence() {
    let d = Declared::default();
    assert!(problems("jobs: {", &d)[0].message.contains("YAML"));
    assert!(problems("name: x\n", &d)[0].message.contains("jobs"));
    let anchor = "base: &b {k: v}\njobs:\n  j:\n    <<: *b\n";
    assert!(problems(anchor, &d)[0].message.contains("merge key"));
}

#[test]
fn an_include_entry_with_a_value_that_is_not_text_is_a_problem() {
    let wf = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    strategy:\n      matrix:\n        include:\n          - os: a\n            more: [x]\n    steps:\n      - run: x\n";
    let p = problems(wf, &Declared::default());
    assert!(p.iter().any(|p| p.message.contains("not text")), "{p:?}");
}

#[test]
fn the_dev_tool_cache_action_always_counts_and_a_comment_is_not_a_dev_call() {
    let wf = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    steps:\n      - run: |\n          # ./dev is not called here\n          echo hi\n      - uses: ./.gitea/actions/dev-tool-cache\n      - run: ./dev ci run ui tests\n";
    assert!(
        model(wf, &Declared::default())
            .unwrap()
            .job("j")
            .unwrap()
            .tool_cache_before_first_dev
    );
    let wf = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    steps:\n      - run: cd x && ./dev ci run ui tests\n      - uses: ./.gitea/actions/dev-tool-cache\n";
    assert!(
        !model(wf, &Declared::default())
            .unwrap()
            .job("j")
            .unwrap()
            .tool_cache_before_first_dev
    );
}

#[test]
fn a_planned_matrix_must_name_one_plan_output() {
    let wf = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    strategy:\n      matrix:\n        include: ${{ fromJSON(needs.plan.outputs.a-b) }}\n    steps:\n      - run: x\n";
    let p = problems(wf, &Declared::default());
    assert!(p.iter().any(|p| p.message.contains("include")), "{p:?}");
}

#[test]
fn a_job_may_need_several_jobs_and_a_condition_on_a_step_that_is_not_text_reads_nothing() {
    let wf = "jobs:\n  j:\n    needs: [plan, other]\n    if: ${{ needs.plan.outputs.ui == 'true' }}\n    steps:\n      - if: true\n        run: x\n";
    let m = model(wf, &Declared::default()).unwrap();
    assert_eq!(m.job("j").unwrap().needs, ["plan", "other"]);
}

/// A workflow that is not ci.yml has no project outputs: its jobs are read for their names and matrices alone.
const APP_WF: &str = r#"
jobs:
  plan:
    name: plan
  e2e:
    name: end-to-end (shard ${{ matrix.shard }}/4)
    if: needs.plan.outputs.run == 'true'
    strategy:
      matrix:
        include: ${{ fromJSON(needs.plan.outputs.e2e_shards) }}
  smoke:
    name: real-app smoke (${{ matrix.os }})
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
  keychain:
    runs-on: macos-latest
"#;

#[test]
fn named_jobs_read_the_names_and_matrices_of_a_workflow_with_no_project_outputs() {
    let jobs = named_jobs(APP_WF).expect("names and matrices read");
    let ids: Vec<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
    assert_eq!(ids, ["plan", "e2e", "smoke", "keychain"]);
    assert_eq!(jobs[1].legs, Legs::Planned("e2e_shards".into()));
    assert_eq!(
        jobs[2].legs,
        Legs::Static(vec![
            leg(&[("os", "ubuntu-latest")]),
            leg(&[("os", "windows-latest")])
        ])
    );
    assert_eq!(
        jobs[3].name, "keychain",
        "a job without a name is named by its id"
    );
    assert_eq!(
        render_name(&jobs[1].name, &leg(&[("shard", "3")])),
        "end-to-end (shard 3/4)"
    );
    // A matrix this reader cannot expand, or a file without jobs, is a problem, never a guess.
    assert!(
        named_jobs("jobs:\n  a:\n    strategy:\n      matrix:\n        exclude: [x]\n").is_err()
    );
    assert!(
        named_jobs("name: x\n").unwrap_err()[0]
            .message
            .contains("no `jobs:`")
    );
}

#[test]
fn a_job_whose_condition_reads_the_webkit_output_says_so() {
    let m = built();
    // The reference-app jobs run only when the planner wants WebKit (a tag, the nightly, a manual ask).
    assert!(m.job("e2e").unwrap().reads_webkit);
    assert!(!m.job("ui").unwrap().reads_webkit && !m.job("crate").unwrap().reads_webkit);
}

#[test]
fn the_producer_is_the_first_tool_cache_use_with_producer_true_directly_or_through_a_declared_action()
 {
    let d = |extra: &[&str]| Declared {
        projects: BTreeMap::from([
            ("a".to_string(), vec!["ui".to_string()]),
            ("b".to_string(), vec!["ui".to_string()]),
            ("c".to_string(), vec!["ui".to_string()]),
        ]),
        tool_cache_actions: set(extra),
    };
    let wf = "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/runner-wait\n        with:\n          producer: \"true\"\n      - run: ./dev plan --ci\n  b:\n    steps:\n      - uses: ./.gitea/actions/dev-tool-cache\n        with:\n          producer: true\n  c:\n    steps:\n      - uses: ./.gitea/actions/runner-wait\n      - uses: ./.gitea/actions/dev-tool-cache\n        with:\n          producer: \"true\"\n";
    let m = model(wf, &d(&["runner-wait"])).unwrap();
    assert!(
        m.job("a").unwrap().tool_cache_producer,
        "through a declared action that hands the input on"
    );
    assert!(m.job("b").unwrap().tool_cache_producer, "the action itself");
    // Train y, run 9243: a consumer use first does the work, so a later producer use does nothing.
    assert!(
        !m.job("c").unwrap().tool_cache_producer,
        "the first use decides"
    );
    // Control: an action the model does not know as a tool-cache use is not the first use.
    let m = model(wf, &d(&[])).unwrap();
    assert!(!m.job("a").unwrap().tool_cache_producer);
    assert!(m.job("c").unwrap().tool_cache_producer);
}
