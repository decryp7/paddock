// ci-runs-reader: tests of the escape monitor: a fixture of the runs list answer for pick_green, no read of any run
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::register::{self, Register, Row, Table};
use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default)]
struct Mem(RefCell<BTreeMap<String, Load>>);

impl Store for Mem {
    fn put(&self, r: &Record) -> Result<(), String> {
        self.0.borrow_mut().insert(r.commit.clone(), Load::Ok(Box::new(r.clone())));
        Ok(())
    }
    fn get(&self, c: &str) -> Load {
        self.0.borrow().get(c).cloned().unwrap_or(Load::Missing)
    }
    fn commits(&self) -> Result<Vec<String>, String> {
        Ok(self.0.borrow().keys().cloned().collect())
    }
    fn remove(&self, c: &str) -> Result<(), String> {
        self.0.borrow_mut().remove(c);
        Ok(())
    }
}

fn c(n: usize) -> String {
    format!("{n:040x}")
}

/// A record for commit `n`: the change reaches `ui` (with a reason); `selected` says whether the plan ran it.
fn rec(n: usize, selected: bool) -> Record {
    let mut r = Record {
        commit: c(n),
        base: Some(c(n.saturating_sub(1))),
        at: 1_000_000,
        reach: BTreeMap::from([("ui".to_string(), vec!["npm ui: a dependency changed".to_string()])]),
        selected: BTreeMap::new(),
    };
    if selected {
        r.selected.insert(
            "ui".into(),
            Sel {
                os: vec!["ubuntu-latest".into()],
                runs: "all".into(),
                targets: None,
            },
        );
    }
    r
}

fn fail_ui() -> Failure {
    Failure {
        project: "ui".into(),
        os: "ubuntu-latest".into(),
        target: "vitest:ui/a.spec.ts".into(),
    }
}

struct Culprit(usize, usize);
impl Runner for Culprit {
    fn fails_at(&mut self, commit: &str, _: &Failure) -> Result<bool, String> {
        self.1 += 1;
        Ok(usize::from_str_radix(commit, 16).unwrap() >= self.0)
    }
}

fn store_of(rs: &[Record]) -> Mem {
    let m = Mem::default();
    for r in rs {
        save(&m, r).unwrap();
    }
    m
}

#[test]
fn an_escape_is_detected_when_a_dropped_target_fails() {
    let m = store_of(&[rec(1, true), rec(2, false), rec(3, true)]);
    let j = judge(&[c(1), c(2), c(3)], &m, &[fail_ui()], &mut NoRunner);
    let e = j.escapes();
    assert_eq!(e.len(), 1, "{j:?}");
    assert_eq!(e[0].commit, c(2));
    assert!(!e[0].bisected, "one commit dropped it: no bisect");
    assert!(e[0].reason.contains("a dependency changed"));
    assert!(!j.clean());
    let lines = j.lines().join("\n");
    assert!(lines.contains("::error::escape: commit 000000000"), "{lines}");
    assert!(
        lines.contains("vitest:ui/a.spec.ts") && lines.contains("npm ui: a dependency changed"),
        "{lines}"
    );
}

#[test]
fn no_escape_when_the_target_was_selected() {
    // Positive control of the test above: the same failure with the plan having run it.
    let m = store_of(&[rec(1, true), rec(2, true), rec(3, true)]);
    let j = judge(&[c(1), c(2), c(3)], &m, &[fail_ui()], &mut NoRunner);
    assert!(j.escapes().is_empty(), "{j:?}");
    assert_eq!(j.outcomes, vec![Outcome::Covered(fail_ui())]);
    assert!(j.clean(), "a red nightly whose targets all ran is no escape");
}

#[test]
fn a_commit_whose_change_could_not_reach_the_project_is_no_escape() {
    let mut r = rec(2, false);
    r.reach.clear();
    let m = store_of(&[rec(1, true), r]);
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut NoRunner);
    assert!(j.escapes().is_empty() && j.clean(), "{j:?}");
}

