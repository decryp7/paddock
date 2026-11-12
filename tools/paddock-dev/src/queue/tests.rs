//! The queue's tick against an in-memory Gitea (the ledger's fake), real git in temporary repositories (a bare origin, the
//! queue's checkout and an author's checkout) and a scripted CI and gates. No real server, no network (ER-14).

use super::ports::*;
use super::real::{self, GitTrain, GiteaLedger};
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
    runs: BTreeMap<String, Vec<CiRun>>,
    verdicts: BTreeMap<String, Verdict>,
    busy: Option<String>,
    dispatched: Vec<Vec<(String, String)>>,
    failures: BTreeMap<u64, Vec<Failure>>,
}

impl FakeCi {
    fn new() -> FakeCi {
        FakeCi {
            runs: BTreeMap::new(),
            verdicts: BTreeMap::new(),
            busy: None,
            dispatched: Vec::new(),
            failures: BTreeMap::new(),
        }
    }
}

impl Ci for FakeCi {
    fn runs(&mut self, head: &str) -> Result<Vec<CiRun>, String> {
        Ok(self.runs.get(head).cloned().unwrap_or_default())
    }
    fn verdict(&mut self, head: &str) -> Verdict {
        self.verdicts
            .get(head)
            .cloned()
            .unwrap_or(Verdict::NotMeasured("no verdict scripted".into()))
    }
    fn dispatch(&mut self, inputs: &[(String, String)]) -> Result<Sent, String> {
        if let Some(who) = &self.busy {
            return Ok(Sent::Busy(who.clone()));
        }
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

struct World {
    repos: Repos,
    gitea: FakeGitea,
    known: Vec<labels::LabelSpec>,
    reach: BTreeMap<String, BTreeSet<String>>,
    gates: FakeGates,
    ci: FakeCi,
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
        }
    }

    /// An approved pull request of a new branch that adds `file`, reaching `projects`.
    fn approved(&mut self, number: u64, file: &str, projects: &[&str]) -> String {
        let name = format!("feat/pr{number}");
        let head = self.repos.branch(&name, file);
        self.gitea.pull(number, &name, &head, "open", None, &["state/approved"]);
        self.reach.insert(head.clone(), projects.iter().map(|s| s.to_string()).collect());
        head
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
        let mut api_http = std::mem::take(&mut self.gitea);
        let result = {
            let mut ledger = GiteaLedger {
                api: Api::new(&mut api_http, FakeGitea::SERVER, FakeGitea::REPO, "t"),
                known: self.known.clone(),
            };
            let mut train = TestTrain {
                inner: GitTrain {
                    root: self.repos.queue.clone(),
                },
                reach: self.reach.clone(),
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
        result.unwrap()
    }

    fn tick(&mut self) -> Tick {
        self.tick_at(12)
    }

    fn run(&mut self, head: &str, id: u64, state: RunState) {
        self.ci.runs.insert(head.to_string(), vec![CiRun { id, state }]);
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
    w.ci.verdicts.insert(head.clone(), Verdict::Green { run: 8001 });
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
        head.clone(),
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
    w.ci.verdicts.insert(new_head.clone(), Verdict::Green { run: 8002 });
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
    w.ci.verdicts
        .insert(head.clone(), Verdict::NotMeasured("the plan job's log could not be read".into()));
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
    assert_eq!(w.gates.ran, 2, "the gates ran again on the rebuilt train");
}

#[test]
fn a_gate_that_could_not_look_is_not_a_pass() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.gates.script = vec![GateOutcome::NotMeasured("node is missing".into())];
    assert!(matches!(w.tick(), Tick::Stuck { measured: false, .. }));
    assert_eq!(w.labels_of(1), vec!["state/approved"], "nothing was taken");
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
    };
    let text = "chore(queue): train round 2\n\nQueue-Prs: 12,3\nQueue-Round: 2\nQueue-Since-Run: 8001\n";
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
