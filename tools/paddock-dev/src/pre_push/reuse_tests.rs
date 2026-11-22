//! The key, the store and the controls of ER-33 for [`super`]: a row whose input changed runs again, and a result that is not a fresh
//! green pass is never stored and never read back. Scratch repositories only (ER-14); nothing here touches the real repository.

use super::*;
use crate::pre_push::exec::{Done, Ending, Exec, Limits, Row, RunOpts, run_all_timed, test_subject};
use crate::pre_push::gates::{Gate, Need, Planned};
use crate::pre_push::progress::Progress;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
            store: d.path().join("store"),
            _d: d,
            root,
        };
        s.git(&["init", "-q", "-b", "main"]);
        s.git(&["config", "user.email", "t@t"]);
        s.git(&["config", "user.name", "t"]);
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
    fn reuse(&self) -> Reuse {
        Reuse::new(&self.root, self.store.clone(), &HashMap::new(), "run-1-abc".into(), false).expect("on")
    }
}

fn gate(id: &str, cwd: &str, program: &str) -> Gate {
    Gate {
        id: id.into(),
        cwd: cwd.into(),
        program: program.into(),
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
    }
}

fn fresh(g: &Gate) -> Row {
    Row::reused(&g.id, &test_subject(), String::new()).tap_fresh()
}

trait Tap {
    fn tap_fresh(self) -> Row;
}
impl Tap for Row {
    /// A row as a real run makes it: clean, not reused.
    fn tap_fresh(mut self) -> Row {
        self.reused = None;
        self
    }
}

#[test]
fn the_checkout_path_and_build_folder_are_not_in_the_key() {
    let args: Vec<String> = ["clippy", "--config", "build.target-dir=/x/y", "--manifest-path", "/w/r/Cargo.toml"]
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
    let c = |id: &str, cwd: &str| class_of(&gate(id, cwd, "cargo"));
    assert_eq!(c("clippy apps/dyno", "apps/dyno"), Some(Class::Cargo(vec!["apps/dyno".into()])));
    assert_eq!(c("conformance", ""), Some(Class::Whole));
    for never in [
        "commit-lint",
        "member-audits",
        "pitwall-deny",
        "dyno-wire-fixtures",
        "something-new",
    ] {
        assert_eq!(c(never, ""), None, "{never} is never reused");
    }
}

#[test]
fn a_record_is_read_only_for_its_own_key_row_and_verdict() {
    let rec = Record {
        row: "fmt crates".into(),
        run: "r1".into(),
        at: 5,
        secs: 1.5,
    };
    let text = rec.json("k1", "dev", &["crates abc".into()]).to_string();
    assert_eq!(Record::parse(&text, "k1", "fmt crates"), Some(rec));
    assert_eq!(Record::parse(&text, "k2", "fmt crates"), None, "another key");
    assert_eq!(Record::parse(&text, "k1", "fmt apps/x"), None, "another row");
    assert_eq!(
        Record::parse(&text.replace("clean", "findings"), "k1", "fmt crates"),
        None,
        "not a clean verdict"
    );
    assert_eq!(
        Record::parse(&text.replace("\"schema\":1", "\"schema\":0"), "k1", "fmt crates"),
        None,
        "an older schema"
    );
    assert_eq!(Record::parse("not json", "k1", "fmt crates"), None);
}

