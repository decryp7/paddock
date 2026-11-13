//! The queue's tick against an in-memory Gitea (the ledger's fake), real git in temporary repositories (a bare origin, the
//! queue's checkout and an author's checkout) and a scripted CI and gates. No real server, no network (ER-14).

use super::ports::*;
use super::real::{self, GitTrain, GiteaLedger, MergeCheckFn};
use super::tick::inputs_for;
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
    fn verify_train(&mut self, b: Option<&str>, h: &str, p: &[TrainMember], m: &Marker) -> Result<Option<String>, String> {
        self.inner.verify_train(b, h, p, m)
    }
    fn affected(&mut self, _main: &str, head: &str) -> Result<BTreeSet<String>, String> {
        Ok(self.reach.get(head).cloned().unwrap_or_default())
    }
    fn reached_workflows(&mut self, _m: &str, _h: &str) -> Result<Vec<String>, String> {
        Ok(self.workflows.clone())
    }
    fn push_train(&mut self, h: &str, o: Option<&str>) -> Result<(), String> {
        self.inner.push_train(h, o)
    }
    fn land(&mut self, h: &str) -> Result<(), String> {
        self.inner.land(h)
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
    fn approved_head(&mut self, pr: &Pull) -> Result<bool, String> {
        self.inner.approved_head(pr)
    }
    fn closed_held(&mut self) -> Result<Vec<u64>, String> {
        self.inner.closed_held()
    }
}

fn no_reach(_: &Path, _: &str, _: &str) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"all": false}))
}

struct World {
    repos: Repos,
    gitea: FakeGitea,
    known: Vec<labels::LabelSpec>,
    reach: BTreeMap<String, BTreeSet<String>>,
    ci: FakeCi,
    workflows: Vec<String>,
    merge_check: Option<MergeCheckFn>,
    on_read: Option<fn(usize, &mut Vec<Pull>)>,
    /// Run the tick on a real `GitTrain::open` (its own clone under this folder) instead of the table-driven one.
    real_train: Option<PathBuf>,
    /// The planner of the real train.
    planner: real::PlannerFn,
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
            ci: FakeCi::new(),
            workflows: vec!["ci.yml".into()],
            merge_check: None,
            on_read: None,
            real_train: None,
            planner: no_reach,
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
            serde_json::json!({"id": self.gitea.reviews.len() + 1, "user": {"login": "review-bot"}, "state": "APPROVED", "commit_id": head, "dismissed": false, "official": true}),
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

    fn main(&self) -> String {
        self.repos.origin_ref("refs/heads/main")
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
            let mut table = TestTrain {
                inner: {
                    let mut g = GitTrain::for_clone(self.repos.queue.clone(), self.repos.origin.to_str().unwrap());
                    if let Some(f) = self.merge_check {
                        g.merge_check = f;
                    }
                    g
                },
                reach: self.reach.clone(),
                workflows: self.workflows.clone(),
            };
            let mut real = self.real_train.as_ref().map(|scratch| {
                let mut g = GitTrain::open(self.repos.origin.to_str().unwrap(), scratch, None, false, true).unwrap();
                g.planner = self.planner;
                g
            });
            let train: &mut dyn Train = match real.as_mut() {
                Some(g) => g,
                None => &mut table,
            };
            let mut ports = Ports {
                ledger: &mut ledger,
                train,
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
    // The next tick dispatches the affected run against main.
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(w.ci.dispatched, vec![vec![("base".to_string(), w.main())]]);
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
            files: vec![],
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
        &vec![("base".to_string(), w.main()), ("since_run".to_string(), "8001".to_string())]
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
                files: vec![],
                blame: Blame::New,
            },
            Failure {
                what: "ui main-red".into(),
                projects: set(&["ui"]),
                files: vec![],
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
            files: vec![],
            blame: Blame::OnMain,
        }],
    );
    assert!(matches!(w.tick(), Tick::Stuck { measured: true, .. }));
    assert_eq!(
        w.labels_of(1),
        vec!["state/in-train", "queue/blocked"],
        "the train is marked blocked on main"
    );
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

fn failure(what: &str, projects: &[&str], blame: Blame) -> Failure {
    Failure {
        what: what.into(),
        projects: set(projects),
        files: vec![],
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
    assert!(w.gitea.comments.len() == 1);
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
fn only_review_bots_official_approval_of_the_exact_head_counts_and_a_later_change_request_or_dismissal_ends_it() {
    use crate::pr::gitea::{Review, approved_at};
    let allowed: &[&str] = &["review-bot"];
    let r = |id: u64, who: &str, state: &str, commit: &str, dismissed: bool, official: bool| Review {
        id,
        reviewer: who.into(),
        state: state.into(),
        commit_id: commit.into(),
        dismissed,
        official,
    };
    let ok = |reviews: &[Review]| approved_at(reviews, "h1", allowed);
    let bot = "review-bot";
    assert!(ok(&[r(1, bot, "APPROVED", "h1", false, true)]));
    assert!(!ok(&[r(1, bot, "APPROVED", "h0", false, true)]), "an approval of an older head");
    assert!(!ok(&[r(1, bot, "APPROVED", "h1", true, true)]), "dismissed");
    assert!(!ok(&[r(1, bot, "COMMENT", "h1", false, true)]), "a comment is not an approval");
    assert!(
        !ok(&[r(1, bot, "APPROVED", "h1", false, false)]),
        "an unofficial review is advice, not an approval"
    );
    for who in ["someone", "a-maintainer", "queue-bot", "release-bot", "author"] {
        assert!(
            !ok(&[r(1, who, "APPROVED", "h1", false, true)]),
            "{who}'s official approval is not review-bot's"
        );
    }
    assert!(
        !ok(&[
            r(1, bot, "APPROVED", "h1", false, true),
            r(2, bot, "REQUEST_CHANGES", "h1", false, true)
        ]),
        "review-bot changed its mind"
    );
    // The server may list them in any order: the id says which is later.
    assert!(ok(&[
        r(2, bot, "APPROVED", "h1", false, true),
        r(1, bot, "REQUEST_CHANGES", "h1", false, true)
    ]));
    assert!(!ok(&[
        r(2, bot, "REQUEST_CHANGES", "h1", false, true),
        r(1, bot, "APPROVED", "h1", false, true)
    ]));
    // A human's change request on the current head blocks even when the bot approved; an old one, a dismissed one or an
    // unofficial one does not; and their approval never approves.
    for (who, commit, dismissed, official, blocks) in [
        ("the-user", "h1", false, true, true),
        ("a-maintainer", "h1", false, true, true),
        ("the-user", "h0", false, true, false),
        ("the-user", "h1", true, true, false),
        ("a-stranger", "h1", false, false, false),
    ] {
        let reviews = [
            r(1, bot, "APPROVED", "h1", false, true),
            r(2, who, "REQUEST_CHANGES", commit, dismissed, official),
        ];
        assert_eq!(!ok(&reviews), blocks, "{who} on {commit} dismissed={dismissed} official={official}");
    }
    assert!(
        !ok(&[r(1, "the-user", "APPROVED", "h1", false, true)]),
        "the user's approval is not review-bot's"
    );
    // A later approval from the human lifts their own change request.
    assert!(ok(&[
        r(1, bot, "APPROVED", "h1", false, true),
        r(2, "the-user", "REQUEST_CHANGES", "h1", false, true),
        r(3, "the-user", "APPROVED", "h1", false, true)
    ]));
    assert!(!ok(&[]) && !approved_at(&[r(1, bot, "APPROVED", "", false, true)], "", allowed));
}

#[test]
fn an_approval_by_any_other_account_gets_no_pull_request_into_a_train() {
    for who in ["a-maintainer", "queue-bot", "release-bot", "author"] {
        let mut w = World::new();
        w.approved(1, "crates/x/a.rs", &["x"]);
        for (_, r) in w.gitea.reviews.iter_mut() {
            r["user"]["login"] = serde_json::json!(who);
        }
        assert!(matches!(w.tick(), Tick::Idle(_)), "{who}");
        assert_eq!(w.labels_of(1), vec!["state/built"], "{who}");
    }
}

#[test]
fn review_bots_approval_does_not_count_on_a_pull_request_review_bot_opened() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.gitea.pulls.iter_mut().find(|p| p["number"] == 1).unwrap()["user"]["login"] = serde_json::json!("review-bot");
    assert!(matches!(w.tick(), Tick::Idle(_)));
    assert_eq!(w.labels_of(1), vec!["state/built"]);
}

#[test]
fn an_unofficial_review_does_not_get_a_pull_request_into_a_train() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    for (_, r) in w.gitea.reviews.iter_mut() {
        r["official"] = serde_json::json!(false);
    }
    assert!(matches!(w.tick(), Tick::Idle(_)));
    assert_eq!(w.labels_of(1), vec!["state/built"]);
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
    assert_eq!(w.ci.dispatched.last().unwrap(), &vec![("base".to_string(), w.main())]);
    w.green("ci.yml", &new, 8003);
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert_eq!(
        w.ci.dispatched.last().unwrap(),
        &vec![("since_run".to_string(), "8002".to_string())]
    );
}

