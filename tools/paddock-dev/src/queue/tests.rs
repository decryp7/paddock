//! The queue's tick against an in-memory Gitea (the ledger's fake), real git in temporary repositories (a bare origin, the
//! queue's checkout and an author's checkout) and a scripted CI and gates. No real server, no network (ER-14).

use super::ports::*;
use super::real::{self, GitTrain, GiteaLedger, MergeCheckFn};
use super::tick::{BuildOutcome, apply_build, build_phase, inputs_for};
use super::tick::{Env, Ports, Tick, tick};
use crate::ci_runs::verdict::Verdict;
use crate::pr::fake::FakeGitea;
use crate::pr::gitea::{Api, Pull};
use crate::pr::labels;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = paddock_proc::command("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A bare origin with `main`, the queue's checkout and an author's checkout.
struct Repos {
    _tmp: tempfile::TempDir,
    origin: PathBuf,
    queue: PathBuf,
    author: PathBuf,
}

impl Repos {
    fn new() -> Repos {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin.git");
        let (queue, author) = (tmp.path().join("queue"), tmp.path().join("author"));
        paddock_proc::command("git")
            .args(["init", "--quiet", "--bare", "-b", "main"])
            .arg(&origin)
            .output()
            .unwrap();
        let o = origin.to_str().unwrap();
        for d in [&queue, &author] {
            paddock_proc::command("git").args(["clone", "--quiet", o]).arg(d).output().unwrap();
        }
        std::fs::create_dir_all(author.join("crates/x")).unwrap();
        std::fs::write(author.join("crates/x/lib.rs"), "fn x() {}\n").unwrap();
        std::fs::write(author.join("README.md"), "readme\n").unwrap();
        git(&author, &["checkout", "--quiet", "-B", "main"]);
        git(&author, &["add", "."]);
        git(&author, &["commit", "--quiet", "-m", "chore: start"]);
        git(&author, &["push", "--quiet", "origin", "main"]);
        Repos {
            _tmp: tmp,
            origin,
            queue,
            author,
        }
    }

    /// A branch from main with one commit that adds `file`; returns its head.
    fn branch(&self, name: &str, file: &str) -> String {
        git(&self.author, &["checkout", "--quiet", "-B", name, "origin/main"]);
        let path = self.author.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{name}\n")).unwrap();
        git(&self.author, &["add", "."]);
        git(&self.author, &["commit", "--quiet", "-m", &format!("feat: {name}")]);
        git(&self.author, &["push", "--quiet", "origin", name]);
        git(&self.author, &["rev-parse", "HEAD"])
    }

    fn origin_ref(&self, r: &str) -> String {
        git(&self.origin, &["rev-parse", r])
    }

    fn main_has(&self, file: &str) -> bool {
        paddock_proc::command("git")
            .arg("-C")
            .arg(&self.origin)
            .args(["cat-file", "-e", &format!("main:{file}")])
            .output()
            .unwrap()
            .status
            .success()
    }
}

/// The real git adapter with the planner's answer replaced by a table (the planner needs the whole repository).
struct TestTrain {
    inner: GitTrain,
    reach: BTreeMap<String, BTreeSet<String>>,
    workflows: Vec<String>,
}

impl Train for TestTrain {
    fn fetch(&mut self, b: &[String]) -> Result<(), String> {
        self.inner.fetch(b)
    }
    fn main_head(&mut self) -> Result<String, String> {
        self.inner.main_head()
    }
    fn train_head(&mut self) -> Result<Option<String>, String> {
        self.inner.train_head()
    }
    fn marker(&mut self, h: &str) -> Result<Option<Marker>, String> {
        self.inner.marker(h)
    }
    fn contains(&mut self, a: &str, d: &str) -> Result<bool, String> {
        self.inner.contains(a, d)
    }
    fn build(&mut self, m: &str, p: &[Pull], k: &Marker) -> Result<Built, String> {
        self.inner.build(m, p, k)
    }
    fn pr_files(&mut self, m: &str, h: &str) -> Result<Vec<String>, String> {
        self.inner.pr_files(m, h)
    }
    fn affected(&mut self, _main: &str, head: &str) -> Result<BTreeSet<String>, String> {
        Ok(self.reach.get(head).cloned().unwrap_or_default())
    }
    fn reached_workflows(&mut self, _m: &str, _h: &str) -> Result<Vec<String>, String> {
        Ok(self.workflows.clone())
    }
    fn checkout(&mut self, rev: &str) -> Result<(), String> {
        self.inner.checkout(rev)
    }
    fn push_train(&mut self, h: &str, o: Option<&str>) -> Result<(), String> {
        self.inner.push_train(h, o)
    }
    fn land(&mut self, h: &str) -> Result<(), String> {
        self.inner.land(h)
    }
}

#[derive(Default)]
struct FakeGates {
    script: Vec<GateOutcome>,
    ran: usize,
}

impl Gates for FakeGates {
    fn run(&mut self, _m: &str, _h: &str) -> GateOutcome {
        self.ran += 1;
        if self.script.is_empty() {
            GateOutcome::Green
        } else {
            self.script.remove(0)
        }
    }
}

struct FakeCi {
    runs: BTreeMap<(String, String), Vec<CiRun>>,
    verdicts: BTreeMap<(String, String), Verdict>,
    workflows_dispatched: Vec<String>,
    busy: Option<String>,
    dispatched: Vec<Vec<(String, String)>>,
    failures: BTreeMap<u64, Vec<Failure>>,
}

impl FakeCi {
    fn new() -> FakeCi {
        FakeCi {
            runs: BTreeMap::new(),
            verdicts: BTreeMap::new(),
            workflows_dispatched: Vec::new(),
            busy: None,
            dispatched: Vec::new(),
            failures: BTreeMap::new(),
        }
    }
}

impl Ci for FakeCi {
    fn runs(&mut self, wf: &str, head: &str) -> Result<Vec<CiRun>, String> {
        Ok(self.runs.get(&(wf.to_string(), head.to_string())).cloned().unwrap_or_default())
    }
    fn verdict(&mut self, wf: &str, head: &str) -> Verdict {
        self.verdicts
            .get(&(wf.to_string(), head.to_string()))
            .cloned()
            .unwrap_or(Verdict::NotMeasured("no verdict scripted".into()))
    }
    fn dispatch(&mut self, wf: &str, inputs: &[(String, String)]) -> Result<Sent, String> {
        if let Some(who) = &self.busy {
            return Ok(Sent::Busy(who.clone()));
        }
        self.workflows_dispatched.push(wf.to_string());
        self.dispatched.push(inputs.to_vec());
        Ok(Sent::Yes)
    }
    fn failures(&mut self, run: u64) -> Result<Vec<Failure>, String> {
        self.failures
            .get(&run)
            .cloned()
            .ok_or_else(|| format!("no failures scripted for {run}"))
    }
}

/// A ledger that lets a test change what a later read of the pull requests sees (a hold set, or a head pushed, while a tick was
/// running).
struct Hooked<'a> {
    inner: GiteaLedger<'a>,
    reads: usize,
    on_read: Option<fn(usize, &mut Vec<Pull>)>,
}

