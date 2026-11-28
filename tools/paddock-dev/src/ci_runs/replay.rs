// ci-runs-reader: the replay suite, real runs through the verdict, reuse, fix-items and trial readers
//! Replays recorded runs of the real server through the readers that decide a landing, and asserts the answers written down in
//! [`SHAPES`] (user decision 96, 2026-11-26).
//!
//! The class: defects in the verdict, reuse and fix-items readers showed up only at a train's landing (the expected map of run 9167,
//! the 200-commit walk of 9150, the first-parent line of 9056), because a narrowed trial never exercises them and a unit test feeds
//! them invented runs. A shape is a folder of `tests/fixtures/runs/<shape>/` made by `./dev ci record-fixture <run>... --name <shape>`
//! (the run, its jobs, the trimmed job logs with the plan job's scope line, earlier attempts, a trial's merge commit); this module
//! serves it as the Gitea server ([`Fake`]) and asks each reader its question. How to add or update a shape: docs/testing/ci-replay.md.
//!
//! What a replay cannot hold is git: the readers take what a commit touched and main's first-parent line as inputs, so a shape
//! declares them ([`Shape::touched`]) and the answer is written down for each.

use super::client::Client;
use super::green::Touched;
use super::testutil::Fake;
use super::{fix_items, reuse, verdict};
use serde_json::{Value, json};
use std::path::PathBuf;

/// What a reader may be told about the change a run tested.
#[derive(Clone, Copy, Debug)]
enum Touch {
    /// No project reached: a FLAKY test anywhere is in untouched code.
    Nothing,
    /// The change cannot be bounded: every target counts as reached.
    Everything,
    /// The reader cannot tell what the change touched.
    Unread,
}

impl Touch {
    fn read(self) -> Option<Touched> {
        self.read_with(&crate::run_scope::ExpectedMap::new())
    }

    /// [`Touch::read`] with the expected map of the run's scope line, which places a job's project (a v3 line holds it).
    fn read_with(self, expected: &crate::run_scope::ExpectedMap) -> Option<Touched> {
        let base = Touched {
            expected: expected.clone(),
            ..Touched::default()
        };
        match self {
            Touch::Nothing => Some(base),
            Touch::Everything => Some(Touched { all: true, ..base }),
            Touch::Unread => None,
        }
    }
}