#[test]
fn a_selection_on_another_os_does_not_cover_the_failure() {
    let mut r = rec(1, true);
    r.selected.get_mut("ui").unwrap().os = vec!["windows-latest".into()];
    let m = store_of(&[r]);
    assert_eq!(judge(&[c(1)], &m, &[fail_ui()], &mut NoRunner).escapes().len(), 1);
}

#[test]
fn labels_cover_what_they_run() {
    let s = |runs: &str, targets: Option<Vec<&str>>| Sel {
        os: vec!["l".into()],
        runs: runs.into(),
        targets: targets.map(|t| t.into_iter().map(String::from).collect()),
    };
    assert!(s("rust", None).covers("l", "nextest:crates"));
    assert!(!s("rust", None).covers("l", "vitest:x"));
    assert!(s("ui", None).covers("l", "vitest:x") && !s("ui", None).covers("l", "nextest:x"));
    // `tests` and `smoke` run no enumerable target: only the named ones count.
    assert!(!s("smoke", None).covers("l", "vitest:x"));
    assert!(s("tests", Some(vec!["vitest:x"])).covers("l", "vitest:x"));
    assert!(!s("tests", Some(vec!["vitest:x"])).covers("l", "vitest:y"));
}

#[test]
fn several_dropping_commits_are_bisected_to_the_culprit() {
    let rs: Vec<Record> = (1..=9).map(|n| rec(n, n % 3 == 0)).collect(); // 1,2,4,5,7,8 dropped it
    let commits: Vec<String> = (1..=9).map(c).collect();
    let m = store_of(&rs);
    let mut runner = Culprit(5, 0);
    let j = judge(&commits, &m, &[fail_ui()], &mut runner);
    let e = j.escapes();
    assert_eq!(e.len(), 1, "{j:?}");
    assert_eq!(e[0].commit, c(5), "the first dropping commit at which the target fails");
    assert!(e[0].bisected);
    assert!(
        runner.1 <= 3 && j.runs_used == runner.1,
        "binary search, not one run per commit: {} runs",
        runner.1
    );
}

#[test]
fn the_bisect_stops_at_the_run_budget_and_says_not_measured() {
    struct Slow;
    impl Runner for Slow {
        fn fails_at(&mut self, _: &str, _: &Failure) -> Result<bool, String> {
            Ok(false)
        }
    }
    // Eleven failures, each needing a bisect of the same two dropping commits: the nightly's budget is MAX_RUNS runs in all.
    let m = store_of(&[rec(1, false), rec(2, false)]);
    let fs: Vec<Failure> = (0..MAX_RUNS + 2)
        .map(|i| Failure {
            project: "ui".into(),
            os: "ubuntu-latest".into(),
            target: format!("vitest:t{i}"),
        })
        .collect();
    let j = judge(&[c(1), c(2)], &m, &fs, &mut Slow);
    assert_eq!(j.runs_used, MAX_RUNS);
    assert_eq!(j.escapes().len(), MAX_RUNS);
    assert_eq!(j.not_measured(), 2, "the failures past the budget are not measured, never guessed");
    assert!(!j.clean());
    assert!(j.lines().join("\n").contains("more than the 10 runs"));
}

#[test]
fn more_than_the_commit_bound_is_not_measured_when_a_bisect_would_be_needed() {
    let n = MAX_COMMITS + 1;
    let rs: Vec<Record> = (1..=n).map(|i| rec(i, false)).collect();
    let commits: Vec<String> = (1..=n).map(c).collect();
    let m = store_of(&rs);
    let mut runner = Culprit(1, 0);
    let j = judge(&commits, &m, &[fail_ui()], &mut runner);
    assert_eq!(runner.1, 0, "no run past the bound");
    assert_eq!(j.not_measured(), 1);
    assert!(j.escapes().is_empty());
    assert!(j.lines().join("\n").contains("over the bisect bound of 30"));
    // Control: at the bound the same shape is bisected.
    let m = store_of(&rs[..MAX_COMMITS]);
    let j = judge(&commits[..MAX_COMMITS], &m, &[fail_ui()], &mut Culprit(7, 0));
    assert_eq!(j.escapes().len(), 1);
    assert_eq!(j.escapes()[0].commit, c(7));
}

