//! Every test job of ci.yml and debrief.yml and the `only_tests` input of the train's fix ladder (rung 1, the failed tests only).
//!
//! A job the plan job can narrow to named tests ([`paddock_dev::plan_since`]'s `TEST_STEPS`, `VITEST_STEPS`, `PLAYWRIGHT_STEPS`) must
//! hand the plan's list to its runner: its step names the plan output or the matrix field that carries it. That part is a check
//! (a regression of the wiring fails). The other part is report-only (ER-41, first run recorded in docs/lessons.md once green on the
//! real tree): a test job that no table lists reruns whole in rung 2; the test prints each of them as a warning, and the set may
//! only shrink as jobs get wired. The workflows are read through the YAML accessor (`gates::yaml`, ER-42), never by grepping text.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::gates::yaml::{self, Node, Value};
use paddock_dev::plan_since::{PLAYWRIGHT_STEPS, TEST_STEPS, VITEST_STEPS};
use regex::Regex;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

fn workflow(name: &str) -> Node {
    let text = std::fs::read_to_string(root().join(".gitea/workflows").join(name)).unwrap();
    yaml::parse(&text).unwrap()
}

/// Every scalar under a node, in order, as one text (a step's script, its `uses`, its `with` and `env` values).
fn flat(n: &Node, out: &mut String) {
    match &n.value {
        Value::Scalar { text, .. } => {
            out.push_str(text);
            out.push('\n');
        }
        Value::Seq(items) => items.iter().for_each(|i| flat(i, out)),
        Value::Map(m) => m.iter().for_each(|(k, v)| {
            out.push_str(k);
            out.push('\n');
            flat(v, out);
        }),
    }
}

fn text_of(n: &Node) -> String {
    let mut s = String::new();
    flat(n, &mut s);
    s
}

/// What a step must mention to hand the plan's test list to its runner: the plan output or matrix field of that kind.
const WIRING: &[&str] = &["filter", "include", "tests"];

/// The violations of the wiring of the jobs the tables list, in one workflow's parsed text: a listed job that is gone, a listed step
/// that is gone, or a listed step that no longer mentions the list the plan writes for it.
fn wiring_violations(file: &str, doc: &Node) -> Vec<String> {
    let jobs = doc.get("jobs").expect("a workflow has jobs");
    let mut out = Vec::new();
    let listed = TEST_STEPS
        .iter()
        .map(|t| (t.0, t.1, t.2))
        .chain(VITEST_STEPS.iter().copied())
        .chain(PLAYWRIGHT_STEPS.iter().copied());
    for (wf, job, steps) in listed.filter(|t| t.0 == file) {
        let Some(j) = jobs.get(job) else {
            out.push(format!("{file}: job {job} is listed as taking a test list but is not in the workflow"));
            continue;
        };
        let Some(job_steps) = j.get("steps") else {
            out.push(format!("{wf}: job {job} has no steps"));
            continue;
        };
        for step in steps {
            // `Memory budget` is continue-on-error and never fails the job: it does not take the list.
            if *step == "Memory budget" {
                continue;
            }
            let found = job_steps.items().iter().find(|s| s.get_text("name") == Some(step));
            match found {
                None => out.push(format!("{file}: job {job} has no step named {step:?}, which the plan narrows to named tests")),
                Some(s) => {
                    let text = text_of(s);
                    if !WIRING.iter().any(|w| text.contains(w)) {
                        out.push(format!(
                            "{file}: step {step:?} of job {job} does not read the test list the plan writes ({})",
                            WIRING.join(", ")
                        ));
                    }
                }
            }
        }
    }
    out
}

/// The test jobs of a workflow no table lists (report-only): a job with a step that runs a test runner.
fn unwired_test_jobs(file: &str, doc: &Node) -> Vec<String> {
    let runner = Regex::new(r"nextest|vitest|ng test|playwright test|ci run \S+ (?:tests|specs|e2e|scripts)\b|app-gates").unwrap();
    let listed = |job: &str| {
        TEST_STEPS.iter().map(|t| (t.0, t.1)).chain(VITEST_STEPS.iter().map(|t| (t.0, t.1))).chain(PLAYWRIGHT_STEPS.iter().map(|t| (t.0, t.1))).any(|(w, j)| w == file && j == job)
    };
    doc.get("jobs")
        .expect("a workflow has jobs")
        .entries()
        .iter()
        .filter(|(job, j)| !listed(job) && j.get("steps").is_some_and(|s| runner.is_match(&text_of(s))))
        .map(|(job, _)| job.clone())
        .collect()
}

#[test]
fn every_job_the_plan_narrows_hands_the_test_list_to_its_runner() {
    for file in ["ci.yml", "debrief.yml"] {
        let v = wiring_violations(file, &workflow(file));
        assert!(v.is_empty(), "{}", v.join("\n"));
    }
}

#[test]
fn both_workflows_declare_the_only_tests_input_next_to_since_run_and_the_plan_step_reads_it() {
    for file in ["ci.yml", "debrief.yml"] {
        let doc = workflow(file);
        let inputs = doc.get("on").and_then(|o| o.get("workflow_dispatch")).and_then(|d| d.get("inputs")).expect("dispatch inputs");
        assert!(inputs.get("since_run").is_some() && inputs.get("only_tests").is_some(), "{file}: only_tests is declared with since_run");
        let plan = doc.get("jobs").and_then(|j| j.get("plan")).map(text_of).expect("a plan job");
        assert!(plan.contains("PLAN_ONLY_TESTS") && plan.contains("github.event.inputs.only_tests"), "{file}: the plan step does not pass only_tests");
    }
}

/// The positive control: a workflow whose runner step no longer mentions the list is a violation, and a listed job that is gone too.
#[test]
fn removing_the_wiring_is_seen() {
    let good = "jobs:\n  crate:\n    steps:\n      - name: Tests (workspace, every feature)\n        run: cargo nextest run -E \"$FILTER\"\n        env:\n          FILTER: ${{ matrix.filter }}\n";
    assert!(wiring_violations("ci.yml", &yaml::parse(good).unwrap()).iter().all(|v| !v.contains("step \"Tests (workspace")));
    let bare = good.replace("-E \"$FILTER\"", "").replace("FILTER: ${{ matrix.filter }}", "X: 1");
    let v = wiring_violations("ci.yml", &yaml::parse(&bare).unwrap());
    assert!(v.iter().any(|v| v.contains("does not read the test list")), "{v:?}");
    let gone = "jobs:\n  other:\n    steps:\n      - run: echo\n";
    assert!(wiring_violations("ci.yml", &yaml::parse(gone).unwrap()).iter().any(|v| v.contains("job crate is listed")));
}

/// Report-only (ER-41): the test jobs that rerun whole. The count is printed; the test fails only when the accessor sees none at all
/// (a reader that finds no test job in the real workflows has stopped seeing them).
#[test]
fn report_the_test_jobs_that_cannot_run_named_tests_yet() {
    let mut all = 0;
    for file in ["ci.yml", "debrief.yml"] {
        let jobs = unwired_test_jobs(file, &workflow(file));
        all += jobs.len();
        for j in &jobs {
            eprintln!("::warning::{file}: test job {j} takes no only_tests list yet: it reruns whole in rung 2 of the fix ladder");
        }
    }
    assert!(all > 0, "the reader found no unwired test job in ci.yml and debrief.yml: it no longer sees test jobs");
    let synthetic = "jobs:\n  t:\n    steps:\n      - run: ./dev ci run conformance tests\n";
    assert_eq!(unwired_test_jobs("ci.yml", &yaml::parse(synthetic).unwrap()), ["t"]);
}
