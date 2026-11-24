//! The register's reader and writer, its growth rule (ER-46) and the release after consecutive green nightlies.

use super::*;
use crate::ci_runs::fix_items;

const SHA: &str = "b3c254a2d50f1f21e4ce1be3721994e8070e6be5";
const CRATE: &str = "crate (windows-latest)";

fn item(job: &str, id: u64, test: &str) -> FixItem {
    FixItem {
        job: job.into(),
        job_id: id,
        test: test.into(),
        url: format!("https://gitea.invalid/example/paddock/actions/runs/8610/jobs/{id}"),
    }
}

fn empty() -> String {
    Register::default().render()
}

#[test]
fn the_ids_name_workspace_binary_and_test_or_spec_and_title() {
    assert_eq!(
        TestId::of(CRATE, "paddock-chromium::it real_browser::a_test").key(),
        "crates paddock-chromium::it real_browser::a_test"
    );
    assert_eq!(
        TestId::of("app spotter (macos-latest)", "spotter-render::real_browser a").key(),
        "apps/spotter spotter-render::real_browser a"
    );
    assert_eq!(
        TestId::of("paddock-dev (ubuntu-latest)", "paddock-dev a::b").key(),
        "tools/paddock-dev paddock-dev a::b"
    );
    // Playwright: the project and the line are dropped, so the same test on two engines and after an edit is one entry.
    let pw = TestId::of(
        "app debrief (ubuntu-latest, e2e-1)",
        "[webkit] › e2e/home.spec.ts:12:7 › home › shows the dashboard",
    );
    assert_eq!(
        pw,
        TestId::Spec {
            spec: "e2e/home.spec.ts".into(),
            title: "home › shows the dashboard".into()
        }
    );
    assert_eq!(
        pw.key(),
        TestId::of("x", "e2e/home.spec.ts:40:1 › home › shows the dashboard").key()
    );
}

#[test]
fn the_register_round_trips_and_refuses_another_policy_or_a_repeated_id() {
    let mut reg = Register::default();
    let done = record(
        &mut reg,
        "ci.yml",
        8610,
        SHA,
        &[item(CRATE, 44761, "paddock-chromium::it a\"b")],
        "2026-11-24",
    );
    assert_eq!(done.added.len(), 1);
    let text = reg.render();
    assert_eq!(Register::parse(&text), Ok(reg.clone()));
    assert!(text.contains("policy = \"shrink-only\""), "{text}");
    assert!(Register::parse(&text.replace("shrink-only", "free")).is_err());
    assert!(Register::parse(&text.replace("release_after = 5", "release_after = 1")).is_err());
    let twice = format!("{text}{}", &text[text.find("\n[[quarantine]]").unwrap()..]);
    assert!(Register::parse(&twice).unwrap_err().contains("repeats"), "{twice}");
    assert!(Register::parse(&format!("{text}\nextra = 1\n")).is_err());
    // The file in the repository is the empty register this writer makes.
    let real = include_str!("../../../../../docs/flaky-quarantine.toml");
    assert_eq!(Register::parse(real).map(|r| r.render()), Ok(real.to_string()));
}

#[test]
fn recording_adds_new_tests_counts_each_run_once_and_is_idempotent() {
    let mut reg = Register::default();
    let a = item(CRATE, 44761, "paddock-chromium::it real_browser::a");
    let a_mac = item("crate (macos-latest)", 44762, "paddock-chromium::it real_browser::a");
    let done = record(&mut reg, "ci.yml", 8610, SHA, &[a.clone(), a_mac.clone()], "2026-11-24");
    assert_eq!((done.added.len(), done.updated.len()), (1, 0), "two OS, one test, one entry");
    let e = &reg.entries[0];
    assert_eq!((e.count, e.first_run, e.last_run), (1, 8610, 8610));
    assert_eq!(e.jobs, vec![CRATE.to_string(), "crate (macos-latest)".to_string()]);
    assert_eq!(e.fix_item, "docs/lessons.md: full ci.yml run 8610 (b3c254a2d)");
    assert_eq!(e.recorded_by, RECORDED_BY);
    // The same run again changes nothing; a newer one counts once.
    assert_eq!(
        record(&mut reg, "ci.yml", 8610, SHA, std::slice::from_ref(&a), "2026-11-25"),
        Recorded::default()
    );
    let done = record(&mut reg, "ci.yml", 8700, SHA, &[a], "2026-11-25");
    assert_eq!(done.updated.len(), 1);
    assert_eq!(
        (reg.entries[0].count, reg.entries[0].last_run, reg.entries[0].added.as_str()),
        (2, 8700, "2026-11-24")
    );
}

