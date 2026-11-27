//! Controls of ADR 0116 §8 for the cache core: the key changes with each part it names, a flake is never a pass, a failing or unreachable
//! store makes everything run, trust and the never-read list hold. Scratch folders only (ER-14).

use super::config::{Config, Mode, removed};
use super::key::{KeyParts, RunnerClass, Target, key};
use super::result::judge;
use super::store::{FsStore, Lookup, Record, Store, StoreError};
use super::*;
use std::collections::HashMap;

const CFG: &str = include_str!("../../../../docs/test-cache.toml");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/ci-runs/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The shipped config with `[switch] green_runs` cleared. The runs in it are a record a train writes (`shadow-run --write`), not part of
/// the schema these tests check, so the tests start from no green run whatever the shipped file holds.
fn cfg_text() -> String {
    let lines: Vec<&str> = CFG
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("green_runs") {
                "green_runs = []"
            } else {
                l
            }
        })
        .collect();
    lines.join("\n") + "\n"
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
        key: key_of(c, &cfg(), class),
        target: c.target.id.clone(),
        class: class.clone(),
        run: "8635".into(),
        job: "45047".into(),
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
    let p = |epoch, class: &RunnerClass, profile| key(&t, "sha256:1", &KeyParts { epoch, class, profile });
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
                profile: "ci"
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
                profile: "ci"
            }
        ),
        "the target"
    );
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

#[test]
fn removing_a_never_cached_entry_is_found_and_adding_one_is_not() {
    let with = |l: &str| CFG.replace("no_test_cache = []", &format!("no_test_cache = [{l}]"));
    assert_eq!(
        removed(&with("\"crate:a\", \"crate:b\""), &with("\"crate:a\"")).unwrap(),
        vec!["crate:b".to_string()]
    );
    assert!(removed(&with("\"crate:a\""), &with("\"crate:a\", \"crate:b\"")).unwrap().is_empty());
    assert!(
        removed(&with("\"crate:a\""), "not toml [").is_err(),
        "a broken file is never a pass"
    );
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
    assert!(j.passed.contains("nxflaky"), "{j:?}");
    assert!(j.tests["nxflaky"] >= 1);
}

#[test]
fn a_failed_or_retried_or_flaky_test_never_makes_a_pass() {
    // TRY 1 FAIL ... FLKY-FL (flaky-result = "fail"), and an always-failing test.
    for f in ["nextest-r1-fail.log", "nextest-r0-fail.log"] {
        let j = judge(&fixture(f), &names(&["nxflaky"]));
        assert!(j.passed.is_empty(), "{f}: {j:?}");
        assert_eq!(j.not_passed.len(), 1);
    }
    // A flaky test of a big run (run 8610): its package is not a pass, the others are.
    let log = fixture("nextest-run8610-crate-flaky-only.log");
    let f = crate::nextest_log::read(&log).unwrap();
    let flaky_pkg = super::result::package_of(&f.flaky.iter().next().expect("a flaky test in the fixture").0).to_string();
    let j = judge(&log, &names(&[&flaky_pkg, "paddock-diag"]));
    assert!(!j.passed.contains(&flaky_pkg), "{flaky_pkg} was flaky");
}