impl Ledger for Hooked<'_> {
    fn open_pulls(&mut self) -> Result<Vec<Pull>, String> {
        self.reads += 1;
        let mut p = self.inner.open_pulls()?;
        if let Some(f) = self.on_read {
            f(self.reads, &mut p);
        }
        Ok(p)
    }
    fn relabel(&mut self, n: u64, c: &[&str]) -> Result<(), String> {
        self.inner.relabel(n, c)
    }
    fn comment(&mut self, n: u64, t: &str) -> Result<(), String> {
        self.inner.comment(n, t)
    }
    fn approved_head(&mut self, n: u64, h: &str) -> Result<bool, String> {
        self.inner.approved_head(n, h)
    }
}

struct World {
    repos: Repos,
    gitea: FakeGitea,
    known: Vec<labels::LabelSpec>,
    reach: BTreeMap<String, BTreeSet<String>>,
    gates: FakeGates,
    ci: FakeCi,
    workflows: Vec<String>,
    merge_check: Option<MergeCheckFn>,
    on_read: Option<fn(usize, &mut Vec<Pull>)>,
}

impl World {
    fn new() -> World {
        let known = labels::parse(include_str!("../../../../.gitea/labels.toml"), &[]).unwrap();
        let mut gitea = FakeGitea::default();
        for k in &known {
            gitea.label(&k.name, "#ededed", k.exclusive_scope().is_some());
        }
        World {
            repos: Repos::new(),
            gitea,
            known,
            reach: BTreeMap::new(),
            gates: FakeGates::default(),
            ci: FakeCi::new(),
            workflows: vec!["ci.yml".into()],
            merge_check: None,
            on_read: None,
        }
    }

    /// An approved pull request of a new branch that adds `file`, reaching `projects`.
    fn approved(&mut self, number: u64, file: &str, projects: &[&str]) -> String {
        let name = format!("feat/pr{number}");
        let head = self.repos.branch(&name, file);
        self.gitea.pull(number, &name, &head, "open", None, &["state/approved"]);
        self.approve(number, &head);
        self.reach.insert(head.clone(), projects.iter().map(|s| s.to_string()).collect());
        head
    }

    /// An approving review of `head` by a reviewer.
    fn approve(&mut self, number: u64, head: &str) {
        self.gitea.reviews.push((
            number,
            serde_json::json!({"user": {"login": "reviewer"}, "state": "APPROVED", "commit_id": head, "dismissed": false}),
        ));
    }

    fn labels_of(&self, number: u64) -> Vec<String> {
        let p = self.gitea.pulls.iter().find(|p| p["number"] == number).unwrap();
        p["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap().to_string())
            .collect()
    }

    fn comments_of(&self, number: u64) -> Vec<String> {
        self.gitea
            .comments
            .iter()
            .filter(|(n, _)| *n == number)
            .map(|(_, t)| t.clone())
            .collect()
    }

    fn train_head(&self) -> String {
        self.repos.origin_ref("refs/heads/train/queue")
    }

    fn tick_at(&mut self, hour: u32) -> Tick {
        self.tick_hour(Some(hour))
    }

    fn tick_hour(&mut self, hour: Option<u32>) -> Tick {
        self.try_tick_hour(hour).unwrap()
    }

    fn try_tick_hour(&mut self, hour: Option<u32>) -> Result<Tick, String> {
        let mut api_http = std::mem::take(&mut self.gitea);
        let result = {
            let mut ledger = Hooked {
                inner: GiteaLedger {
                    api: Api::new(&mut api_http, FakeGitea::SERVER, FakeGitea::REPO, "t"),
                    known: self.known.clone(),
                },
                reads: 0,
                on_read: self.on_read,
            };
            let mut train = TestTrain {
                inner: {
                    let mut g = GitTrain::new(self.repos.queue.clone(), None);
                    if let Some(f) = self.merge_check {
                        g.merge_check = f;
                    }
                    g
                },
                reach: self.reach.clone(),
                workflows: self.workflows.clone(),
            };
            let mut ports = Ports {
                ledger: &mut ledger,
                train: &mut train,
                gates: &mut self.gates,
                ci: &mut self.ci,
            };
            tick(&mut ports, &Env { hour })
        };
        self.gitea = api_http;
        result
    }

    fn tick(&mut self) -> Tick {
        self.tick_at(12)
    }

    fn run(&mut self, head: &str, id: u64, state: RunState) {
        self.run_of("ci.yml", head, id, state);
    }

    fn run_of(&mut self, wf: &str, head: &str, id: u64, state: RunState) {
        self.ci.runs.insert((wf.to_string(), head.to_string()), vec![CiRun { id, state }]);
    }

    fn green(&mut self, wf: &str, head: &str, id: u64) {
        self.run_of(wf, head, id, RunState::Success);
        self.ci
            .verdicts
            .insert((wf.to_string(), head.to_string()), Verdict::Green { run: id });
    }
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn one_green_pull_request_is_built_dispatched_and_landed() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    let Tick::Built { head, prs, .. } = w.tick() else {
        panic!("a train is built first")
    };
    assert_eq!(prs, vec![1]);
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
    assert_eq!(w.train_head(), head, "the train branch is pushed");
    assert_eq!(w.gates.ran, 1, "the local gates ran before any runner was used");
    // The next tick dispatches the affected run against main.
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(w.ci.dispatched, vec![vec![("base".to_string(), "origin/main".to_string())]]);
    // The run is going: wait.
    w.run(&head, 8001, RunState::Running);
    assert_eq!(w.tick(), Tick::Waiting(8001));
    // A full green run lands it by a fast-forward of the tested head.
    w.run(&head, 8001, RunState::Success);
    w.ci.verdicts
        .insert(("ci.yml".to_string(), head.clone()), Verdict::Green { run: 8001 });
    assert_eq!(
        w.tick(),
        Tick::Landed {
            head: head.clone(),
            prs: vec![1]
        }
    );
    assert_eq!(w.repos.origin_ref("refs/heads/main"), head);
    assert!(w.repos.main_has("crates/x/new.rs"));
    assert_eq!(w.labels_of(1), vec!["state/landed"]);
    assert!(w.comments_of(1).iter().any(|c| c.contains("landed on main")));
}

#[test]
fn a_green_partial_run_gets_the_full_run_before_landing() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    w.tick();
    w.run(&head, 8001, RunState::Success);
    w.ci.verdicts.insert(
        ("ci.yml".to_string(), head.clone()),
        Verdict::Red {
            run: None,
            reason: "only partial runs".into(),
        },
    );
    let before = w.repos.origin_ref("refs/heads/main");
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(
        w.ci.dispatched.last().unwrap(),
        &Vec::<(String, String)>::new(),
        "the full run has no base and no since_run"
    );
    assert_eq!(
        w.repos.origin_ref("refs/heads/main"),
        before,
        "nothing landed on a partial run (ER-8)"
    );
}