/// The register and lessons of a landing step: `ci fix-items`' two writers over the base's files.
fn recorded(base_reg: &str, base_lessons: &str, run: u64, items: &[FixItem]) -> (String, String) {
    let mut reg = Register::parse(base_reg).unwrap();
    record(&mut reg, "ci.yml", run, SHA, items, "2026-11-24");
    let lessons = fix_items::add_to_lessons(base_lessons, "ci.yml", run, SHA, items).unwrap_or_else(|| base_lessons.to_string());
    (reg.render(), lessons)
}

#[test]
fn the_register_grows_only_through_the_tools_own_recording() {
    let lessons = "# Lessons\n\nA paragraph.\n";
    let items = [item(CRATE, 44761, "paddock-chromium::it real_browser::a")];
    // The tool's landing step: the entry and the lessons bullet of its run, in one diff. Not growth.
    let (reg, new_lessons) = recorded(&empty(), lessons, 8610, &items);
    assert_eq!(growth(Some(&empty()), &reg, lessons, &new_lessons), Ok(vec![]));
    // An absent base register is an empty one.
    assert_eq!(growth(None, &reg, lessons, &new_lessons), Ok(vec![]));
    // Positive control (ER-33): the same entry without the bullet in the diff is growth by hand.
    let found = growth(Some(&empty()), &reg, lessons, lessons).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("ci fix-items"), "{found:?}");
    // A bullet the base already had witnesses nothing: the entry was not recorded in this diff.
    assert_eq!(growth(Some(&empty()), &reg, &new_lessons, &new_lessons).unwrap().len(), 1);
    // An entry written by hand in the tool's shape but for a test the bullet does not name is growth.
    let forged = reg.replace("real_browser::a", "real_browser::b");
    assert_eq!(growth(Some(&empty()), &forged, lessons, &new_lessons).unwrap().len(), 1);
    // One that claims another recorder, or more than its one run, is growth.
    assert_eq!(
        growth(Some(&empty()), &reg.replace(RECORDED_BY, "hand"), lessons, &new_lessons)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        growth(Some(&empty()), &reg.replace("count = 1", "count = 3"), lessons, &new_lessons)
            .unwrap()
            .len(),
        1
    );
    // A shrink and a count that grows on a known entry are not growth.
    assert_eq!(growth(Some(&reg), &empty(), &new_lessons, &new_lessons), Ok(vec![]));
    let (later, later_lessons) = recorded(&reg, &new_lessons, 8700, &items);
    assert!(later.contains("count = 2"), "{later}");
    assert_eq!(growth(Some(&reg), &later, &new_lessons, &later_lessons), Ok(vec![]));
    // A register that cannot be read is not measured, never a pass.
    assert!(growth(Some(&empty()), "not toml [", lessons, lessons).is_err());
}

#[test]
fn the_growth_check_reads_the_merge_base_of_a_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let mut c = crate::git_env::git();
        c.arg("-C")
            .arg(root)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args);
        let out = c.output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join(REGISTER), empty()).unwrap();
    std::fs::write(root.join(fix_items::LESSONS), "# Lessons\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["branch", "target"]);
    let items = [item(CRATE, 44761, "paddock-chromium::it a")];
    // By hand: the register only.
    let (reg, lessons) = recorded(&empty(), "# Lessons\n", 8610, &items);
    std::fs::write(root.join(REGISTER), &reg).unwrap();
    git(&["commit", "-q", "-am", "hand"]);
    assert_eq!(growth_in(root, "target").unwrap().len(), 1);
    // The tool's step: the lessons bullet in the same diff.
    std::fs::write(root.join(fix_items::LESSONS), &lessons).unwrap();
    git(&["commit", "-q", "-am", "tool"]);
    assert_eq!(growth_in(root, "target"), Ok(vec![]));
    // No such ref: not measured.
    assert!(growth_in(root, "no-such-ref").is_err());
}

fn nightly(run: u64, jobs: &[(&str, &str)]) -> Nightly {
    Nightly {
        run,
        jobs: jobs
            .iter()
            .map(|(n, c)| NightlyJob {
                name: n.to_string(),
                conclusion: c.to_string(),
            })
            .collect(),
    }
}