#[test]
fn the_inputs_of_each_workflow_are_the_ones_it_declares() {
    assert_eq!(inputs_for("ci.yml", false, None, "M"), vec![("base".into(), "M".into())]);
    assert_eq!(inputs_for("ci.yml", true, None, "M"), Vec::<(String, String)>::new());
    assert_eq!(inputs_for("debrief.yml", false, None, "M"), Vec::<(String, String)>::new());
    assert_eq!(inputs_for("debrief.yml", true, None, "M"), vec![("bundles".into(), "true".into())]);
    assert_eq!(
        inputs_for("debrief.yml", false, Some(5), "M"),
        vec![("since_run".into(), "5".into())]
    );
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

// ---- blame only what can be proved -----------------------------------------------------------------------------------------------

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
    let t = w.tick();
    assert!(
        matches!(&t, Tick::Stuck { why, measured: true } if why.contains("runs everything")),
        "{t:?}"
    );
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

// ---- no code of a pull request runs in a tick ----------------------------------------------------------------------------------

/// A branch whose files try every way a repository can make a tool run its code: hooks, a git attributes filter, a cargo
/// wrapper, a rust-toolchain path, a lefthook file, an npm script and a gate script. Each one touches `sentinel`.
fn booby_trapped(w: &World, name: &str, sentinel: &Path) -> String {
    git(&w.repos.author, &["checkout", "--quiet", "-B", name, "origin/main"]);
    let dir = &w.repos.author;
    let script = format!("#!/bin/sh\ntouch '{}'\n", sentinel.display());
    let put = |path: &str, text: &str, exec: bool| {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        #[cfg(unix)]
        if exec {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let _ = exec;
    };
    for hook in [
        "pre-commit",
        "post-checkout",
        "post-merge",
        "post-commit",
        "commit-msg",
        "pre-push",
        "reference-transaction",
    ] {
        put(&format!(".githooks/{hook}"), &script, true);
        put(&format!(".git-hooks/{hook}"), &script, true);
    }
    put("scripts/gate.sh", &script, true);
    put("scripts/toolchain/bin/cargo", &script, true);
    put(
        ".cargo/config.toml",
        "[build]\nrustc-wrapper = \"./scripts/gate.sh\"\n[alias]\nmetadata = \"!./scripts/gate.sh\"\n",
        false,
    );
    put("rust-toolchain.toml", "[toolchain]\npath = \"./scripts/toolchain\"\n", false);
    put(".gitattributes", "* filter=evil\n*.txt diff=evil merge=evil\n", false);
    put(
        "lefthook.yml",
        "pre-commit:\n  commands:\n    gate:\n      run: ./scripts/gate.sh\n",
        false,
    );
    put(
        "package.json",
        "{\"scripts\": {\"postinstall\": \"./scripts/gate.sh\", \"prepare\": \"./scripts/gate.sh\", \"test\": \"./scripts/gate.sh\"}}\n",
        false,
    );
    put(
        "conformance/bin/paddock-conformance.mjs",
        "require('fs').writeFileSync(process.env.S || '/nonexistent', 'x');\n",
        false,
    );
    put(
        "Cargo.toml",
        "[package]\nname = \"trap\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
        false,
    );
    put("src/lib.rs", "", false);
    put(
        "build.rs",
        &format!(
            "fn main() {{ std::fs::write({:?}, \"x\").unwrap(); }}\n",
            sentinel.to_string_lossy()
        ),
        false,
    );
    put(
        "crates/x/build.rs",
        &format!(
            "fn main() {{ std::fs::write({:?}, \"x\").unwrap(); }}\n",
            sentinel.to_string_lossy()
        ),
        false,
    );
    git(&w.repos.author, &["add", "-A"]);
    git(
        &w.repos.author,
        &["commit", "--quiet", "-m", "feat: a branch that tries to run itself"],
    );
    git(&w.repos.author, &["push", "--quiet", "origin", name]);
    git(&w.repos.author, &["rev-parse", "HEAD"])
}

#[test]
fn a_train_full_of_things_that_run_executes_nothing_during_a_tick() {
    let mut w = World::new();
    let sentinel_dir = tempfile::tempdir().unwrap();
    let sentinel = sentinel_dir.path().join("sentinel");
    let head = booby_trapped(&w, "feat/pr1", &sentinel);
    w.gitea.pull(1, "feat/pr1", &head, "open", None, &["state/approved"]);
    w.approve(1, &head);
    // Positive control: the files do run when something runs them, so the sentinel can see a violation.
    git(&w.repos.author, &["checkout", "--quiet", "feat/pr1"]);
    let ran = paddock_proc::command("sh")
        .arg("scripts/gate.sh")
        .current_dir(&w.repos.author)
        .status()
        .unwrap();
    assert!(ran.success() && sentinel.exists(), "the sentinel mechanism works");
    std::fs::remove_file(&sentinel).unwrap();
    // The whole life of the train on a real clone: build, dispatch, wait, a failed run triaged, the retry, the landing.
    let scratch = tempfile::tempdir().unwrap();
    w.real_train = Some(scratch.path().to_path_buf());
    let Tick::Built { head: train, .. } = w.tick() else {
        panic!("built")
    };
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    w.run(&train, 7001, RunState::Running);
    assert!(matches!(w.tick(), Tick::Waiting(_)));
    w.green("ci.yml", &train, 7001);
    assert!(matches!(w.tick(), Tick::Landed { .. }));
    assert_eq!(w.repos.origin_ref("refs/heads/main"), train);
    assert!(!sentinel.exists(), "nothing of the pull request ran in a tick");
}

#[test]
fn every_git_child_runs_with_hooks_off_no_file_transport_and_no_token_but_the_push_gets_one_scoped_to_it() {
    let scratch = tempfile::tempdir().unwrap();
    let mut g = GitTrain::for_clone(scratch.path().to_path_buf(), "https://example.invalid/o/r.git");
    g.token = Some("s3cret-token".into());
    g.allow_file = false;
    let args = |c: &std::process::Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
    let envs = |c: &std::process::Command| {
        c.get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
            .collect::<BTreeMap<_, _>>()
    };
    for kind in [real::Child::Local, real::Child::Authed] {
        let c = g.command(None, kind);
        let a = args(&c);
        for want in [
            "core.hooksPath=/dev/null",
            "protocol.file.allow=never",
            "core.fsmonitor=false",
            "credential.helper=",
        ] {
            assert!(a.iter().any(|x| x == want), "{kind:?} lacks -c {want}: {a:?}");
        }
        assert!(!a.iter().any(|x| x.contains("s3cret")), "the token is never an argument");
        let e = envs(&c);
        assert_eq!(e.get("GIT_TERMINAL_PROMPT"), Some(&Some("0".into())));
        assert!(
            e.get("GIT_CONFIG_GLOBAL").is_some_and(|v| v.is_some()),
            "no global git configuration"
        );
    }
    let local = envs(&g.command(None, real::Child::Local));
    assert_eq!(local.get("GITEA_TOKEN"), Some(&None));
    assert!(
        !local.contains_key("GIT_ASKPASS") || local.get("GIT_ASKPASS") == Some(&None),
        "a local child has no askpass"
    );
    assert!(!local.contains_key("PADDOCK_QUEUE_PUSH_TOKEN"));
    let push = envs(&g.command(None, real::Child::Authed));
    assert_eq!(push.get("PADDOCK_QUEUE_PUSH_TOKEN"), Some(&Some("s3cret-token".into())));
    assert!(
        push.get("GIT_ASKPASS")
            .is_some_and(|v| v.as_deref().is_some_and(|p| p.ends_with("askpass.sh")))
    );
    // Production never allows the file transport; the tests' local origin needs it.
    g.allow_file = true;
    assert!(
        args(&g.command(None, real::Child::Local))
            .iter()
            .any(|x| x == "protocol.file.allow=user")
    );
}

#[test]
fn the_planner_child_and_the_merge_check_run_without_the_queue_token() {
    let parent: Vec<String> = ["PATH", "GITEA_TOKEN", "QUEUE_BOT_TOKEN", "CARGO_REGISTRIES_X_TOKEN", "MY_SECRET"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut cmd = paddock_proc::command("true");
    crate::child_env::scrub(&mut cmd, &parent);
    let envs: BTreeMap<String, Option<String>> = cmd
        .get_envs()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
        .collect();
    for gone in ["GITEA_TOKEN", "QUEUE_BOT_TOKEN", "CARGO_REGISTRIES_X_TOKEN", "MY_SECRET"] {
        assert_eq!(envs.get(gone), Some(&None), "{gone}");
    }
    assert!(!envs.contains_key("PATH"));
}

#[test]
fn open_makes_its_own_clone_of_the_given_url_and_resets_a_wrong_origin() {
    let w = World::new();
    let scratch = tempfile::tempdir().unwrap();
    let url = w.repos.origin.to_str().unwrap();
    let g = GitTrain::open(url, scratch.path(), None, false, true).unwrap();
    assert!(
        g.root.join(".git").exists() && g.root != w.repos.queue,
        "a clone of its own, not the job's checkout"
    );
    git(&g.root, &["remote", "set-url", "origin", "https://example.invalid/evil.git"]);
    let g = GitTrain::open(url, scratch.path(), None, false, true).unwrap();
    assert_eq!(
        git(&g.root, &["remote", "get-url", "origin"]),
        url,
        "the URL is the queue's, whatever the clone says"
    );
}

#[test]
fn the_train_is_pushed_under_a_lease_so_a_train_someone_else_moved_is_not_overwritten() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    // Someone else moves the train branch.
    let other = w.repos.branch("other", "docs/other.md");
    git(
        &w.repos.author,
        &["push", "--quiet", "--force", "origin", &format!("{other}:refs/heads/train/queue")],
    );
    let mut g = GitTrain::for_clone(w.repos.queue.clone(), w.repos.origin.to_str().unwrap());
    git(&w.repos.queue, &["fetch", "--quiet", "origin"]);
    let (stale, none) = (g.push_train(&head, Some(&head)), g.push_train(&head, None));
    assert!(stale.is_err(), "a lease on a head the branch no longer has refuses: {stale:?}");
    assert!(none.is_err(), "no lease on an existing branch refuses: {none:?}");
    assert_eq!(w.train_head(), other, "the other train is untouched");
    assert!(g.push_train(&head, Some(&other)).is_ok(), "the right lease replaces it");
}

// ---- the hour --------------------------------------------------------------------------------------------------------------------

#[test]
fn the_singapore_hour_is_utc_plus_eight_from_the_clock_alone() {
    use std::time::{Duration, UNIX_EPOCH};
    let at = |secs: u64| super::singapore_hour(UNIX_EPOCH + Duration::from_secs(secs));
    // 2026-11-12T15:59:59Z is 23:59:59 in Singapore; one second later it is midnight; 19:59:59Z is 03:59:59; 20:00:00Z is 04:00.
    let day = 1_794_441_600; // 2026-11-12T00:00:00Z
    assert_eq!(at(day + 15 * 3600 + 3599), Some(23));
    assert_eq!(at(day + 16 * 3600), Some(0));
    assert_eq!(at(day + 19 * 3600 + 3599), Some(3));
    assert_eq!(at(day + 20 * 3600), Some(4));
    assert_eq!(at(day), Some(8));
    assert_eq!(at(0), Some(8));
    assert!(
        super::singapore_hour(UNIX_EPOCH - Duration::from_secs(1)).is_none(),
        "a clock before 1970 is not measured"
    );
}

#[test]
fn a_held_tick_does_not_exit_zero() {
    assert_eq!(super::exit_of(&Tick::Held(3)), paddock_cli::Exit::Refused);
    assert_eq!(super::exit_of(&Tick::Idle("x".into())), paddock_cli::Exit::Ok);
    assert_eq!(
        super::exit_of(&Tick::Stuck {
            why: "x".into(),
            measured: false
        }),
        paddock_cli::Exit::NotMeasured
    );
    assert_eq!(
        super::exit_of(&Tick::Stuck {
            why: "x".into(),
            measured: true
        }),
        paddock_cli::Exit::Findings
    );
}

// ---- workflow discovery from paddock.ci.json -------------------------------------------------------------------------------------

fn reaches_debrief(_: &Path, _: &str, _: &str) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"apps": ["debrief"], "crates": false}))
}
fn reaches_evilapp(_: &Path, _: &str, _: &str) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"apps": ["evilapp"]}))
}
fn reaches_nothing(_: &Path, _: &str, _: &str) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"apps": [], "crates": true}))
}
fn reaches_everything(_: &Path, _: &str, _: &str) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"all": true}))
}