#[test]
fn cargo_paths_name_the_workspace_its_manifests_and_its_path_dependencies() {
    let json =
        r#"{"workspace_root":"/w/r/apps/dyno","packages":[{"dependencies":[{"name":"a","path":"/w/r/crates/a"},{"name":"serde"}]}]}"#;
    let (root, own, deps) = cargo_paths(json, "/w/r", "apps/dyno").unwrap();
    assert_eq!(root, "apps/dyno");
    assert!(own.contains("apps/dyno") && own.contains("apps/dyno/Cargo.lock") && own.contains("apps/dyno/Cargo.toml"));
    assert_eq!(deps.into_iter().collect::<Vec<_>>(), vec!["crates/a"]);
    assert!(cargo_paths(r#"{"workspace_root":"/elsewhere","packages":[]}"#, "/w/r", "x").is_err());
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
    let later = std::time::SystemTime::now() + MAX_AGE + Duration::from_secs(60);
    assert_eq!(evict(d.path(), later, MAX_AGE, 100), 3, "age");
    assert!(d.path().join("notes.txt").exists());
}

/// ER-33 control: the same inputs hit; one changed input file (a commit) misses; an uncommitted change is never reused or stored.
#[test]
fn one_changed_input_file_runs_the_row_again() {
    let s = Scratch::new();
    s.commit("a.txt", "one\n");
    let r = s.reuse();
    let g = gate("conformance", "", "");
    let Lookup::Miss(Some(prep)) = r.lookup(&g, &test_subject()) else {
        panic!("an empty store misses")
    };
    r.store(&g, &prep, &fresh(&g));
    assert!(matches!(r.lookup(&g, &test_subject()), Lookup::Hit(_)), "same tree, same key: hit");
    let hit = match r.lookup(&g, &test_subject()) {
        Lookup::Hit(row) => *row,
        _ => unreachable!(),
    };
    assert!(
        hit.reused.as_deref().is_some_and(|n| n.contains("run-1-abc")),
        "the row names the earlier run"
    );
    assert_eq!(hit.kind(), crate::pre_push::exec::Kind::Passed);
    // A new run (a new `Reuse`) after one input file changed: the key differs, so the row runs.
    s.commit("a.txt", "two\n");
    let r = s.reuse();
    assert!(
        matches!(r.lookup(&g, &test_subject()), Lookup::Miss(Some(_))),
        "an input changed: the row runs"
    );
    s.write("a.txt", "three\n");
    assert!(
        matches!(s.reuse().lookup(&g, &test_subject()), Lookup::Miss(None)),
        "a dirty tree is never reused or stored"
    );
}

#[test]
fn only_a_fresh_pass_is_stored() {
    let s = Scratch::new();
    s.commit("a.txt", "one\n");
    let r = s.reuse();
    let g = gate("gates-register", "", "");
    let Lookup::Miss(Some(prep)) = r.lookup(&g, &test_subject()) else {
        panic!()
    };
    let not_run = [
        Row::not_measured("no docker", "start it"),
        Row::broken("cannot read", "fix"),
        Row::skipped("not reached"),
        Row::failed("gates-register", &test_subject(), "red".into(), String::new(), String::new(), 1.0),
        Row::reused("gates-register", &test_subject(), "earlier".into()),
    ];
    for row in &not_run {
        r.store(&g, &prep, row);
        assert!(
            matches!(r.lookup(&g, &test_subject()), Lookup::Miss(Some(_))),
            "{:?} must not be stored",
            row.kind()
        );
    }
    let mut excused = Row::not_measured("x", "y");
    excused.excused_by = Some("person".into());
    r.store(&g, &prep, &excused);
    assert!(matches!(r.lookup(&g, &test_subject()), Lookup::Miss(Some(_))));
    assert!(
        !r.store.exists() || std::fs::read_dir(&r.store).unwrap().count() == 0,
        "nothing was written"
    );
}

#[test]
fn a_cargo_row_keys_on_its_workspace_and_path_dependencies_only() {
    let s = Scratch::new();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-toolchain.toml"),
        s.root.join("rust-toolchain.toml"),
    )
    .unwrap();
    for (dir, deps) in [("lib", ""), ("app", "lib = { path = \"../lib\" }\n")] {
        s.write(
            &format!("{dir}/Cargo.toml"),
            &format!("[package]\nname = \"{dir}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\n{deps}"),
        );
        s.write(&format!("{dir}/src/lib.rs"), "");
    }
    s.commit("other/readme.txt", "x\n");
    let r = s.reuse();
    let g = gate("fmt app", "app", "cargo");
    let key = |r: &Reuse| r.prepare(&g).expect("the key is made").key;
    let k1 = key(&r);
    s.commit("other/readme.txt", "changed\n");
    assert_eq!(
        key(&s.reuse()),
        k1,
        "a file outside the workspace and its path dependencies does not change the key"
    );
    s.commit("lib/src/lib.rs", "pub fn f() {}\n");
    assert_ne!(key(&s.reuse()), k1, "a path dependency's file does");
    let k2 = key(&s.reuse());
    s.commit("app/src/lib.rs", "pub fn g() {}\n");
    assert_ne!(key(&s.reuse()), k2, "the workspace's own file does");
}

struct Counting {
    reuse: Option<Reuse>,
    ran: AtomicUsize,
    pass: bool,
}

impl Exec for Counting {
    fn has_tool(&self, _: &str) -> bool {
        true
    }
    fn docker_up(&self) -> bool {
        true
    }
    fn run(&self, _: &Gate, _: Limits) -> Result<Done, String> {
        self.ran.fetch_add(1, Ordering::SeqCst);
        Ok(Done {
            success: self.pass,
            how: if self.pass { "exit 0".into() } else { "exit 1".into() },
            tail: String::new(),
            log: None,
            timing: None,
            ending: Ending::Exited,
        })
    }
    fn reuse(&self) -> Option<&Reuse> {
        self.reuse.as_ref()
    }
}

fn run_once(ex: &Counting) -> Vec<(String, Row)> {
    let stop = AtomicBool::new(false);
    let subject = test_subject();
    run_all_timed(
        &[Planned::Run(gate("conformance", "", ""))],
        ex,
        &RunOpts {
            workers: 1,
            budget: Duration::from_secs(60),
            queue: Duration::from_secs(60),
            stop: &stop,
            subject: &subject,
        },
    )
    .0
}

/// Through the executor: the second run of an unchanged tree starts nothing; a failing row is never reused; `--no-reuse` runs it.
#[test]
fn the_second_run_of_an_unchanged_tree_starts_nothing() {
    let s = Scratch::new();
    s.commit("a.txt", "one\n");
    let ok = Counting {
        reuse: Some(s.reuse()),
        ran: AtomicUsize::new(0),
        pass: true,
    };
    assert!(run_once(&ok)[0].1.reused.is_none());
    let rows = run_once(&ok);
    assert!(rows[0].1.reused.is_some(), "reused");
    assert_eq!(ok.ran.load(Ordering::SeqCst), 1, "the row ran once in two runs");
    s.commit("a.txt", "two\n");
    let ok = Counting {
        reuse: Some(s.reuse()),
        ran: AtomicUsize::new(1),
        pass: true,
    };
    run_once(&ok);
    assert_eq!(ok.ran.load(Ordering::SeqCst), 2, "a changed input ran it again");

    let s2 = Scratch::new();
    s2.commit("a.txt", "one\n");
    let bad = Counting {
        reuse: Some(s2.reuse()),
        ran: AtomicUsize::new(0),
        pass: false,
    };
    run_once(&bad);
    run_once(&bad);
    assert_eq!(bad.ran.load(Ordering::SeqCst), 2, "a failure is never reused");

    let off = Reuse::new(&s.root, s.store.clone(), &HashMap::new(), "r".into(), true);
    assert!(off.is_none(), "--no-reuse");
    let env: HashMap<String, String> = [(ENV_OFF.to_string(), "1".to_string())].into();
    assert!(
        Reuse::new(&s.root, s.store.clone(), &env, "r".into(), false).is_none(),
        "PADDOCK_NO_REUSE"
    );
}
