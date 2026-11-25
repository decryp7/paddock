// ci-runs-reader: the tests of the dispatch guards, over the fake runs API
use super::*;
use crate::ci_runs::testutil::Fake;
use serde_json::{Value, json};

const HEAD: &str = "1111111111111111111111111111111111111111";
const OLD: &str = "0000000000000000000000000000000000000000";

fn run(id: u64, kind: Option<RunKind>, head: &str, conclusion: &str) -> TrainRun {
    TrainRun {
        id,
        head: head.into(),
        finished: !conclusion.is_empty(),
        conclusion: conclusion.into(),
        kind,
    }
}

const TRAIN: &str = "train/2026-11-24-1";

#[test]
fn a_full_run_after_a_red_full_run_is_refused_until_a_failed_only_round_is_green_on_the_head() {
    // Newest first: round 8870 (since 8868) failed on the head; full run 8868 failed on an older head.
    let mut runs = vec![
        run(8870, Some(RunKind::Since(8868)), HEAD, "failure"),
        run(8868, Some(RunKind::Full), OLD, "failure"),
    ];
    let Finding::Refuse { why, next } = failed_first(&runs, HEAD, "ci.yml", TRAIN) else {
        panic!("{:?}", failed_first(&runs, HEAD, "ci.yml", TRAIN))
    };
    assert!(why.contains("last full run 8868") && why.contains("failure"), "{why}");
    // The exact failed-only command: the newest red run of the chain.
    assert!(
        next.contains(&format!("./dev ci dispatch ci.yml --ref {TRAIN} --input since_run=8870 --max 2h")),
        "{next}"
    );
    assert!(next.contains("--full-anyway"), "{next}");
    // A green round on an older head does not count: the head moved after it.
    runs.insert(0, run(8871, Some(RunKind::Since(8870)), OLD, "success"));
    assert!(matches!(failed_first(&runs, HEAD, "ci.yml", TRAIN), Finding::Refuse { .. }));
    // A round still going is named as the thing to wait for.
    runs.insert(0, run(8872, Some(RunKind::Since(8870)), HEAD, ""));
    let Finding::Refuse { next, .. } = failed_first(&runs, HEAD, "ci.yml", TRAIN) else {
        panic!()
    };
    assert!(next.contains("wait for failed-only round(s) 8872"), "{next}");
    // Positive control: the same round green on the head lets the full run go.
    runs[0] = run(8872, Some(RunKind::Since(8870)), HEAD, "success");
    assert!(matches!(failed_first(&runs, HEAD, "ci.yml", TRAIN), Finding::Pass(_)));
}

#[test]
fn a_red_full_run_without_a_round_names_itself_and_a_green_or_missing_one_passes() {
    let runs = vec![run(8868, Some(RunKind::Full), HEAD, "failure")];
    let Finding::Refuse { next, .. } = failed_first(&runs, HEAD, "ci.yml", TRAIN) else {
        panic!()
    };
    assert!(next.contains("--input since_run=8868"), "{next}");
    assert!(matches!(
        failed_first(&[run(8868, Some(RunKind::Full), HEAD, "success")], HEAD, "ci.yml", TRAIN),
        Finding::Pass(_)
    ));
    assert!(matches!(failed_first(&[], HEAD, "ci.yml", TRAIN), Finding::Pass(_)));
    // A cancelled full run measured nothing: the older one decides. Member runs and parts never count.
    let runs = vec![
        run(8880, Some(RunKind::Full), HEAD, "cancelled"),
        run(8879, Some(RunKind::Member), HEAD, "failure"),
        run(8878, Some(RunKind::Full), OLD, "failure"),
    ];
    let Finding::Refuse { why, .. } = failed_first(&runs, HEAD, "ci.yml", TRAIN) else {
        panic!()
    };
    assert!(why.contains("8878"), "{why}");
    // A finished run whose kind cannot be told may be a newer full run: not measured, never a pass.
    let runs = vec![run(8881, None, HEAD, "success"), run(8878, Some(RunKind::Full), OLD, "failure")];
    assert!(matches!(failed_first(&runs, HEAD, "ci.yml", TRAIN), Finding::NotMeasured(_)));
}

