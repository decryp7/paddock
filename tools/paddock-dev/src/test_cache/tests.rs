//! Controls of ADR 0116 §8 for the cache core: the key changes with each part it names, a flake is never a pass, a failing or unreachable
//! store makes everything run, trust and the never-read list hold. Scratch folders only (ER-14).

use super::config::{Config, Mode};
use super::key::{KeyParts, RunnerClass, Target, key};
use super::result::{Judged, TestOutcome, judge};
use super::store::{FsStore, Lookup, Record, Store, StoreError};
use super::*;
use crate::job_outcome::JobOutcome;
use std::collections::HashMap;

/// The settings these tests run on: a copy of the shape `docs/test-cache.toml` has, owned by the tests. The shipped file is a register
/// the tools write (`shadow-run --write` adds to `[switch] green_runs`), so no test asserts its content; the one test that reads it
/// checks only its shape, through [`Config::read`] ([`the_shipped_file_parses_and_holds_its_invariants`]).
const CFG: &str = "# The test-result cache (ADR 0116), a copy for the tests.
epoch = 1
nightly_paused = true
max_age_hours = 48
profile = \"ci\"
no_test_cache = []

# The switch (ER-41): reuse is report-only (shadow) until two green train runs are recorded here by run id.
[switch]
green_runs = []
";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/ci-runs/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn cfg_text() -> String {
    CFG.to_string()
}

/// The shipped file, through its accessor and by shape only: it parses, the cache epoch and profile exist, the age rule is usable and
/// the switch lists run ids. Whatever a tool has written into it (`green_runs`) or the user listed in `no_test_cache` is not asserted.
#[test]
fn the_shipped_file_parses_and_holds_its_invariants() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let c = Config::read(&root).unwrap();
    assert!(c.epoch >= 1, "the epoch starts at 1");
    assert!(!c.profile.is_empty());
    assert!(
        !c.nightly_paused || c.max_age_hours > 0,
        "a paused nightly needs a usable age limit"
    );
    assert!(c.switch.green_runs.iter().all(|r| *r > 0), "a run id is positive");
    // a never-cached crate entry names a crate of the tree (a typo would silently cache it); which entries there are is not asserted
    for n in &c.no_test_cache {
        if let Some(dir) = n.strip_prefix("crate:") {
            assert!(
                root.join(dir).join("Cargo.toml").is_file(),
                "no_test_cache entry {n} names no crate"
            );
        }
    }
    // the text a tool edits stays editable: the line `with_green_run` rewrites is there
    let text = std::fs::read_to_string(root.join(super::config::FILE)).unwrap();
    assert!(
        with_green_run(&text, 1).is_ok(),
        "shadow-run --write can still add a run to the shipped file"
    );
}

fn cfg() -> Config {
    Config::parse(&cfg_text()).unwrap()
}

fn cand(dir: &str, hash: &str) -> Candidate {
    Candidate {
        target: Target::rust(dir, dir, "crates"),
        node_hash: hash.into(),
        dirty: false,
        blind: Vec::new(),
    }
}

fn rec(c: &Candidate, class: &RunnerClass, at: u64) -> Record {
    Record {
        key: key_of(c, &cfg(), class, ""),
        target: c.target.id.clone(),
        class: class.clone(),
        filter: String::new(),
        run: "8635".into(),
        job: "crate (linux)".into(),
        attempt: 1,
        git_ref: "refs/heads/main".into(),
        commit: "0ae8b4794abc".into(),
        tests: 12,
        secs: 1.5,
        at,
        os_build: String::new(),
    }
}

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |k| m.get(k).cloned()
}

#[test]
fn the_key_changes_with_each_part_and_only_with_them() {
    let t = Target::rust("crates/a", "a", "crates");
    let linux = RunnerClass::of("linux", "x86_64");
    let p = |epoch, class: &RunnerClass, profile| {
        key(
            &t,
            "sha256:1",
            &KeyParts {
                epoch,
                class,
                profile,
                filter: "",
            },
        )
    };
    let base = p(1, &linux, "ci");
    assert_eq!(base, p(1, &linux, "ci"));
    assert_ne!(base, p(2, &linux, "ci"), "the epoch");
    assert_ne!(base, p(1, &RunnerClass::of("windows", "x86_64"), "ci"), "the runner class");
    assert_ne!(base, p(1, &linux, "default"), "the profile");
    assert_ne!(
        base,
        key(
            &t,
            "sha256:2",
            &KeyParts {
                epoch: 1,
                class: &linux,
                profile: "ci",
                filter: ""
            }
        ),
        "the node hash"
    );
    let other = Target::rust("crates/b", "b", "crates");
    assert_ne!(
        base,
        key(
            &other,
            "sha256:1",
            &KeyParts {
                epoch: 1,
                class: &linux,
                profile: "ci",
                filter: ""
            }
        ),
        "the target"
    );
    let f = |filter: &str| {
        key(
            &t,
            "sha256:1",
            &KeyParts {
                epoch: 1,
                class: &linux,
                profile: "ci",
                filter,
            },
        )
    };
    assert_ne!(base, f("test(/^ci_host::/)"), "the filter");
    assert_ne!(f("test(/^a::/)"), f("test(/^b::/)"), "a different filter");
    assert_eq!(
        f("test(/^a::/) &  not  package(=b)"),
        f(" test(/^a::/) & not package(=b)\n"),
        "whitespace is not a filter"
    );
    assert_eq!(base, f("  "), "a blank filter is the whole package");
    assert_eq!(RunnerClass::of("macos", "aarch64").0, "macOS-ARM64");
}

#[test]
fn a_config_with_no_green_run_is_in_shadow_and_two_distinct_green_runs_switch_it_on() {
    let c = cfg();
    assert_eq!(c.mode(), Mode::Shadow);
    let mut on = c.clone();
    on.switch.green_runs = vec![9001, 9001];
    assert_eq!(on.mode(), Mode::Shadow, "one run twice is one run");
    on.switch.green_runs = vec![9001, 9002];
    assert_eq!(on.mode(), Mode::On);
    assert!(Config::parse("epoch = 1\nbogus = 2\n").is_err(), "an unknown key is an error");
    assert!(Config::parse(&CFG.replace("max_age_hours = 48", "max_age_hours = 0")).is_err());
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// A whole run of one passing test, in the status lines nextest 0.9.149 prints (as the captured fixtures show them).
fn clean_log() -> String {
    [
        "    Starting 1 test across 1 binary",
        "        PASS [   0.061s] (1/1) nxflaky t::passes",
        "     Summary [   0.061s] 1 test run: 1 passed, 0 skipped",
    ]
    .join("\n")
}

#[test]
fn a_whole_clean_run_passes_its_package() {
    let j = judge(&clean_log(), &names(&["nxflaky"]));
    assert!(matches!(j.outcome("nxflaky"), TestOutcome::Passed { tests } if tests >= 1), "{j:?}");
}

#[test]
fn a_failed_or_retried_or_flaky_test_never_makes_a_pass() {
    // TRY 1 FAIL ... FLKY-FL (flaky-result = "fail"), and an always-failing test.
    for f in ["nextest-r1-fail.log", "nextest-r0-fail.log"] {
        let j = judge(&fixture(f), &names(&["nxflaky"]));
        assert!(matches!(j.outcome("nxflaky"), TestOutcome::Failed { .. }), "{f}: {j:?}");
    }
    // A flaky test of a big run (run 8610): its package is not a pass, the others are.
    let log = fixture("nextest-run8610-crate-flaky-only.log");
    let f = crate::nextest_log::read(&log).unwrap();
    let flaky_pkg = super::result::package_of(&f.flaky.iter().next().expect("a flaky test in the fixture").0).to_string();
    let j = judge(&log, &names(&[&flaky_pkg, "paddock-diag"]));
    assert!(matches!(j.outcome(&flaky_pkg), TestOutcome::Failed { .. }), "{flaky_pkg} was flaky");
}

#[test]
fn a_run_cut_short_or_without_the_package_is_not_a_pass() {
    let log = clean_log();
    let cut: String = log.lines().filter(|l| !l.contains("Summary")).collect::<Vec<_>>().join("\n");
    let not_seen = |j: Judged, p: &str| matches!(j.outcome(p), TestOutcome::NotSeen { .. });
    assert!(
        not_seen(judge(&cut, &names(&["nxflaky"])), "nxflaky"),
        "no summary: crashed or stopped"
    );
    assert!(not_seen(judge(&log, &names(&["other"])), "other"), "a package the log never ran");
    assert!(not_seen(judge("nothing here", &names(&["nxflaky"])), "nxflaky"));
}

struct Down;
impl Store for Down {
    fn get_many(&self, _: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        Err(StoreError("connection refused".into()))
    }
    fn put_many(&self, _: &[Record]) -> Result<(), StoreError> {
        Err(StoreError("connection refused".into()))
    }
    fn delete_many(&self, _: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        Err(StoreError("connection refused".into()))
    }
}

fn scratch() -> (tempfile::TempDir, FsStore) {
    let d = tempfile::tempdir().unwrap();
    let s = FsStore::new(&d.path().join("tc"));
    (d, s)
}

const NOW: u64 = 1_800_000_000;
const OK: &dyn Fn(&Record) -> Result<(), super::trust::Distrust> = &|_| Ok(());

#[test]
fn an_unchanged_target_reuses_and_a_changed_one_runs() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let (a, b) = (cand("crates/a", "h1"), cand("crates/b", "h2"));
    s.put_many(&[rec(&a, &class, NOW - 60), rec(&b, &class, NOW - 60)]).unwrap();
    let b2 = cand("crates/b", "h2-changed");
    let (d, notice) = decide(&[a.clone(), b2], &cfg(), &class, "", &s, NOW, OK);
    assert!(notice.is_none());
    assert!(matches!(d[0], Decision::Reuse(_)));
    assert_eq!(d[1], Decision::Run(Why::Miss));
    // The same record is not read on another runner class: one that is not this host's (on the Windows runner, Windows-X64 is
    // `here()`, so a fixed Windows class read its own record back; run 9146).
    let other = [RunnerClass::of("windows", "x86_64"), RunnerClass::of("linux", "x86_64")]
        .into_iter()
        .find(|c| *c != class)
        .expect("two different classes");
    let (d, _) = decide(&[a], &cfg(), &other, "", &s, NOW, OK);
    assert_eq!(d[0], Decision::Run(Why::Miss));
}