#[test]
fn the_reached_workflows_come_from_the_apps_paddock_ci_json_on_main_and_the_plan() {
    let w = World::new();
    let put = |path: &str, text: &str| {
        let p = w.repos.author.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    git(&w.repos.author, &["checkout", "--quiet", "main"]);
    put("apps/debrief/paddock.ci.json", "{\"workflow\": \"debrief.yml\", \"cargo\": \".\"}");
    put("apps/pitwall/paddock.ci.json", "{\"workflow\": \"pitwall.yml\"}");
    put("apps/demo/paddock.ci.json", "{\"cargo\": \".\"}");
    put("apps/odd/paddock.ci.json", "{\"workflow\": \"../evil.yml\"}");
    put("apps/broken/paddock.ci.json", "not json");
    git(&w.repos.author, &["add", "-A"]);
    git(&w.repos.author, &["commit", "--quiet", "-m", "chore: apps"]);
    git(&w.repos.author, &["push", "--quiet", "origin", "main"]);
    // A branch that renames Debrief's workflow and gives the demo one: main's files decide, not the pull request's.
    git(&w.repos.author, &["checkout", "--quiet", "-B", "evil", "main"]);
    put("apps/debrief/paddock.ci.json", "{\"workflow\": \"evil.yml\"}");
    put("apps/demo/paddock.ci.json", "{\"workflow\": \"demo.yml\"}");
    put("apps/evilapp/paddock.ci.json", "{\"workflow\": \"evilapp.yml\"}");
    git(&w.repos.author, &["add", "-A"]);
    git(&w.repos.author, &["commit", "--quiet", "-m", "feat: evil"]);
    git(&w.repos.author, &["push", "--quiet", "origin", "evil"]);
    let scratch = tempfile::tempdir().unwrap();
    let mut g = GitTrain::open(w.repos.origin.to_str().unwrap(), scratch.path(), None, false, true).unwrap();
    g.fetch(&["main".into(), "evil".into()]).unwrap();
    let (main, evil) = (g.main_head().unwrap(), w.repos.origin_ref("refs/heads/evil"));
    g.planner = reaches_debrief;
    assert_eq!(g.reached_workflows(&main, &evil).unwrap(), vec!["ci.yml", "debrief.yml"]);
    g.planner = reaches_evilapp;
    assert_eq!(
        g.reached_workflows(&main, &evil).unwrap(),
        vec!["ci.yml"],
        "an app that exists only in the pull request is not read from the pull request"
    );
    g.planner = reaches_nothing;
    assert_eq!(g.reached_workflows(&main, &evil).unwrap(), vec!["ci.yml"]);
    g.planner = reaches_everything;
    assert_eq!(
        g.reached_workflows(&main, &evil).unwrap(),
        vec!["ci.yml", "debrief.yml", "pitwall.yml"]
    );
}

#[test]
fn the_pure_workflow_selection_follows_the_plan_and_ignores_names_that_are_not_files_of_the_workflows_folder() {
    let apps: BTreeMap<String, String> = [("debrief", "debrief.yml"), ("dyno", "dyno.yml")]
        .iter()
        .map(|(a, w)| (a.to_string(), w.to_string()))
        .collect();
    assert_eq!(real::select_workflows(&apps, &set(&["debrief"])), vec!["ci.yml", "debrief.yml"]);
    assert_eq!(real::select_workflows(&apps, &set(&[])), vec!["ci.yml"]);
    assert_eq!(
        real::select_workflows(&apps, &set(&["*"])),
        vec!["ci.yml", "debrief.yml", "dyno.yml"]
    );
    let tree = "apps/a\napps/b\nnot-apps/c\n";
    let found = real::app_workflows(tree, &mut |p| match p {
        "apps/a/paddock.ci.json" => Some("{\"workflow\": \"a.yml\"}".into()),
        "apps/b/paddock.ci.json" => Some("{\"workflow\": \"sub/b.yml\"}".into()),
        _ => None,
    });
    assert_eq!(found.into_iter().collect::<Vec<_>>(), vec![("a".to_string(), "a.yml".to_string())]);
}

// ---- blocked trains, closed holds, cancelled runs, resume ---------------------------------------------------------------------

#[test]
fn a_train_that_fails_only_on_mains_own_tests_is_blocked_not_triaged_again_and_a_fix_gets_through() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.run(&head, 9100, RunState::Failed("failure".into()));
    w.ci.failures.insert(9100, vec![failure("x t", &["x"], Blame::OnMain)]);
    assert!(matches!(w.tick(), Tick::Stuck { measured: true, .. }));
    assert_eq!(w.labels_of(1), vec!["state/in-train", "queue/blocked"]);
    assert_eq!(w.comments_of(1).len(), 1);
    // The next ticks do not triage (the failures are no longer even scripted) and do not comment again.
    w.ci.failures.clear();
    for _ in 0..3 {
        let t = w.tick();
        assert!(matches!(&t, Tick::Stuck { why, .. } if why.contains("blocked")), "{t:?}");
    }
    assert_eq!(w.comments_of(1).len(), 1);
    // A pull request that fixes main is approved: the train is released and everything is built together.
    w.approved(2, "docs/fix.md", &["docs"]);
    assert!(matches!(w.tick(), Tick::Idle(_)));
    assert_eq!(w.labels_of(1), vec!["state/approved"]);
    let Tick::Built { prs, .. } = w.tick() else { panic!() };
    assert_eq!(prs, vec![1, 2]);
    assert_eq!(w.labels_of(1), vec!["state/in-train"], "the blocked mark is gone");
}