#[test]
fn two_pull_requests_one_breaks_it_is_dropped_with_a_comment_and_the_other_retries_and_lands() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    w.approved(2, "ui/new.ts", &["ui"]);
    let Tick::Built { head, prs, .. } = w.tick() else { panic!() };
    assert_eq!(prs, vec![1, 2]);
    w.tick();
    w.run(&head, 8001, RunState::Failed("failure".into()));
    w.ci.failures.insert(
        8001,
        vec![Failure {
            what: "crate (ubuntu-latest): x tests::it_breaks".into(),
            projects: set(&["x"]),
            blame: Blame::New,
        }],
    );
    let Tick::Dropped {
        dropped,
        rebuilt: Some(new_head),
    } = w.tick()
    else {
        panic!("the suspect is dropped and the rest rebuilt")
    };
    assert_eq!(dropped, vec![1]);
    assert_eq!(w.labels_of(1), vec!["state/fixes"]);
    let c = w.comments_of(1).join("\n");
    assert!(
        c.contains("x tests::it_breaks") && c.contains("run 8001"),
        "the comment names the failed test and the run: {c}"
    );
    assert_eq!(w.labels_of(2), vec!["state/in-train"], "the other pull request stays in the train");
    assert_eq!(w.train_head(), new_head);
    assert_ne!(new_head, head);
    // The retry reruns only what failed.
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(
        w.ci.dispatched.last().unwrap(),
        &vec![
            ("base".to_string(), "origin/main".to_string()),
            ("since_run".to_string(), "8001".to_string())
        ]
    );
    w.run(&new_head, 8002, RunState::Success);
    w.ci.verdicts
        .insert(("ci.yml".to_string(), new_head.clone()), Verdict::Green { run: 8002 });
    assert_eq!(
        w.tick(),
        Tick::Landed {
            head: new_head,
            prs: vec![2]
        }
    );
    assert!(w.repos.main_has("ui/new.ts"));
    assert!(!w.repos.main_has("crates/x/new.rs"), "the dropped pull request did not land");
}

#[test]
fn several_suspects_are_all_dropped_and_a_failure_that_also_fails_on_main_blames_nobody() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "crates/x/b.rs", &["x"]);
    w.approved(3, "ui/c.ts", &["ui"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    w.tick();
    w.run(&head, 9001, RunState::Failed("failure".into()));
    w.ci.failures.insert(
        9001,
        vec![
            Failure {
                what: "x t".into(),
                projects: set(&["x"]),
                blame: Blame::New,
            },
            Failure {
                what: "ui main-red".into(),
                projects: set(&["ui"]),
                blame: Blame::OnMain,
            },
        ],
    );
    let Tick::Dropped { dropped, .. } = w.tick() else { panic!() };
    assert_eq!(
        dropped,
        vec![1, 2],
        "Q3: every suspect, no bisection; #3 reaches only the failure that is main's"
    );
    assert_eq!(w.labels_of(3), vec!["state/in-train"]);
    // Only failures of main's own: the train stops, nobody is blamed.
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    w.tick();
    w.run(&head, 9002, RunState::Failed("failure".into()));
    w.ci.failures.insert(
        9002,
        vec![Failure {
            what: "x t".into(),
            projects: set(&["x"]),
            blame: Blame::OnMain,
        }],
    );
    assert!(matches!(w.tick(), Tick::Stuck { measured: true, .. }));
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
}

#[test]
fn a_hold_label_stops_the_tick_before_anything_changes() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    w.approved(2, "ui/new.ts", &["ui"]);
    w.gitea.pulls.iter_mut().find(|p| p["number"] == 2).unwrap()["labels"] = serde_json::json!([{"name": "queue/hold"}]);
    assert_eq!(w.tick(), Tick::Held(2));
    assert_eq!(w.labels_of(1), vec!["state/approved"]);
    assert!(w.gitea.comments.is_empty());
    assert_eq!(w.gates.ran, 0);
    let train = paddock_proc::command("git")
        .arg("-C")
        .arg(&w.repos.origin)
        .args(["rev-parse", "--verify", "--quiet", "refs/heads/train/queue"])
        .output()
        .unwrap();
    assert!(!train.status.success(), "no train branch was pushed");
}

#[test]
fn the_backup_window_dispatches_nothing_until_it_ends() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    w.tick();
    assert_eq!(w.tick_at(0), Tick::Window);
    assert_eq!(w.tick_at(3), Tick::Window);
    assert!(w.ci.dispatched.is_empty());
    assert!(matches!(w.tick_at(4), Tick::Dispatched { .. }));
    assert_eq!(w.ci.dispatched.len(), 1);
}

#[test]
fn a_busy_runner_means_no_dispatch_and_the_next_tick_looks_again() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    w.tick();
    w.ci.busy = Some("run 7959 on d7-mac-mini".into());
    assert_eq!(w.tick(), Tick::RunnerBusy("run 7959 on d7-mac-mini".into()));
    assert!(w.ci.dispatched.is_empty());
    w.ci.busy = None;
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
}

#[test]
fn a_verdict_that_is_not_measured_never_lands() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    w.tick();
    w.run(&head, 8001, RunState::Success);
    w.ci.verdicts.insert(
        ("ci.yml".to_string(), head.clone()),
        Verdict::NotMeasured("the plan job's log could not be read".into()),
    );
    let before = w.repos.origin_ref("refs/heads/main");
    let t = w.tick();
    assert!(matches!(t, Tick::Stuck { measured: false, .. }), "{t:?}");
    assert!(t.text().starts_with("::error::"));
    assert_eq!(w.repos.origin_ref("refs/heads/main"), before);
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
}