#[test]
fn the_record_step_never_replaces_a_trusted_or_undecided_record_and_names_the_untrusted() {
    // Run 9209 (red) re-stored the records run 9180 (green) had written under the same keys; every later lookup then refused them.
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let (a, b, c, n, o) = (cand("a", "h1"), cand("b", "h2"), cand("c", "h3"), cand("n", "h4"), cand("o", "h5"));
    s.put_many(&[
        rec(&a, &class, NOW - 60),
        rec(&b, &class, NOW - 60),
        rec(&c, &class, NOW - 60),
        rec(&o, &class, NOW - 49 * 3600),
    ])
    .unwrap();
    // a's record is trusted, b's refused (its job failed), c's undecided (the API did not answer), o's too old (48 hours while the
    // nightly is paused); n has none.
    let trust = |r: &Record| -> Result<(), super::trust::Distrust> {
        match r.target.as_str() {
            "rust:b" => Err("job `crate (linux)` of run 8635 attempt 1 did not end success".into()),
            "rust:c" => Err(super::trust::Distrust::Unknown("run 8635 could not be read: 502".into())),
            _ => Ok(()),
        }
    };
    let meta = RunMeta {
        run: "9209".into(),
        job: "crate (ubuntu-latest, 2/3)".into(),
        attempt: 1,
        git_ref: "refs/heads/main".into(),
        commit: "c0ffee".into(),
        at: NOW,
        os_build: String::new(),
    };
    let log = [
        "    Starting 5 tests across 5 binaries",
        "        PASS [   0.010s] ( 1/5) a t::ok",
        "        PASS [   0.010s] ( 2/5) b t::ok",
        "        PASS [   0.010s] ( 3/5) c t::ok",
        "        PASS [   0.010s] ( 4/5) n t::ok",
        "        PASS [   0.010s] ( 5/5) o t::ok",
        "     Summary [   0.050s] 5 tests run: 5 passed, 0 failed, 0 skipped",
    ]
    .join("\n");
    let cands = [a.clone(), b.clone(), c.clone(), n.clone(), o.clone()];
    let (lines, stored) = super::cmd::record_with(&log, &cands, &cfg(), &class, "", &meta, &s, &trust);
    assert_eq!(
        stored, 3,
        "b (refused), n (missing) and o (too old) are replaced or written: {lines:?}"
    );
    let key = |c: &Candidate| Lookup {
        target: c.target.id.clone(),
        class: class.clone(),
        key: key_of(c, &cfg(), &class, ""),
    };
    let back: Vec<String> = s
        .get_many(&[key(&a), key(&b), key(&c), key(&n), key(&o)])
        .unwrap()
        .into_iter()
        .map(|r| r.unwrap().run)
        .collect();
    assert_eq!(
        back,
        ["8635", "9209", "8635", "9209", "9209"],
        "a trusted and c undecided stay; b, n and o are this run's"
    );
    let shadow = lines.iter().find(|l| l.starts_with("test-cache shadow:")).expect("a shadow line");
    assert_eq!(
        shadow,
        "test-cache shadow: matched 1, passed 1, findings 0, untrusted 1 (1: job `crate (linux)` of run 8635 attempt 1 did not end \
         success), undecided 1 (1: run 8635 could not be read: 502)"
    );
    // The reader of the shadow lines reads the counts of the longer line.
    let (k, sum) = read_shadow(std::slice::from_ref(shadow)).unwrap();
    assert_eq!((k, sum.matched, sum.passed, sum.findings.len()), (1, 1, 1, 0));
    // ER-27: the record line says why each pass was held back and what the lookup found, never a bare count.
    assert!(
        lines.iter().any(|l| l.ends_with(
            "recorded 3 packages of 5 candidates (5 passed here; not in this leg's partition or without a test in the log: 0; failed or not clean: 0); \
             held back 2: 1 already trusted, 1 undecided (no definite answer)"
        )),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l
            == "test-cache lookup: reused 1 of 5; ran because: no record 1, record too old 1, record untrusted 1, record trust undecided 1"),
        "{lines:?}"
    );
    // A store whose lookup fails but whose writes work (an S3 GET times out, the PUT goes through): nothing is replaced, because nothing
    // is known.
    let blind = BlindStore::default();
    let (lines, stored) = super::cmd::record_with(&log, &cands, &cfg(), &class, "", &meta, &blind, &trust);
    assert_eq!((stored, blind.put.get()), (0, 0), "{lines:?}");
}

/// Run 9335: a lookup that timed out left every Linux and Windows record step with `held back 14` and no cause. The cause is said once as a
/// notice, no shadow line is printed (nothing was measured, ER-33), and every held record is counted under the failed lookup.
#[test]
fn a_record_step_whose_lookup_failed_says_so_and_counts_its_held_records_under_it() {
    let class = RunnerClass::here();
    let (a, b) = (cand("a", "h1"), cand("b", "h2"));
    let meta = RunMeta {
        run: "9335".into(),
        job: "crate (ubuntu-latest, 1/3)".into(),
        attempt: 1,
        git_ref: "refs/heads/train/x".into(),
        commit: "c0ffee".into(),
        at: NOW,
        os_build: String::new(),
    };
    let log = [
        "    Starting 2 tests across 2 binaries",
        "        PASS [   0.010s] ( 1/2) a t::ok",
        "        PASS [   0.010s] ( 2/2) b t::ok",
        "     Summary [   0.050s] 2 tests run: 2 passed, 0 failed, 0 skipped",
    ]
    .join("\n");
    let blind = BlindStore::default();
    let (lines, stored) = super::cmd::record_with(&log, &[a, b], &cfg(), &class, "", &meta, &blind, OK);
    assert_eq!((stored, blind.put.get()), (0, 0), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("::notice::test-cache: the store lookup failed: read timed out.")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("test-cache shadow:")),
        "nothing was measured: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l == "test-cache lookup: reused 0 of 2; ran because: reuse off 2 (read timed out)"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.ends_with("held back 2: 2 store lookup failed (read timed out)")),
        "{lines:?}"
    );
}

/// A leg runs a third of the packages: the others are "not in this leg's partition", a number the line states (run 9329 read "61 in the
/// log" for a log that held 14 of them). Never-cached, blind and dirty targets are held back by name too.
#[test]
fn the_record_line_counts_what_the_leg_did_not_run_and_names_blind_never_and_dirty_holds() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let (ran, outside) = (cand("ran", "h1"), cand("outside", "h2"));
    let mut blind = cand("blind", "h3");
    blind.blind = vec!["crates/blind/src/x.rs: `.ancestors()`".into()];
    let never = cand("never", "h4");
    let mut dirty = cand("dirty", "h5");
    dirty.dirty = true;
    let mut c = cfg();
    c.no_test_cache = vec!["crate:never".into()];
    let meta = RunMeta {
        run: "1".into(),
        job: "j".into(),
        attempt: 1,
        git_ref: "refs/heads/train/x".into(),
        commit: "c0ffee".into(),
        at: NOW,
        os_build: String::new(),
    };
    let log = [
        "    Starting 3 tests across 3 binaries",
        "        PASS [   0.010s] ( 1/3) ran t::ok",
        "        PASS [   0.010s] ( 2/3) blind t::ok",
        "        PASS [   0.010s] ( 3/3) never t::ok",
        "     Summary [   0.050s] 3 tests run: 3 passed, 0 failed, 0 skipped",
    ]
    .join("\n");
    let cands = [ran, outside, blind, never, dirty];
    let (lines, stored) = super::cmd::record_with(&log, &cands, &c, &class, "", &meta, &s, OK);
    assert_eq!(stored, 1, "only the target with no record is written: {lines:?}");
    assert!(
        lines.iter().any(|l| l.ends_with(
            "recorded 1 packages of 5 candidates (3 passed here; not in this leg's partition or without a test in the log: 2; failed or not clean: 0); \
             held back 2: 2 blind or never cached"
        )),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l == "test-cache lookup: reused 0 of 5; ran because: no record 2, blind 1, never cached 1, dirty 1"),
        "{lines:?}"
    );
}

