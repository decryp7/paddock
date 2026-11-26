//! The key, the store and the controls of ER-33 for [`super`]: an unchanged tree reuses every row, a changed input runs that row
//! again, a new tool version runs everything, results that are not a fresh green pass are never stored or read back, and with reuse
//! off everything runs. Scratch repositories only (ER-14); nothing here touches the real repository or starts a real gate.

use super::*;
use crate::pre_push::exec::{Done, Ending, Exec, Kind, Limits, Row, RunOpts, run_all_timed, test_subject};
use crate::pre_push::gates::{Need, Planned};
use crate::pre_push::progress::Progress;
use paddock_repo_graph::Node;
use std::sync::atomic::AtomicBool;

struct Scratch {
    _d: tempfile::TempDir,
    root: PathBuf,
    store: PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        std::fs::create_dir_all(&root).unwrap();
        let s = Scratch {
            store: d.path().join("state").join(DIR),
            _d: d,
            root,
        };
        s.git(&["init", "-q", "-b", "main"]);
        s.git(&["config", "user.email", "t@t"]);
        s.git(&["config", "user.name", "t"]);
        s.git(&["config", "commit.gpgsign", "false"]);
        // Two independent workspaces, `app` depending on `lib` by path, and one file outside both.
        s.write("lib/Cargo.toml", "[package]\nname = \"lib\"\n");
        s.write("lib/src/lib.rs", "\n");
        s.write("app/Cargo.toml", "[package]\nname = \"app\"\n");
        s.write("app/src/lib.rs", "\n");
        s.write("other/Cargo.toml", "[package]\nname = \"other\"\n");
        s.write("other/src/lib.rs", "\n");
        s.commit("readme.txt", "one\n");
        s
    }
    fn git(&self, args: &[&str]) -> String {
        crate::plan_repo::git_text(&self.root, args).unwrap_or_else(|| panic!("git {args:?}"))
    }
    fn write(&self, file: &str, text: &str) {
        let p = self.root.join(file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    fn commit(&self, file: &str, text: &str) {
        self.write(file, text);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", file]);
    }
    fn reuse_with(&self, tool: &str, env: &HashMap<String, String>, off: bool) -> Option<Reuse> {
        Reuse::new(
            &self.root,
            self.store.clone(),
            env,
            "run-1-abc".into(),
            Ok(Dev {
                path: "/held/dev-1234".into(),
                hash: tool.into(),
            }),
            graph(),
            off,
        )
    }
    fn reuse(&self) -> Reuse {
        self.reuse_with("tool-1", &HashMap::new(), false).expect("on")
    }
    fn records(&self) -> usize {
        std::fs::read_dir(&self.store).map_or(0, |d| d.count())
    }
}

/// The planner's Cargo graph of the scratch repository: `app` depends on `lib`; `other` stands alone.
fn graph() -> Graph {
    let mut g = Graph::new();
    let n = |name: &str, deps: &[&str]| Node {
        name: name.into(),
        workspace: name.into(),
        deps: deps.iter().map(|d| d.to_string()).collect(),
    };
    g.insert("lib", n("lib", &[]));
    g.insert("app", n("app", &["lib"]));
    g.insert("other", n("other", &[]));
    g
}

fn gate(id: &str, cwd: &str) -> Gate {
    Gate {
        id: id.into(),
        cwd: cwd.into(),
        program: "stand-in".into(),
        args: vec![],
        env: vec![],
        heavy: false,
        need: Need::Nothing,
        rank: 1,
        next: String::new(),
        workspaces: vec![],
        notice: None,
        progress: Progress::Lines,
        build_dir: None,
        ci_env: false,
    }
}

/// One whole-tree row and two Cargo rows of independent workspaces.
fn rows() -> Vec<Planned> {
    vec![
        Planned::Run(gate("conformance", "")),
        Planned::Run(gate("fmt app", "app")),
        Planned::Run(gate("fmt other", "other")),
    ]
}

/// An executor that records which gates it started and passes (or fails) them all.
struct Counting {
    ran: Mutex<Vec<String>>,
    pass: bool,
}

impl Counting {
    fn new(pass: bool) -> Counting {
        Counting {
            ran: Mutex::new(Vec::new()),
            pass,
        }
    }
    fn take(&self) -> Vec<String> {
        let mut v = std::mem::take(&mut *self.ran.lock().unwrap());
        v.sort();
        v
    }
}

impl Exec for Counting {
    fn has_tool(&self, _: &str) -> bool {
        false
    }
    fn run(&self, g: &Gate, _: Limits) -> Result<Done, String> {
        self.ran.lock().unwrap().push(g.id.clone());
        Ok(Done {
            success: self.pass,
            how: if self.pass { "exit 0".into() } else { "exit 1".into() },
            tail: String::new(),
            log: None,
            timing: None,
            ending: Ending::Exited,
        })
    }
}

fn run(planned: &[Planned], ex: &Counting, reuse: Option<&Reuse>) -> Vec<(String, Row)> {
    let stop = AtomicBool::new(false);
    let subject = test_subject();
    run_all_timed(
        planned,
        ex,
        &RunOpts {
            workers: 2,
            queue_stall: Duration::from_secs(60),
            stop: &stop,
            subject: &subject,
            reuse,
        },
    )
    .0
}

fn all(v: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = v.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

/// A re-run of an unchanged tree reuses every row: nothing starts, every row passes, and the table and JSON say when it was green.
#[test]
fn a_rerun_of_an_unchanged_tree_reuses_every_row() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let first = run(&rows(), &ex, Some(&s.reuse()));
    assert_eq!(
        ex.take(),
        all(&["conformance", "fmt app", "fmt other"]),
        "the first run runs every row"
    );
    assert!(first.iter().all(|(_, r)| r.reused.is_none()));
    assert_eq!(s.records(), 3, "three green results stored");

    let second = run(&rows(), &ex, Some(&s.reuse()));
    assert!(ex.take().is_empty(), "the second run of the same tree starts nothing");
    for (id, r) in &second {
        assert_eq!(r.kind(), Kind::Passed, "{id}");
        let green = r.reused.as_ref().unwrap_or_else(|| panic!("{id} is reused"));
        assert_eq!(green.run, "run-1-abc");
    }
    let report = crate::pre_push::render_timed("head", &second, &Default::default());
    assert!(
        report.text.contains("reused (green at 20") && report.text.contains("Z)"),
        "the table says when it was green: {}",
        report.text
    );
    assert!(report.data.to_string().contains("\"green_at\""), "{}", report.data);
}

/// A changed input runs that row again; the rows whose inputs did not change are still reused.
#[test]
fn a_changed_input_runs_that_row_again() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let planned = vec![Planned::Run(gate("fmt app", "app")), Planned::Run(gate("fmt other", "other"))];
    run(&planned, &ex, Some(&s.reuse()));
    ex.take();
    s.commit("app/src/lib.rs", "pub fn f() {}\n");
    run(&planned, &ex, Some(&s.reuse()));
    assert_eq!(ex.take(), all(&["fmt app"]), "the workspace's own file changed: only its row runs");
    s.commit("lib/src/lib.rs", "pub fn g() {}\n");
    run(&planned, &ex, Some(&s.reuse()));
    assert_eq!(
        ex.take(),
        all(&["fmt app"]),
        "a path dependency's file changed: the row that depends on it runs"
    );
    s.commit("readme.txt", "two\n");
    run(&planned, &ex, Some(&s.reuse()));
    assert!(ex.take().is_empty(), "a file no Cargo row reads changes nothing");
    run(&[Planned::Run(gate("conformance", ""))], &ex, Some(&s.reuse()));
    s.commit("readme.txt", "three\n");
    run(&[Planned::Run(gate("conformance", ""))], &ex, Some(&s.reuse()));
    assert_eq!(
        ex.take(),
        all(&["conformance", "conformance"]),
        "a whole-tree row runs after any commit"
    );
}