#[test]
fn one_dropping_commit_needs_no_bisect() {
    let m = store_of(&[rec(1, true), rec(2, false)]);
    let mut runner = Culprit(0, 0);
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut runner);
    assert_eq!((runner.1, j.runs_used), (0, 0));
    assert_eq!(j.escapes().len(), 1);
}

#[test]
fn a_runner_that_cannot_run_is_not_measured() {
    let m = store_of(&[rec(1, false), rec(2, false)]);
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut NoRunner);
    assert_eq!(j.not_measured(), 1);
    assert!(j.escapes().is_empty() && !j.clean());
}

#[test]
fn missing_and_unreadable_records_are_not_measured_never_clean() {
    let m = store_of(&[rec(1, true)]);
    m.0.borrow_mut().insert(c(2), Load::Unreadable("version 9 is not 1".into()));
    // No failures at all: still not clean, because the commits could not be judged.
    let j = judge(&[c(1), c(2), c(3)], &m, &[], &mut NoRunner);
    assert_eq!(j.unreadable.len(), 2, "{j:?}");
    assert!(!j.clean());
    let lines = j.lines().join("\n");
    assert!(
        lines.contains("unreadable: version 9") && lines.contains("no record was saved"),
        "{lines}"
    );
    // A failure with an unreadable record is not measured either.
    let j = judge(&[c(1), c(2)], &m, &[fail_ui()], &mut NoRunner);
    assert_eq!(j.not_measured(), 1);
    // Control: every record readable and no failure is clean.
    let ok = store_of(&[rec(1, true), rec(2, true)]);
    assert!(judge(&[c(1), c(2)], &ok, &[], &mut NoRunner).clean());
}

#[test]
fn the_chain_follows_bases_to_the_previous_nightly_and_ends_at_a_missing_record() {
    let m = store_of(&[rec(5, true), rec(8, true)]);
    // 8's base is 7 in `rec`; make the ranges real: 8 covers 5..8, 5 covers 2..5.
    let (mut a, mut b) = (rec(8, true), rec(5, true));
    a.base = Some(c(5));
    b.base = Some(c(2));
    let m2 = store_of(&[a, b]);
    let reached = |x: &str| x == c(2);
    assert_eq!(chain(&m2, &c(8), &reached), vec![c(5), c(8)]);
    // A missing record in the middle ends the chain at that commit, which judge reports.
    let m3 = store_of(&[{
        let mut r = rec(8, true);
        r.base = Some(c(5));
        r
    }]);
    let ch = chain(&m3, &c(8), &reached);
    assert_eq!(ch, vec![c(5), c(8)]);
    assert!(!judge(&ch, &m3, &[], &mut NoRunner).clean());
    // A record with no base ran everything and ends the chain.
    let mut full = rec(8, true);
    full.base = None;
    assert_eq!(chain(&store_of(&[full]), &c(8), &reached), vec![c(8)]);
    // A cycle never loops.
    let mut x = rec(8, true);
    x.base = Some(c(8));
    assert_eq!(chain(&store_of(&[x]), &c(8), &reached), vec![c(8)]);
    let _ = m;
}