#[test]
fn a_blocked_train_is_rebuilt_when_main_moves_and_unhold_makes_it_triage_afresh() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.run(&head, 9100, RunState::Failed("failure".into()));
    w.ci.failures.insert(9100, vec![failure("x t", &["x"], Blame::OnMain)]);
    w.tick();
    // unhold clears the blocked mark: the failed run is judged again, not rerun blindly.
    let mut api_http = std::mem::take(&mut w.gitea);
    {
        let mut l = GiteaLedger {
            api: Api::new(&mut api_http, FakeGitea::SERVER, FakeGitea::REPO, "t"),
            known: w.known.clone(),
        };
        assert_eq!(super::unhold(&mut l).unwrap(), vec![1]);
    }
    w.gitea = api_http;
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
    w.ci.failures.insert(9100, vec![failure("x t", &["x"], Blame::New)]);
    assert!(matches!(w.tick(), Tick::Dropped { .. }), "triaged afresh: now it is new");
    // And a blocked train whose main moved is rebuilt.
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.run(&head, 9100, RunState::Failed("failure".into()));
    w.ci.failures.insert(9100, vec![failure("x t", &["x"], Blame::OnMain)]);
    w.tick();
    w.repos.branch("hand", "docs/hand.md");
    git(&w.repos.author, &["push", "--quiet", "origin", "hand:main"]);
    let Tick::Built { head: new, .. } = w.tick() else {
        panic!("rebuilt on the new main")
    };
    assert_ne!(new, head);
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
}

#[test]
fn a_hold_does_not_outlive_its_closed_pull_request() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    set_labels(&mut w, 2, &["state/approved", "queue/hold"]);
    assert_eq!(w.tick(), Tick::Held(2));
    w.gitea.pulls.iter_mut().find(|p| p["number"] == 2).unwrap()["state"] = serde_json::json!("closed");
    let Tick::Built { prs, .. } = w.tick() else {
        panic!("the queue runs again")
    };
    assert_eq!(prs, vec![1]);
    assert_eq!(w.labels_of(2), vec!["state/approved"], "the hold is released");
    assert!(w.comments_of(2).join("\n").contains("closed"));
}

#[test]
fn a_cancelled_run_stops_the_queue_and_is_never_triaged_or_blamed() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.run(&head, 9200, RunState::Failed("cancelled".into()));
    // Nothing is scripted for triage: a tick that triaged a cancelled run would fail here.
    let t = w.tick();
    assert!(
        matches!(&t, Tick::Stuck { why, measured: true } if why.contains("cancelled")),
        "{t:?}"
    );
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
}

#[test]
fn a_train_whose_main_moved_is_rebuilt_even_when_its_pull_requests_did_not_change() {
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"])]);
    w.green("ci.yml", &head, 8001);
    // Only main moves (by hand): the marker and the pull request heads are all still right.
    w.repos.branch("hand", "docs/hand.md");
    git(&w.repos.author, &["push", "--quiet", "origin", "hand:main"]);
    let before = w.repos.origin_ref("refs/heads/main");
    let Tick::Built { head: new, prs, .. } = w.tick() else {
        panic!("a green run of the old train is not a landing")
    };
    assert_ne!(new, head);
    assert_eq!(prs, vec![1]);
    assert_eq!(
        w.repos.origin_ref("refs/heads/main"),
        before,
        "the old tested train did not land on a main it does not contain"
    );
}