/// An uncommitted change to an input is never reused and never stored.
#[test]
fn an_uncommitted_input_is_never_reused_or_stored() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    run(&rows(), &ex, Some(&s.reuse()));
    ex.take();
    s.write("app/src/lib.rs", "dirty\n");
    let before = s.records();
    run(&rows(), &ex, Some(&s.reuse()));
    assert_eq!(
        ex.take(),
        all(&["conformance", "fmt app"]),
        "the dirty workspace and the whole tree run"
    );
    assert_eq!(s.records(), before, "nothing stored for a dirty input");
}

/// A row `dev` computes itself (its program is this binary).
fn dev_row(id: &str) -> Planned {
    let mut g = gate(id, "");
    g.program = "/held/dev-1234".into();
    Planned::Run(g)
}

/// Another `dev` binary runs again the rows `dev` computes itself, and only those: a Cargo row it merely starts is keyed on its
/// sources and the toolchain (user, 2026-11-25).
#[test]
fn a_new_dev_reruns_the_rows_dev_computes_and_reuses_the_cargo_rows() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let planned = vec![
        dev_row("gates-register"),
        tests_row("app"),
        Planned::Run(gate("fmt other", "other")),
    ];
    run(&planned, &ex, Some(&s.reuse()));
    ex.take();
    let newer = s.reuse_with("tool-2", &HashMap::new(), false).unwrap();
    let got = run(&planned, &ex, Some(&newer));
    assert_eq!(ex.take(), all(&["gates-register"]), "another dev: only the row dev computes runs");
    assert!(got.iter().filter(|(id, _)| id != "gates-register").all(|(_, r)| r.reused.is_some()));
    // Positive control: the same rows with the first dev again are all reused, the dev row included.
    run(&planned, &ex, Some(&s.reuse()));
    assert!(ex.take().is_empty());
    // A build variable is part of a Cargo row's key too.
    let env: HashMap<String, String> = [("RUSTFLAGS".to_string(), "-Dwarnings".to_string())].into();
    run(&rows(), &ex, Some(&s.reuse()));
    ex.take();
    run(&rows(), &ex, Some(&s.reuse_with("tool-1", &env, false).unwrap()));
    assert_eq!(ex.take(), all(&["fmt app", "fmt other"]), "another RUSTFLAGS: the Cargo rows run");
}