#[test]
fn a_conflict_drops_the_later_pull_request_at_once_and_the_train_goes_on() {
    let mut w = World::new();
    w.approved(1, "crates/x/same.rs", &["x"]);
    w.approved(2, "crates/x/same.rs", &["x"]);
    let Tick::Built { prs, dropped, .. } = w.tick() else { panic!() };
    assert_eq!(prs, vec![1]);
    assert_eq!(dropped.len(), 1);
    assert_eq!(w.labels_of(2), vec!["state/fixes"]);
    assert!(
        w.comments_of(2).join("\n").contains("crates/x/same.rs"),
        "the comment names the conflicting file"
    );
}

#[test]
fn a_gate_that_fails_on_the_train_drops_its_culprit_before_any_runner_is_used() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    w.gates.script = vec![GateOutcome::Red(vec![GateFailure {
        name: "conformance".into(),
        detail: "C99 in ui/b.ts".into(),
        files: vec!["ui/b.ts".into()],
    }])];
    let Tick::Built { prs, dropped, .. } = w.tick() else { panic!() };
    assert_eq!(prs, vec![1]);
    assert_eq!(dropped.iter().map(|(n, _)| *n).collect::<Vec<_>>(), vec![2]);
    assert_eq!(w.labels_of(2), vec!["state/fixes"]);
    assert!(w.comments_of(2).join("\n").contains("C99 in ui/b.ts"));
    assert!(w.ci.dispatched.is_empty());
    assert_eq!(
        w.gates.ran, 3,
        "the gates ran on the train, on main to tell main's failures apart, and on the rebuilt train"
    );
}

#[test]
fn a_gate_that_could_not_look_is_not_a_pass() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.gates.script = vec![GateOutcome::NotMeasured("node is missing".into())];
    assert!(matches!(w.tick(), Tick::Stuck { measured: false, .. }));
    assert_eq!(
        w.labels_of(1),
        vec!["state/approved", "queue/hold"],
        "the queue stopped itself with a hold; nothing was taken"
    );
}