// ---- the gates the tick no longer runs are jobs of the dispatched workflow ------------------------------------------------------------

/// ci.yml as the tests read it: a Windows checkout has CRLF line endings (`core.autocrlf`), and every search below is for `\n`.
fn ci_yml() -> String {
    include_str!("../../../../.gitea/workflows/ci.yml").replace("\r\n", "\n")
}

fn job_block(yml: &str, job: &str) -> String {
    let head = format!("\n  {job}:\n");
    let start = yml.find(&head).unwrap_or_else(|| panic!("ci.yml has no job {job}")) + 1;
    let rest = &yml[start..];
    let end = rest[head.len() - 1..]
        .find("\n  ")
        .and_then(|_| {
            // The next job starts at a line indented by exactly two spaces that is a key.
            rest.lines()
                .skip(1)
                .scan(rest.lines().next().map_or(0, |l| l.len() + 1), |off, l| {
                    let at = *off;
                    *off += l.len() + 1;
                    Some((at, l))
                })
                .find(|(_, l)| {
                    l.len() > 2
                        && l.starts_with("  ")
                        && !l.starts_with("   ")
                        && l.trim_end().ends_with(':')
                        && !l.trim_start().starts_with('#')
                })
                .map(|(at, _)| at)
        })
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn the_local_gates_are_the_first_jobs_of_a_trains_dispatch_run_and_run_without_a_token() {
    let ci = ci_yml();
    let gates = job_block(&ci, "train-gates");
    for needle in [
        "./dev gates check",
        "./dev ci run conformance paddock",
        "cargo hakari generate --diff",
        "cargo metadata --locked --format-version 1",
        "node scripts/publish.mjs --offline",
        "./dev gen check --base \"$TRAIN_BASE\"",
        "github.event.inputs.base != ''",
        "needs: [plan]",
    ] {
        assert!(gates.contains(needle), "train-gates lacks `{needle}`:\n{gates}");
    }
    assert!(
        !gates.contains("secrets.") && !gates.to_lowercase().contains("token"),
        "the gates job holds no token"
    );
    // The conformance list is in one place, the contract (conformance/paddock.ci.json), run by both jobs through the executor.
    assert_eq!(
        ci.matches("--only C06").count(),
        0,
        "ci.yml holds no copy of the conformance arguments"
    );
    assert!(job_block(&ci, "conformance").contains("./dev ci run conformance paddock"));
    assert!(
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../conformance/paddock.ci.json"))
            .unwrap()
            .contains("\"only\": [")
    );
    // The commit lint is the commits job, which runs on every dispatch.
    assert!(job_block(&ci, "commits").contains("commit-lint"));
    // The planner's table knows the job, so a rerun of its failure plans it.
    assert!(crate::plan_gitea::CI_JOBS.iter().any(|(j, _)| *j == "train-gates"));
}

#[test]
fn ci_yml_is_read_only_and_its_checkouts_keep_no_credentials() {
    let ci = ci_yml();
    let top = ci.split("\njobs:\n").next().unwrap();
    assert!(
        top.contains("\npermissions:\n  contents: read\n  actions: read\n"),
        "workflow-level read-only permissions"
    );
    let checkouts = ci.matches("uses: actions/checkout@v4").count();
    assert!(checkouts > 0);
    assert_eq!(
        ci.matches("persist-credentials: false").count(),
        checkouts,
        "every checkout drops its credentials, as tag.yml's does"
    );
    // Repeated keys (run 7998: two `with:` in a checkout step) are C111's: it parses every workflow with unique keys.
}

/// The shell of a step: the `run:` text of the step named `name` in a job block.
fn step_run(block: &str, name: &str) -> String {
    let at = block.find(&format!("- name: {name}")).unwrap_or_else(|| panic!("no step {name}"));
    let rest = &block[at..];
    let run = rest.find("run:").unwrap_or_else(|| panic!("step {name} has no run"));
    let body = &rest[run + 4..];
    let end = body.find("\n      - ").unwrap_or(body.len());
    let text = &body[..end];
    // `run: |` blocks: strip the indentation; a one-liner is the rest of the line.
    if let Some(block) = text.trim_start().strip_prefix('|') {
        block.lines().map(|l| l.trim_start_matches(' ')).collect::<Vec<_>>().join("\n")
    } else {
        text.trim().to_string()
    }
}

fn repo_root() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

#[test]
fn the_lockfile_loop_of_the_train_gates_runs_on_the_real_repository_and_visits_no_fixture() {
    let ci = ci_yml();
    let script = step_run(
        &job_block(&ci, "train-gates"),
        "Lockfiles are current (cargo metadata of every workspace)",
    );
    // The real loop, on the real repository, with a cargo that only says where it was run.
    let shim = tempfile::tempdir().unwrap();
    let log = shim.path().join("visited");
    let cargo = shim.path().join("cargo");
    std::fs::write(&cargo, format!("#!/bin/sh\npwd >> '{}'\nexit 0\n", log.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", shim.path().display(), std::env::var("PATH").unwrap_or_default());
    let out = paddock_proc::command("sh")
        .args(["-ec", &script])
        .current_dir(repo_root())
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let visited = std::fs::read_to_string(&log).unwrap();
    assert!(
        visited.lines().any(|l| l.ends_with("/crates")) && visited.lines().any(|l| l.ends_with("/tools/paddock-dev")),
        "{visited}"
    );
    assert!(
        !visited.contains("tests/golden/fixture"),
        "the golden fixture is a repository inside the repository, not a workspace:\n{visited}"
    );
    // And every folder it visits is one `cargo metadata` can resolve, not a fixture a test makes.
    for l in visited.lines() {
        let dir = if cfg!(windows) { native_path_of_bash(l) } else { l.to_string() };
        assert!(
            Path::new(&dir).join("Cargo.toml").exists(),
            "{l} has no Cargo.toml (looked at {dir})"
        );
    }
}

/// What Git bash's `pwd` prints on Windows (`/c/Users/x`) as the path Windows itself reads (`C:/Users/x`); any other path is unchanged.
/// Rust's `Path::exists` cannot resolve the first form, so the CI job on Windows (run 8034) reported "has no Cargo.toml" for a full tree.
fn native_path_of_bash(p: &str) -> String {
    let b = p.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
        return format!("{}:{}", b[1].to_ascii_uppercase() as char, &p[2..]);
    }
    p.to_string()
}

#[test]
fn a_git_bash_path_is_read_as_the_windows_path_it_names() {
    assert_eq!(
        native_path_of_bash("/c/Users/x/paddock-checkout-Windows-s1/apps/debrief"),
        "C:/Users/x/paddock-checkout-Windows-s1/apps/debrief"
    );
    assert_eq!(native_path_of_bash("/Volumes/d7/x"), "/Volumes/d7/x");
    assert_eq!(native_path_of_bash("C:/x"), "C:/x");
}

#[test]
fn the_publish_dry_run_of_the_train_gates_installs_and_builds_what_it_needs() {
    let ci = ci_yml();
    let gates = job_block(&ci, "train-gates");
    let publish = gates.find("node scripts/publish.mjs").expect("a publish dry run");
    assert!(
        !gates[publish..].lines().next().unwrap().contains("--skip-build"),
        "there is no ui/dist here: --skip-build would fail"
    );
    let install = gates.find("run: npm ci").expect("npm ci");
    assert!(install < publish, "the packages are installed before the dry run builds them");
    // The script accepts exactly the arguments the step gives it.
    let script = std::fs::read_to_string(repo_root().join("scripts/publish.mjs")).unwrap();
    assert!(script.contains("'--offline'") && script.contains("'--skip-build'"));
}

#[test]
fn the_commits_job_lints_a_real_train_with_the_base_the_tick_dispatches() {
    use crate::plan_ci::{LintInput, commit_lint_range};
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    let Tick::Built { head, .. } = w.tick() else { panic!() };
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    let base = w.ci.dispatched[0]
        .iter()
        .find(|(k, _)| k == "base")
        .map(|(_, v)| v.clone())
        .unwrap();
    let known = |_: &str| true;
    let input = |base: &str, sha: &str| {
        commit_lint_range(
            &LintInput {
                event: "workflow_dispatch",
                reference: "refs/heads/train/queue",
                before: "",
                sha: Some(sha),
                base,
                base_ref: "",
                app: "",
            },
            &known,
        )
    };
    // What the job asks `./dev plan --commit-range` with the dispatched value: a range, which the real lint accepts.
    let range = input(&base, &head).unwrap().expect("a train's dispatch is linted");
    assert_eq!(range.range, format!("{base}..{head}"));
    git(&w.repos.queue, &["fetch", "--quiet", "origin"]);
    let lint = repo_root().join("conformance/bin/paddock-conformance.mjs");
    let out = paddock_proc::command("node")
        .arg(&lint)
        .args(["commit-lint", "--range", &range.range])
        .current_dir(&w.repos.queue)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "the commit lint of a real train failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // The old input was a ref name, which the range refuses: every train run went red on it.
    assert!(input("origin/main", &head).is_err());
}

// ---- a train is a train only when its structure says so -------------------------------------------------------------------------------

/// One merge of a hand-made train: the branch and approved head its message names, the commit actually merged, an extra file put
/// into the merge.
struct Merge<'a> {
    branch: &'a str,
    msg_head: &'a str,
    parent2: &'a str,
    extra: Option<&'a str>,
}

/// The merge of a pull request the way the queue writes it (the approved head, nothing extra).
fn plain<'a>(branch: &'a str, head: &'a str) -> Merge<'a> {
    Merge {
        branch,
        msg_head: head,
        parent2: head,
        extra: None,
    }
}

const MARKER_BODY: &str = "Queue-Prs: 1\nQueue-Round: 1";

/// A branch pushed over `train/queue` by hand, from main: `before` (a non-merge commit), the merges with the queue's own messages,
/// then the marker commit - `marker_message` (default: the queue's), an extra file in it, or a second parent - and optionally one
/// more commit on top that copies the marker's message.
struct Forged<'a> {
    before: Option<&'a str>,
    merges: Vec<Merge<'a>>,
    marker_message: Option<&'a str>,
    marker_file: Option<&'a str>,
    marker_second_parent: Option<&'a str>,
    on_top: Option<&'a str>,
    prs: &'a str,
    /// Extra trailer lines appended to the marker body.
    body_extra: Option<&'a str>,
    /// A third parent for the first merge (an octopus merge).
    octopus: Option<&'a str>,
}