#[test]
fn report_only_prints_what_it_would_refuse_and_goes_on_blocking_refuses_and_an_override_is_logged() {
    let refuse = Finding::Refuse {
        why: "the last full run 8868 is failure".into(),
        next: "./dev ci dispatch ci.yml --ref train/x --input since_run=8868 --max 2h".into(),
    };
    let report = Mode::ReportOnly("no first green use is recorded".into());
    let mut log = Vec::new();
    let d = decide(Guard::FailedFirst, &report, &refuse, None, &mut log);
    let text = String::from_utf8(log).unwrap();
    assert!(
        matches!(&d, Decision::Go(row) if row["result"] == "refuse" && row["mode"] == "report-only"),
        "{d:?}"
    );
    assert!(
        text.starts_with("::warning::") && text.contains("would refuse") && text.contains("since_run=8868"),
        "{text}"
    );
    // Blocking: the dispatch stops with the ::error:: and the exact command.
    let blocking = Mode::Blocking { first_green: 8900 };
    let mut log = Vec::new();
    let d = decide(Guard::FailedFirst, &blocking, &refuse, None, &mut log);
    let text = String::from_utf8(log).unwrap();
    assert!(
        matches!(&d, Decision::Stop { not_measured: false, message } if message.contains("since_run=8868")),
        "{d:?}"
    );
    assert!(text.starts_with("::error::dispatch: refused by guard failed-first"), "{text}");
    // Not measured blocks too, with its own exit.
    let d = decide(
        Guard::GreenMain,
        &blocking,
        &Finding::NotMeasured("no nightly".into()),
        None,
        &mut Vec::new(),
    );
    assert!(matches!(d, Decision::Stop { not_measured: true, .. }));
    // The coordinator's override goes on and logs the reason.
    let mut log = Vec::new();
    let d = decide(Guard::FailedFirst, &blocking, &refuse, Some("test renamed in the fix"), &mut log);
    let text = String::from_utf8(log).unwrap();
    assert!(matches!(&d, Decision::Go(row) if row["overridden"] == "test renamed in the fix"));
    assert!(text.contains("--full-anyway \"test renamed in the fix\""), "{text}");
    // Positive control: a pass goes on in both modes without a warning.
    let mut log = Vec::new();
    assert!(matches!(
        decide(Guard::FailedFirst, &blocking, &Finding::Pass("green".into()), None, &mut log),
        Decision::Go(_)
    ));
    assert!(!String::from_utf8(log).unwrap().contains("::"));
}

#[test]
fn the_register_switches_a_guard_to_blocking_once_and_the_repositorys_starts_report_only() {
    let empty = "# header\n[first_green]\n";
    assert!(matches!(mode_of(Ok(empty), Guard::GreenMain), Mode::ReportOnly(_)));
    let one = with_first_green(empty, Guard::GreenMain, 8901).unwrap();
    assert_eq!(mode_of(Ok(&one), Guard::GreenMain), Mode::Blocking { first_green: 8901 });
    assert!(matches!(mode_of(Ok(&one), Guard::FailedFirst), Mode::ReportOnly(_)));
    assert!(
        with_first_green(&one, Guard::GreenMain, 8902)
            .unwrap_err()
            .contains("already recorded")
    );
    // An unreadable or malformed register never switches a guard on by accident.
    assert!(matches!(mode_of(Err("missing"), Guard::GreenMain), Mode::ReportOnly(_)));
    assert!(matches!(
        mode_of(Ok("[first_green]\ngreen-mian = 1\n"), Guard::GreenMain),
        Mode::ReportOnly(_)
    ));
    assert!(matches!(
        mode_of(Ok("[first_green]\n\"green-main\" = \"x\"\n"), Guard::GreenMain),
        Mode::ReportOnly(_)
    ));
    let real = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(REGISTER)).unwrap();
    for g in [Guard::FailedFirst, Guard::GreenMain] {
        assert!(recorded(&real, g).is_ok(), "{REGISTER} reads");
    }
}

fn apps() -> BTreeSet<String> {
    ["debrief", "dyno", "pitwall"].map(String::from).into()
}

fn main_run(id: u64, workflow: &str, sha: &str, failed: &[&str]) -> MainRun {
    MainRun {
        id,
        workflow: workflow.into(),
        sha: sha.into(),
        failed: failed.iter().map(|s| s.to_string()).collect(),
        created_ms: Some(NOW - 3_600_000.0),
    }
}

/// 2026-11-24T08:00:00Z.
const NOW: f64 = 1_795_507_200_000.0;