#[test]
fn a_tick_that_ended_after_the_push_to_main_finishes_the_labels_next_time() {
    let mut w = World::new();
    w.approved(1, "crates/x/new.rs", &["x"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    // main is fast-forwarded (by the tick that then died), the labels are not set.
    git(&w.repos.author, &["fetch", "--quiet", "origin"]);
    git(&w.repos.author, &["push", "--quiet", "origin", &format!("{head}:refs/heads/main")]);
    assert_eq!(w.tick(), Tick::Landed { head, prs: vec![1] });
    assert_eq!(w.labels_of(1), vec!["state/landed"]);
}

#[test]
fn a_moved_main_rebuilds_the_train_and_a_moved_head_sends_the_pull_request_back() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    let Tick::Built { head: old, .. } = w.tick() else { panic!() };
    // Someone lands a commit on main by hand, and the author of #2 pushes again.
    w.repos.branch("hand", "docs/hand.md");
    git(&w.repos.author, &["push", "--quiet", "origin", "hand:main"]);
    let new2 = {
        git(&w.repos.author, &["checkout", "--quiet", "feat/pr2"]);
        std::fs::write(w.repos.author.join("ui/more.ts"), "more\n").unwrap();
        git(&w.repos.author, &["add", "."]);
        git(&w.repos.author, &["commit", "--quiet", "-m", "feat: more"]);
        git(&w.repos.author, &["push", "--quiet", "origin", "feat/pr2"]);
        git(&w.repos.author, &["rev-parse", "HEAD"])
    };
    w.gitea.pulls.iter_mut().find(|p| p["number"] == 2).unwrap()["head"]["sha"] = serde_json::json!(new2);
    let Tick::Built { head, prs, .. } = w.tick() else {
        panic!("rebuilt")
    };
    assert_ne!(head, old);
    assert_eq!(prs, vec![1], "#2's head moved after it entered the train");
    assert_eq!(w.labels_of(2), vec!["state/built"]);
    assert!(w.repos.main_has("docs/hand.md"));
}

#[test]
fn nothing_approved_is_idle_and_a_tick_twice_changes_nothing() {
    let mut w = World::new();
    assert!(matches!(w.tick(), Tick::Idle(_)));
    w.approved(1, "crates/x/a.rs", &["x"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    w.tick();
    w.run(&head, 5, RunState::Running);
    assert_eq!(w.tick(), Tick::Waiting(5));
    assert_eq!(w.tick(), Tick::Waiting(5));
    assert_eq!(w.train_head(), head, "a waiting tick never rebuilds the train");
    assert_eq!(w.ci.dispatched.len(), 1);
}

#[test]
fn a_batch_is_at_most_six() {
    let mut w = World::new();
    for n in 1..=8 {
        w.approved(n, &format!("docs/p{n}.md"), &["docs"]);
    }
    let Tick::Built { prs, .. } = w.tick() else { panic!() };
    assert_eq!(prs, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(w.labels_of(7), vec!["state/approved"]);
}

#[test]
fn the_marker_round_trips_and_rows_name_their_projects() {
    let m = Marker {
        prs: vec![3, 12],
        round: 2,
        since_run: Some(8001),
        since_workflow: Some("debrief.yml".into()),
    };
    let text = "chore(queue): train round 2\n\nQueue-Prs: 12,3\nQueue-Round: 2\nQueue-Since-Run: 8001\nQueue-Since-Workflow: debrief.yml\n";
    assert_eq!(real::parse_marker(text), Some(m));
    assert_eq!(real::parse_marker("feat: nothing\n"), None);
    use crate::ci_runs::triage::Test;
    assert_eq!(
        real::projects_of_row("crate (ubuntu-latest)", &Test::Named("paddock-service per_machine::t".into())),
        set(&["paddockservice"])
    );
    assert_eq!(real::projects_of_row("app dyno (macos-latest)", &Test::WholeJob), set(&["dyno"]));
    assert_eq!(real::projects_of_row("ui", &Test::WholeJob), set(&["ui"]));
    let plan = serde_json::json!({"all": false, "crates": true, "ui": false, "paddockDev": true, "apps": ["dyno"], "packages": {"crates/paddock-service": ["paddock-service"]}});
    assert_eq!(
        real::projects_of_plan(&plan),
        set(&["crate", "paddockdev", "dyno", "paddockservice"])
    );
    assert_eq!(real::projects_of_plan(&serde_json::json!({"all": true})), set(&["*"]));
}

#[test]
fn the_local_gates_include_the_checks_only_a_later_ci_gate_saw_on_a_merged_tree() {
    let g = real::gate_commands(
        "dev",
        "node conformance/bin/paddock-conformance.mjs --app paddock --only C06",
        "a..b",
    );
    let find = |needle: &str| {
        g.iter()
            .find(|c| c.args.iter().any(|a| a == needle))
            .unwrap_or_else(|| panic!("no gate runs {needle}"))
    };
    assert_eq!(find("hakari").args, ["hakari", "generate", "--diff"]);
    assert_eq!(find("hakari").dir, "crates");
    let publish = find("scripts/publish.mjs");
    assert_eq!(publish.args, ["scripts/publish.mjs", "--skip-build", "--offline"]);
    assert!(g.iter().any(|c| c.name == "gates check") && g.iter().any(|c| c.args.contains(&"commit-lint".to_string())));
}

#[test]
fn a_gate_that_cannot_start_is_not_measured_not_a_failure_of_a_pull_request() {
    let g = real::GateCmd {
        name: "x",
        program: "paddock-no-such-program".into(),
        args: vec![],
        dir: ".",
    };
    let tmp = tempfile::tempdir().unwrap();
    assert!(
        real::LocalGates {
            root: tmp.path().to_path_buf()
        }
        .run_one(&g)
        .is_err()
    );
}

fn failure(what: &str, projects: &[&str], blame: Blame) -> Failure {
    Failure {
        what: what.into(),
        projects: set(projects),
        blame,
    }
}

/// A train with these pull requests approved, built and dispatched; returns its head.
fn dispatched_train(w: &mut World, prs: &[(u64, &str, &[&str])]) -> String {
    for (n, file, projects) in prs {
        w.approved(*n, file, projects);
    }
    let Tick::Built { head, .. } = w.tick() else { panic!("built") };
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    head
}

fn set_labels(w: &mut World, number: u64, names: &[&str]) {
    let l: Vec<serde_json::Value> = names.iter().map(|n| serde_json::json!({ "name": n })).collect();
    w.gitea.pulls.iter_mut().find(|p| p["number"] == number).unwrap()["labels"] = serde_json::Value::Array(l);
}

// ---- approval is tied to a commit ------------------------------------------------------------------------------------------------

#[test]
fn a_label_without_a_review_is_not_an_approval() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.gitea.reviews.clear();
    let t = w.tick();
    assert!(matches!(t, Tick::Idle(_)), "{t:?}");
    assert_eq!(w.labels_of(1), vec!["state/built"]);
    assert!(w.comments_of(1).join("\n").contains("no approving review"));
}

#[test]
fn a_head_that_moves_after_approval_needs_a_new_approval() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    // The author pushes again: the review is of the old head.
    git(&w.repos.author, &["checkout", "--quiet", "feat/pr1"]);
    std::fs::write(w.repos.author.join("crates/x/more.rs"), "more\n").unwrap();
    git(&w.repos.author, &["add", "."]);
    git(&w.repos.author, &["commit", "--quiet", "-m", "feat: more"]);
    git(&w.repos.author, &["push", "--quiet", "origin", "feat/pr1"]);
    let new = git(&w.repos.author, &["rev-parse", "HEAD"]);
    w.gitea.pulls.iter_mut().find(|p| p["number"] == 1).unwrap()["head"]["sha"] = serde_json::json!(new);
    assert!(matches!(w.tick(), Tick::Idle(_)));
    assert_eq!(w.labels_of(1), vec!["state/built"]);
    assert!(w.gates.ran == 0 && w.gitea.comments.len() == 1);
    // Approving the new head puts it back in the queue.
    set_labels(&mut w, 1, &["state/approved"]);
    w.approve(1, &new);
    assert!(matches!(w.tick(), Tick::Built { .. }));
}

#[test]
fn a_hand_set_in_train_label_never_joins_a_train() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    w.gitea.reviews.retain(|(n, _)| *n != 1);
    set_labels(&mut w, 1, &["state/in-train"]);
    let Tick::Built { prs, .. } = w.tick() else { panic!() };
    assert_eq!(prs, vec![2], "#1 has no approval of its head");
    assert_eq!(w.labels_of(1), vec!["state/built"]);
}

#[test]
fn only_an_approval_of_the_exact_head_counts_and_a_later_change_request_or_dismissal_ends_it() {
    use crate::pr::gitea::{Review, approved_at};
    let r = |who: &str, state: &str, commit: &str, dismissed: bool| Review {
        reviewer: who.into(),
        state: state.into(),
        commit_id: commit.into(),
        dismissed,
    };
    assert!(approved_at(&[r("a", "APPROVED", "h1", false)], "h1"));
    assert!(
        !approved_at(&[r("a", "APPROVED", "h0", false)], "h1"),
        "an approval of an older head"
    );
    assert!(!approved_at(&[r("a", "APPROVED", "h1", true)], "h1"), "dismissed");
    assert!(
        !approved_at(&[r("a", "COMMENT", "h1", false)], "h1"),
        "a comment is not an approval"
    );
    assert!(
        !approved_at(&[r("a", "APPROVED", "h1", false), r("a", "REQUEST_CHANGES", "h1", false)], "h1"),
        "the reviewer changed their mind"
    );
    assert!(
        !approved_at(&[r("a", "APPROVED", "h1", false), r("b", "REQUEST_CHANGES", "h1", false)], "h1"),
        "another reviewer asks for changes"
    );
    assert!(approved_at(
        &[r("a", "REQUEST_CHANGES", "h1", false), r("a", "APPROVED", "h1", false)],
        "h1"
    ));
    assert!(!approved_at(&[], "h1") && !approved_at(&[r("a", "APPROVED", "", false)], ""));
}

// ---- debts and every reached workflow --------------------------------------------------------------------------------------------

#[test]
fn a_pull_request_that_owes_windows_tests_or_a_rule_54_bundle_is_not_taken() {
    for debt in ["owes/windows-tests", "owes/rule54-bundle"] {
        let mut w = World::new();
        w.approved(1, "crates/x/a.rs", &["x"]);
        set_labels(&mut w, 1, &["state/approved", debt]);
        assert!(matches!(w.tick(), Tick::Idle(_)));
        assert!(w.labels_of(1).contains(&"state/built".to_string()) && w.labels_of(1).contains(&debt.to_string()));
        assert!(w.comments_of(1).join("\n").contains(debt));
        assert_eq!(w.gates.ran, 0);
    }
}

#[test]
fn every_reached_workflow_must_be_full_green_before_main_moves() {
    let mut w = World::new();
    w.workflows = vec!["ci.yml".into(), "debrief.yml".into()];
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    assert_eq!(w.ci.workflows_dispatched, vec!["ci.yml"]);
    let before = w.repos.origin_ref("refs/heads/main");
    // ci.yml is green; debrief.yml has not run: it is dispatched, and main does not move.
    w.green("ci.yml", &head, 8001);
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(w.ci.workflows_dispatched, vec!["ci.yml", "debrief.yml"]);
    assert_eq!(w.repos.origin_ref("refs/heads/main"), before);
    // debrief.yml green but only partially: its full run (bundles) is dispatched, not a landing.
    w.run_of("debrief.yml", &head, 8002, RunState::Success);
    w.ci.verdicts.insert(
        ("debrief.yml".into(), head.clone()),
        Verdict::Red {
            run: None,
            reason: "partial".into(),
        },
    );
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(w.ci.dispatched.last().unwrap(), &vec![("bundles".to_string(), "true".to_string())]);
    assert_eq!(w.repos.origin_ref("refs/heads/main"), before);
    // Not measured for the second workflow: never lands.
    w.ci.verdicts
        .insert(("debrief.yml".into(), head.clone()), Verdict::NotMeasured("log".into()));
    assert!(matches!(w.tick(), Tick::Stuck { measured: false, .. }));
    assert_eq!(w.repos.origin_ref("refs/heads/main"), before);
    // Both full green: it lands.
    w.green("debrief.yml", &head, 8003);
    assert!(matches!(w.tick(), Tick::Landed { .. }));
    assert_eq!(w.repos.origin_ref("refs/heads/main"), head);
}

#[test]
fn a_failure_in_an_app_workflow_drops_its_suspect_and_the_retry_reruns_only_that_workflow() {
    let mut w = World::new();
    w.workflows = vec!["ci.yml".into(), "debrief.yml".into()];
    let head = dispatched_train(&mut w, &[(1, "apps/debrief/a.rs", &["debrief"]), (2, "ui/b.ts", &["ui"])]);
    w.green("ci.yml", &head, 8001);
    w.tick();
    w.run_of("debrief.yml", &head, 8002, RunState::Failed("failure".into()));
    w.ci.failures
        .insert(8002, vec![failure("rust-test: debrief t", &["debrief"], Blame::New)]);
    let Tick::Dropped {
        dropped,
        rebuilt: Some(new),
    } = w.tick()
    else {
        panic!()
    };
    assert_eq!(dropped, vec![1]);
    assert!(w.comments_of(1).join("\n").contains("debrief.yml"));
    // On the new head ci.yml runs affected (no since_run: its run was green), debrief.yml reruns the failed run.
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(
        w.ci.dispatched.last().unwrap(),
        &vec![("base".to_string(), "origin/main".to_string())]
    );
    w.green("ci.yml", &new, 8003);
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(
        w.ci.dispatched.last().unwrap(),
        &vec![("since_run".to_string(), "8002".to_string())]
    );
}

#[test]
fn the_inputs_of_each_workflow_are_the_ones_it_declares() {
    assert_eq!(inputs_for("ci.yml", false, None), vec![("base".into(), "origin/main".into())]);
    assert_eq!(inputs_for("ci.yml", true, None), Vec::<(String, String)>::new());
    assert_eq!(inputs_for("debrief.yml", false, None), Vec::<(String, String)>::new());
    assert_eq!(inputs_for("debrief.yml", true, None), vec![("bundles".into(), "true".into())]);
    assert_eq!(inputs_for("debrief.yml", false, Some(5)), vec![("since_run".into(), "5".into())]);
}

// ---- the token stays away from PR code -------------------------------------------------------------------------------------------

#[test]
fn a_gate_subprocess_sees_no_token_and_no_git_credentials() {
    let gone = [
        "GITEA_TOKEN",
        "QUEUE_BOT_TOKEN",
        "GIT_ASKPASS",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "NPM_AUTH_THING",
        "MY_SECRET",
        "RUNNER_PASSWORD",
        "GITHUB_TOKEN",
    ];
    let mut parent: Vec<String> = gone.iter().map(|s| s.to_string()).collect();
    parent.extend(["PATH".to_string(), "HOME".to_string()]);
    let mut cmd = paddock_proc::command("true");
    real::scrub(&mut cmd, &parent);
    let envs: BTreeMap<String, Option<String>> = cmd
        .get_envs()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
        .collect();
    for name in gone {
        assert_eq!(envs.get(name), Some(&None), "{name} must be removed from the gate's environment");
    }
    assert!(
        !envs.contains_key("PATH") && !envs.contains_key("HOME"),
        "ordinary variables are left alone"
    );
    assert_eq!(envs.get("GIT_CONFIG_NOSYSTEM"), Some(&Some("1".into())));
    assert!(
        envs.get("GIT_CONFIG_GLOBAL").is_some_and(|v| v.is_some()),
        "no global git configuration (credential helpers)"
    );
}

#[test]
fn the_gate_command_reads_the_conformance_line_from_origin_main_not_from_the_train() {
    let w = World::new();
    let ci = |line: &str| {
        format!("jobs:\n  c:\n    steps:\n      - run: node conformance/bin/paddock-conformance.mjs --app paddock --root . --only {line}\n")
    };
    git(&w.repos.author, &["checkout", "--quiet", "main"]);
    std::fs::create_dir_all(w.repos.author.join(".gitea/workflows")).unwrap();
    std::fs::write(w.repos.author.join(".gitea/workflows/ci.yml"), ci("C06,C14")).unwrap();
    git(&w.repos.author, &["add", "."]);
    git(&w.repos.author, &["commit", "--quiet", "-m", "ci: gates"]);
    git(&w.repos.author, &["push", "--quiet", "origin", "main"]);
    git(&w.repos.queue, &["fetch", "--quiet", "origin"]);
    git(&w.repos.queue, &["checkout", "--quiet", "--detach", "origin/main"]);
    // A pull request that weakens its own gate: the train's ci.yml names no checks at all.
    std::fs::write(w.repos.queue.join(".gitea/workflows/ci.yml"), ci("C00")).unwrap();
    let line = real::conformance_line(&w.repos.queue).unwrap();
    assert!(line.ends_with("--only C06,C14"), "{line}");
}

#[test]
fn building_and_gating_is_tokenless_data_and_the_token_step_applies_it() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "crates/x/a.rs", &["x"]);
    let mut train = TestTrain {
        inner: GitTrain::new(w.repos.queue.clone(), None),
        reach: w.reach.clone(),
        workflows: vec!["ci.yml".into()],
    };
    train.fetch(&["main".into(), "feat/pr1".into(), "feat/pr2".into()]).unwrap();
    let prs: Vec<Pull> = {
        let mut api_http = std::mem::take(&mut w.gitea);
        let mut l = GiteaLedger {
            api: Api::new(&mut api_http, FakeGitea::SERVER, FakeGitea::REPO, "t"),
            known: w.known.clone(),
        };
        let p = l.open_pulls().unwrap();
        w.gitea = api_http;
        p
    };
    let requests_before = w.gitea.requests.len();
    let out = build_phase(&mut train, &mut w.gates, prs, 1, None, None).unwrap();
    assert_eq!(w.gitea.requests.len(), requests_before, "the build half touches no ledger");
    assert!(w.gitea.comments.is_empty() && w.labels_of(1) == vec!["state/approved"]);
    assert_eq!(out.merged, vec![1]);
    assert_eq!(out.dropped.len(), 1);
    // The outcome is data: written, read back, and applied by the token half.
    let again = BuildOutcome::from_json(&out.to_json()).unwrap();
    assert_eq!(again, out);
    assert_eq!(BuildOutcome::from_json(&serde_json::json!({"nonsense": 1})), None);
    let mut api_http = std::mem::take(&mut w.gitea);
    let t = {
        let mut ledger = GiteaLedger {
            api: Api::new(&mut api_http, FakeGitea::SERVER, FakeGitea::REPO, "t"),
            known: w.known.clone(),
        };
        let mut ports = Ports {
            ledger: &mut ledger,
            train: &mut train,
            gates: &mut w.gates,
            ci: &mut w.ci,
        };
        apply_build(&mut ports, &again).unwrap()
    };
    w.gitea = api_http;
    assert!(matches!(t, Tick::Built { .. }));
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
    assert_eq!(w.labels_of(2), vec!["state/fixes"]);
    assert_eq!(w.train_head(), out.head.unwrap());
}