/// A verdict, as the table writes it. `Red` and `NotMeasured` carry a phrase their reason holds.
#[derive(Debug)]
enum V {
    Green(u64),
    Red(Option<u64>, &'static str),
    NotMeasured(&'static str),
}

/// What a reuse decision is, as the table writes it.
#[derive(Debug)]
enum R {
    Skip(u64),
    Run(&'static str),
}

/// What the trial reader says, as the table writes it.
#[derive(Debug)]
enum T {
    Green,
    NotMeasured(&'static str),
}

/// One recorded shape and what every reader must answer on it.
struct Shape {
    /// The folder below `tests/fixtures/runs`.
    name: &'static str,
    /// The workflow file whose runs are asked.
    workflow: &'static str,
    /// The run whose commit is asked about (its `head_sha` is read from the recording).
    run: u64,
    /// The run that asks, left out of the list (a push run asking whether an earlier run tested its commit).
    asking: Option<u64>,
    /// What the change touched, for the FLAKY grade.
    touched: Touch,
    /// The verdict on the declared scope (the basis that decides once a producer is recorded, ER-41).
    on_scope: V,
    /// The verdict on the old `plan verdict: complete=` line (the report-only phase).
    on_line: V,
    /// The verdict a landing reads (`ci verdict --landing`): a run of its own diff's narrowed selection lands the train too.
    on_landing: V,
    /// The reuse decision for the asking push, `None` when the shape is no push.
    reuse: Option<R>,
    /// The member's trial verdict, `None` when the shape is no trial.
    trial: Option<T>,
    /// Why: one line.
    why: &'static str,
}

/// The shapes. Add one when a reader changes (docs/testing/ci-replay.md); each answer has its reason in `why`.
///
/// Recorded on origin/main 167e0473a (v2 scope lines, the rows marked D2) and 696f472f4 (v3, after job-model WP-2, ADR 0123): a v2 line
/// is never reused (D2), so the readers say "not measured" for it, expected and not a fault. The old-line column is the report-only
/// phase's basis and does not read the scope line.
const SHAPES: &[Shape] = &[
    Shape {
        name: "green-train",
        workflow: "ci.yml",
        run: 9170,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 2"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 2"),
        reuse: None,
        trial: None,
        why: "D2: a v2 line (a train landing run of its diff's selection, smoke icons) is not measured by the v3 readers; before WP-2 it landed the train",
    },
    Shape {
        name: "green-refused-by-old-reader",
        workflow: "ci.yml",
        run: 9167,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(Some(9167), "reference apps (shard 1/2)"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Red(Some(9167), "reference apps (shard 1/2)"),
        reuse: None,
        trial: None,
        why: "v3 line from before the planner fix: its expected map lists the e2e legs a train skips, so the green run is refused as not run (the 9167 defect, now a planner-side fix; 9180 is the fixed shape)",
    },
    Shape {
        name: "red-one-flaky",
        workflow: "ci.yml",
        run: 9168,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 2"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 2"),
        reuse: None,
        trial: None,
        why: "D2: a v2 line, not measured; the FLAKY grade itself is replayed from this run's logs in the fix-items test below",
    },
    Shape {
        name: "push-reused",
        workflow: "ci.yml",
        run: 9170,
        asking: Some(9171),
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 2"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 2"),
        reuse: Some(R::Run("version 2")),
        trial: None,
        why: "D2: the push 9171 asks about a commit whose run has a v2 line, so it is not reused and the push runs in full (before WP-2 it skipped, 9170)",
    },
    Shape {
        name: "failed-only-round",
        workflow: "ci.yml",
        run: 9147,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 2"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 2"),
        reuse: None,
        trial: None,
        why: "D2: the green failed-only round (9147 reran the one job of 9146) has a v2 line: not measured; it proved nothing for the commit before either (ER-10)",
    },
    Shape {
        name: "failed-only-round",
        workflow: "ci.yml",
        run: 9146,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 2"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 2"),
        reuse: None,
        trial: None,
        why: "D2: the red failed-only round (a test failed both tries), a v2 line: not measured",
    },
    Shape {
        name: "nightly",
        workflow: "nightly.yml",
        run: 9103,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 2"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 2"),
        reuse: None,
        trial: None,
        why: "D2: a scheduled nightly (red, 6 jobs failed), a v2 line. The nightly declares ci.yml's scope, so it never decides a landing or a tag",
    },
    Shape {
        name: "member-trial",
        workflow: "ci.yml",
        run: 9163,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 2"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 2"),
        reuse: None,
        trial: Some(T::NotMeasured("version 2")),
        why: "D2: a member's trial with a v2 line: not measured, never green (a trial is no landing proof either)",
    },
    Shape {
        name: "green-full-v3",
        workflow: "ci.yml",
        run: 9180,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Green(9180),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Green(9180),
        reuse: None,
        trial: None,
        why: "a full green train run with a v3 line and a clean shadow: the newest full run decides, green",
    },
    Shape {
        name: "push-reused-v3",
        workflow: "ci.yml",
        run: 9180,
        asking: Some(9181),
        touched: Touch::Nothing,
        on_scope: V::Green(9180),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Green(9180),
        reuse: Some(R::Skip(9180)),
        trial: None,
        why: "the push 9181 asks about the commit train run 9180 fully tested: it skips and reuses 9180 (ER-49)",
    },
    Shape {
        name: "member-trial-v3",
        workflow: "ci.yml",
        run: 9179,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(None, ""),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Red(None, ""),
        reuse: None,
        trial: Some(T::Green),
        why: "a member's trial with a v3 line that ran only its affected jobs (eslint and icons not run): the trial reader grades a trial on its own scope line (every expected job ran and passed), so it is green; the full scope is the landing verdict's",
    },
];

/// A recorded artifact reader shape: what the real server answered for a run's artifacts (the list, the redirect of the download, the
/// zip), replayed through the escape monitor's store (`escape::artifact::ArtifactStore`, ADR 0117 amendment D13).
struct ArtifactShape {
    /// The folder below `tests/fixtures/runs`.
    name: &'static str,
    /// The run whose `plan-record` artifact is read.
    run: u64,
    /// The artifact's id in the recording.
    artifact: u64,
    /// Why: one line.
    why: &'static str,
}

/// The artifact shapes. Add one when the reader of an Actions API endpoint other than runs and jobs changes.
const ARTIFACT_SHAPES: &[ArtifactShape] = &[ArtifactShape {
    name: "plan-record-artifact",
    run: 9272,
    artifact: 94252,
    why: "Gitea 28.4, trial of fix/plan-records-artifact: the plan job's v4 upload is listed by the REST API and its zip download answers 302 to a signed /zip/raw URL (a v3 upload was not listed: docs/research/2026-11-ci-artifacts-v4.md)",
}];