#[test]
fn a_red_foundation_or_active_app_on_main_stops_a_train_and_an_inactive_apps_failure_is_its_fix_item() {
    let nightly = main_run(
        8890,
        "nightly.yml",
        HEAD,
        &[
            "everything / app pitwall (ubuntu-latest, e2e-1)",
            "installers (Pitwall, no publish)",
            "everything / crate (windows-latest)",
        ],
    );
    let push = main_run(8891, "ci.yml", HEAD, &["app dyno (macos-latest)"]);
    let mut none_active = |_: &str| Ok(BTreeSet::new());
    let g = green_main(Some(&nightly), Some(&push), &apps(), NOW, &mut none_active);
    let Finding::Refuse { why, next } = &g.finding else {
        panic!("{g:?}")
    };
    assert!(
        why.contains("crate (windows-latest)") && !why.contains("pitwall") && !why.contains("dyno"),
        "{why}"
    );
    assert!(next.contains("--red-main-anyway"), "{next}");
    assert_eq!(g.fix_items["pitwall"].len(), 2);
    assert_eq!(g.fix_items["dyno"], ["ci.yml: app dyno (macos-latest)"]);
    // Only inactive apps red: the train goes, the fix items are printed.
    let nightly = main_run(8890, "nightly.yml", HEAD, &["everything / app pitwall (ubuntu-latest, e2e-1)"]);
    let g = green_main(Some(&nightly), Some(&push), &apps(), NOW, &mut none_active);
    assert!(matches!(g.finding, Finding::Pass(_)), "{g:?}");
    assert_eq!(g.fix_items.len(), 2);
    // Positive control: with dyno active at the push run's commit, its failure stops the train.
    let mut dyno_at_head = |sha: &str| Ok(if sha == OLD { ["dyno".to_string()].into() } else { BTreeSet::new() });
    let push_old = main_run(8891, "ci.yml", OLD, &["app dyno (macos-latest)"]);
    let g = green_main(Some(&nightly), Some(&push_old), &apps(), NOW, &mut dyno_at_head);
    assert!(
        matches!(&g.finding, Finding::Refuse { why, .. } if why.contains("app dyno")),
        "{g:?}"
    );
    // Nothing to read, or an unreadable active set: not measured.
    assert!(matches!(
        green_main(None, Some(&push), &apps(), NOW, &mut none_active).finding,
        Finding::NotMeasured(_)
    ));
    // A nightly from three days ago speaks for an older main (the schedule is off): not measured, though it was green.
    let mut stale = main_run(8800, "nightly.yml", HEAD, &[]);
    stale.created_ms = Some(NOW - 72.0 * 3_600_000.0);
    let g = green_main(
        Some(&stale),
        Some(&main_run(8891, "ci.yml", HEAD, &[])),
        &apps(),
        NOW,
        &mut none_active,
    );
    assert!(matches!(&g.finding, Finding::NotMeasured(why) if why.contains("72 h old")), "{g:?}");
    let mut broken = |_: &str| Err("HTTP 500".to_string());
    assert!(matches!(
        green_main(Some(&nightly), Some(&push), &apps(), NOW, &mut broken).finding,
        Finding::NotMeasured(_)
    ));
}

fn job(id: u64, name: &str, conclusion: &str) -> Value {
    json!({"id": id, "name": name, "conclusion": conclusion, "status": "completed", "runner_id": 1,
           "started_at": "2026-11-24T01:00:00Z", "completed_at": "2026-11-24T01:10:00Z",
           "steps": [{"name": "run", "conclusion": conclusion, "started_at": "2026-11-24T01:00:00Z", "completed_at": "2026-11-24T01:10:00Z"}]})
}

fn caller(id: u64, name: &str) -> Value {
    json!({"id": id, "name": name, "conclusion": "failure", "status": "completed", "runner_id": 0,
           "started_at": "2026-11-24T01:00:00Z", "completed_at": "2026-11-24T01:10:00Z", "steps": []})
}

fn jobs_of(list: Vec<Value>) -> Vec<TimedJob> {
    let mut f = Fake::default();
    let url = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions/runs/1/jobs?limit=50&page=1";
    f.json(url, json!({"jobs": list, "total_count": 0}));
    let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
    c.backoff_ms = 0;
    c.timed_jobs(1).unwrap().unwrap()
}

#[test]
fn a_callers_and_an_aggregates_failure_are_not_counted_twice_and_a_cancelled_job_is_not_measured() {
    let jobs = jobs_of(vec![
        caller(1, "everything"),
        job(2, "everything / crate (windows-latest)", "failure"),
        job(3, "installers verdict (the nightly built them, or it fails)", "failure"),
        job(4, "fuzz (paddock-ipc)", "cancelled"),
        job(5, "C100 (every check runs somewhere)", "success"),
        job(6, "mark the green nightly", "skipped"),
    ]);
    assert_eq!(
        failed_jobs(&jobs),
        [
            "everything / crate (windows-latest)",
            "fuzz (paddock-ipc) (cancelled: not measured)"
        ]
    );
    // Positive control: an aggregate that failed alone is a failure of its own.
    let alone = jobs_of(vec![job(3, "installers verdict (the nightly built them, or it fails)", "failure")]);
    assert_eq!(failed_jobs(&alone).len(), 1);
}