// ---- blame only what can be proved -----------------------------------------------------------------------------------------------

#[test]
fn a_gate_failure_no_pull_request_explains_stops_the_queue_instead_of_dropping_everyone() {
    for files in [vec![], vec!["docs/elsewhere.md".to_string()]] {
        let mut w = World::new();
        w.approved(1, "crates/x/a.rs", &["x"]);
        w.approved(2, "ui/b.ts", &["ui"]);
        w.gates.script = vec![GateOutcome::Red(vec![GateFailure {
            name: "conformance".into(),
            detail: "something".into(),
            files,
        }])];
        let t = w.tick();
        assert!(matches!(t, Tick::Stuck { measured: true, .. }), "{t:?}");
        assert_eq!(w.labels_of(2), vec!["state/approved"], "nobody was dropped");
        assert_eq!(w.labels_of(1), vec!["state/approved", "queue/hold"], "the queue holds itself");
        assert!(w.comments_of(1).join("\n").contains("queue/hold"));
        assert!(matches!(w.tick(), Tick::Held(1)), "and stays stopped, with no new comment");
        assert_eq!(w.gitea.comments.len(), 1);
    }
}

#[test]
fn a_gate_that_also_fails_on_main_blames_no_pull_request() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    let red = || {
        GateOutcome::Red(vec![GateFailure {
            name: "conformance".into(),
            detail: "C99 in crates/x/a.rs".into(),
            files: vec!["crates/x/a.rs".into()],
        }])
    };
    w.gates.script = vec![red(), red()];
    let t = w.tick();
    assert!(
        matches!(&t, Tick::Stuck { why, measured: true } if why.contains("also fails on main")),
        "{t:?}"
    );
    assert_eq!(w.labels_of(1), vec!["state/approved", "queue/hold"]);
}

