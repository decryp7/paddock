//! The positive controls of ADR 0085 §5.1: a clean job passes, an old-mtime job passes, every planted leak fails and names
//! its probe, and turning that probe off lets the same leak through (the mutation check: the test depends on the probe).

use super::fixture::{Fixture, Leak};
use super::ledger::Kind;
use super::verify::{Env, VerifyOptions, verify};

fn job() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::build(dir.path());
    (dir, f)
}

fn lines(v: &super::verify::Verdict) -> String {
    v.findings.iter().map(|f| f.line()).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_clean_job_passes_and_counts_what_it_saw() {
    let (_d, f) = job();
    let v = f.verify();
    assert!(v.passed(), "{}", lines(&v));
    assert_eq!(v.folders_confirmed, 8, "six from setup, dev-tool and angular; sccache is off");
    assert_eq!(v.crates_matched, 2);
    assert_eq!(v.tauri_files_matched, 1);
    assert!(v.notices.is_empty(), "{:?}", v.notices);
    assert!(v.summary().contains("2 crate archives match Cargo.lock"), "{}", v.summary());
}

#[test]
fn registry_files_with_old_modification_times_pass() {
    let (_d, f) = job();
    f.age_registry();
    let v = f.verify();
    assert!(v.passed(), "an honest run with 2006 times must not be red: {}", lines(&v));
}

#[test]
fn without_pins_verify_fails_unless_a_local_trial_allows_it_and_never_on_a_tag() {
    let (_d, mut f) = job();
    f.plant(Leak::TauriPinsMissing);
    let v = f.verify();
    assert!(
        v.findings
            .iter()
            .any(|x| x.is_broken() && x.what.contains("tauri-tools: not measured")),
        "{}",
        lines(&v)
    );
    // A local trial may allow it, and the notice says so.
    f.opts.allow_unpinned_tauri = true;
    let v = f.verify();
    assert!(v.passed(), "{}", lines(&v));
    assert!(v.notices.iter().any(|n| n.contains("not measured")), "{:?}", v.notices);
    // On a tag the flag itself is refused, with or without pins.
    f.env.0.insert("GITHUB_REF".into(), "refs/tags/dyno-v1.2.3".into());
    let v = f.verify();
    assert!(v.findings.iter().any(|x| x.what.contains("refused on a tag run")), "{}", lines(&v));
    f.opts.tauri_pins = Some(f.root.join("tauri-pins.txt"));
    let v = f.verify();
    assert!(v.findings.iter().any(|x| x.what.contains("refused on a tag run")), "{}", lines(&v));
}

#[test]
fn every_planted_leak_fails_and_names_its_probe_and_the_probe_is_what_catches_it() {
    for leak in Leak::all() {
        let (_d, mut f) = job();
        f.plant(leak);
        let v = f.verify();
        let hit = v.findings.iter().any(|x| x.probe == leak.probe() && x.what.contains(leak.needle()));
        assert!(
            hit,
            "{leak:?}: no {} finding containing {:?}; got:\n{}",
            leak.probe().id(),
            leak.needle(),
            lines(&v)
        );
        for finding in &v.findings {
            assert!(
                finding.line().starts_with("::error::release-isolation "),
                "{leak:?}: {}",
                finding.line()
            );
            assert!(finding.line().contains("Next: "), "{leak:?}: a finding without its next step");
        }
        // Mutation check: with the probe removed the leak goes unseen by it.
        f.opts.skip = vec![leak.probe()];
        let v = f.verify();
        assert!(
            !v.findings.iter().any(|x| x.probe == leak.probe()),
            "{leak:?}: the skipped probe still reported: {}",
            lines(&v)
        );
        assert!(
            !v.findings.iter().any(|x| x.what.contains(leak.needle()) && x.probe == leak.probe()),
            "{leak:?}"
        );
    }
}

#[test]
fn a_missing_kind_or_no_crates_is_check_broken_never_a_pass() {
    for leak in [
        Leak::LedgerMissingKind,
        Leak::NoCrates,
        Leak::SccacheStatsMissing,
        Leak::NpmGlobalUnknown,
        Leak::TauriPinsMissing,
    ] {
        let (_d, mut f) = job();
        f.plant(leak);
        let v = f.verify();
        assert!(!v.passed());
        assert!(
            v.findings.iter().any(|x| x.is_broken() && x.probe == leak.probe()),
            "{leak:?}: {}",
            lines(&v)
        );
    }
}

#[test]
fn an_empty_or_missing_ledger_is_check_broken() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("ledger.tsv");
    let opts = VerifyOptions::new(ledger.clone());
    let v = verify(&opts, &Env::default());
    assert!(v.findings[0].is_broken(), "missing file: {}", lines(&v));
    std::fs::write(&ledger, format!("#job\t{}\t1\n", dir.path().display())).unwrap();
    let v = verify(&opts, &Env::default());
    assert!(
        v.findings.iter().any(|f| f.is_broken() && f.what.contains("0 lines")),
        "{}",
        lines(&v)
    );
    for kind in Kind::DEFAULT_EXPECTED {
        assert!(
            v.findings.iter().any(|f| f.what.contains(kind.name())),
            "{kind} is named as missing"
        );
    }
}

#[test]
fn a_job_that_compiles_nothing_names_it_and_the_crate_check_says_so() {
    let (_d, mut f) = job();
    f.plant(Leak::NoCrates);
    f.opts.expect.retain(|k| *k != Kind::CargoHome);
    let v = f.verify();
    assert!(v.passed(), "{}", lines(&v));
    assert!(v.notices.iter().any(|n| n.contains("crates: not checked")), "{:?}", v.notices);
}