const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";

fn listed(id: u64, path: &str, event: &str, conclusion: &str, title: &str, sha: &str) -> Value {
    json!({"id": id, "path": path, "event": event, "status": "completed", "conclusion": conclusion, "head_sha": sha,
           "display_title": title, "created_at": "2026-11-24T05:00:00Z"})
}

#[test]
fn main_is_read_from_the_newest_nightly_that_measured_something_and_the_newest_push_run() {
    let mut f = Fake::default();
    f.json(
        &format!("{API}/runs?branch=main&limit=50&page=1"),
        json!({"total_count": 5, "workflow_runs": [
            listed(905, "nightly.yml@refs/heads/main", "schedule", "cancelled", "", HEAD),
            listed(904, "nightly.yml@refs/heads/main", "schedule", "success", "", HEAD),
            listed(903, "ci.yml@refs/heads/main", "push", "failure", "fix: x", HEAD),
            listed(902, "ci.yml@refs/heads/main", "push", "success", "fix: y", OLD),
            listed(901, "nightly.yml@refs/heads/main", "schedule", "failure", "", OLD),
        ]}),
    );
    // 904: the gate said "nothing to do" (main unchanged): every other job skipped, so it measured nothing.
    f.json(
        &format!("{API}/runs/904/jobs?limit=50&page=1"),
        json!({"jobs": [job(40, "gate (main changed since the last green nightly?)", "success"), job(41, "everything / crate (ubuntu-latest)", "skipped")]}),
    );
    f.json(
        &format!("{API}/runs/903/jobs?limit=50&page=1"),
        json!({"jobs": [job(30, "app dyno (macos-latest)", "failure")]}),
    );
    f.json(
        &format!("{API}/runs/901/jobs?limit=50&page=1"),
        json!({"jobs": [job(10, "gate (main changed since the last green nightly?)", "success"), job(11, "everything / crate (windows-latest)", "failure")]}),
    );
    let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
    c.backoff_ms = 0;
    let (nightly, push) = read_main(&mut c).unwrap();
    let at = |mut r: MainRun| {
        r.created_ms = super::super::time::ts("2026-11-24T05:00:00Z");
        r
    };
    assert_eq!(
        nightly,
        Some(at(main_run(901, "nightly.yml", OLD, &["everything / crate (windows-latest)"])))
    );
    assert_eq!(push, Some(at(main_run(903, "ci.yml", HEAD, &["app dyno (macos-latest)"]))));
}

#[test]
fn a_trains_runs_are_read_down_to_its_last_full_run_with_the_kind_from_the_token_or_the_plan_log() {
    let mut f = Fake::default();
    let path = format!("ci.yml@refs/heads/{TRAIN}");
    f.json(
        &format!("{API}/runs?event=workflow_dispatch&branch={TRAIN}&limit=50&page=1"),
        json!({"total_count": 5, "workflow_runs": [
            listed(8873, &format!("debrief.yml@refs/heads/{TRAIN}"), "workflow_dispatch", "failure", "debrief [dispatch 0123456789abcdef.full]", HEAD),
            listed(8872, &path, "workflow_dispatch", "failure", "ci [dispatch 0123456789abcdef.since-8868]", HEAD),
            listed(8868, &path, "workflow_dispatch", "failure", "chore(train): merge x", OLD),
            listed(8860, &path, "workflow_dispatch", "success", "ci [dispatch 0123456789abcdef.full]", OLD),
        ]}),
    );
    // 8868 has no token (dispatched before it): its plan job says it was full.
    f.json(
        &format!("{API}/runs/8868/jobs?limit=50&page=1"),
        json!({"jobs": [job(70, "plan (affected projects)", "success")]}),
    );
    f.log(
        70,
        "2026-11-23T10:00:00Z ::notice::...\n2026-11-23T10:00:01Z plan mode: full=true\n",
    );
    let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
    c.backoff_ms = 0;
    let runs = read_train(&mut c, "ci.yml", TRAIN).unwrap();
    assert_eq!(
        runs,
        [
            run(8872, Some(RunKind::Since(8868)), HEAD, "failure"),
            run(8868, Some(RunKind::Full), OLD, "failure")
        ],
        "another workflow's run is left out, and the read stops at the last full run"
    );
    // Positive control: without the plan log the untagged run is unknown, and the guard says not measured.
    f.remove_json(&format!("{API}/runs/8868/jobs?limit=50&page=1"));
    let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
    c.backoff_ms = 0;
    let runs = read_train(&mut c, "ci.yml", TRAIN).unwrap();
    assert_eq!(runs.last().unwrap().kind, None);
    assert!(matches!(failed_first(&runs, HEAD, "ci.yml", TRAIN), Finding::NotMeasured(_)));
}