#[test]
fn a_record_round_trips_and_a_bad_one_is_refused_with_why() {
    let r = rec(3, false);
    assert_eq!(Record::parse(&serde_json::to_string(&r.to_json()).unwrap()).unwrap(), r);
    assert!(Record::parse("{").unwrap_err().contains("not JSON"));
    assert!(Record::parse(r#"{"version":2}"#).unwrap_err().contains("version"));
    let mut v = r.to_json();
    v["commit"] = "../x".into();
    assert!(Record::parse(&v.to_string()).unwrap_err().contains("commit"));
    assert!(
        save(
            &Mem::default(),
            &Record {
                commit: "--force".into(),
                ..r
            }
        )
        .is_err()
    );
}

#[test]
fn records_older_than_fourteen_days_are_pruned_and_newer_are_kept() {
    let day = 86_400;
    let (mut old, mut edge) = (rec(1, true), rec(2, true));
    old.at = 0;
    edge.at = day; // 14 days to the second day boundary: kept at exactly 14 days
    let m = store_of(&[old, edge]);
    m.0.borrow_mut().insert(c(3), Load::Unreadable("x".into()));
    let gone = prune(&m, RETENTION_DAYS * day + day).unwrap();
    assert_eq!(gone, vec![c(1)], "15 days old goes, 14 stays, unreadable stays");
    assert!(matches!(m.get(&c(2)), Load::Ok(_)) && matches!(m.get(&c(3)), Load::Unreadable(_)));
}

/// CI run 8976: the plan job checks out with `persist-credentials: false`, so the push of the records asked for a user name and
/// failed. The push and the fetch send the job's token the way actions/checkout does (an `http.extraheader` of basic
/// `x-access-token:<token>`), in git's environment only: never on the command line.
#[test]
fn the_records_push_and_fetch_carry_the_jobs_token_as_checkouts_header() {
    let d = tempfile::tempdir().unwrap();
    let s = GitRefs { root: d.path() };
    let env = |c: &std::process::Command| {
        c.get_envs()
            .filter_map(|(k, v)| Some((k.to_string_lossy().to_string(), v?.to_string_lossy().to_string())))
            .collect::<BTreeMap<_, _>>()
    };
    let with = s.remote(Some("t"));
    let e = env(&with);
    assert_eq!(e.get("GIT_CONFIG_COUNT").map(String::as_str), Some("1"), "{e:?}");
    assert_eq!(e.get("GIT_CONFIG_KEY_0").map(String::as_str), Some("http.extraheader"));
    // base64("x-access-token:t")
    assert_eq!(
        e.get("GIT_CONFIG_VALUE_0").map(String::as_str),
        Some("AUTHORIZATION: basic eC1hY2Nlc3MtdG9rZW46dA==")
    );
    assert_eq!(
        e.get("GIT_TERMINAL_PROMPT").map(String::as_str),
        Some("0"),
        "never a prompt for a user name"
    );
    assert!(
        with.get_args().all(|a| !a.to_string_lossy().contains("eC1hY2Nlc3MtdG9rZW46dA")),
        "never on the command line"
    );
    // Control: no token, no header (a local `dev plan --ci` or a clone with its own credentials).
    let without = s.remote(None);
    assert!(!env(&without).contains_key("GIT_CONFIG_VALUE_0"));
    assert_eq!(
        super::job_token(&|k| (k == "GITEA_TOKEN").then(|| "g".to_string())).as_deref(),
        Some("g")
    );
    assert_eq!(
        super::job_token(&|k| match k {
            "PLAN_TOKEN" => Some("p".into()),
            "GITEA_TOKEN" => Some("g".into()),
            _ => None,
        })
        .as_deref(),
        Some("p")
    );
    assert_eq!(super::job_token(&|_| Some("  ".into())), None);
}

#[test]
fn the_git_ref_store_saves_reads_lists_and_removes() {
    let d = tempfile::tempdir().unwrap();
    let init = crate::git_env::git().arg("-C").arg(d.path()).args(["init", "-q"]).status().unwrap();
    assert!(init.success());
    let s = GitRefs { root: d.path() };
    assert_eq!(s.get(&c(1)), Load::Missing);
    save(&s, &rec(1, true)).unwrap();
    assert!(matches!(s.get(&c(1)), Load::Ok(r) if *r == rec(1, true)));
    assert_eq!(s.commits().unwrap(), vec![c(1)]);
    // A ref that holds another commit's record, or text that is no record, is unreadable, not a record.
    let git = |args: &[&str]| {
        let mut c = crate::git_env::git();
        let o = c.arg("-C").arg(d.path()).args(args).output().unwrap();
        assert!(o.status.success());
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    std::fs::write(d.path().join("junk"), "not a record").unwrap();
    let blob = git(&["hash-object", "-w", "junk"]);
    git(&["update-ref", &format!("{REF_PREFIX}{}", c(2)), &blob]);
    assert!(matches!(s.get(&c(2)), Load::Unreadable(_)));
    // A record saved under another commit's name is refused too.
    let other = serde_json::to_string(&rec(5, true).to_json()).unwrap();
    std::fs::write(d.path().join("other"), other).unwrap();
    let blob = git(&["hash-object", "-w", "other"]);
    git(&["update-ref", &format!("{REF_PREFIX}{}", c(3)), &blob]);
    assert!(matches!(s.get(&c(3)), Load::Unreadable(w) if w.contains("is for")));
    s.remove(&c(2)).unwrap();
    s.remove(&c(3)).unwrap();
    s.remove(&c(1)).unwrap();
    assert_eq!(s.get(&c(1)), Load::Missing);
    assert!(s.remove("--all").is_err());
}

fn row(run: u64, escapes: u64, nm: u64, un: u64) -> Row {
    Row {
        run,
        date: "2026-11-24".into(),
        head: c(1),
        commits: 3,
        escapes,
        not_measured: nm,
        unreadable: un,
    }
}

fn reg(rows: Vec<Row>) -> Register {
    Register {
        required: register::REQUIRED,
        tables: vec![Table {
            file: "plan_gitea.rs".into(),
            contains: None,
        }],
        nightly: rows,
    }
}

#[test]
fn five_consecutive_clean_nightlies_free_the_tables_and_not_four() {
    let four: Vec<Row> = (1..=4).map(|i| row(i, 0, 0, 0)).collect();
    let gone = |_: &Table| false;
    let e = register::gate(&reg(four.clone()), &gone).unwrap_err();
    assert!(e.contains("only 4 of 5") && e.contains("plan_gitea.rs"), "{e}");
    let mut five = four;
    five.push(row(5, 0, 0, 0));
    assert!(register::gate(&reg(five), &gone).is_ok());
}

#[test]
fn an_escape_a_not_measured_or_an_unreadable_record_starts_the_count_again() {
    for bad in [row(3, 1, 0, 0), row(3, 0, 1, 0), row(3, 0, 0, 1)] {
        let mut rows: Vec<Row> = (1..=6).map(|i| row(i, 0, 0, 0)).collect();
        rows[2] = bad;
        let r = reg(rows);
        assert_eq!(register::consecutive_clean(&r), 3, "runs 4 to 6 only");
        assert!(register::gate(&r, &|_| false).is_err());
    }
}

#[test]
fn the_tables_present_pass_whatever_the_count_and_an_empty_register_is_refused() {
    assert!(register::gate(&reg(vec![]), &|_| true).is_ok());
    let mut r = reg(vec![]);
    r.tables.clear();
    assert!(register::gate(&r, &|_| true).unwrap_err().contains("guard nothing"));
}

#[test]
fn the_register_parses_and_refuses_a_lowered_count() {
    let text = "version = 1\nrequired = 5\n[[table]]\nfile = \"a.rs\"\ncontains = \"static X\"\n[[nightly]]\nrun = 1\ndate = \"2026-11-24\"\nhead = \"abc\"\ncommits = 2\nescapes = 0\nnot_measured = 0\nunreadable = 0\n";
    let r = register::parse(text).unwrap();
    assert_eq!((r.nightly.len(), r.tables[0].contains.as_deref()), (1, Some("static X")));
    let e = register::parse(&text.replace("required = 5", "required = 1")).unwrap_err();
    assert!(e.contains("below the 5"), "{e}");
    assert!(register::parse(&text.replace("escapes = 0\n", "")).unwrap_err().contains("escapes"));
}

#[test]
fn the_register_writer_appends_one_row_per_run() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("docs")).unwrap();
    std::fs::write(d.path().join(register::FILE), "version = 1\n[[table]]\nfile = \"a.rs\"\n").unwrap();
    register::append(d.path(), &row(7, 0, 0, 0)).unwrap();
    assert_eq!(register::load(d.path()).unwrap().nightly, vec![row(7, 0, 0, 0)]);
    assert!(register::append(d.path(), &row(7, 0, 0, 0)).unwrap_err().contains("already"));
    assert!(
        register::append(
            d.path(),
            &Row {
                date: "x\"".into(),
                ..row(8, 0, 0, 0)
            }
        )
        .is_err()
    );
}

#[test]
fn the_tree_gate_sees_a_deleted_table_and_its_control_a_present_one() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("docs")).unwrap();
    std::fs::write(
        d.path().join(register::FILE),
        "version = 1\n[[table]]\nfile = \"plan_gitea.rs\"\n[[table]]\nfile = \"plan.rs\"\ncontains = \"static GLOBAL\"\n",
    )
    .unwrap();
    // Both deleted: refused.
    assert!(register::check_tree(d.path()).is_err());
    // The file there but the table in it gone (GLOBAL removed): refused.
    std::fs::write(d.path().join("plan_gitea.rs"), "").unwrap();
    std::fs::write(d.path().join("plan.rs"), "// no table\n").unwrap();
    assert!(register::check_tree(d.path()).unwrap_err().contains("plan.rs"));
    // Control: all there.
    std::fs::write(d.path().join("plan.rs"), "static GLOBAL: X = 1;\n").unwrap();
    assert!(register::check_tree(d.path()).is_ok());
}