/// The reasons are counted from the decisions alone: the lookup line of a local run, a plan and a record step is one function.
#[test]
fn the_reasons_of_a_lookup_are_counted_per_reason_and_a_full_reuse_says_no_reason() {
    use super::reasons::Tally;
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let a = cand("crates/a", "h");
    s.put_many(&[rec(&a, &class, NOW - 60)]).unwrap();
    let all = decide(std::slice::from_ref(&a), &cfg(), &class, "", &s, NOW, OK).0;
    assert_eq!(Tally::of(&all).lookup_line(), "reused 1 of 1");
    let off = decide(&[a.clone(), a], &cfg(), &class, "", &Down, NOW, OK).0;
    assert_eq!(
        Tally::of(&off).lookup_line(),
        "reused 0 of 2; ran because: reuse off 2 (connection refused)"
    );
}

/// A store whose lookups fail and whose writes succeed, counting the records it was given.
#[derive(Default)]
struct BlindStore {
    put: std::cell::Cell<usize>,
}
impl Store for BlindStore {
    fn get_many(&self, _: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        Err(StoreError("read timed out".into()))
    }
    fn put_many(&self, r: &[Record]) -> Result<(), StoreError> {
        self.put.set(self.put.get() + r.len());
        Ok(())
    }
    fn delete_many(&self, _: &[Lookup]) -> Result<Vec<Option<Record>>, StoreError> {
        Ok(Vec::new())
    }
}

#[test]
fn an_unreachable_store_makes_everything_run() {
    let class = RunnerClass::here();
    let (d, notice) = decide(&[cand("crates/a", "h"), cand("crates/b", "h")], &cfg(), &class, "", &Down, NOW, OK);
    assert!(d.iter().all(|x| matches!(x, Decision::Run(Why::Off(_)))));
    assert!(notice.unwrap().contains("refused"));
}

/// Decision 113 (user, 2026-11-27): paddock-dev's whole-repository tests (tree_*, graph_*, live_*) read the whole tree, which its node
/// key does not cover, so `docs/test-cache.toml` lists it in `no_test_cache` and it is never offered for reuse even when a record of
/// the same key exists. The settings are the tests' copy with that entry: the shipped file's list is not asserted (see [`CFG`]), only
/// that each of its crate entries names a real crate ([`the_shipped_file_parses_and_holds_its_invariants`]). No check compares the
/// list with the merge base (the removal guard ADR 0116 §3 names was never called and was deleted); a removal is the user's approval.
#[test]
fn a_never_cached_paddock_dev_is_not_offered_for_reuse_but_other_targets_still_are() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let dev = Candidate {
        target: Target::rust("tools/paddock-dev", "paddock-dev", "tools/paddock-dev"),
        node_hash: "h".into(),
        dirty: false,
        blind: Vec::new(),
    };
    let other = cand("crates/a", "h");
    s.put_many(&[rec(&dev, &class, NOW - 3600), rec(&other, &class, NOW - 3600)])
        .unwrap();
    let never_dev = Config::parse(&CFG.replace("no_test_cache = []", "no_test_cache = [\"crate:tools/paddock-dev\"]")).unwrap();
    let (d, _) = decide(&[dev.clone(), other], &never_dev, &class, "", &s, NOW, OK);
    assert_eq!(
        d[0],
        Decision::Run(Why::Never),
        "paddock-dev runs although a record of its key exists"
    );
    assert!(
        matches!(d[1], Decision::Reuse(_)),
        "the control: another target with a record is still reused"
    );
}

#[test]
fn never_cached_dirty_old_and_untrusted_records_run() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let a = cand("crates/a", "h");
    s.put_many(&[rec(&a, &class, NOW - 3600)]).unwrap();
    let mut c = cfg();
    c.no_test_cache = vec!["crate:crates/a".into()];
    assert_eq!(
        decide(std::slice::from_ref(&a), &c, &class, "", &s, NOW, OK).0[0],
        Decision::Run(Why::Never)
    );
    let mut dirty = a.clone();
    dirty.dirty = true;
    assert_eq!(decide(&[dirty], &cfg(), &class, "", &s, NOW, OK).0[0], Decision::Run(Why::Dirty));
    // 48 hours while the nightly is paused, no limit once it runs.
    let old = NOW + 49 * 3600;
    assert_eq!(
        decide(std::slice::from_ref(&a), &cfg(), &class, "", &s, old, OK).0[0],
        Decision::Run(Why::TooOld)
    );
    let mut running = cfg();
    running.nightly_paused = false;
    assert!(matches!(
        decide(std::slice::from_ref(&a), &running, &class, "", &s, old, OK).0[0],
        Decision::Reuse(_)
    ));
    let deny: &dyn Fn(&Record) -> Result<(), super::trust::Distrust> = &|_| Err("run 8635 is on a branch".into());
    assert!(matches!(
        decide(&[a], &cfg(), &class, "", &s, NOW, deny).0[0],
        Decision::Run(Why::Untrusted(_))
    ));
}

#[test]
fn a_corrupt_or_foreign_record_is_a_miss() {
    let (d, s) = scratch();
    let class = RunnerClass::here();
    let a = cand("crates/a", "h");
    let r = rec(&a, &class, NOW);
    s.put_many(std::slice::from_ref(&r)).unwrap();
    let file = std::fs::read_dir(d.path().join("tc")).unwrap().next().unwrap().unwrap().path();
    let mut f = vec![file];
    let mut json = None;
    while let Some(p) = f.pop() {
        if p.is_dir() {
            f.extend(std::fs::read_dir(&p).unwrap().map(|e| e.unwrap().path()));
        } else {
            json = Some(p);
        }
    }
    let json = json.unwrap();
    std::fs::write(&json, "{not json").unwrap();
    assert_eq!(
        decide(std::slice::from_ref(&a), &cfg(), &class, "", &s, NOW, OK).0[0],
        Decision::Run(Why::Miss)
    );
    // A record of another key sitting under this key's name is refused by its own fields.
    let mut other = r.clone();
    other.key = "deadbeef".into();
    std::fs::write(&json, serde_json::to_string(&other.json()).unwrap()).unwrap();
    assert_eq!(decide(&[a], &cfg(), &class, "", &s, NOW, OK).0[0], Decision::Run(Why::Miss));
}

#[test]
fn eviction_removes_old_records_only() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let a = cand("crates/a", "h");
    s.put_many(&[rec(&a, &class, NOW)]).unwrap();
    let real_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(s.evict(real_now, super::store::EVICT_AFTER), 0);
    assert_eq!(s.evict(real_now + 31 * 24 * 3600, super::store::EVICT_AFTER), 1);
    assert_eq!(s.usage().1, 0);
}

#[test]
fn the_reused_line_names_target_key_and_run() {
    let l = rec(&cand("crates/a", "h"), &RunnerClass::of("windows", "x86_64"), NOW).line();
    assert!(l.starts_with("test-cache: reused target=rust:crates/a@Windows-X64 key="), "{l}");
    assert!(
        l.contains("from run 8635 job `crate (linux)` attempt 1 (commit 0ae8b4794, 12 tests, 2027-"),
        "{l}"
    );
}

#[test]
fn tags_releases_the_nightly_and_an_off_dispatch_read_nothing() {
    assert!(read_allowed(&env(&[("GITHUB_REF", "refs/heads/train/x")])).is_ok());
    for e in [
        vec![("GITHUB_REF", "refs/tags/v1.0.0")],
        vec![(
            "GITHUB_WORKFLOW_REF",
            "decryp7/paddock/.gitea/workflows/dyno-release.yml@refs/heads/main",
        )],
        vec![("GITHUB_WORKFLOW_REF", "decryp7/paddock/.gitea/workflows/tag.yml@refs/heads/main")],
        vec![(
            "GITHUB_WORKFLOW_REF",
            "decryp7/paddock/.gitea/workflows/nightly.yml@refs/heads/main",
        )],
        vec![("GITHUB_EVENT_NAME", "schedule")],
        vec![("PLAN_TEST_CACHE", "off")],
    ] {
        assert!(read_allowed(&env(&e)).is_err(), "{e:?}");
    }
}