#[test]
fn only_a_workflow_that_takes_since_run_has_the_failed_first_guard() {
    let read = |name: &str| {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.gitea/workflows")
                .join(name),
        )
        .ok()
    };
    assert!(takes_since_run("ci.yml", &read));
    assert!(takes_since_run("debrief.yml", &read));
    assert!(!takes_since_run("dyno-release.yml", &read));
}

/// A full run may go without a failed-only round when the round would add nothing (user, 2026-11-25): the commits since the red full
/// run only revert members cleanly, or the round would rerun everything anyway (a full run on the same head). The full run before
/// landing stays mandatory: these only skip the round before it.
#[test]
fn a_full_run_goes_without_a_round_after_a_clean_revert_or_when_the_round_would_be_full_anyway() {
    let runs = vec![run(8868, Some(RunKind::Full), OLD, "failure")];
    let refused = failed_first(&runs, HEAD, "ci.yml", TRAIN);
    assert!(matches!(refused, Finding::Refuse { .. }));
    let revert = Ok(Some("abc1234567 reverts the merge def4567890 exactly".to_string()));
    let Finding::Pass(why) = without_round(refused.clone(), revert, Ok(None)) else {
        panic!()
    };
    assert!(
        why.contains("no failed-only round is needed") && why.contains("reverts the merge"),
        "{why}"
    );
    assert!(why.contains("full run before landing"), "{why}");
    let full = Ok(Some("run 8868's plan job failed: nothing after it ran".to_string()));
    assert!(matches!(without_round(refused.clone(), Ok(None), full), Finding::Pass(_)));
    // Positive control: neither applies, or neither can be told: the refusal stands (never a pass on what could not be read).
    assert_eq!(without_round(refused.clone(), Ok(None), Ok(None)), refused);
    assert_eq!(without_round(refused.clone(), Err("no git".into()), Err("no API".into())), refused);
    // A pass or a not-measured finding is left as it is.
    let pass = Finding::Pass("green".into());
    assert_eq!(without_round(pass.clone(), revert_ok(), Ok(None)), pass);
    let nm = Finding::NotMeasured("x".into());
    assert_eq!(without_round(nm.clone(), revert_ok(), Ok(None)), nm);
}

fn revert_ok() -> Result<Option<String>, String> {
    Ok(Some("r".into()))
}

/// A clean revert: the commit undoes exactly the lines a member's merge brought (`git revert -m 1`, or the train's own
/// `revert: pull <branch> (merge <sha>)` without git's trailer); a revert with a change of its own, or of a commit that is not a merge,
/// is not one.
#[test]
fn a_clean_revert_of_a_member_merge_is_told_from_a_revert_with_more_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let o = crate::git_env::git()
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "train"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "base"]);
    let full = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-qb", "member"]);
    std::fs::write(root.join("b.txt"), "member line\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "feat: member"]);
    git(&["checkout", "-q", "train"]);
    git(&["merge", "-q", "--no-ff", "-m", "chore(train): merge member", "member"]);
    let merge = git(&["rev-parse", "HEAD"]);
    // The train's own style: no trailer, the merge named in the subject.
    git(&["rm", "-q", "b.txt"]);
    git(&["commit", "-qm", &format!("revert: pull member (merge {}) from train", &merge[..9])]);
    let head = git(&["rev-parse", "HEAD"]);
    let found = clean_reverts(root, &merge, &head).unwrap();
    assert!(found.as_deref().is_some_and(|w| w.contains(&merge[..10])), "{found:?}");
    // Not clean: the same revert with a change of its own.
    git(&["revert", "--no-edit", "HEAD"]);
    git(&["rm", "-q", "b.txt"]);
    std::fs::write(root.join("a.txt"), "one\nextra\n").unwrap();
    git(&["add", "-A"]);
    git(&[
        "commit",
        "-qm",
        &format!("revert: pull member (merge {}) again, and more", &merge[..9]),
    ]);
    let dirty = git(&["rev-parse", "HEAD"]);
    assert_eq!(clean_reverts(root, &format!("{dirty}~1"), &dirty).unwrap(), None);
    // Not a member's merge: a revert of a plain commit.
    assert_eq!(
        clean_reverts(root, &full, &full).unwrap(),
        None,
        "nothing since the red run: no allowance"
    );
    // Every commit since the red run must be a clean revert: a fix beside one is not.
    assert_eq!(clean_reverts(root, &merge, &dirty).unwrap(), None);
}