#[test]
fn a_failed_job_maps_to_its_project_and_an_unknown_one_is_unmapped() {
    use crate::plan_since::{Leg, Rerun, Scope, SincePlan};
    let job = |key: &str, leg: Leg| Rerun {
        id: 1,
        name: format!("job {key}"),
        key: Some(key.into()),
        leg,
        scope: Scope::Job("test".into()),
        vitest: vec![],
    };
    let plan = SincePlan {
        run: 1,
        workflow: "nightly.yml".into(),
        head: c(1),
        reruns: vec![
            job("ui", Leg::default()),
            job(
                "app",
                Leg {
                    os: Some("macos-14".into()),
                    app: Some("debrief".into()),
                    ..Leg::default()
                },
            ),
            job("mystery", Leg::default()),
            job("plan", Leg::default()),
        ],
        not_rerun: 0,
        everything: None,
        full_ran: false,
        expected: ["ui", "plan"]
            .iter()
            .map(|n| {
                let projects = if *n == "ui" { ["ui", "eslint", "icons"].map(String::from).into() } else { ["everything".to_string()].into() };
                (format!("job {n}"), crate::run_scope::Expected { job: n.to_string(), projects, ..Default::default() })
            })
            .collect(),
    };
    let (f, un) = cmd::failures_of(&plan);
    assert!(
        f.iter()
            .any(|x| x.project == "ui" && x.os == "ubuntu-latest" && x.target == WHOLE_JOB)
    );
    assert!(f.iter().any(|x| x.project == "app:debrief" && x.os == "macos-14"));
    assert_eq!(un.len(), 1, "{un:?}");
    assert_eq!(f.len(), 2, "the plan job is no failure of a project");
}