/// Not measured and inactive are never passes: a row skipped as inactive or planned as not measured stays so even when its id has a
/// green record, and a gate that ends not measured, fails or is skipped is never stored.
#[test]
fn not_measured_and_inactive_are_never_reused() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    run(&rows(), &ex, Some(&s.reuse()));
    ex.take();
    let planned = vec![
        Planned::Skip {
            id: "fmt app".into(),
            reason: format!("{} (the app is not active)", crate::pre_push::gates::inactive_label()),
        },
        Planned::NotMeasured {
            id: "conformance".into(),
            reason: "environment not ready".into(),
            next: "install".into(),
        },
    ];
    let got = run(&planned, &ex, Some(&s.reuse()));
    assert_eq!(got[0].1.kind(), Kind::Skipped);
    assert_eq!(got[1].1.kind(), Kind::NotMeasured);
    assert!(got.iter().all(|(_, r)| r.reused.is_none()));

    // A gate that cannot run (its tool is missing) ends not measured: never stored, so never reused next time.
    let s = Scratch::new();
    let mut g = gate("fmt app", "app");
    g.need = Need::Tool("cargo-missing".into());
    for _ in 0..2 {
        let got = run(&[Planned::Run(g.clone())], &ex, Some(&s.reuse()));
        assert_eq!(got[0].1.kind(), Kind::NotMeasured);
        assert!(got[0].1.reused.is_none());
    }
    assert_eq!(s.records(), 0, "a not-measured row is never stored");

    // A failure is never stored: it runs every time.
    let bad = Counting::new(false);
    run(&rows(), &bad, Some(&s.reuse()));
    run(&rows(), &bad, Some(&s.reuse()));
    assert_eq!(bad.take().len(), 6, "every failing row ran twice");
    assert_eq!(s.records(), 0);
}

#[test]
fn only_a_fresh_pass_is_stored() {
    let s = Scratch::new();
    let r = s.reuse();
    let g = gate("gates-register", "");
    let Lookup::Miss(Some(prep)) = r.lookup(&g) else {
        panic!("an empty store misses with a key")
    };
    let green = Reused {
        at: 1,
        run: "earlier".into(),
    };
    let mut excused = Row::not_measured("x", "y");
    excused.excused_by = Some("person".into());
    for row in [
        Row::not_measured("no docker", "start it"),
        Row::broken("cannot read", "fix"),
        Row::skipped("not reached"),
        Row::failed("gates-register", &test_subject(), "red".into(), String::new(), String::new(), 1.0),
        Row::reused("gates-register", &test_subject(), green),
        excused,
    ] {
        r.store(&g, &prep, &row);
        assert!(matches!(r.lookup(&g), Lookup::Miss(Some(_))), "{:?} must not be stored", row.kind());
    }
    assert_eq!(s.records(), 0, "nothing was written");
}