fn dir_of(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runs").join(name)
}

/// The server of a shape: the recorded files plus the lists the readers ask for (`head_sha=`, `event=workflow_dispatch`).
fn serve(name: &str) -> Fake {
    let mut f = Fake::from_dir(&dir_of(name), "example/paddock");
    let api = "https://gitea.invalid/api/v1/repos/example/paddock/actions";
    let runs = f.take_runs();
    let mut shas: Vec<String> = runs.iter().filter_map(|r| r["head_sha"].as_str().map(str::to_string)).collect();
    shas.sort();
    shas.dedup();
    for s in shas {
        let of: Vec<&Value> = runs.iter().filter(|r| r["head_sha"].as_str() == Some(&s)).collect();
        f.json(
            &format!("{api}/runs?head_sha={s}&limit=50&page=1"),
            json!({"total_count": of.len(), "workflow_runs": of}),
        );
    }
    let dispatched: Vec<&Value> = runs.iter().filter(|r| r["event"] == "workflow_dispatch").collect();
    f.json(
        &format!("{api}/runs?event=workflow_dispatch&limit=50&page=1"),
        json!({"total_count": dispatched.len(), "workflow_runs": dispatched}),
    );
    f
}

fn client(f: &mut Fake) -> Client<'_> {
    Client::new(f, "https://gitea.invalid", "example/paddock", "t")
}

fn run_json(name: &str, run: u64) -> Value {
    serde_json::from_slice(&std::fs::read(dir_of(name).join(format!("run-{run}.json"))).unwrap()).unwrap()
}

fn matches(got: &verdict::Verdict, want: &V) -> bool {
    use verdict::Verdict as G;
    match (got, want) {
        (G::Green { run }, V::Green(w)) => run == w,
        (G::Red { run, reason }, V::Red(w, phrase)) => run == w && reason.contains(phrase),
        (G::NotMeasured(why), V::NotMeasured(phrase)) => why.contains(phrase),
        _ => false,
    }
}

fn landing_of(s: &Shape) -> verdict::Verdict {
    let sha = run_json(s.name, s.run)["head_sha"].as_str().unwrap().to_string();
    let mut f = serve(s.name);
    let touched = s.touched;
    verdict::landing_verdict_with(&mut client(&mut f), s.workflow, &sha, s.asking, true, &mut |_, _| touched.read()).0
}

fn verdict_of(s: &Shape, scope: bool, touched: Touch) -> verdict::Verdict {
    let sha = run_json(s.name, s.run)["head_sha"].as_str().unwrap().to_string();
    let mut f = serve(s.name);
    verdict::verdict_with_touched_for(
        &mut client(&mut f),
        s.workflow,
        &sha,
        s.asking,
        scope,
        false,
        &mut |_| Vec::new(),
        &mut |_, _| touched.read(),
    )
}