#[test]
fn a_run_cut_short_or_without_the_package_is_not_a_pass() {
    let log = clean_log();
    let cut: String = log.lines().filter(|l| !l.contains("Summary")).collect::<Vec<_>>().join("\n");
    assert!(
        judge(&cut, &names(&["nxflaky"])).passed.is_empty(),
        "no summary: crashed or stopped"
    );
    assert!(judge(&log, &names(&["other"])).passed.is_empty(), "a package the log never ran");
    assert!(judge("nothing here", &names(&["nxflaky"])).passed.is_empty());
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
const OK: &dyn Fn(&Record) -> Result<(), String> = &|_| Ok(());

#[test]
fn an_unchanged_target_reuses_and_a_changed_one_runs() {
    let (_d, s) = scratch();
    let class = RunnerClass::here();
    let (a, b) = (cand("crates/a", "h1"), cand("crates/b", "h2"));
    s.put_many(&[rec(&a, &class, NOW - 60), rec(&b, &class, NOW - 60)]).unwrap();
    let b2 = cand("crates/b", "h2-changed");
    let (d, notice) = decide(&[a.clone(), b2], &cfg(), &class, &s, NOW, OK);
    assert!(notice.is_none());
    assert!(matches!(d[0], Decision::Reuse(_)));
    assert_eq!(d[1], Decision::Run(Why::Miss));
    // The same record is not read on another runner class: one that is not this host's (on the Windows runner, Windows-X64 is
    // `here()`, so a fixed Windows class read its own record back; run 9146).
    let other = [RunnerClass::of("windows", "x86_64"), RunnerClass::of("linux", "x86_64")]
        .into_iter()
        .find(|c| *c != class)
        .expect("two different classes");
    let (d, _) = decide(&[a], &cfg(), &other, &s, NOW, OK);
    assert_eq!(d[0], Decision::Run(Why::Miss));
}

#[test]
fn an_unreachable_store_makes_everything_run() {
    let class = RunnerClass::here();
    let (d, notice) = decide(&[cand("crates/a", "h"), cand("crates/b", "h")], &cfg(), &class, &Down, NOW, OK);
    assert!(d.iter().all(|x| matches!(x, Decision::Run(Why::Off(_)))));
    assert!(notice.unwrap().contains("refused"));
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
        decide(std::slice::from_ref(&a), &c, &class, &s, NOW, OK).0[0],
        Decision::Run(Why::Never)
    );
    let mut dirty = a.clone();
    dirty.dirty = true;
    assert_eq!(decide(&[dirty], &cfg(), &class, &s, NOW, OK).0[0], Decision::Run(Why::Dirty));
    // 48 hours while the nightly is paused, no limit once it runs.
    let old = NOW + 49 * 3600;
    assert_eq!(
        decide(std::slice::from_ref(&a), &cfg(), &class, &s, old, OK).0[0],
        Decision::Run(Why::TooOld)
    );
    let mut running = cfg();
    running.nightly_paused = false;
    assert!(matches!(
        decide(std::slice::from_ref(&a), &running, &class, &s, old, OK).0[0],
        Decision::Reuse(_)
    ));
    let deny: &dyn Fn(&Record) -> Result<(), String> = &|_| Err("run 8635 is on a branch".into());
    assert!(matches!(
        decide(&[a], &cfg(), &class, &s, NOW, deny).0[0],
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
        decide(std::slice::from_ref(&a), &cfg(), &class, &s, NOW, OK).0[0],
        Decision::Run(Why::Miss)
    );
    // A record of another key sitting under this key's name is refused by its own fields.
    let mut other = r.clone();
    other.key = "deadbeef".into();
    std::fs::write(&json, serde_json::to_string(&other.json()).unwrap()).unwrap();
    assert_eq!(decide(&[a], &cfg(), &class, &s, NOW, OK).0[0], Decision::Run(Why::Miss));
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
    assert!(l.contains("from run 8635 job 45047 (commit 0ae8b4794, 12 tests, 2027-"), "{l}");
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
    let (dec, notice) = decide(&[c], &cfg(), &class, &s, NOW, OK);
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
    assert!(j.passed.is_empty(), "{j:?}");
    assert!(j.not_passed[0].1.contains("failed"), "{j:?}");
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
        key: key_of(c, &cfg(), &class),
    };
    let gone = invalidate(&s, &[ask(&a)]).unwrap();
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].run, "8635");
    // Only `a` went; a second delete finds nothing, and `b` still reuses.
    assert!(invalidate(&s, &[ask(&a)]).unwrap().is_empty());
    let (d, _) = decide(&[a, b], &cfg(), &class, &s, NOW, OK);
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