/// Parallel workers store their passes at the same moment: every record lands. Two writers in one process in the same second used
/// one temporary file name, so one rename found no file ("No such file or directory") and that pass was lost (macOS, 2026-11-25).
#[test]
fn passes_stored_at_once_by_parallel_workers_all_land() {
    let s = Scratch::new();
    let r = s.reuse();
    let gates = [gate("fmt app", "app"), gate("fmt other", "other"), gate("conformance", "")];
    let preps: Vec<Prepared> = gates
        .iter()
        .map(|g| match r.lookup(g) {
            Lookup::Miss(Some(p)) => p,
            _ => panic!("{}: an empty store misses with a key", g.id),
        })
        .collect();
    let pass = |id: &str| {
        let done = Done {
            success: true,
            how: "exit 0".into(),
            tail: String::new(),
            log: None,
            timing: None,
            ending: Ending::Exited,
        };
        Row::from_run(id, &test_subject(), &done, "", 1.0)
    };
    for round in 0..20 {
        for f in std::fs::read_dir(&s.store).into_iter().flatten().flatten() {
            std::fs::remove_file(f.path()).unwrap();
        }
        let start = std::sync::Barrier::new(gates.len());
        std::thread::scope(|t| {
            for (g, prep) in gates.iter().zip(&preps) {
                let (r, start, row) = (&r, &start, pass(&g.id));
                t.spawn(move || {
                    start.wait();
                    r.store(g, prep, &row);
                });
            }
        });
        let stored = gates.iter().filter(|g| matches!(r.lookup(g), Lookup::Hit(_))).count();
        assert_eq!(stored, gates.len(), "round {round}: every pass stored at once lands");
    }
}

/// Positive control: with reuse switched off (`--no-reuse` or `PADDOCK_NO_REUSE`), everything runs although green records exist.
#[test]
fn with_reuse_off_everything_runs() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    run(&rows(), &ex, Some(&s.reuse()));
    run(&rows(), &ex, Some(&s.reuse()));
    assert_eq!(ex.take().len(), 3, "on: the second run started nothing");
    assert!(s.reuse_with("tool-1", &HashMap::new(), true).is_none(), "--no-reuse");
    let env: HashMap<String, String> = [(ENV_OFF.to_string(), "1".to_string())].into();
    assert!(s.reuse_with("tool-1", &env, false).is_none(), "PADDOCK_NO_REUSE=1");
    let env0: HashMap<String, String> = [(ENV_OFF.to_string(), "0".to_string())].into();
    assert!(s.reuse_with("tool-1", &env0, false).is_some(), "PADDOCK_NO_REUSE=0 leaves it on");
    let got = run(&rows(), &ex, None);
    assert_eq!(ex.take(), all(&["conformance", "fmt app", "fmt other"]), "off: every row runs");
    assert!(got.iter().all(|(_, r)| r.reused.is_none()));
}

/// ER-33 control: when the tool version cannot be known the key cannot be made, so every row runs (never a reuse on an unknown tool).
#[test]
fn an_unknown_tool_version_means_no_reuse() {
    let s = Scratch::new();
    let r = Reuse::new(
        &s.root,
        s.store.clone(),
        &HashMap::new(),
        "r".into(),
        Err("it is a test program".into()),
        graph(),
        false,
    )
    .unwrap();
    assert!(matches!(r.lookup(&gate("conformance", "")), Lookup::Miss(None)));
}

