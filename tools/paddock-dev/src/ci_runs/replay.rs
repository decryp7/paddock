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
        match self {
            Touch::Nothing => Some(Touched::default()),
            Touch::Everything => Some(Touched {
                all: true,
                ..Touched::default()
            }),
            Touch::Unread => None,
        }
    }
}

/// A verdict, as the table writes it. `Red` and `NotMeasured` carry a phrase their reason holds.
#[derive(Debug)]
enum V {
    Green(u64),
    GreenWithFixItems(u64, &'static [&'static str]),
    Red(Option<u64>, &'static str),
    NotMeasured(&'static str),
}

/// What a reuse decision is, as the table writes it.
#[derive(Debug)]
enum R {
    Skip(u64),
}

/// What the trial reader says, as the table writes it.
#[derive(Debug)]
enum T {
    Green(u64),
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
/// Recorded on origin/main 167e0473a (scope lines v2, plus run 9167's v3). When the job model's WP-2 (ADR 0123, train u) lands, the
/// v2 shapes read as "not reused" (D2) and 9167 reads as a landing run: update their rows, never loosen them.
const SHAPES: &[Shape] = &[
    Shape {
        name: "green-train",
        workflow: "ci.yml",
        run: 9170,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(None, "a landing run"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Green(9170),
        reuse: None,
        trial: None,
        why: "a train's dispatch ran its own diff's selection (smoke icons, need_whole false): it lands the train but is no full run",
    },
    Shape {
        name: "green-refused-by-old-reader",
        workflow: "ci.yml",
        run: 9167,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::NotMeasured("version 3"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::NotMeasured("version 3"),
        reuse: None,
        trial: None,
        why: "a green run whose scope line is v3 (the expected map): this reader knows v2 and must say not measured, never green; train u's reader flips it",
    },
    Shape {
        name: "red-one-flaky",
        workflow: "ci.yml",
        run: 9168,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(None, "a landing run"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::GreenWithFixItems(
            9168,
            &["paddock-proc usage::tests::the_os_source_lists_the_children_of_one_pid_without_a_table"],
        ),
        reuse: None,
        trial: None,
        why: "one FLAKY test of crate (windows-latest), the run red: untouched code makes it a fix item that lands (ER-12); never a full green",
    },
    Shape {
        name: "push-reused",
        workflow: "ci.yml",
        run: 9170,
        asking: Some(9171),
        touched: Touch::Nothing,
        on_scope: V::Red(None, "a landing run"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Green(9170),
        reuse: Some(R::Skip(9170)),
        trial: None,
        why: "the push 9171 asks about the commit train run 9170 tested; its base (main's run 9074, itself a reuse of 9073) is the landing's, so 9171 skips (ER-49)",
    },
    Shape {
        name: "failed-only-round",
        workflow: "ci.yml",
        run: 9147,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(None, "rerun of an earlier run's failures"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Red(None, "rerun of an earlier run's failures"),
        reuse: None,
        trial: None,
        why: "the green failed-only round (run 9147 reran the one job of 9146): proves nothing for the commit, a landing needs a full green run (ER-10)",
    },
    Shape {
        name: "failed-only-round",
        workflow: "ci.yml",
        run: 9146,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(None, "rerun of an earlier run's failures"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Red(None, "rerun of an earlier run's failures"),
        reuse: None,
        trial: None,
        why: "the red failed-only round (a test failed both tries): a partial run, so never the verdict of its commit either",
    },
    Shape {
        name: "nightly",
        workflow: "nightly.yml",
        run: 9103,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(None, "declared the scope of ci.yml, not of this workflow"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Red(None, "declared the scope of ci.yml, not of this workflow"),
        reuse: None,
        trial: None,
        why: "a scheduled nightly (red, 6 jobs failed) declares ci.yml's scope: it never decides a landing or a tag",
    },
    Shape {
        name: "member-trial",
        workflow: "ci.yml",
        run: 9163,
        asking: None,
        touched: Touch::Nothing,
        on_scope: V::Red(None, "did not cover what a push to main does"),
        on_line: V::Red(None, "plan verdict: complete="),
        on_landing: V::Red(None, "did not cover what a push to main does"),
        reuse: None,
        trial: Some(T::Green(9163)),
        why: "a member's trial ran its affected jobs: green for the trial reader, never a landing proof (jobs eslint and icons were not run)",
    },
];

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
    let mut c = Client::new(f, "https://gitea.invalid", "example/paddock", "t");
    c.backoff_ms = 0;
    c
}

fn run_json(name: &str, run: u64) -> Value {
    serde_json::from_slice(&std::fs::read(dir_of(name).join(format!("run-{run}.json"))).unwrap()).unwrap()
}

fn matches(got: &verdict::Verdict, want: &V) -> bool {
    use verdict::Verdict as G;
    match (got, want) {
        (G::Green { run }, V::Green(w)) => run == w,
        (G::GreenWithFixItems { run, items }, V::GreenWithFixItems(w, tests)) => {
            run == w && items.len() == tests.len() && items.iter().zip(*tests).all(|(i, t)| i.test.contains(t))
        }
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
                (verdict::Trial::Green { run }, T::Green(w)) => *run == *w,
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
    let mut named: Vec<String> = SHAPES.iter().map(|s| s.name.to_string()).collect();
    named.sort();
    named.dedup();
    assert_eq!(
        folders, named,
        "a recording with no row in SHAPES is never replayed; a row with no recording replays nothing"
    );
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
    // Run 9168 (red-one-flaky): the one grade of ER-12 on all three answers the reader gives for what the change touched.
    let shape = |touched| Shape {
        name: "red-one-flaky",
        workflow: "ci.yml",
        run: 9168,
        asking: None,
        touched,
        on_scope: V::Green(0),
        on_line: V::Green(0),
        on_landing: V::Green(0),
        reuse: None,
        trial: None,
        why: "",
    };
    let landing = |t| landing_of(&shape(t));
    let verdict::Verdict::GreenWithFixItems { run, items } = landing(Touch::Nothing) else {
        panic!("untouched is a fix item")
    };
    assert_eq!(run, 9168);
    let lines = fix_items::lines("ci.yml", run, &items);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("- ci.yml: run 9168, job crate (windows-latest) (id 51509): paddock-proc usage::tests::the_os_source_lists"),
        "{lines:?}"
    );
    // The lessons entry is added once, and a second call adds nothing (a retried landing writes it once).
    let sha = run_json("red-one-flaky", 9168)["head_sha"].as_str().unwrap().to_string();
    let once = fix_items::add_to_lessons("# Lessons\n", "ci.yml", run, &sha, &items).expect("added");
    assert!(
        once.contains("full ci.yml run 9168 (167e0473a)") && once.contains("(job 51509, crate (windows-latest);"),
        "{once}"
    );
    assert_eq!(fix_items::add_to_lessons(&once, "ci.yml", run, &sha, &items), None);
    // Reached code: red, naming the test. Unread: not measured, never a pass.
    assert!(matches!(landing(Touch::Everything), verdict::Verdict::Red { run: Some(9168), reason } if reason.contains("a FLAKY test")));
    assert!(
        matches!(landing(Touch::Unread), verdict::Verdict::NotMeasured(why) if why.contains("cannot be read")),
        "{:?}",
        landing(Touch::Unread)
    );
}

#[test]
fn controls_a_push_without_a_base_runs_and_another_members_trial_is_no_run() {
    // The same recordings with the signal hidden go the other way (ER-33): no first-parent line for the push means the landing's base
    // cannot be matched, so the push runs its own; a trial of another head of the branch is not this member's.
    let sha = run_json("push-reused", 9171)["head_sha"].as_str().unwrap().to_string();
    let mut f = serve("push-reused");
    let got = reuse::decide_with_touched(
        &mut client(&mut f),
        "ci.yml",
        &sha,
        Some(9171),
        true,
        &mut |_| Vec::new(),
        &mut |_, _| Some(Touched::default()),
        &mut |_, _| false,
    );
    assert!(
        matches!(&got, reuse::Reuse::Run(why) if why.contains("no complete commit")),
        "{got:?}"
    );
    let run = run_json("member-trial", 9163);
    let branch = run["head_branch"].as_str().unwrap().strip_prefix("trial/").unwrap().to_string();
    let mut f = serve("member-trial");
    let other = verdict::trial(&mut client(&mut f), "ci.yml", &branch, &"0".repeat(40));
    assert_eq!(other, verdict::Trial::NoRun);
}