#[test]
fn only_main_and_train_runs_write_in_ci_and_never_a_rerun() {
    let ci = |r: &'static str| vec![("GITHUB_ACTIONS", "true"), ("GITHUB_REF", r)];
    assert!(write_allowed(&env(&ci("refs/heads/main"))).is_ok());
    assert!(write_allowed(&env(&ci("refs/heads/train/2026-11-26-m"))).is_ok());
    assert!(
        write_allowed(&env(&ci("refs/heads/feat/x"))).is_err(),
        "a branch run reads, never writes"
    );
    let mut rerun = ci("refs/heads/main");
    rerun.push(("PLAN_SINCE_RUN", "8635"));
    assert!(write_allowed(&env(&rerun)).is_err());
    assert!(write_allowed(&env(&[])).is_ok(), "a local run writes its own store");
}

#[test]
fn the_s3_store_reads_its_keys_from_the_standard_sources_and_never_prints_them() {
    use super::s3::{BUCKET, S3Store};
    assert_eq!(BUCKET, "test-results-paddock");
    let d = tempfile::tempdir().unwrap();
    let env: HashMap<String, String> = [
        ("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
        ("AWS_SECRET_ACCESS_KEY", "s3cr3t-value"),
        ("PADDOCK_SCCACHE_ENV_FILE", d.path().join("absent.env").to_str().unwrap()),
        ("PADDOCK_SCCACHE_S3_ENDPOINT", "10.0.0.9:15000"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let s = S3Store::from_env(&env, d.path().join("scratch")).unwrap();
    let shown = format!("{s:?}");
    assert_eq!(shown, "S3Store(http://10.0.0.9:15000/test-results-paddock)");
    assert!(!shown.contains("s3cr3t"));
    let mut none = env.clone();
    none.remove("AWS_SECRET_ACCESS_KEY");
    let e = S3Store::from_env(&none, d.path().join("x")).unwrap_err();
    assert!(e.contains("no S3 credentials") && !e.contains("AKIDEXAMPLE"), "{e}");
}

#[test]
fn an_s3_store_that_cannot_be_reached_fails_the_call_so_everything_runs() {
    use super::s3::S3Store;
    let d = tempfile::tempdir().unwrap();
    let env: HashMap<String, String> = [
        ("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
        ("AWS_SECRET_ACCESS_KEY", "s3cr3t-value"),
        ("PADDOCK_SCCACHE_ENV_FILE", d.path().join("absent.env").to_str().unwrap()),
        // A black-hole port on the loopback: nothing listens (ER-33 fail-open control; no real service is touched, ER-14).
        ("PADDOCK_SCCACHE_S3_ENDPOINT", "127.0.0.1:9"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let s = S3Store::from_env(&env, d.path().join("scratch")).unwrap();
    let class = RunnerClass::here();
    let c = cand("crates/a", "h");
    let (dec, notice) = decide(&[c], &cfg(), &class, "", &s, NOW, OK);
    assert!(matches!(dec[0], Decision::Run(Why::Off(_))));
    let n = notice.expect("a notice names the cause");
    assert!(!n.contains("s3cr3t"), "{n}");
}

#[test]
fn the_exclusion_filter_names_the_reused_packages_only() {
    let (a, b) = (Target::rust("crates/a", "a", "crates"), Target::rust("crates/b", "b", "crates"));
    assert_eq!(exclude_filter(&[]), "");
    assert_eq!(exclude_filter(&[&a, &b]), "not (package(=a) | package(=b))");
}

/// One package, two binaries: the lib's test passes and an integration binary's test fails. The lib's binary is clean on its own, so
/// only the package-level check (`bad` in `judge`) keeps the package from being a pass.
#[test]
fn one_failing_binary_makes_its_whole_package_not_a_pass() {
    let log = [
        "    Starting 2 tests across 2 binaries",
        "        PASS [   0.010s] (1/2) pk t::ok",
        "        FAIL [   0.010s] (2/2) pk::it t::broken",
        "     Summary [   0.020s] 2 tests run: 1 passed, 1 failed, 0 skipped",
        "        FAIL [   0.010s] (2/2) pk::it t::broken",
    ]
    .join("\n");
    let j = judge(&log, &names(&["pk"]));
    assert!(
        matches!(j.outcome("pk"), TestOutcome::Failed { why } if why.contains("failed")),
        "{j:?}"
    );
}

/// A test that failed once and passed on its retry (`TRY 1 FAIL` then `PASS`) is a flake, which `ran_binaries` alone catches: the
/// binary is not clean, so the package is not a pass even where the package-level check is not reached.
#[test]
fn a_pass_on_retry_leaves_its_binary_unclean() {
    let log = [
        "    Starting 1 test across 1 binary",
        "  TRY 1 FAIL [   0.010s] (1/1) pk t::flaky",
        "  TRY 2 PASS [   0.010s] (1/1) pk t::flaky",
    ]
    .join("\n");
    let ran = crate::nextest_log::ran_binaries(&log);
    assert!(ran.clean.is_empty() && ran.not_clean.contains("pk"), "{ran:?}");
}

#[test]
fn the_nightly_deletes_the_cached_pass_of_a_target_that_fails_and_names_its_run() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let (a, b) = (cand("crates/a", "h1"), cand("crates/b", "h2"));
    s.put_many(&[rec(&a, &class, NOW), rec(&b, &class, NOW)]).unwrap();
    let ask = |c: &Candidate| Lookup {
        target: c.target.id.clone(),
        class: class.clone(),
        key: key_of(c, &cfg(), &class, ""),
    };
    let gone = invalidate(&s, &[ask(&a)]).unwrap();
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].run, "8635");
    // Only `a` went; a second delete finds nothing, and `b` still reuses.
    assert!(invalidate(&s, &[ask(&a)]).unwrap().is_empty());
    let (d, _) = decide(&[a, b], &cfg(), &class, "", &s, NOW, OK);
    assert_eq!(d[0], Decision::Run(Why::Miss));
    assert!(matches!(d[1], Decision::Reuse(_)));
}

#[test]
fn the_nightly_is_recognised_by_its_event_or_its_workflow() {
    assert!(is_nightly(&env(&[("GITHUB_EVENT_NAME", "schedule")])));
    assert!(is_nightly(&env(&[(
        "GITHUB_WORKFLOW_REF",
        "decryp7/paddock/.gitea/workflows/nightly.yml@refs/heads/main"
    )])));
    assert!(!is_nightly(&env(&[("GITHUB_EVENT_NAME", "push")])));
}

/// A run's facts for the trust tests: its ref and its jobs (display name, ended success).
fn run_facts(git_ref: &str, jobs: &[(&str, bool)]) -> super::trust::RunFacts {
    super::trust::RunFacts {
        git_ref: git_ref.into(),
        jobs: jobs
            .iter()
            .map(|(n, ok)| (n.to_string(), if *ok { JobOutcome::Passed } else { JobOutcome::Failed }))
            .collect(),
    }
}

/// A CI record of `job` in attempt `attempt` of `run`.
fn ci_rec(run: &str, job: &str, attempt: u64) -> Record {
    Record {
        run: run.into(),
        job: job.into(),
        attempt,
        ..rec(&cand("crates/a", "h"), &RunnerClass::here(), NOW)
    }
}

#[test]
fn a_record_counts_only_when_its_run_is_a_main_or_train_run_and_its_own_job_succeeded() {
    use super::trust::Provenance;
    let world = |id: u64, attempt: u64| match (id, attempt) {
        (8635, 1) => Ok(Some(run_facts("refs/heads/main", &[("crate (linux)", true)]))),
        (8636, 1) => Ok(Some(run_facts("refs/heads/train/x", &[("crate (linux)", true)]))),
        (8637, 1) => Ok(Some(run_facts("refs/heads/feat/branch", &[("crate (linux)", true)]))),
        (8638, 1) => Ok(Some(run_facts("refs/heads/main", &[("crate (linux)", false)]))),
        (8639, 1) => Err("503".to_string()),
        (8641, 1) => Ok(Some(super::trust::RunFacts {
            git_ref: "refs/heads/main".into(),
            jobs: vec![("crate (linux)".into(), JobOutcome::Unfinished)],
        })),
        (8640, 1) => Ok(Some(run_facts(
            "refs/heads/main",
            &[("crate (linux)", true), ("crate (linux)", true)],
        ))),
        _ => Ok(None),
    };
    let calls = std::cell::Cell::new(0);
    let p = Provenance::new(|id, attempt| {
        calls.set(calls.get() + 1);
        world(id, attempt)
    });
    assert!(p.check(&ci_rec("8635", "crate (linux)", 1)).is_ok());
    assert!(p.check(&ci_rec("8635", "crate (linux)", 1)).is_ok());
    assert_eq!(calls.get(), 1, "one call per distinct run attempt");
    assert!(p.check(&ci_rec("8636", "crate (linux)", 1)).is_ok());
    assert!(
        p.check(&ci_rec("8637", "crate (linux)", 1))
            .unwrap_err()
            .reason()
            .contains("not main or a train"),
        "a branch run"
    );
    assert!(
        p.check(&ci_rec("8638", "crate (linux)", 1))
            .unwrap_err()
            .reason()
            .contains("did not end success"),
        "a failed job"
    );
    // No answer is not a no: an unreadable run and a job still running are undecided, so their records are never replaced.
    let unread = p.check(&ci_rec("8639", "crate (linux)", 1)).unwrap_err();
    assert!(
        matches!(&unread, super::trust::Distrust::Unknown(w) if w.contains("could not be read")),
        "an unreadable run is undecided: {unread:?}"
    );
    let running = p.check(&ci_rec("8641", "crate (linux)", 1)).unwrap_err();
    assert!(
        matches!(&running, super::trust::Distrust::Unknown(w) if w.contains("has not finished")),
        "a job still running is undecided: {running:?}"
    );
    assert!(
        matches!(
            p.check(&ci_rec("8638", "crate (linux)", 1)),
            Err(super::trust::Distrust::Refused(_))
        ),
        "a failed job is a definite no"
    );
    let no_token = Provenance::new(|_, _| Err("no API token (PLAN_TOKEN or GITEA_TOKEN)".to_string()));
    assert!(matches!(
        no_token.check(&ci_rec("8635", "crate (linux)", 1)),
        Err(super::trust::Distrust::Unknown(_))
    ));
    assert!(
        p.check(&ci_rec("9999", "crate (linux)", 1))
            .unwrap_err()
            .reason()
            .contains("does not exist")
    );
    assert!(
        p.check(&ci_rec("8635", "crate (macos-latest)", 1))
            .unwrap_err()
            .reason()
            .contains("has no job"),
        "a job the run lacks"
    );
    assert!(
        p.check(&ci_rec("8635", "crate (linux)", 2))
            .unwrap_err()
            .reason()
            .contains("does not exist"),
        "an attempt the server does not have"
    );
    assert!(
        p.check(&ci_rec("8640", "crate (linux)", 1))
            .unwrap_err()
            .reason()
            .contains("several jobs"),
        "two jobs of one name are ambiguous, so untrusted"
    );
    assert!(
        p.check(&ci_rec("local-12", "", 0))
            .unwrap_err()
            .reason()
            .contains("not a CI run id"),
        "a local record in CI"
    );
}

#[test]
fn a_passing_jobs_records_are_trusted_in_a_run_where_another_job_is_red_and_a_failed_jobs_are_not() {
    use super::trust::Provenance;
    // Run 8800 ended red (the macOS leg failed), but the Linux leg of the same run passed.
    let p = Provenance::new(|_, _| {
        Ok(Some(run_facts(
            "refs/heads/train/x",
            &[
                ("crate (ubuntu-latest)", true),
                ("crate (macos-latest)", false),
                ("paddock-dev (windows-latest)", true),
            ],
        )))
    });
    assert!(
        p.check(&ci_rec("8800", "crate (ubuntu-latest)", 1)).is_ok(),
        "the passing job of a red run"
    );
    assert!(p.check(&ci_rec("8800", "paddock-dev (windows-latest)", 1)).is_ok());
    assert!(
        p.check(&ci_rec("8800", "crate (macos-latest)", 1))
            .unwrap_err()
            .reason()
            .contains("did not end success"),
        "the failed job's own records"
    );
}

#[test]
fn a_record_that_names_no_job_or_no_attempt_is_untrusted() {
    use super::trust::Provenance;
    let p = Provenance::new(|_, _| Ok(Some(run_facts("refs/heads/main", &[("crate (linux)", true)]))));
    assert!(
        p.check(&ci_rec("8635", "", 1)).unwrap_err().reason().contains("names no job"),
        "an unknown job counts as untrusted"
    );
    assert!(
        p.check(&ci_rec("8635", "crate (linux)", 0))
            .unwrap_err()
            .reason()
            .contains("names no job")
    );
}

#[test]
fn ci_uses_its_own_test_cache_key_before_the_machines_usual_source() {
    use super::s3::S3Store;
    let d = tempfile::tempdir().unwrap();
    let env: HashMap<String, String> = [
        ("PADDOCK_TEST_CACHE_KEY_ID", "CIKEY"),
        ("PADDOCK_TEST_CACHE_SECRET", "ci-secret"),
        ("PADDOCK_SCCACHE_ENV_FILE", d.path().join("absent.env").to_str().unwrap()),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert!(
        S3Store::from_env(&env, d.path().join("s")).is_ok(),
        "no AWS_* needed when CI's key is there"
    );
    let mut half = env.clone();
    half.remove("PADDOCK_TEST_CACHE_SECRET");
    let e = S3Store::from_env(&half, d.path().join("s")).unwrap_err();
    assert!(
        e.contains("PADDOCK_TEST_CACHE_KEY_ID") && !e.contains("CIKEY"),
        "half a key falls back, then says what is missing: {e}"
    );
}

#[test]
fn record_stores_a_package_only_when_all_its_tests_passed_first_time_in_a_whole_log() {
    let class = RunnerClass::here();
    let meta = RunMeta {
        run: "8700".into(),
        job: String::new(),
        attempt: 0,
        git_ref: "refs/heads/main".into(),
        commit: "c0ffee".into(),
        at: NOW,
        os_build: String::new(),
    };
    let mut dirty = cand("dirty", "h");
    dirty.dirty = true;
    let cands = [
        cand("good", "h"),
        cand("bad", "h"),
        cand("retried", "h"),
        cand("absent", "h"),
        dirty,
    ];
    let log = |extra: &[&str]| {
        let mut l = vec![
            "    Starting 6 tests across 5 binaries".to_string(),
            "        PASS [   0.010s] ( 1/6) good t::a".to_string(),
            "        PASS [   0.010s] ( 2/6) dirty t::a".to_string(),
            "  TRY 1 FAIL [   0.010s] ( 3/6) retried t::flaky".to_string(),
            "  TRY 2 PASS [   0.010s] ( 3/6) retried t::flaky".to_string(),
            "        PASS [   0.010s] ( 4/6) bad t::ok".to_string(),
            "        FAIL [   0.010s] ( 5/6) bad::it t::broken".to_string(),
            "     Summary [   0.050s] 6 tests run: 5 passed, 1 failed, 0 skipped".to_string(),
            "        FAIL [   0.010s] ( 5/6) bad::it t::broken".to_string(),
        ];
        l.extend(extra.iter().map(|s| s.to_string()));
        l.join("\n")
    };
    let got: Vec<String> = records_from_log(&log(&[]), &cands, &cfg(), &class, "", &meta)
        .into_iter()
        .map(|r| r.target)
        .collect();
    assert_eq!(
        got,
        vec!["rust:good"],
        "only the package whose tests all passed the first time: {got:?}"
    );
    // A log with no summary (the run was cut short) stores nothing at all, even for the package that passed.
    let cut: String = log(&[]).lines().filter(|l| !l.contains("Summary")).collect::<Vec<_>>().join("\n");
    assert!(records_from_log(&cut, &cands, &cfg(), &class, "", &meta).is_empty());
}

fn leg(os: &str, packages: &str, filter: &str) -> serde_json::Value {
    serde_json::json!({"os": os, "test_packages": packages, "test_filter": filter, "test_skip": ""})
}

fn reuse_plan() -> super::plan::ReusePlan {
    super::plan::ReusePlan::parse(
        r#"{"macOS-ARM64": {"": {"a": "rust:crates/a@macOS-ARM64 from run 8635", "b": "rust:crates/b@macOS-ARM64 from run 8635"},
                            "test(/^ci_host::/)": {"c": "rust:crates/c@macOS-ARM64 from run 8635"}},
            "Linux-X64": {"": {"a": "rust:crates/a@Linux-X64 from run 8635"}}}"#,
    )
    .unwrap()
}

#[test]
fn a_whole_workspace_leg_gets_an_exclusion_and_keeps_its_filter_empty_so_the_run_stays_whole() {
    let applied = super::plan::apply(
        vec![leg("macos-latest", "", ""), leg("windows-latest", "", "")],
        "crate",
        &reuse_plan(),
        false,
    );
    assert_eq!(applied.legs[0]["test_cache_exclude"], "not (package(=a) | package(=b))");
    assert_eq!(
        applied.legs[0]["test_filter"], "",
        "the declared filter is untouched: `whole` stays true"
    );
    assert!(
        applied.legs[1].get("test_cache_exclude").is_none(),
        "no record for Windows: nothing excluded"
    );
    assert_eq!(applied.reused["crate@macos-latest"].len(), 2);
    assert!(!applied.reused.contains_key("crate@windows-latest"));
}

#[test]
fn a_package_list_leg_loses_the_reused_packages_and_ends_when_none_is_left() {
    let some = super::plan::apply(vec![leg("macos-latest", "a c", "")], "crate", &reuse_plan(), false);
    assert_eq!(some.legs[0]["test_packages"], "c");
    assert_eq!(some.legs[0]["test_skip"], "");
    let all = super::plan::apply(vec![leg("macos-latest", "a b", "")], "crate", &reuse_plan(), false);
    assert_eq!(all.legs[0]["test_packages"], "");
    assert_eq!(
        all.legs[0]["test_skip"], "true",
        "nothing left to test on this runner: said, never an empty list (which means everything)"
    );
    let none = super::plan::apply(vec![leg("macos-latest", "c", "")], "crate", &reuse_plan(), false);
    assert_eq!(none.legs[0], leg("macos-latest", "c", ""));
    assert!(none.reused.is_empty());
}

#[test]
fn a_filtered_leg_reuses_only_a_pass_of_its_own_filter() {
    // The leg's filter has an entry: its package is left out, the filter stays (the run stays declared as it was).
    let same = super::plan::apply(vec![leg("macos-latest", "c", "test(/^ci_host::/)")], "crate", &reuse_plan(), false);
    assert_eq!(same.legs[0]["test_skip"], "true", "the only package has a pass under this filter");
    assert_eq!(same.legs[0]["test_filter"], "test(/^ci_host::/)");
    assert_eq!(same.reused["crate@macos-latest"].len(), 1);
    // The spelling of the filter is not part of it.
    let spaced = super::plan::apply(
        vec![leg("macos-latest", "c", "  test(/^ci_host::/) ")],
        "crate",
        &reuse_plan(),
        false,
    );
    assert_eq!(spaced.legs[0]["test_skip"], "true");
    // Another filter finds no pass: the unfiltered pass of `a` does not stand in for a filtered run, nor the other way round.
    let other = super::plan::apply(vec![leg("macos-latest", "a c", "test(/^other::/)")], "crate", &reuse_plan(), false);
    assert_eq!(other.legs[0], leg("macos-latest", "a c", "test(/^other::/)"));
    assert!(other.reused.is_empty());
    let whole = super::plan::apply(vec![leg("macos-latest", "c", "")], "crate", &reuse_plan(), false);
    assert!(
        whole.reused.is_empty(),
        "the filtered pass of `c` is not a pass of the whole package"
    );
    // A one-crate workspace (no package list): an exclusion, which the step ANDs with the leg's filter.
    let ws = super::plan::apply(
        vec![leg("macos-latest", "", "test(/^ci_host::/)")],
        "paddock-dev",
        &reuse_plan(),
        true,
    );
    assert_eq!(ws.legs[0]["test_cache_exclude"], "not (package(=c))");
}

#[test]
fn the_coverage_run_and_a_leg_with_nothing_to_test_are_left_alone() {
    // The crate job's Linux leg runs no tests (the coverage job does, uncached, D4).
    let linux_crate = super::plan::apply(vec![leg("ubuntu-latest", "", "")], "crate", &reuse_plan(), false);
    assert!(linux_crate.reused.is_empty());
    // The same Linux leg of paddock-dev runs its tests plainly, so it is cacheable there.
    let linux_dev = super::plan::apply(vec![leg("ubuntu-latest", "", "")], "paddock-dev", &reuse_plan(), true);
    assert_eq!(linux_dev.legs[0]["test_cache_exclude"], "not (package(=a))");
    let mut skip = leg("macos-latest", "a", "");
    skip["test_skip"] = serde_json::json!("true");
    assert!(super::plan::apply(vec![skip], "crate", &reuse_plan(), false).reused.is_empty());
}

#[test]
fn a_plan_file_that_is_not_a_class_package_note_map_is_refused() {
    for bad in [
        "[]",
        "{\"macOS-ARM64\": []}",
        "{\"macOS-ARM64\": {\"\": []}}",
        "{\"macOS-ARM64\": {\"\": {\"a\": \"\"}}}",
        "{\"macOS-ARM64\": {\"a\": \"note\"}}",
        "not json",
    ] {
        assert!(super::plan::ReusePlan::parse(bad).is_err(), "{bad}");
    }
    let p = reuse_plan();
    assert_eq!(super::plan::ReusePlan::parse(&p.json().to_string()).unwrap(), p);
}

#[test]
fn a_matched_target_that_ran_and_failed_is_a_soundness_finding_and_a_missing_one_is_not() {
    let (a, b, c) = (cand("a", "h"), cand("b", "h"), cand("c", "h"));
    let log = [
        "    Starting 3 tests across 3 binaries",
        "        PASS [   0.010s] ( 1/3) a t::ok",
        "        FAIL [   0.010s] ( 2/3) b t::broken",
        "        PASS [   0.010s] ( 3/3) other t::ok",
        "     Summary [   0.050s] 3 tests run: 2 passed, 1 failed, 0 skipped",
        "        FAIL [   0.010s] ( 2/3) b t::broken",
    ]
    .join("\n");
    let judged = judge(&log, &names(&["a", "b", "c"]));
    let s = shadow_outcome(&[&a, &b, &c], &judged);
    assert_eq!((s.matched, s.passed, s.findings.clone()), (2, 1, vec!["rust:b".to_string()]));
    assert_eq!(s.line(), "test-cache shadow: matched 2, passed 1, findings 1");
    // A log that is not whole proves nothing: no match counted, no finding.
    let cut = judge("garbage", &names(&["a"]));
    assert_eq!(shadow_outcome(&[&a], &cut), Shadow::default());
}

#[test]
fn a_filtered_local_run_reports_matched_targets_it_never_ran_as_not_seen_never_as_findings() {
    // Train x's member gate: `test-cache run --filter 'package(=a)'` over a workspace whose other targets b and c matched local records.
    // Only a ran; b was outside the filter (not seen) and must not be a finding. d ran and failed without a record: not stored.
    let (a, b, c, d) = (cand("a", "h"), cand("b", "h"), cand("c", "h"), cand("d", "h"));
    let log = [
        "    Starting 2 tests across 2 binaries",
        "        PASS [   0.010s] ( 1/2) a t::ok",
        "        FAIL [   0.010s] ( 2/2) d t::broken",
        "     Summary [   0.050s] 2 tests run: 1 passed, 1 failed, 0 skipped",
        "        FAIL [   0.010s] ( 2/2) d t::broken",
    ]
    .join("\n");
    let judged = judge(&log, &names(&["a", "b", "c", "d"]));
    let out = super::run_outcome(&[&a, &b, &c, &d], &[&a, &b, &c], &judged);
    assert_eq!(
        out.shadow,
        shadow_outcome(&[&a, &b, &c], &judged),
        "one rule for the local run and the CI record step"
    );
    assert!(
        out.shadow.findings.is_empty(),
        "a target outside the filter is not a finding: {:?}",
        out.shadow.findings
    );
    assert_eq!((out.shadow.matched, out.shadow.passed), (1, 1));
    assert_eq!(out.not_seen, 2, "b and c are not seen");
    assert_eq!(out.not_stored.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), vec!["rust:d"]);
    // A matched target that ran and failed is still the finding.
    let red = super::run_outcome(&[&a, &d], &[&a, &d], &judged);
    assert_eq!(red.shadow.findings, vec!["rust:d".to_string()]);
    assert!(red.not_stored.is_empty() && red.not_seen == 0);
}

/// User decision 106: one outcome type, matched by every reader. A log where `a` passed, `b` failed and `c` is not in it gives each
/// reader each case, and not seen is never read as failed.
#[test]
fn every_reader_of_the_test_outcome_handles_passed_failed_and_not_seen() {
    let (a, b, c) = (cand("a", "h"), cand("b", "h"), cand("c", "h"));
    let log = [
        "    Starting 2 tests across 2 binaries",
        "        PASS [   0.010s] ( 1/2) a t::ok",
        "        FAIL [   0.010s] ( 2/2) b t::broken",
        "     Summary [   0.050s] 2 tests run: 1 passed, 1 failed, 0 skipped",
        "        FAIL [   0.010s] ( 2/2) b t::broken",
    ]
    .join("\n");
    let asked = names(&["a", "b", "c"]);
    let judged = judge(&log, &asked);
    assert!(matches!(judged.outcome("a"), TestOutcome::Passed { tests: 1 }));
    assert!(matches!(judged.outcome("b"), TestOutcome::Failed { .. }));
    assert!(matches!(judged.outcome("c"), TestOutcome::NotSeen { .. }));
    assert!(matches!(judged.outcome("never-asked"), TestOutcome::NotSeen { .. }));
    // The shadow summary: passed counts, failed is a finding, not seen is neither.
    let s = shadow_outcome(&[&a, &b, &c], &judged);
    assert_eq!((s.matched, s.passed, s.findings), (2, 1, vec!["rust:b".to_string()]));
    // The run finding: failed without a record is not stored, not seen is counted apart.
    let out = super::run_outcome(&[&a, &b, &c], &[&a], &judged);
    assert_eq!(out.not_stored.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), vec!["rust:b"]);
    assert_eq!((out.not_seen, out.shadow.findings.len()), (1, 0));
    // The record: only the pass is stored.
    let meta = RunMeta {
        run: "1".into(),
        job: String::new(),
        attempt: 0,
        git_ref: "refs/heads/main".into(),
        commit: "c0ffee".into(),
        at: NOW,
        os_build: String::new(),
    };
    let recs = records_from_log(&log, &[a, b, c], &cfg(), &RunnerClass::here(), "", &meta);
    assert_eq!(recs.iter().map(|r| r.target.as_str()).collect::<Vec<_>>(), vec!["rust:a"]);
}