/// A row that starts this binary again names it by a held link whose name carries the run's pid: the key must not depend on it.
#[test]
fn the_dev_binarys_path_is_not_in_the_key_but_its_hash_is() {
    let s = Scratch::new();
    let key = |path: &str, hash: &str| {
        let r = Reuse::new(
            &s.root,
            s.store.clone(),
            &HashMap::new(),
            "r".into(),
            Ok(Dev {
                path: path.into(),
                hash: hash.into(),
            }),
            graph(),
            false,
        )
        .unwrap();
        let mut g = gate("conformance", "");
        g.program = path.into();
        r.prepare(&g).unwrap().key
    };
    assert_eq!(
        key("/h/dev-1", "a"),
        key("/h/dev-2", "a"),
        "another link name, same binary: same key"
    );
    assert_ne!(key("/h/dev-1", "a"), key("/h/dev-1", "b"), "another binary: another key");
}

#[test]
fn the_checkout_path_and_build_folder_are_not_in_the_key() {
    let args: Vec<String> = [
        "clippy",
        "--config",
        "build.target-dir='/x/y'",
        "--manifest-path",
        "/w/r/Cargo.toml",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        normalised_args(&args, "/w/r"),
        vec!["clippy", "--manifest-path", "<root>/Cargo.toml"]
    );
}

#[test]
fn only_rows_that_read_nothing_but_the_tree_have_a_class() {
    let c = |id: &str, cwd: &str| class_of(&gate(id, cwd));
    assert_eq!(c("clippy apps/dyno", "apps/dyno"), Some(Class::Cargo(vec!["apps/dyno".into()])));
    assert_eq!(c("conformance", ""), Some(Class::Whole));
    assert_eq!(
        c("member-audits", ""),
        Some(Class::Whole),
        "the audits read the whole tree: keyed like conformance"
    );
    for never in ["commit-lint", "locks-guard", "pitwall-deny", "dyno-wire-fixtures", "something-new"] {
        assert_eq!(c(never, ""), None, "{never} is never reused");
    }
}

#[test]
fn cargo_inputs_follow_path_dependencies_and_their_workspace_settings() {
    let got = cargo_inputs(&graph(), &["app".to_string()]).unwrap();
    for p in [
        "app",
        "app/Cargo.lock",
        "lib",
        "lib/Cargo.toml",
        "lib/Cargo.lock",
        "rust-toolchain.toml",
        ".cargo",
    ] {
        assert!(got.contains(p), "{p} in {got:?}");
    }
    assert!(!got.iter().any(|p| p.starts_with("other")), "{got:?}");
    assert!(
        cargo_inputs(&graph(), &["nowhere".to_string()]).is_err(),
        "an unknown workspace cannot be keyed"
    );
}

#[test]
fn a_record_is_read_only_for_its_own_key_row_and_verdict() {
    let rec = Record {
        row: "fmt crates".into(),
        green: Reused { at: 5, run: "r1".into() },
        secs: 1.5,
    };
    let text = rec.json("k1", &["crates abc".into()]).to_string();
    assert_eq!(Record::parse(&text, "k1", "fmt crates"), Some(rec));
    assert_eq!(Record::parse(&text, "k2", "fmt crates"), None, "another key");
    assert_eq!(Record::parse(&text, "k1", "fmt apps/x"), None, "another row");
    assert_eq!(
        Record::parse(&text.replace("clean", "findings"), "k1", "fmt crates"),
        None,
        "not clean"
    );
    assert_eq!(
        Record::parse(&text.replace("\"schema\":1", "\"schema\":0"), "k1", "fmt crates"),
        None,
        "an older schema"
    );
    assert_eq!(Record::parse("not json", "k1", "fmt crates"), None);
}

#[test]
fn the_key_depends_on_where_fields_split() {
    assert_ne!(digest(&["ab".into(), "c".into()]), digest(&["a".into(), "bc".into()]));
    assert_eq!(digest(&["a".into()]), digest(&["a".into()]));
}

#[test]
fn the_green_time_is_rfc_3339_utc() {
    let g = Reused {
        at: 1_795_536_000,
        run: "r".into(),
    };
    assert_eq!(g.text(), "reused (green at 2026-11-24T16:00:00Z)");
}