#[test]
fn every_shape_is_answered_as_written_down() {
    assert!(!SHAPES.is_empty(), "the replay suite holds no shape");
    let mut wrong = Vec::new();
    for s in SHAPES {
        let (a, b) = (verdict_of(s, true, s.touched), verdict_of(s, false, s.touched));
        if !matches(&a, &s.on_scope) {
            wrong.push(format!("{}: on the scope {a:?}, wanted {:?} ({})", s.name, s.on_scope, s.why));
        }
        if !matches(&b, &s.on_line) {
            wrong.push(format!("{}: on the old line {b:?}, wanted {:?} ({})", s.name, s.on_line, s.why));
        }
        let c = landing_of(s);
        if !matches(&c, &s.on_landing) {
            wrong.push(format!("{}: on landing {c:?}, wanted {:?} ({})", s.name, s.on_landing, s.why));
        }
        if let Some(want) = &s.reuse {
            let sha = run_json(s.name, s.asking.unwrap())["head_sha"].as_str().unwrap().to_string();
            let mut f = serve(s.name);
            let got = reuse::decide_with_touched(
                &mut client(&mut f),
                s.workflow,
                &sha,
                s.asking,
                true,
                &mut |tips| tips.iter().filter(|t| **t != sha).cloned().collect(),
                &mut |_, _| s.touched.read(),
                &mut |_, _| false,
            );
            let ok = match (&got, want) {
                (reuse::Reuse::Skip { run, .. }, R::Skip(w)) => run == w,
                (reuse::Reuse::Run(why), R::Run(phrase)) => why.contains(phrase),
                _ => false,
            };
            if !ok {
                wrong.push(format!("{}: reuse {got:?}, wanted {want:?} ({})", s.name, s.why));
            }
        }
        if let Some(want) = &s.trial {
            let run = run_json(s.name, s.run);
            let branch = run["head_branch"].as_str().unwrap().strip_prefix("trial/").unwrap().to_string();
            let commit = std::fs::read(dir_of(s.name).join(format!("commit-{}.json", run["head_sha"].as_str().unwrap()))).unwrap();
            let commit: Value = serde_json::from_slice(&commit).unwrap();
            let member = commit["parents"][1]["sha"].as_str().unwrap().to_string();
            let mut f = serve(s.name);
            let got = verdict::trial(&mut client(&mut f), s.workflow, &branch, &member);
            let ok = match (&got, want) {
                (verdict::Trial::Green { run }, T::Green) => *run == s.run,
                (verdict::Trial::NotMeasured(why), T::NotMeasured(phrase)) => why.contains(phrase),
                _ => false,
            };
            if !ok {
                wrong.push(format!("{}: trial {got:?}, wanted {want:?} ({})", s.name, s.why));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_recorded_folder_has_a_shape_and_every_shape_a_folder() {
    let mut folders: Vec<String> = std::fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    folders.sort();
    let mut named: Vec<String> = SHAPES
        .iter()
        .map(|s| s.name.to_string())
        .chain(ARTIFACT_SHAPES.iter().map(|s| s.name.to_string()))
        .collect();
    named.sort();
    named.dedup();
    assert_eq!(
        folders, named,
        "a recording with no row in SHAPES is never replayed; a row with no recording replays nothing"
    );
}

#[test]
fn a_recorded_plan_record_artifact_is_read_back_through_the_store() {
    use crate::escape::artifact::{ARTIFACT_NAME, ArtifactStore};
    use crate::escape::{Load, Store};
    for a in ARTIFACT_SHAPES {
        let head = run_json(a.name, a.run)["head_sha"].as_str().unwrap().to_string();
        let mut f = serve(a.name);
        // The list the real server answered names the artifact the reader looks for, and it is not expired.
        let list = client(&mut f).run_artifacts(a.run, ARTIFACT_NAME).unwrap().unwrap();
        assert_eq!(
            list.iter().map(|x| (x.id, x.name.as_str(), x.expired)).collect::<Vec<_>>(),
            vec![(a.artifact, ARTIFACT_NAME, false)],
            "{}: {}",
            a.name,
            a.why
        );
        // The download answered a redirect to the same artifact's /zip/raw (the recorder keeps the path, not the signature).
        let redirect = std::fs::read_to_string(dir_of(a.name).join(format!("artifact-{}.redirect.txt", a.artifact))).unwrap();
        assert_eq!(
            redirect.trim(),
            format!(
                "https://gitea.invalid/api/v1/repos/example/paddock/actions/artifacts/{}/zip/raw",
                a.artifact
            ),
            "{}: the redirect the real server sent",
            a.name
        );
        // The store reads the real zip back to the record of that commit.
        let store = ArtifactStore::new(client(&mut f));
        match store.get(&head) {
            Load::Ok(r) => {
                assert_eq!(r.commit, head, "{}", a.name);
                assert!(!r.selected.is_empty() && !r.reach.is_empty(), "{}: a record of a real plan", a.name);
            }
            other => panic!("{}: {other:?}", a.name),
        }
        // Control: a commit the server has no run for is Missing, never a record.
        assert_eq!(store.get(&"0".repeat(40)), Load::Missing, "{}", a.name);
    }
}

#[test]
fn a_wrong_answer_is_caught() {
    // The positive control (ER-33): the comparison goes red when the answer differs, so a table row cannot pass by accident.
    use verdict::Verdict as G;
    assert!(matches(&G::Green { run: 1 }, &V::Green(1)));
    assert!(!matches(&G::Green { run: 1 }, &V::Green(2)));
    assert!(!matches(&G::Green { run: 1 }, &V::Red(Some(1), "")));
    assert!(!matches(&G::NotMeasured("a b".into()), &V::NotMeasured("c")));
    assert!(!matches(
        &G::Red {
            run: None,
            reason: "x".into()
        },
        &V::Red(Some(1), "x")
    ));
}

#[test]
fn a_flaky_test_is_a_fix_item_in_untouched_code_and_red_in_reached_code_and_not_measured_unread() {
    // Run 9168 (red-one-flaky, a v2 line the verdict no longer reads, D2): its failed job's real log graded by ER-12's reader on all three
    // answers for what the change touched, then the fix items recorded as a landing writes them.
    use super::green::{FailedJob, GradedResult, grade_failures};
    let jobs: Value = serde_json::from_slice(&std::fs::read(dir_of("red-one-flaky").join("jobs-9168.json")).unwrap()).unwrap();
    let job = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["conclusion"] == "failure")
        .unwrap();
    let id = job["id"].as_u64().unwrap();
    let failed = FailedJob {
        name: job["name"].as_str().unwrap().to_string(),
        id,
        url: job["html_url"].as_str().unwrap().to_string(),
        failed_steps: job["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["conclusion"] == "failure")
            .map(|s| s["name"].as_str().unwrap().to_string())
            .collect(),
        log: Some(std::fs::read_to_string(dir_of("red-one-flaky").join(format!("log-{id}.txt"))).unwrap()),
    };
    // The job names come from the expected map of a v3 scope line: the plan job of run 9180 (green-full-v3), the same workflow.
    let plan = std::fs::read_dir(dir_of("green-full-v3"))
        .unwrap()
        .filter_map(|e| std::fs::read_to_string(e.unwrap().path()).ok())
        .find(|t| t.contains("plan scope: "))
        .unwrap();
    let expected = crate::run_scope::read(&plan).unwrap().unwrap().expected;
    let grade = |t: Touch| grade_failures("ci.yml", std::slice::from_ref(&failed), &expected, &mut || t.read_with(&expected));
    let r = grade(Touch::Nothing);
    let GradedResult::GreenWithFixItems(items) = r else {
        panic!("untouched is a fix item: {r:?}")
    };
    let lines = fix_items::lines("ci.yml", 9168, &items);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("- ci.yml: run 9168, job crate (windows-latest) (id 51509): paddock-proc usage::tests::the_os_source_lists"),
        "{lines:?}"
    );
    // The lessons entry is added once, and a second call adds nothing (a retried landing writes it once).
    let sha = run_json("red-one-flaky", 9168)["head_sha"].as_str().unwrap().to_string();
    let once = fix_items::add_to_lessons("# Lessons\n", "ci.yml", 9168, &sha, &items).expect("added");
    assert!(
        once.contains("full ci.yml run 9168 (167e0473a)") && once.contains("(job 51509, crate (windows-latest);"),
        "{once}"
    );
    assert_eq!(fix_items::add_to_lessons(&once, "ci.yml", 9168, &sha, &items), None);
    // Reached code: red, naming the test. Unread: not measured, never a pass.
    assert!(matches!(grade(Touch::Everything), GradedResult::Red(why) if why.contains("a FLAKY test")));
    assert!(matches!(grade(Touch::Unread), GradedResult::NotMeasured(why) if why.contains("cannot be read")));
}

#[test]
fn controls_a_push_without_a_base_runs_and_another_members_trial_is_no_run() {
    // The same recordings with the signal hidden go the other way (ER-33): with the log of the landing run's plan job unreadable the
    // push cannot tell it was a full run, so it runs its own; a trial of another head of the branch is not this member's.
    let sha = run_json("push-reused-v3", 9181)["head_sha"].as_str().unwrap().to_string();
    let mut f = serve("push-reused-v3");
    let jobs: Value = serde_json::from_slice(&std::fs::read(dir_of("push-reused-v3").join("jobs-9180.json")).unwrap()).unwrap();
    let plan = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["name"].as_str().unwrap().starts_with("plan"))
        .unwrap();
    f.fail(plan["id"].as_u64().unwrap(), "hidden by the control");
    let got = reuse::decide_with_touched(
        &mut client(&mut f),
        "ci.yml",
        &sha,
        Some(9181),
        true,
        &mut |_| Vec::new(),
        &mut |_, _| Some(Touched::default()),
        &mut |_, _| false,
    );
    assert!(
        matches!(&got, reuse::Reuse::Run(why) if why.contains("hidden by the control")),
        "{got:?}"
    );
    let run = run_json("member-trial", 9163);
    let branch = run["head_branch"].as_str().unwrap().strip_prefix("trial/").unwrap().to_string();
    let mut f = serve("member-trial");
    let other = verdict::trial(&mut client(&mut f), "ci.yml", &branch, &"0".repeat(40));
    assert_eq!(other, verdict::Trial::NoRun);
}