#[test]
fn a_failures_file_is_parsed_and_a_bad_one_refused() {
    let f = cmd::read_failures(r#"[{"project":"ui","os":"ubuntu-latest"},{"project":"crates","os":"x","target":"nextest:a"}]"#).unwrap();
    assert_eq!(f[0].target, WHOLE_JOB);
    assert!(cmd::read_failures(r#"[{"os":"x"}]"#).is_err() && cmd::read_failures("{}").is_err());
}

#[test]
fn the_previous_green_nightly_is_the_newest_green_run_of_the_workflow_but_not_this_one() {
    let body = serde_json::json!({"workflow_runs": [
        {"id": 9, "run_number": 9, "path": "nightly.yml@refs/heads/main", "conclusion": "success", "head_sha": "cur"},
        {"id": 8, "run_number": 8, "path": "nightly.yml@refs/heads/main", "conclusion": "failure", "head_sha": "red"},
        {"id": 7, "run_number": 7, "path": "ci.yml@refs/heads/main", "conclusion": "success", "head_sha": "other-workflow"},
        {"id": 6, "run_number": 6, "path": "nightly.yml@refs/heads/main", "conclusion": "success", "head_sha": "green"},
    ]});
    assert_eq!(cmd::pick_green(&body, 9).as_deref(), Some("green"));
    assert_eq!(cmd::pick_green(&serde_json::json!({"workflow_runs": []}), 9), None);
}