#[test]
fn eviction_drops_old_records_and_keeps_the_newest() {
    let d = tempfile::tempdir().unwrap();
    // Modified times are set, not waited for: k4 is the newest, k0 the oldest.
    let now = std::time::SystemTime::now();
    for i in 0..5u64 {
        let f = std::fs::File::create(d.path().join(format!("k{i}.json"))).unwrap();
        f.set_modified(now - Duration::from_secs(100 - i)).unwrap();
    }
    std::fs::write(d.path().join("notes.txt"), "kept: not ours").unwrap();
    assert_eq!(evict(d.path(), std::time::SystemTime::now(), MAX_AGE, 3), 2, "count");
    assert!(d.path().join("k4.json").exists() && !d.path().join("k0.json").exists());
    // A crashed writer's temporary file (paddock_env::write_atomic's name) goes with the old records.
    std::fs::write(d.path().join(".k9.json.123.0.tmp"), "{").unwrap();
    let later = std::time::SystemTime::now() + MAX_AGE + Duration::from_secs(60);
    assert_eq!(evict(d.path(), later, MAX_AGE, 100), 4, "age");
    assert!(d.path().join("notes.txt").exists());
}

// The member gate on a train (user, 2026-11-25): each member's author ran the same gate on the member branch, in their own worktree;
// the integrator's run on the train head, in another worktree, reuses a member's green row when the train's inputs of that row are
// the ones the member passed, and runs it again otherwise. The records are shared through the store, as the real store in the
// repository's common git folder is shared by every worktree.

impl Scratch {
    /// A linked worktree `name` on a new branch of the same name from `base`, beside the scratch repository.
    fn worktree(&self, name: &str, base: &str) -> PathBuf {
        let dir = self.store.parent().unwrap().parent().unwrap().join(name);
        self.git(&["worktree", "add", "-q", "-b", name, &dir.to_string_lossy(), base]);
        dir
    }
    /// Reuse for the checkout at `root` with the tool version `tool`, the scratch store shared.
    fn reuse_in(&self, root: &Path, tool: &str) -> Reuse {
        Reuse::new(
            root,
            self.store.clone(),
            &HashMap::new(),
            "run-1-abc".into(),
            Ok(Dev {
                path: "/held/dev-1234".into(),
                hash: tool.into(),
            }),
            graph(),
            false,
        )
        .expect("on")
    }
}

fn git_in(root: &Path, args: &[&str]) -> String {
    crate::plan_repo::git_text(root, args).unwrap_or_else(|| panic!("git {args:?} in {}", root.display()))
}

fn commit_in(root: &Path, file: &str, text: &str) {
    let p = root.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-q", "-m", file]);
}

fn tests_row(ws: &str) -> Planned {
    Planned::Run(gate(&format!("tests {ws}"), ws))
}

/// Member `a` changes `app` (which depends on `lib`), member `b` changes `other`; each runs its own member gate in its own worktree.
/// Returns the train worktree with both merged (`--no-ff`, as a train merges).
fn train_of_two(s: &Scratch, ex: &Counting, a_passes: bool) -> PathBuf {
    let a = s.worktree("member-a", "main");
    commit_in(&a, "app/src/lib.rs", "pub fn a() {}\n");
    let bad = Counting::new(false);
    let own = if a_passes { ex } else { &bad };
    run(&[tests_row("app")], own, Some(&s.reuse_in(&a, "tool-1")));
    let b = s.worktree("member-b", "main");
    commit_in(&b, "other/src/lib.rs", "pub fn b() {}\n");
    run(&[tests_row("other")], ex, Some(&s.reuse_in(&b, "tool-1")));
    ex.take();
    let t = s.worktree("train", "main");
    git_in(&t, &["merge", "-q", "--no-ff", "-m", "merge a", "member-a"]);
    git_in(&t, &["merge", "-q", "--no-ff", "-m", "merge b", "member-b"]);
    t
}

/// A workspace only one member touched reuses that member's pass on the train head, though the train's whole tree is new and the
/// train is another worktree: the member's tests rows do not run again.
#[test]
fn a_train_reuses_each_members_pass_of_a_workspace_only_it_touched() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let t = train_of_two(&s, &ex, true);
    let got = run(&[tests_row("app"), tests_row("other")], &ex, Some(&s.reuse_in(&t, "tool-1")));
    assert!(ex.take().is_empty(), "each workspace has its member's green row");
    assert!(got.iter().all(|(_, r)| r.kind() == Kind::Passed && r.reused.is_some()), "{got:?}");
    // Positive control: the same train with reuse off runs both rows.
    run(&[tests_row("app"), tests_row("other")], &ex, None);
    assert_eq!(ex.take(), all(&["tests app", "tests other"]));
}