fn forge(w: &World, f: Forged) -> String {
    let a = &w.repos.author;
    git(a, &["fetch", "--quiet", "origin"]);
    git(a, &["checkout", "--quiet", "-B", "forged", "origin/main"]);
    let put = |file: &str| {
        std::fs::write(a.join(file), "evil\n").unwrap();
        git(a, &["add", file]);
    };
    if let Some(file) = f.before {
        put(file);
        git(a, &["commit", "--quiet", "-m", "chore: sneaked in"]);
    }
    for m in &f.merges {
        let mut args = vec!["merge", "--no-ff", "--no-commit", "--quiet", m.parent2];
        if let Some(o) = f.octopus.filter(|_| std::ptr::eq(m, &f.merges[0])) {
            args.push(o);
        }
        git(a, &args);
        if let Some(x) = m.extra {
            put(x);
        }
        let msg = format!("Merge {} ({}) into train", m.branch, &m.msg_head[..9]);
        git(a, &["commit", "--quiet", "-m", &msg]);
    }
    let mut body = format!("Queue-Prs: {}\nQueue-Round: 1", f.prs);
    if let Some(x) = f.body_extra {
        body.push_str(&format!("\n{x}"));
    }
    let subject = f.marker_message.unwrap_or("chore(queue): train round 1");
    if let Some(x) = f.marker_file {
        put(x);
    }
    if let Some(p2) = f.marker_second_parent {
        git(a, &["merge", "--no-ff", "-s", "ours", "--quiet", "-m", subject, "-m", &body, p2]);
    } else {
        git(a, &["commit", "--quiet", "--allow-empty", "-m", subject, "-m", &body]);
    }
    if let Some(x) = f.on_top {
        put(x);
        git(a, &["commit", "--quiet", "-m", subject, "-m", &body]);
    }
    git(a, &["push", "--quiet", "--force", "origin", "forged:refs/heads/train/queue"]);
    git(a, &["rev-parse", "HEAD"])
}

fn forged_default<'a>(merges: Vec<Merge<'a>>) -> Forged<'a> {
    Forged {
        before: None,
        merges,
        marker_message: None,
        marker_file: None,
        marker_second_parent: None,
        on_top: None,
        prs: "1",
        body_extra: None,
        octopus: None,
    }
}

fn forged_train_is_rebuilt_not_landed(make: impl FnOnce(&World, &str) -> String, what: &str) {
    let mut w = World::new();
    let pr_head = w.approved(1, "crates/x/a.rs", &["x"]);
    let Tick::Built { head, .. } = w.tick() else { panic!("built") };
    let forged = make(&w, &pr_head);
    assert_ne!(forged, head);
    w.green("ci.yml", &forged, 9001);
    let before = w.main();
    let t = w.tick();
    assert!(
        matches!(t, Tick::Built { .. }),
        "{what}: the forged head must be rebuilt, got {t:?}"
    );
    assert_eq!(w.main(), before, "{what}: nothing landed");
    assert_ne!(w.train_head(), forged, "{what}: the forged branch was replaced");
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
}