#[test]
fn the_shadow_lines_of_several_jobs_sum_and_no_line_is_not_measured() {
    let logs = vec![
        "2026-11-26T00:00:00Z test-cache shadow: matched 3, passed 3, findings 0".to_string(),
        "noise\n2026-11-26T00:00:01Z test-cache shadow: matched 2, passed 1, findings 1\n".to_string(),
        "no such line here".to_string(),
    ];
    let (n, sum) = read_shadow(&logs).unwrap();
    assert_eq!((n, sum.matched, sum.passed, sum.findings.len()), (2, 5, 4, 1));
    assert!(read_shadow(&["nothing".to_string()]).is_none());
}

#[test]
fn a_run_is_added_to_the_green_runs_once_and_the_file_still_parses() {
    let one = with_green_run(&cfg_text(), 9001).unwrap();
    assert_eq!(Config::parse(&one).unwrap().switch.green_runs, vec![9001]);
    assert_eq!(Config::parse(&one).unwrap().mode(), Mode::Shadow);
    assert_eq!(with_green_run(&one, 9001).unwrap(), one, "the same run twice changes nothing");
    let two = with_green_run(&one, 9002).unwrap();
    assert_eq!(
        Config::parse(&two).unwrap().mode(),
        Mode::On,
        "two distinct clean runs switch it on"
    );
    assert!(two.contains("# The switch (ER-41)"), "the comments stay");
    assert!(with_green_run("epoch = 1\n", 1).is_err(), "a text that does not parse is an error");
}