#[test]
fn the_files_a_gate_names_are_read_from_its_output() {
    let text = "error[C99]: crates/x/src/lib.rs:12: bad\n  --> ui/projects/a.ts:3:4\nsee `docs/rules.md` and (apps/dyno/x.rs) not a/b\nreadme.md and 1.2.3\n";
    assert_eq!(
        real::files_named(text),
        vec!["crates/x/src/lib.rs", "ui/projects/a.ts", "docs/rules.md", "apps/dyno/x.rs"]
    );
}

// ---- the backup window everywhere ------------------------------------------------------------------------------------------------

#[test]
fn the_backup_window_holds_the_landing_too_and_an_unread_hour_is_not_measured() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.green("ci.yml", &head, 8001);
    let before = w.repos.origin_ref("refs/heads/main");
    assert_eq!(w.tick_at(2), Tick::Window, "a fast-forward starts push-to-main CI");
    assert_eq!(w.repos.origin_ref("refs/heads/main"), before);
    let t = w.tick_hour(None);
    assert!(matches!(t, Tick::Stuck { measured: false, .. }), "{t:?}");
    assert_eq!(w.repos.origin_ref("refs/heads/main"), before);
    assert!(matches!(w.tick_at(4), Tick::Landed { .. }));
    // An unread hour never dispatches either.
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.tick();
    assert!(matches!(w.tick_hour(None), Tick::Stuck { measured: false, .. }));
    assert!(w.ci.dispatched.is_empty());
}

// ---- the surviving mutants -------------------------------------------------------------------------------------------------------

fn finds_bad_txt(req: &crate::merge_check::Request) -> Result<paddock_cli::Report, paddock_cli::CliError> {
    let root = req.root.clone().unwrap();
    let tree = git(&root, &["ls-tree", "-r", "--name-only", "HEAD"]);
    if tree.lines().any(|l| l == "bad.txt") {
        return Err(paddock_cli::CliError::new(
            paddock_cli::Exit::Findings,
            "a hunk of the other side is missing",
        ));
    }
    Ok(paddock_cli::Report::new(serde_json::json!({}), "clean"))
}

fn cannot_look(_: &crate::merge_check::Request) -> Result<paddock_cli::Report, paddock_cli::CliError> {
    Err(paddock_cli::CliError::failure("merge check NOT MEASURED"))
}