// One test per forged shape (each builds its own world, about 2 s): in one test they ran 24 s in series and hit the CI bound.
#[test]
fn a_hand_pushed_train_is_rebuilt_a_commit_on_top_of_the_real_train() {
    forged_train_is_rebuilt_not_landed(
        |w, _| {
            // The reviewer's attack: the real train plus evil.txt, with the marker's own message.
            let legit = w.train_head();
            let a = &w.repos.author;
            git(a, &["fetch", "--quiet", "origin"]);
            git(a, &["checkout", "--quiet", "-B", "forged", &legit]);
            let message = git(a, &["log", "-1", "--format=%B"]);
            std::fs::write(a.join("evil.txt"), "evil\n").unwrap();
            git(a, &["add", "evil.txt"]);
            git(a, &["commit", "--quiet", "-m", &message]);
            git(a, &["push", "--quiet", "--force", "origin", "forged:refs/heads/train/queue"]);
            git(a, &["rev-parse", "HEAD"])
        },
        "a commit on top of the real train",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_an_empty_commit_on_top_of_the_real_train() {
    forged_train_is_rebuilt_not_landed(
        |w, _| {
            // An empty commit with the marker's message on top: the tree is right, the chain is not.
            let legit = w.train_head();
            let a = &w.repos.author;
            git(a, &["fetch", "--quiet", "origin"]);
            git(a, &["checkout", "--quiet", "-B", "forged", &legit]);
            let message = git(a, &["log", "-1", "--format=%B"]);
            git(a, &["commit", "--quiet", "--allow-empty", "-m", &message]);
            git(a, &["push", "--quiet", "--force", "origin", "forged:refs/heads/train/queue"]);
            git(a, &["rev-parse", "HEAD"])
        },
        "an empty commit on top of the real train",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_the_marker_commit_changes_the_tree() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            forge(
                w,
                Forged {
                    marker_file: Some("evil.txt"),
                    ..forged_default(vec![plain("feat/pr1", pr)])
                },
            )
        },
        "the marker commit changes the tree",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_merge_that_is_not_what_merging_gives() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            forge(
                w,
                forged_default(vec![Merge {
                    extra: Some("evil.txt"),
                    ..plain("feat/pr1", pr)
                }]),
            )
        },
        "a merge that is not what merging gives",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_merge_of_a_commit_that_is_not_the_approved_head() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            // A descendant of the approved head (not the approved head) is merged, under the approved head's message.
            let a = &w.repos.author;
            git(a, &["checkout", "--quiet", "-B", "descendant", pr]);
            std::fs::write(a.join("sneaky.txt"), "x\n").unwrap();
            git(a, &["add", "."]);
            git(a, &["commit", "--quiet", "-m", "feat: sneaky"]);
            let sneaky = git(a, &["rev-parse", "HEAD"]);
            forge(
                w,
                forged_default(vec![Merge {
                    parent2: &sneaky,
                    ..plain("feat/pr1", pr)
                }]),
            )
        },
        "a merge of a commit that is not the approved head",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_an_extra_merge_the_marker_does_not_list() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            let other = w.repos.branch("other", "docs/other.md");
            forge(w, forged_default(vec![plain("feat/pr1", pr), plain("other", &other)]))
        },
        "an extra merge the marker does not list",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_no_no_ff_merge() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            // No merge commit at all: the pull request's head fast-forwarded, then the marker.
            let a = &w.repos.author;
            git(a, &["fetch", "--quiet", "origin"]);
            git(a, &["checkout", "--quiet", "-B", "forged", pr]);
            git(
                a,
                &[
                    "commit",
                    "--quiet",
                    "--allow-empty",
                    "-m",
                    "chore(queue): train round 1",
                    "-m",
                    MARKER_BODY,
                ],
            );
            git(a, &["push", "--quiet", "--force", "origin", "forged:refs/heads/train/queue"]);
            git(a, &["rev-parse", "HEAD"])
        },
        "no --no-ff merge",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_non_merge_commit_between_main_and_the_first_merge() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            forge(
                w,
                Forged {
                    before: Some("sneaked.txt"),
                    ..forged_default(vec![plain("feat/pr1", pr)])
                },
            )
        },
        "a non-merge commit between main and the first merge",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_merge_with_a_third_parent() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            // An octopus merge: a third parent that changes nothing, so the tree is still what merging two parents gives.
            let a = &w.repos.author;
            git(a, &["checkout", "--quiet", "-B", "empty", "origin/main"]);
            git(a, &["commit", "--quiet", "--allow-empty", "-m", "chore: nothing"]);
            git(a, &["push", "--quiet", "--force", "origin", "empty"]);
            let empty = git(a, &["rev-parse", "HEAD"]);
            forge(
                w,
                Forged {
                    octopus: Some(&empty),
                    ..forged_default(vec![plain("feat/pr1", pr)])
                },
            )
        },
        "a merge with a third parent",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_since_run_on_round_1() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            forge(
                w,
                Forged {
                    body_extra: Some("Queue-Since-Run: 5\nQueue-Since-Workflow: ci.yml"),
                    ..forged_default(vec![plain("feat/pr1", pr)])
                },
            )
        },
        "a since-run on round 1",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_marker_commit_with_two_parents() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            let other = w.repos.branch("other", "docs/other.md");
            forge(
                w,
                Forged {
                    marker_second_parent: Some(&other),
                    ..forged_default(vec![plain("feat/pr1", pr)])
                },
            )
        },
        "a marker commit with two parents",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_marker_message_with_skip_ci() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            forge(
                w,
                Forged {
                    marker_message: Some("chore(queue): train round 1 [skip ci]"),
                    ..forged_default(vec![plain("feat/pr1", pr)])
                },
            )
        },
        "a marker message with [skip ci]",
    );
}

#[test]
fn a_hand_pushed_train_is_rebuilt_a_merge_message_with_skip_ci() {
    forged_train_is_rebuilt_not_landed(
        |w, pr| {
            // The merge message carries the directive; everything else is right.
            let a = &w.repos.author;
            git(a, &["fetch", "--quiet", "origin"]);
            git(a, &["checkout", "--quiet", "-B", "forged", "origin/main"]);
            git(a, &["merge", "--no-ff", "--no-commit", "--quiet", pr]);
            git(
                a,
                &[
                    "commit",
                    "--quiet",
                    "-m",
                    &format!("Merge feat/pr1 ({}) into train [skip ci]", &pr[..9]),
                ],
            );
            git(
                a,
                &[
                    "commit",
                    "--quiet",
                    "--allow-empty",
                    "-m",
                    "chore(queue): train round 1",
                    "-m",
                    MARKER_BODY,
                ],
            );
            git(a, &["push", "--quiet", "--force", "origin", "forged:refs/heads/train/queue"]);
            git(a, &["rev-parse", "HEAD"])
        },
        "a merge message with [skip ci]",
    );
}

#[test]
fn a_train_built_by_hand_exactly_the_way_the_queue_builds_one_is_accepted() {
    let mut w = World::new();
    let pr_head = w.approved(1, "crates/x/a.rs", &["x"]);
    w.tick();
    let hand = forge(&w, forged_default(vec![plain("feat/pr1", &pr_head)]));
    w.run(&hand, 9002, RunState::Running);
    assert_eq!(w.tick(), Tick::Waiting(9002), "a structurally right train is judged, not rebuilt");
}

#[test]
fn a_rejected_marker_steers_nothing_in_the_rebuild() {
    // Two pull requests are in the train. A forged head lists three in its marker, claims a failed run to rerun and has a failed run
    // in CI: if the marker drove the rebuild, the new train would rerun only that run's failures (since_run) and be round 2.
    let mut w = World::new();
    let h1 = w.approved(1, "crates/x/a.rs", &["x"]);
    w.approved(2, "ui/b.ts", &["ui"]);
    w.tick();
    let forged = forge(
        &w,
        Forged {
            prs: "1,2,3",
            marker_message: Some("chore(queue): train round 4"),
            ..forged_default(vec![plain("feat/pr1", &h1)])
        },
    );
    w.run(&forged, 5555, RunState::Failed("failure".into()));
    let Tick::Built { head, prs, .. } = w.tick() else {
        panic!("rebuilt")
    };
    assert_eq!(prs, vec![1, 2], "the pull requests are the labelled ones, not the forged marker's");
    let marker = GitTrain::for_clone(w.repos.queue.clone(), w.repos.origin.to_str().unwrap());
    git(&w.repos.queue, &["fetch", "--quiet", "origin"]);
    let m = real::parse_marker(&git(&w.repos.queue, &["show", "-s", "--format=%B", &head])).unwrap();
    let _ = marker;
    assert_eq!(
        (m.round, m.since_run),
        (1, None),
        "a fresh round: nothing a forged marker says steers it"
    );
    assert!(matches!(w.tick(), Tick::Dispatched { .. }));
    assert!(
        !w.ci.dispatched.last().unwrap().iter().any(|(k, _)| k == "since_run"),
        "no since_run from a forged run"
    );
}