fn s3_with(endpoint: &str, ssl: Option<&str>) -> super::s3::S3Store {
    let d = tempfile::tempdir().unwrap();
    let mut env: HashMap<String, String> = [
        ("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
        ("AWS_SECRET_ACCESS_KEY", "s3cr3t-value"),
        ("PADDOCK_SCCACHE_ENV_FILE", d.path().join("absent.env").to_str().unwrap()),
        ("PADDOCK_SCCACHE_S3_ENDPOINT", endpoint),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if let Some(v) = ssl {
        env.insert("PADDOCK_SCCACHE_S3_SSL".into(), v.into());
    }
    super::s3::S3Store::from_env(&env, d.path().join("scratch")).unwrap()
}

#[test]
fn the_endpoint_forms_of_the_repository_variable_all_make_one_address() {
    // CI's exact form (run 9135 log: `PADDOCK_SCCACHE_S3_ENDPOINT: http://172.16.1.220:15000`) was turned into `http://http://...`.
    let shown = |e: &str, ssl| format!("{:?}", s3_with(e, ssl));
    assert_eq!(
        shown("http://172.16.1.220:15000", None),
        "S3Store(http://172.16.1.220:15000/test-results-paddock)"
    );
    assert_eq!(
        shown("172.16.1.220:15000", None),
        "S3Store(http://172.16.1.220:15000/test-results-paddock)",
        "the bare host:port of a local setup"
    );
    assert_eq!(
        shown("http://172.16.1.220:15000/", None),
        "S3Store(http://172.16.1.220:15000/test-results-paddock)",
        "a trailing slash"
    );
    assert_eq!(
        shown("https://store.lan:9000", None),
        "S3Store(https://store.lan:9000/test-results-paddock)"
    );
    assert_eq!(
        shown("172.16.1.220:15000", Some("true")),
        "S3Store(https://172.16.1.220:15000/test-results-paddock)",
        "the SSL switch of the compile cache"
    );
    // The same reading as the compile cache's (one definition, ER-22): what it refuses, the store refuses too.
    let d = tempfile::tempdir().unwrap();
    let bad = |e: &str| {
        let env: HashMap<String, String> = [
            ("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
            ("AWS_SECRET_ACCESS_KEY", "s3cr3t-value"),
            ("PADDOCK_SCCACHE_ENV_FILE", d.path().join("absent.env").to_str().unwrap()),
            ("PADDOCK_SCCACHE_S3_ENDPOINT", e),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        super::s3::S3Store::from_env(&env, d.path().join("s")).unwrap_err()
    };
    assert!(bad("http://user:pw@host:1").contains("no user or password") && !bad("http://user:pw@host:1").contains("pw@"));
    assert!(bad("http://host:1/path").contains("no path"));
    assert!(
        bad("http://http://host:1").contains("no path"),
        "a doubled scheme is refused, never sent"
    );
}

#[test]
fn a_redirect_is_never_followed_and_is_named_with_its_target() {
    let e = super::s3::parse_done("308|http://172.16.1.220:15000/test-results-paddock/v1/x|/tmp/body-0\n")
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("308") && e.contains("a redirect to http://172.16.1.220:15000/test-results-paddock/v1/x") && e.contains("not followed"),
        "{e}"
    );
    let none = super::s3::parse_done("301||/tmp/b\n").unwrap_err().to_string();
    assert!(none.contains("an address it did not give"), "{none}");
    // A normal answer and a miss still parse.
    assert_eq!(
        super::s3::parse_done("200||/tmp/a\n404||/tmp/b\n").unwrap(),
        vec![(200, "/tmp/a".to_string()), (404, "/tmp/b".to_string())]
    );
    assert!(super::s3::parse_done("garbage").is_err());
    // The whole call fails on a redirect, so the caller runs everything (fail open), whichever transfer it was.
    assert!(super::s3::parse_done("200||/tmp/a\n308|http://x/|/tmp/b\n").is_err());
}

#[test]
fn a_target_with_a_read_the_graph_cannot_resolve_is_never_reused_and_says_why() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let mut a = cand("crates/a", "h");
    // A valid, trusted record exists under the key: only the blind read stands between it and a reuse (ER-33).
    s.put_many(&[rec(&a, &class, NOW - 3600)]).unwrap();
    assert!(matches!(
        decide(std::slice::from_ref(&a), &cfg(), &class, "", &s, NOW, OK).0[0],
        Decision::Reuse(_)
    ));
    a.blind = vec!["crates/a/tests/it.rs: `.ancestors()` walks up from a path".into()];
    let d = decide(std::slice::from_ref(&a), &cfg(), &class, "", &s, NOW, OK).0.remove(0);
    let Decision::Run(Why::Blind(w)) = &d else { panic!("{d:?}") };
    assert!(
        w.contains("crates/a/tests/it.rs") && w.contains("ancestors"),
        "the reason names the site: {w}"
    );
    assert!(d_text(&d).contains("cannot be resolved"), "{}", d_text(&d));
    // The log still records its pass: recording continues, only reading is refused.
    let meta = RunMeta {
        run: "1".into(),
        job: String::new(),
        attempt: 0,
        git_ref: String::new(),
        commit: String::new(),
        at: NOW,
        os_build: String::new(),
    };
    a.target.package = "nxflaky".into();
    assert_eq!(
        records_from_log(&clean_log(), std::slice::from_ref(&a), &cfg(), &class, "", &meta).len(),
        1
    );
}

fn d_text(d: &Decision) -> String {
    match d {
        Decision::Run(w) => w.text(),
        Decision::Reuse(_) => String::new(),
    }
}

#[test]
fn a_filtered_host_leg_records_under_its_filter_and_an_identical_one_reuses_but_another_filter_does_not() {
    let (_d, s) = scratch();
    let class = RunnerClass::of("macos", "aarch64");
    let a = cand("a", "h");
    let filter = "test(/^ci_host::/)";
    let log = [
        "    Starting 1 test across 1 binary",
        "        PASS [   0.010s] ( 1/1) a ci_host::t",
        "     Summary [   0.010s] 1 test run: 1 passed, 0 skipped",
    ]
    .join("\n");
    let meta = RunMeta {
        run: "8900".into(),
        job: "crate (macos-latest)".into(),
        attempt: 1,
        git_ref: "refs/heads/main".into(),
        commit: "c0ffee".into(),
        at: NOW,
        os_build: String::new(),
    };
    let put = records_from_log(&log, std::slice::from_ref(&a), &cfg(), &class, filter, &meta);
    assert_eq!(put.len(), 1, "a filtered leg records the package it ran");
    assert_eq!(
        (put[0].filter.as_str(), put[0].job.as_str(), put[0].attempt),
        (filter, "crate (macos-latest)", 1)
    );
    s.put_many(&put).unwrap();
    let ask = |f: &str| decide(std::slice::from_ref(&a), &cfg(), &class, f, &s, NOW, OK).0.remove(0);
    assert!(matches!(ask(filter), Decision::Reuse(_)), "a later identical leg reuses");
    assert!(
        matches!(ask("  test(/^ci_host::/)\n"), Decision::Reuse(_)),
        "spelling is not part of the filter"
    );
    assert_eq!(
        ask("test(/^other::/)"),
        Decision::Run(Why::Miss),
        "a different filter does not reuse"
    );
    assert_eq!(
        ask(""),
        Decision::Run(Why::Miss),
        "a filtered pass is not a pass of the whole package"
    );
    // And the other way round: a whole-package record is not read by a filtered leg.
    let whole = records_from_log(&log, std::slice::from_ref(&a), &cfg(), &class, "", &meta);
    let (_d2, s2) = scratch();
    s2.put_many(&whole).unwrap();
    assert_eq!(
        decide(std::slice::from_ref(&a), &cfg(), &class, filter, &s2, NOW, OK).0[0],
        Decision::Run(Why::Miss)
    );
}

#[test]
fn a_record_of_the_schema_before_the_filter_key_is_refused() {
    let class = RunnerClass::here();
    let a = cand("crates/a", "h");
    let mut old = rec(&a, &class, NOW).json();
    old["schema"] = serde_json::json!(1);
    assert!(Record::parse(&old.to_string(), &rec(&a, &class, NOW).key, &a.target.id, &class).is_none());
}

#[test]
fn the_record_steps_carry_their_jobs_name_and_the_leg_filter_as_the_workflow_names_them() {
    // The reader finds a record's job by the display name the writer passes, so the step's copy of the name must be the job's own.
    let text = include_str!("../../../../.gitea/workflows/ci.yml");
    let docs = yaml_rust2::YamlLoader::load_from_str(text).unwrap();
    for job in ["crate", "paddock-dev"] {
        let j = &docs[0]["jobs"][job];
        let name = j["name"].as_str().unwrap_or_else(|| panic!("job {job} has no name"));
        let step = j["steps"]
            .as_vec()
            .unwrap()
            .iter()
            .find(|s| s["name"].as_str() == Some("Record test results (test cache)"))
            .unwrap_or_else(|| panic!("job {job} has no record step"));
        assert_eq!(step["env"]["PADDOCK_JOB_NAME"].as_str(), Some(name), "{job}: the step's job name");
        assert_eq!(
            step["env"]["PADDOCK_RUN_ATTEMPT"].as_str(),
            Some("${{ github.run_attempt }}"),
            "{job}"
        );
        assert_eq!(step["env"]["LEG_FILTER"].as_str(), Some("${{ matrix.test_filter }}"), "{job}");
        assert!(
            !step["if"].as_str().unwrap_or("").contains("test_filter"),
            "{job}: a filtered leg records too (its filter is in the key)"
        );
        assert!(
            step["run"].as_str().unwrap_or("").contains("--filter \"$LEG_FILTER\""),
            "{job}: the record is keyed by the filter"
        );
    }
}

/// `ci test-cache run` end to end with a stand-in nextest, environment and trust check and a scratch store: the wiring of
/// `cmd::run_with` (the lookup that guards the write rule, the outcome that sorts the targets, the notices), not only the helpers it
/// calls (review of train x: the helpers were tested, `run` was not).
mod run_wiring {
    use super::super::cmd::{TestCacheArgs, Trust, run_with};
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Wrap {
        #[command(flatten)]
        args: TestCacheArgs,
    }

    fn real_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// The text of one `run` over `cands` on `store` under `env` (a stand-in), where nextest printed `log`.
    fn go(cands: &[Candidate], store: &dyn Store, env: &[(&str, &str)], log: &str) -> String {
        let root = tempfile::tempdir().unwrap();
        let args = Wrap::try_parse_from(["x", "run"]).unwrap().args;
        let env = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| (*v).to_string());
        let trust: Trust = Box::new(|_| Ok(()));
        let nextest = |_: &[String], _: bool, _: bool| Ok((log.to_string(), true));
        let class = RunnerClass::here();
        run_with(&args, root.path(), &cfg(), cands, store, &class, &env, &trust, &nextest)
            .unwrap()
            .text
    }

    const TAG: &[(&str, &str)] = &[("GITHUB_REF", "refs/tags/v1")];

    /// Reads off (a tag run) must not hide that the store is also down; with reads on, the one notice names the cause once.
    #[test]
    fn a_run_that_may_not_read_still_says_the_store_is_down() {
        let a = cand("nxflaky", "h");
        let off = go(std::slice::from_ref(&a), &Down, TAG, &clean_log());
        assert!(off.contains("reuse is off for this run: a tag run"), "{off}");
        assert!(off.contains("could not be read either: connection refused"), "{off}");
        let on = go(std::slice::from_ref(&a), &Down, &[], &clean_log());
        assert!(on.contains("reuse is off for this run: connection refused"), "{on}");
        assert!(!on.contains("either"), "one cause, said once: {on}");
    }

    /// A log that cannot be read whole judges nothing: it is "not measured" with the reason, never "outside its filter".
    #[test]
    fn an_unreadable_log_is_not_measured_with_its_reason_not_outside_the_filter() {
        let (_d, s) = scratch();
        let a = cand("nxflaky", "h");
        let text = go(std::slice::from_ref(&a), &s, &[], "garbage");
        assert!(
            text.contains("not measured: the run's output could not be read whole (no nextest run in the output)"),
            "{text}"
        );
        assert!(!text.contains("outside its filter"), "{text}");
        // Control: a whole log with no test of the target keeps the filter wording.
        let b = cand("b", "h");
        let text = go(&[a, b], &s, &[], &clean_log());
        assert!(text.contains("outside its filter"), "{text}");
        assert!(!text.contains("not measured: the run's output"), "{text}");
    }

    /// The outcome sorts the targets: a matched target the output never saw is not a finding, one that ran and failed is.
    #[test]
    fn the_run_sorts_matched_targets_by_the_outcome_not_by_the_lookup_alone() {
        let (_d, s) = scratch();
        let class = RunnerClass::here();
        let (a, b) = (cand("nxflaky", "h"), cand("b", "h"));
        s.put_many(&[rec(&b, &class, real_now())]).unwrap();
        let text = go(&[a.clone(), b.clone()], &s, &[], &clean_log());
        assert!(!text.contains("soundness finding"), "b matched and was never seen: {text}");
        let failed = [
            "        FAIL [   0.010s] ( 1/1) b t::broken",
            "     Summary [   0.050s] 1 tests run: 0 passed, 1 failed, 0 skipped",
        ];
        let log = format!("    Starting 1 tests across 1 binaries\n{}\n{}", failed[0], failed[1]);
        let text = go(&[a, b], &s, &[], &log);
        assert!(text.contains("soundness finding: rust:b"), "{text}");
    }

    /// The write rule reads the lookup, not the reads-off decisions: a run that may not read still replaces a record that was too old,
    /// and never a trusted one.
    #[test]
    fn the_write_rule_follows_the_lookup_even_when_reads_are_off() {
        let (_d, s) = scratch();
        let class = RunnerClass::here();
        let (a, b) = (cand("nxflaky", "h"), cand("b", "h"));
        s.put_many(&[rec(&b, &class, real_now())]).unwrap();
        let both = [
            "    Starting 2 tests across 2 binaries",
            "        PASS [   0.061s] (1/2) nxflaky t::passes",
            "        PASS [   0.061s] (2/2) b t::ok",
            "     Summary [   0.061s] 2 tests run: 2 passed, 0 skipped",
        ]
        .join("\n");
        let text = go(&[a.clone(), b.clone()], &s, TAG, &both);
        // nxflaky had no record (replaceable), b's is trusted (held back): one stored, one held.
        assert!(text.contains("stored 1 pass(es); held back 1: 1 already trusted"), "{text}");
        let _ = a;
    }
}

#[test]
fn a_crate_that_depends_on_a_blind_crate_is_blind_through_it_and_an_independent_one_is_not() {
    use paddock_repo_graph::model::nodes::{Edge, EdgeKind, Model, ModelNode, NodeKind};
    // a -> b -> c, and d alone: only c reads something the graph cannot resolve.
    let mut m = Model::new();
    for d in ["a", "b", "c", "d"] {
        m.add_node(ModelNode::new(
            format!("crate:crates/{d}"),
            NodeKind::Project,
            &[&format!("crates/{d}/")],
        ));
    }
    m.add_edge(Edge::new("crate:crates/a", "crate:crates/b", EdgeKind::Depends));
    m.add_edge(Edge::new("crate:crates/b", "crate:crates/c", EdgeKind::Depends));
    let own = std::collections::BTreeMap::from([("crates/c".to_string(), vec!["crates/c/src/x.rs: `.ancestors()`".to_string()])]);
    let got = super::spread_blind(&m, &own);
    assert_eq!(got["crates/c"], own["crates/c"], "its own reasons stay as they were");
    for dependent in ["crates/a", "crates/b"] {
        assert_eq!(got[dependent], vec!["blind through crates/c".to_string()], "{dependent}");
    }
    assert!(
        !got.contains_key("crates/d"),
        "control: a crate that depends on nothing blind stays cacheable"
    );
    // And the reason a run prints for the dependent names the dependency.
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let mut a = cand("crates/a", "h");
    s.put_many(&[rec(&a, &class, NOW - 3600)]).unwrap();
    a.blind = got["crates/a"].clone();
    let d = decide(std::slice::from_ref(&a), &cfg(), &class, "", &s, NOW, OK).0.remove(0);
    assert!(d_text(&d).contains("blind through crates/c"), "{}", d_text(&d));
}