#[test]
fn an_entry_is_released_after_release_after_consecutive_green_nightlies_only() {
    let mut reg = Register::default();
    record(
        &mut reg,
        "ci.yml",
        8610,
        SHA,
        &[item(CRATE, 1, "paddock-chromium::it a")],
        "2026-11-24",
    );
    let green = |run| nightly(run, &[(CRATE, "success"), ("plan", "success")]);
    let idle = |run| nightly(run, &[("gate (main changed since the last green nightly?)", "success")]);
    // Four green, an idle night (the gate skipped the work: neither counts nor stops), then a red one: four, not released.
    let four: Vec<Nightly> = vec![
        green(10),
        idle(9),
        green(8),
        green(7),
        green(6),
        nightly(5, &[(CRATE, "failure")]),
        green(4),
    ];
    let (n, why) = streak(&reg.entries[0], &four);
    assert_eq!(n, 4);
    assert!(why.unwrap().contains("run 5"));
    let mut kept = reg.clone();
    assert_eq!(release(&mut kept, &four), vec![(reg.entries[0].id.clone(), 4, false)]);
    assert_eq!(kept.entries.len(), 1);
    // Five green in a row: released.
    let five: Vec<Nightly> = vec![green(11), green(10), green(8), green(7), green(6)];
    let mut gone = reg.clone();
    assert!(release(&mut gone, &five)[0].2);
    assert!(gone.entries.is_empty());
    // A nightly that ran only some of the test's jobs stops the count.
    let mut two_os = reg.clone();
    record(
        &mut two_os,
        "ci.yml",
        8610,
        SHA,
        &[item("crate (macos-latest)", 2, "paddock-chromium::it a")],
        "2026-11-24",
    );
    let (n, why) = streak(&two_os.entries[0], &five);
    assert_eq!(n, 0);
    assert!(why.unwrap().contains("every job"));
}

#[test]
fn the_first_use_is_recorded_once() {
    let text = with_first_use(Some(&empty()), "ci.yml", 9000, "2026-11-24").unwrap();
    let reg = Register::parse(&text).unwrap();
    assert_eq!(reg.first_use.as_ref().map(|f| f.run), Some(9000));
    assert!(
        with_first_use(Some(&text), "ci.yml", 9001, "2026-11-25")
            .unwrap_err()
            .contains("already")
    );
    assert!(with_first_use(None, "ci.yml", 9000, "2026-11-24").is_ok());
    // The checkout's register decides the level; an absent one keeps the job level.
    let dir = tempfile::tempdir().unwrap();
    assert!(!narrow_in(dir.path()));
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join(REGISTER), &text).unwrap();
    assert!(narrow_in(dir.path()));
    std::fs::write(dir.path().join(REGISTER), "broken [").unwrap();
    assert!(!narrow_in(dir.path()), "an unreadable register keeps the stricter level");
}

#[test]
fn a_first_use_written_by_hand_is_growth_and_one_on_a_recorded_run_is_not() {
    let items = [item(CRATE, 44761, "paddock-chromium::it a")];
    let (_, lessons) = recorded(&empty(), "# Lessons\n", 8610, &items);
    let on_recorded = with_first_use(Some(&empty()), "ci.yml", 8610, "2026-11-24").unwrap();
    assert_eq!(first_use_growth(Some(&empty()), &on_recorded, &lessons), Ok(None));
    // Positive control (ER-33): a run no fix-item bullet records, written by hand.
    let by_hand = with_first_use(Some(&empty()), "ci.yml", 9999, "2026-11-24").unwrap();
    let found = first_use_growth(Some(&empty()), &by_hand, &lessons).unwrap();
    assert!(found.is_some_and(|f| f.contains("quarantine-record")));
    // A first use the merge base already has is not growth; changing it is.
    assert_eq!(first_use_growth(Some(&by_hand), &by_hand, "# Lessons\n"), Ok(None));
    let changed = by_hand.replace("run = 9999", "run = 9998");
    assert!(first_use_growth(Some(&by_hand), &changed, "# Lessons\n").unwrap().is_some());
    assert!(has_fix_items_of(&lessons, "ci.yml", 8610) && !has_fix_items_of(&lessons, "ci.yml", 861));
}
