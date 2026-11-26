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

fn cfg() -> Config {
    Config::parse(CFG).unwrap()
}

fn cand(dir: &str, hash: &str) -> Candidate {
    Candidate {
        target: Target::rust(dir, dir, "crates"),
        node_hash: hash.into(),
        dirty: false,
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
fn the_shipped_config_is_in_shadow_and_two_distinct_green_runs_switch_it_on() {
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
    // The same record is not read on another runner class.
    let (d, _) = decide(&[a], &cfg(), &RunnerClass::of("windows", "x86_64"), &s, NOW, OK);
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