/// A workspace two members touched has inputs neither member passed: its row runs on the train; the other is still reused.
#[test]
fn a_workspace_two_members_touched_runs_on_the_train() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let t = train_of_two(&s, &ex, true);
    // A third member touches `app` too (another file, no conflict) and passes its own gate.
    let c = s.worktree("member-c", "main");
    commit_in(&c, "app/src/more.rs", "pub fn c() {}\n");
    run(&[tests_row("app")], &ex, Some(&s.reuse_in(&c, "tool-1")));
    ex.take();
    git_in(&t, &["merge", "-q", "--no-ff", "-m", "merge c", "member-c"]);
    run(&[tests_row("app"), tests_row("other")], &ex, Some(&s.reuse_in(&t, "tool-1")));
    assert_eq!(ex.take(), all(&["tests app"]), "app has a's and c's changes together");
}

/// A conflict resolution that writes what no member had, and a train fix commit, run the rows whose inputs they change.
#[test]
fn a_conflict_resolution_or_a_train_fix_runs_the_rows_it_changes() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let t = train_of_two(&s, &ex, true);
    // Member d edits the line member a edited: the merge conflicts and the integrator writes both.
    let d = s.worktree("member-d", "main");
    commit_in(&d, "app/src/lib.rs", "pub fn d() {}\n");
    run(&[tests_row("app")], &ex, Some(&s.reuse_in(&d, "tool-1")));
    ex.take();
    assert!(
        crate::plan_repo::git_text(&t, &["merge", "-q", "--no-ff", "-m", "merge d", "member-d"]).is_none(),
        "it conflicts"
    );
    commit_in(&t, "app/src/lib.rs", "pub fn a() {}\npub fn d() {}\n");
    run(&[tests_row("app"), tests_row("other")], &ex, Some(&s.reuse_in(&t, "tool-1")));
    assert_eq!(ex.take(), all(&["tests app"]), "the resolved file is in no member's tree");
    // A fix commit on the train to `other`: its row runs, app's (green on this train now) is reused.
    commit_in(&t, "other/src/lib.rs", "pub fn b() {}\npub fn fix() {}\n");
    run(&[tests_row("app"), tests_row("other")], &ex, Some(&s.reuse_in(&t, "tool-1")));
    assert_eq!(ex.take(), all(&["tests other"]));
}

/// A nextest row as the gate plans it (cargo starts it), so the key holds the toolchain and nextest versions.
fn nextest_row(ws: &str) -> Planned {
    let mut g = gate(&format!("tests {ws}"), ws);
    g.program = "cargo".into();
    g.args = vec!["nextest".into(), "run".into(), "--profile".into(), "ci".into()];
    Planned::Run(g)
}

/// The probe answers a test gives instead of starting cargo (the probes' cache is filled first, so nothing real runs).
fn with_versions(r: Reuse, rustc: &str, nextest: &str) -> Reuse {
    let mut m = r.probes.lock().unwrap();
    m.insert("cargo --version".into(), Ok("cargo 1.0.0".into()));
    m.insert("rustc -vV".into(), Ok(rustc.into()));
    m.insert("cargo nextest --version".into(), Ok(nextest.into()));
    drop(m);
    r
}

/// The train's toolchain or nextest differs from the member's: its tests rows run. Another `dev` alone does not run them.
#[test]
fn a_train_with_another_toolchain_or_nextest_runs_the_tests_rows() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let a = s.worktree("member-a", "main");
    commit_in(&a, "app/src/lib.rs", "pub fn a() {}\n");
    run(
        &[nextest_row("app")],
        &ex,
        Some(&with_versions(s.reuse_in(&a, "tool-1"), "rustc 1.90.0", "nextest 0.9.100")),
    );
    ex.take();
    let t = s.worktree("train", "main");
    git_in(&t, &["merge", "-q", "--no-ff", "-m", "merge a", "member-a"]);
    run(
        &[nextest_row("app")],
        &ex,
        Some(&with_versions(s.reuse_in(&t, "tool-2"), "rustc 1.90.0", "nextest 0.9.100")),
    );
    assert!(ex.take().is_empty(), "same toolchain and nextest, another dev: reused");
    run(
        &[nextest_row("app")],
        &ex,
        Some(&with_versions(s.reuse_in(&t, "tool-1"), "rustc 1.91.0", "nextest 0.9.100")),
    );
    assert_eq!(ex.take(), all(&["tests app"]), "another rustc");
    run(
        &[nextest_row("app")],
        &ex,
        Some(&with_versions(s.reuse_in(&t, "tool-1"), "rustc 1.90.0", "nextest 0.9.101")),
    );
    assert_eq!(ex.take(), all(&["tests app"]), "another nextest");
}