#[test]
fn a_record_counts_only_when_its_run_is_a_main_or_train_run_with_a_successful_job() {
    use super::trust::{Provenance, RunFacts};
    let class = RunnerClass::here();
    let facts = |r: &str, ok: bool| RunFacts {
        git_ref: r.into(),
        jobs: vec![(45047, ok)],
        success: ok,
    };
    let world = |id: u64| match id {
        8635 => Ok(Some(facts("refs/heads/main", true))),
        8636 => Ok(Some(facts("refs/heads/train/x", true))),
        8637 => Ok(Some(facts("refs/heads/feat/branch", true))),
        8638 => Ok(Some(facts("refs/heads/main", false))),
        8639 => Err("503".to_string()),
        _ => Ok(None),
    };
    let calls = std::cell::Cell::new(0);
    let p = Provenance::new(|id| {
        calls.set(calls.get() + 1);
        world(id)
    });
    let r = |run: &str, job: &str| Record {
        run: run.into(),
        job: job.into(),
        ..rec(&cand("crates/a", "h"), &class, NOW)
    };
    assert!(p.check(&r("8635", "45047")).is_ok());
    assert!(p.check(&r("8635", "45047")).is_ok());
    assert_eq!(calls.get(), 1, "one call per distinct run");
    assert!(p.check(&r("8636", "45047")).is_ok());
    assert!(
        p.check(&r("8637", "45047")).unwrap_err().contains("not main or a train"),
        "a branch run"
    );
    assert!(
        p.check(&r("8638", "45047")).unwrap_err().contains("did not end success"),
        "a failed job"
    );
    assert!(
        p.check(&r("8639", "45047")).unwrap_err().contains("could not be read"),
        "an unreadable run refuses"
    );
    assert!(p.check(&r("9999", "45047")).unwrap_err().contains("does not exist"));
    assert!(p.check(&r("8635", "1")).unwrap_err().contains("no job 1"), "a job the run lacks");
    assert!(
        p.check(&r("local-12", "")).unwrap_err().contains("not a CI run id"),
        "a local record in CI"
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
    let got: Vec<String> = records_from_log(&log(&[]), &cands, &cfg(), &class, &meta)
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
    assert!(records_from_log(&cut, &cands, &cfg(), &class, &meta).is_empty());
}

fn leg(os: &str, packages: &str, filter: &str) -> serde_json::Value {
    serde_json::json!({"os": os, "test_packages": packages, "test_filter": filter, "test_skip": ""})
}

fn reuse_plan() -> super::plan::ReusePlan {
    super::plan::ReusePlan::parse(
        r#"{"macOS-ARM64": {"a": "rust:crates/a@macOS-ARM64 from run 8635", "b": "rust:crates/b@macOS-ARM64 from run 8635"},
            "Linux-X64": {"a": "rust:crates/a@Linux-X64 from run 8635"}}"#,
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
fn a_narrowed_leg_and_the_coverage_run_are_left_alone() {
    // A filtered leg is not a whole-package run, and the crate job's Linux leg runs no tests (the coverage job does, uncached, D4).
    let narrowed = super::plan::apply(vec![leg("macos-latest", "", "package(=a)")], "crate", &reuse_plan(), false);
    assert!(narrowed.legs[0].get("test_cache_exclude").is_none() && narrowed.reused.is_empty());
    let linux_crate = super::plan::apply(vec![leg("ubuntu-latest", "", "")], "crate", &reuse_plan(), false);
    assert!(linux_crate.reused.is_empty());
    // The same Linux leg of paddock-dev runs its tests plainly, so it is cacheable there.
    let linux_dev = super::plan::apply(vec![leg("ubuntu-latest", "", "")], "paddock-dev", &reuse_plan(), true);
    assert_eq!(linux_dev.legs[0]["test_cache_exclude"], "not (package(=a))");
}

#[test]
fn a_plan_file_that_is_not_a_class_package_note_map_is_refused() {
    for bad in ["[]", "{\"macOS-ARM64\": []}", "{\"macOS-ARM64\": {\"a\": \"\"}}", "not json"] {
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
    // The nightly invalidates only what ran and failed: d, never b or c (not seen) or a (passed).
    assert!(out.ran_and_failed("rust:d"));
    assert!(!out.ran_and_failed("rust:a") && !out.ran_and_failed("rust:b") && !out.ran_and_failed("rust:c"));
    // A matched target that ran and failed is still the finding.
    let red = super::run_outcome(&[&a, &d], &[&a, &d], &judged);
    assert_eq!(red.shadow.findings, vec!["rust:d".to_string()]);
    assert!(red.not_stored.is_empty() && red.not_seen == 0 && red.ran_and_failed("rust:d"));
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
        decide(std::slice::from_ref(&a), &cfg(), &class, &s, NOW, OK).0[0],
        Decision::Reuse(_)
    ));
    a.blind = vec!["crates/a/tests/it.rs: `.ancestors()` walks up from a path".into()];
    let d = decide(std::slice::from_ref(&a), &cfg(), &class, &s, NOW, OK).0.remove(0);
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
        git_ref: String::new(),
        commit: String::new(),
        at: NOW,
        os_build: String::new(),
    };
    a.target.package = "nxflaky".into();
    assert_eq!(
        records_from_log(&clean_log(), std::slice::from_ref(&a), &cfg(), &class, &meta).len(),
        1
    );
}

fn d_text(d: &Decision) -> String {
    match d {
        Decision::Run(w) => w.text(),
        Decision::Reuse(_) => String::new(),
    }
}