#[test]
fn a_forged_marker_on_a_commit_main_already_has_does_not_label_pull_requests_landed() {
    let mut w = World::new();
    let pr_head = w.approved(1, "crates/x/a.rs", &["x"]);
    w.tick();
    // A forged train (the marker on top of an extra commit) lands on main by someone's push; the queue must not call it its own.
    let forged = forge(
        &w,
        Forged {
            on_top: Some("evil.txt"),
            ..forged_default(vec![plain("feat/pr1", &pr_head)])
        },
    );
    git(
        &w.repos.author,
        &["push", "--quiet", "origin", &format!("{forged}:refs/heads/main")],
    );
    let t = w.tick();
    assert!(!matches!(t, Tick::Landed { .. }), "{t:?}");
    assert!(!w.labels_of(1).contains(&"state/landed".to_string()));
}

// ---- self approval ----------------------------------------------------------------------------------------------------------------

// ---- the window's message ---------------------------------------------------------------------------------------------------------

#[test]
fn an_unread_hour_says_the_clock_not_a_tool() {
    let mut w = World::new();
    w.approved(1, "crates/x/a.rs", &["x"]);
    w.tick();
    let t = w.tick_hour(None);
    assert!(
        matches!(&t, Tick::Stuck { why, .. } if why.contains("system clock") && !why.contains("date")),
        "{t:?}"
    );
}

// ---- gate jobs map to pull requests by the files they name -----------------------------------------------------------------------

#[test]
fn a_gate_job_failure_is_mapped_to_the_pull_requests_that_change_its_files_and_stops_when_none_does() {
    let gate = |files: &[&str]| Failure {
        what: "train gates (...): the job failed outside any test".into(),
        projects: set(&["traingate"]),
        files: files.iter().map(|s| s.to_string()).collect(),
        blame: Blame::New,
    };
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"]), (2, "ui/b.ts", &["ui"])]);
    w.run(&head, 9300, RunState::Failed("failure".into()));
    w.ci.failures.insert(9300, vec![gate(&["ui/b.ts"])]);
    let Tick::Dropped { dropped, .. } = w.tick() else { panic!() };
    assert_eq!(dropped, vec![2], "only the pull request that changes the named file");
    let mut w = World::new();
    let head = dispatched_train(&mut w, &[(1, "crates/x/a.rs", &["x"]), (2, "ui/b.ts", &["ui"])]);
    w.run(&head, 9301, RunState::Failed("failure".into()));
    w.ci.failures.insert(9301, vec![gate(&["docs/elsewhere.md"])]);
    assert!(
        matches!(w.tick(), Tick::Stuck { measured: true, .. }),
        "no pull request changes that file: the queue stops"
    );
    assert_eq!(w.labels_of(1), vec!["state/in-train"]);
    assert_eq!(w.labels_of(2), vec!["state/in-train"]);
}

#[test]
fn the_files_a_gate_log_names_are_read_from_it() {
    let text = "error[C99]: crates/x/src/lib.rs:12: bad\n  --> ui/projects/a.ts:3:4\nsee `docs/rules.md` and (apps/dyno/x.rs) not a/b\nreadme.md and 1.2.3\n";
    assert_eq!(
        real::files_named(text),
        vec!["crates/x/src/lib.rs", "ui/projects/a.ts", "docs/rules.md", "apps/dyno/x.rs"]
    );
}

// ---- children carry no token ------------------------------------------------------------------------------------------------------

#[test]
fn the_planner_child_runs_in_the_trusted_checkout_without_the_token() {
    let parent: Vec<String> = ["PATH", "GITEA_TOKEN", "QUEUE_BOT_TOKEN", "CARGO_REGISTRY_TOKEN"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let c = real::planner_command(Path::new("/x/dev"), Path::new("/x/trusted"), "m", "h", &parent);
    assert_eq!(c.get_current_dir(), Some(Path::new("/x/trusted")));
    assert_eq!(
        c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        ["plan", "m", "h"]
    );
    let envs: BTreeMap<String, Option<String>> = c
        .get_envs()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
        .collect();
    for gone in ["GITEA_TOKEN", "QUEUE_BOT_TOKEN", "CARGO_REGISTRY_TOKEN"] {
        assert_eq!(envs.get(gone), Some(&None), "{gone}");
    }
}

#[test]
fn the_merge_checks_git_children_have_hooks_off_and_no_token() {
    let root = tempfile::tempdir().unwrap();
    let g = crate::merge_check::git::Git::new(root.path());
    let parent: Vec<String> = ["GITEA_TOKEN", "QUEUE_BOT_TOKEN", "SOME_SECRET"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let c = g.command_for(&["log"], &parent);
    let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(args.iter().any(|a| a == "core.hooksPath=/dev/null"), "{args:?}");
    let envs: BTreeMap<String, Option<String>> = c
        .get_envs()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
        .collect();
    for gone in ["GITEA_TOKEN", "QUEUE_BOT_TOKEN", "SOME_SECRET"] {
        assert_eq!(envs.get(gone), Some(&None), "{gone}");
    }
}

// ---- the planner never runs in the merge clone ------------------------------------------------------------------------------------

static PLANNER_DIRS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// The most dangerous thing a planner can do: `cargo check` (it runs build scripts, wrappers and toolchain files) in the folder
/// it is given. Run in a pull request's tree it would execute the pull request.
fn cargo_planner(trusted: &Path, _main: &str, _head: &str) -> Result<serde_json::Value, String> {
    PLANNER_DIRS.lock().unwrap().push(trusted.to_path_buf());
    let _ = paddock_proc::command("cargo")
        .args(["check", "--offline", "--quiet"])
        .current_dir(trusted)
        .output();
    Ok(serde_json::json!({"all": false}))
}

#[test]
fn the_planner_runs_cargo_only_in_the_checkout_of_main_never_in_the_merge_clone() {
    let mut w = World::new();
    let sentinel_dir = tempfile::tempdir().unwrap();
    let sentinel = sentinel_dir.path().join("sentinel");
    let head = booby_trapped(&w, "feat/pr1", &sentinel);
    w.gitea.pull(1, "feat/pr1", &head, "open", None, &["state/approved"]);
    w.approve(1, &head);
    // Positive control: cargo check in the pull request's own tree runs its build script and its toolchain wrapper.
    git(&w.repos.author, &["checkout", "--quiet", "feat/pr1"]);
    let _ = paddock_proc::command("cargo")
        .args(["check", "--offline", "--quiet"])
        .current_dir(&w.repos.author)
        .output();
    assert!(sentinel.exists(), "cargo check in a pull request's tree executes it (the control)");
    std::fs::remove_file(&sentinel).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    w.real_train = Some(scratch.path().to_path_buf());
    w.planner = cargo_planner;
    let Tick::Built { head: train, .. } = w.tick() else {
        panic!("built")
    };
    assert!(
        matches!(w.tick(), Tick::Dispatched { .. }),
        "dispatching asks the planner for the workflows"
    );
    w.green("ci.yml", &train, 7100);
    assert!(matches!(w.tick(), Tick::Landed { .. }));
    let ran_in: Vec<PathBuf> = PLANNER_DIRS
        .lock()
        .unwrap()
        .iter()
        .filter(|d| d.starts_with(scratch.path()))
        .cloned()
        .collect();
    assert!(!ran_in.is_empty(), "the planner was asked");
    for d in &ran_in {
        assert_eq!(d, &scratch.path().join("trusted"), "the planner runs in the checkout of main only");
    }
    assert!(!sentinel.exists(), "cargo never ran a pull request's code");
}