/// A member whose own row failed left no record: the train runs that row; the other member's pass is reused.
#[test]
fn a_members_failed_row_is_never_reused_on_the_train() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let t = train_of_two(&s, &ex, false);
    run(&[tests_row("app"), tests_row("other")], &ex, Some(&s.reuse_in(&t, "tool-1")));
    assert_eq!(ex.take(), all(&["tests app"]));
}

/// The tests read more than their Cargo inputs: the shared UI package's generated files and tokens (crates' and Dyno's tests) and the
/// workflows (Dyno's and Pitwall's release tests). A change there runs the tests rows, not the format row of the same workspace.
#[test]
fn a_tests_row_also_keys_on_what_tests_read_outside_the_workspace() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let planned = vec![tests_row("app"), Planned::Run(gate("fmt app", "app"))];
    run(&planned, &ex, Some(&s.reuse()));
    ex.take();
    for (n, file) in ["ui/projects/paddock-ui/shell/generated/x.ts", ".gitea/workflows/app-release.yml"]
        .iter()
        .enumerate()
    {
        s.commit(file, &format!("{n}\n"));
        run(&planned, &ex, Some(&s.reuse()));
        assert_eq!(ex.take(), all(&["tests app"]), "{file}");
    }
    s.commit("ui/projects/gallery/x.ts", "gallery\n");
    run(&planned, &ex, Some(&s.reuse()));
    assert!(ex.take().is_empty(), "a file outside what the tests read changes nothing");
}

#[test]
fn the_member_gates_test_rows_have_a_class() {
    let c = |id: &str, cwd: &str, ws: &[&str]| {
        let mut g = gate(id, cwd);
        g.workspaces = ws.iter().map(|w| w.to_string()).collect();
        class_of(&g)
    };
    assert_eq!(c("tests apps/dyno", "apps/dyno", &[]), Some(Class::Tests(vec!["apps/dyno".into()])));
    // The developer tool's tests read the whole repository (contracts, goldens, workflows): keyed on the whole tree.
    assert_eq!(c("tests tools/paddock-dev", "tools/paddock-dev", &[]), Some(Class::Whole));
    assert_eq!(c("tests", "", &[]), None, "the summary row of a skip is never a run");
}

/// The member audits (ADR 0116 amendment 2026-11-26) are reused on an exact-tree repeat and run again once the tree changes: the member
/// gate's pass is the push's, and the train-audits guard's, as long as nothing under the tree moved.
#[test]
fn the_member_audits_are_reused_on_the_same_tree_and_run_again_when_the_tree_changes() {
    let s = Scratch::new();
    let ex = Counting::new(true);
    let audits = || vec![Planned::Run(gate("member-audits", "tools/paddock-dev"))];
    run(&audits(), &ex, Some(&s.reuse()));
    assert_eq!(ex.take(), all(&["member-audits"]), "the first run runs it");
    let again = run(&audits(), &ex, Some(&s.reuse()));
    assert!(ex.take().is_empty(), "the same tree: the audits are not run again");
    assert!(
        again.iter().all(|(_, r)| r.reused.is_some() && r.kind() == Kind::Passed),
        "{again:?}"
    );
    // Positive control: a change anywhere in the tree (a file the audits could read) runs them again.
    s.commit("docs/elsewhere.md", "changed\n");
    run(&audits(), &ex, Some(&s.reuse()));
    assert_eq!(ex.take(), all(&["member-audits"]), "a changed tree: the audits run again");
}