#[test]
fn a_merge_the_merge_check_flags_is_dropped_and_left_out_of_the_train() {
    let mut w = World::new();
    w.merge_check = Some(finds_bad_txt);
    w.approved(1, "bad.txt", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    let Tick::Built { head, prs, dropped } = w.tick() else { panic!() };
    assert_eq!(prs, vec![2]);
    assert_eq!(dropped.len(), 1);
    assert_eq!(w.labels_of(1), vec!["state/fixes"]);
    assert!(w.comments_of(1).join("\n").contains("a hunk of the other side is missing"));
    assert!(
        !git(&w.repos.origin, &["ls-tree", "-r", "--name-only", &head])
            .lines()
            .any(|l| l == "bad.txt"),
        "the flagged merge is not in the train"
    );
}

#[test]
fn a_merge_check_that_could_not_look_stops_the_tick_with_an_error() {
    let mut w = World::new();
    w.merge_check = Some(cannot_look);
    w.approved(1, "crates/x/a.rs", &["x"]);
    let e = w.try_tick_hour(Some(12)).unwrap_err();
    assert!(e.contains("did not look"), "{e}");
    assert_eq!(w.labels_of(1), vec!["state/approved"]);
}

#[cfg(unix)]
#[test]
fn a_fast_forward_after_which_main_is_not_the_train_head_is_an_error_not_a_landing() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.green("ci.yml", &head, 8001);
    // The server accepts the push and then moves main back (a hook, a mirror): what the queue pushed is not what main is.
    let main = w.repos.origin_ref("refs/heads/main");
    let hook = w.repos.origin.join("hooks/post-receive");
    std::fs::write(&hook, format!("#!/bin/sh\ngit update-ref refs/heads/main {main}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = w.try_tick_hour(Some(12)).unwrap_err();
    assert!(e.contains("not the train head"), "{e}");
    assert_eq!(w.labels_of(1), vec!["state/in-train"], "nothing is marked landed");
}

#[test]
fn a_flaky_failure_is_still_red_and_blamed_unless_it_also_failed_on_main() {
    use crate::ci_runs::triage::Status;
    assert_eq!(
        real::blame_of(&Status::Flaky {
            count: 2,
            runs: 9,
            also_on_baseline: false
        }),
        Blame::Flaky
    );
    assert_eq!(
        real::blame_of(&Status::Flaky {
            count: 2,
            runs: 9,
            also_on_baseline: true
        }),
        Blame::OnMain
    );
    assert_eq!(real::blame_of(&Status::OnBaseline), Blame::OnMain);
    assert_eq!(real::blame_of(&Status::Unknown("x".into())), Blame::Unknown);
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"]), (2, "ui/b.ts", &["ui"])]);
    w.run(&head, 8001, RunState::Failed("failure".into()));
    w.ci.failures.insert(8001, vec![failure("x flaky_t", &["x"], Blame::Flaky)]);
    let Tick::Dropped { dropped, .. } = w.tick() else {
        panic!("a flaky failure keeps the run red and is blamed (ER-12)")
    };
    assert_eq!(dropped, vec![1]);
}

#[test]
fn an_unknown_row_blames_nobody_and_stops_the_queue() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"]), (2, "ui/b.ts", &["ui"])]);
    w.run(&head, 8001, RunState::Failed("failure".into()));
    w.ci.failures.insert(
        8001,
        vec![failure("x t", &["x"], Blame::New), failure("ui u", &["ui"], Blame::Unknown)],
    );
    let t = w.tick();
    assert!(matches!(t, Tick::Stuck { measured: false, .. }), "{t:?}");
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
    assert_eq!(w.labels_of(2), vec!["state/in-train"]);
}

#[test]
fn the_queue_stops_asking_for_runs_after_three_on_one_head() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    let run = |id| CiRun {
        id,
        state: RunState::Success,
    };
    w.ci.runs.insert(("ci.yml".into(), head.clone()), vec![run(3), run(2), run(1)]);
    w.ci.verdicts.insert(
        ("ci.yml".into(), head.clone()),
        Verdict::Red {
            run: None,
            reason: "partial".into(),
        },
    );
    let n = w.ci.dispatched.len();
    assert!(matches!(w.tick(), Tick::Stuck { measured: true, .. }));
    assert_eq!(w.ci.dispatched.len(), n, "no fourth run");
    w.ci.runs.insert(("ci.yml".into(), head), vec![run(2), run(1)]);
    assert!(
        matches!(w.tick(), Tick::Dispatched { .. }),
        "two runs: the full one is still asked for"
    );
}

// ---- P2s --------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_hold_set_while_the_runs_went_stops_the_landing() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.green("ci.yml", &head, 8001);
    w.on_read = Some(|n, pulls| {
        if n == 2 {
            pulls[0].labels.push("queue/hold".into());
        }
    });
    let before = w.repos.origin_ref("refs/heads/main");
    assert_eq!(w.tick(), Tick::Held(1));
    assert_eq!(w.repos.origin_ref("refs/heads/main"), before);
}

#[test]
fn a_head_that_moved_after_green_is_not_marked_landed() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.green("ci.yml", &head, 8001);
    w.on_read = Some(|n, pulls| {
        if n >= 3 {
            pulls[0].head = "f".repeat(40);
        }
    });
    let Tick::Landed { prs, .. } = w.tick() else { panic!() };
    assert!(prs.is_empty(), "its new commits did not land");
    assert_eq!(w.labels_of(1), vec!["state/built"]);
    assert!(w.comments_of(1).join("\n").contains("new commits"));
}

#[test]
fn a_pull_request_whose_plan_runs_everything_is_not_a_suspect_for_everything() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["*"]), (2, "ui/b.ts", &["ui"])]);
    w.run(&head, 8001, RunState::Failed("failure".into()));
    w.ci.failures.insert(8001, vec![failure("ui u", &["ui"], Blame::New)]);
    let Tick::Dropped { dropped, .. } = w.tick() else { panic!() };
    assert_eq!(dropped, vec![2], "the precise suspect only");
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
    // Only the wildcard pull request could explain it: the queue stops, it does not guess.
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["*"]), (2, "ui/b.ts", &["ui"])]);
    w.run(&head, 8001, RunState::Failed("failure".into()));
    w.ci.failures.insert(8001, vec![failure("x t", &["x"], Blame::New)]);
    assert!(matches!(w.tick(), Tick::Stuck { measured: true, .. }));
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
}

#[test]
fn hold_and_unhold_set_and_clear_the_label_without_touching_the_others() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    let mut api_http = std::mem::take(&mut w.gitea);
    {
        let mut l = GiteaLedger {
            api: Api::new(&mut api_http, FakeGitea::SERVER, FakeGitea::REPO, "t"),
            known: w.known.clone(),
        };
        super::hold(&mut l, 2).unwrap();
        assert_eq!(l.open_pulls().unwrap()[1].labels, vec!["state/approved", "queue/hold"]);
        assert_eq!(super::unhold(&mut l).unwrap(), vec![2]);
        assert_eq!(l.open_pulls().unwrap()[1].labels, vec!["state/approved"]);
        assert!(super::unhold(&mut l).unwrap().is_empty());
    }
    w.gitea = api_http;
}
