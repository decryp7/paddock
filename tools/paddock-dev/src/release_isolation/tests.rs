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

/// One planted leak fails its probe and names it. Each leak is its own test (`leak_tests!`), so nextest runs them in
/// parallel and one fixture build is one test's cost: the single loop over all leaks took 42.6 s alone and timed out at
/// 60 s on a loaded runner (ADR 0071).
fn a_planted_leak_fails_and_names_its_probe(leak: Leak) {
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
}

/// Makes one test per leak and a list of the variant names it covers.
macro_rules! leak_tests {
    ($list:ident; $($name:ident => $leak:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                a_planted_leak_fails_and_names_its_probe(Leak::$leak);
            }
        )*
        const $list: &[&str] = &[$(stringify!($leak)),*];
    };
}

leak_tests! { COMMON_LEAKS;
    ledger_missing_kind => LedgerMissingKind,
    ledger_shared_mode => LedgerSharedMode,
    folder_replaced => FolderReplaced,
    kept_folder => KeptFolder,
    outside_job_root => OutsideJobRoot,
    cargo_home_elsewhere => CargoHomeElsewhere,
    sccache_dir_shared => SccacheDirShared,
    rustc_wrapper => RustcWrapper,
    sccache_requests => SccacheRequests,
    sccache_stats_missing => SccacheStatsMissing,
    crate_hash_mismatch => CrateHashMismatch,
    crate_not_in_lock => CrateNotInLock,
    no_crates => NoCrates,
    config_in_parent => ConfigInParent,
    config_in_cargo_home => ConfigInCargoHome,
    config_untracked_in_clone => ConfigUntrackedInClone,
    ci_toml_extra => CiTomlExtra,
    git_hooks_path => GitHooksPath,
    git_instead_of => GitInsteadOf,
    user_npmrc => UserNpmrc,
    tauri_file_tampered => TauriFileTampered,
    npm_global_npmrc => NpmGlobalNpmrc,
    npm_missing => NpmMissing,
    tauri_pins_missing => TauriPinsMissing,
    allow_unpinned_on_tag => AllowUnpinnedOnTag,
    other_registry_index => OtherRegistryIndex,
    git_checkouts_in_cargo_home => GitCheckoutsInCargoHome,
    dev_tool_wrong_name => DevToolWrongName,
    dev_tool_off => DevToolOff,
    folder_kind_run => FolderKindRun,
    config_tracked_modified => ConfigTrackedModified,
    clone_not_at_tag => CloneNotAtTag,
    allow_unpinned_in_ci_release_mode => AllowUnpinnedInCiReleaseMode,
}

#[cfg(unix)]
leak_tests! { OS_LEAKS;
    angular_link => AngularLink,
    link_registry_src => LinkRegistrySrc,
    link_node_modules => LinkNodeModules,
    link_target_release => LinkTargetRelease,
    git_missing => GitMissing,
    npm_homebrew_link => NpmHomebrewLink,
}

#[cfg(windows)]
leak_tests! { OS_LEAKS;
    link_junction => LinkJunction,
}

/// A new `Leak` variant without its own test above fails here, so the per-leak tests cannot fall behind `Leak::all()`.
#[test]
fn every_leak_has_its_own_test() {
    let mut covered: Vec<String> = COMMON_LEAKS.iter().chain(OS_LEAKS).map(|s| s.to_string()).collect();
    let mut all: Vec<String> = Leak::all().iter().map(|l| format!("{l:?}")).collect();
    covered.sort();
    all.sort();
    assert_eq!(covered, all, "add a `leak_tests!` line for each missing leak, drop the stale ones");
}

#[test]
fn a_missing_kind_or_no_crates_is_check_broken_never_a_pass() {
    for leak in [
        Leak::LedgerMissingKind,
        Leak::NoCrates,
        Leak::SccacheStatsMissing,
        Leak::NpmMissing,
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

#[cfg(unix)]
#[test]
fn a_missing_git_is_check_broken() {
    let (_d, mut f) = job();
    f.plant(Leak::GitMissing);
    let v = f.verify();
    assert!(
        v.findings.iter().any(|x| x.probe == super::verify::Probe::Git && x.is_broken()),
        "{}",
        lines(&v)
    );
}

#[cfg(unix)]
#[test]
fn links_that_stay_inside_the_job_pass() {
    let (_d, f) = job();
    // `node_modules` of the clone may be the folder `setup` made for it.
    let nm = f.clone.join("node_modules");
    std::os::unix::fs::symlink(f.node_modules(), &nm).unwrap();
    // An npm workspace link inside it points into the clone.
    std::fs::create_dir_all(f.clone.join("ui")).unwrap();
    std::os::unix::fs::symlink(f.clone.join("ui"), f.node_modules().join("@paddock-ui")).unwrap();
    let v = f.verify();
    assert!(v.passed(), "{}", lines(&v));
}

#[cfg(unix)]
#[test]
fn a_node_modules_link_to_another_folder_of_the_job_is_refused() {
    let (_d, f) = job();
    std::os::unix::fs::symlink(&f.npm, f.clone.join("node_modules")).unwrap();
    let v = f.verify();
    assert!(
        v.findings
            .iter()
            .any(|x| x.probe == super::verify::Probe::Links && x.what.contains("not to the node-modules folder")),
        "{}",
        lines(&v)
    );
}

mod path_door {
    //! ADR 0021 amendment 2026-11-11: a resolved path is compared only through `PathOps`, with the OS and the resolver injected
    //! (as `ci_host::build_folder` does with `Os::Windows`), so a verbatim `\\?\` folder is tested on every OS.
    use super::super::config::inside_tagged_tree;
    use super::super::verify::PathOps;
    use paddock_env::Os;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn verbatim(p: &Path) -> std::io::Result<PathBuf> {
        Ok(PathBuf::from(format!(r"\\?\{}", p.display())))
    }

    fn windows() -> PathOps {
        PathOps {
            os: Os::Windows,
            resolve: verbatim,
        }
    }

    #[test]
    fn a_verbatim_folder_and_a_plain_spelling_are_the_same_place() {
        let paths = windows();
        let folder = paths.real(Path::new(r"C:\ci\ws")).unwrap();
        assert_eq!(
            folder,
            Path::new(r"\\?\C:\ci\ws"),
            "the injected resolver gives the verbatim spelling"
        );
        assert!(paths.same(&folder, Path::new(r"c:\CI\ws")));
        assert!(paths.starts_with(Path::new(r"\\?\C:\ci\ws\.cargo\config.toml"), Path::new(r"C:\ci\ws")));
        assert!(!paths.starts_with(Path::new(r"\\?\C:\ci\wsx\a"), Path::new(r"C:\ci\ws")));
        assert_eq!(
            paths
                .strip_prefix(Path::new(r"\\?\C:\ci\ws\.cargo\config.toml"), Path::new(r"C:\ci\ws"))
                .as_deref(),
            Some(".cargo/config.toml")
        );
    }

    #[test]
    fn a_config_file_of_the_tagged_tree_is_found_whichever_side_is_verbatim() {
        let files: BTreeSet<String> = [".cargo/config.toml".to_string()].into();
        let paths = windows();
        let clone = paths.real(Path::new(r"C:\ci\ws")).unwrap(); // what `Ctx::folder` returns: verbatim
        // What a probe resolved on its own may come back plain or verbatim; both are inside the tree.
        assert!(inside_tagged_tree(
            &paths,
            Path::new(r"C:\ci\ws\.cargo\config.toml"),
            &clone,
            &files
        ));
        assert!(inside_tagged_tree(
            &paths,
            Path::new(r"\\?\C:\ci\ws\.cargo\config.toml"),
            &clone,
            &files
        ));
        assert!(!inside_tagged_tree(
            &paths,
            Path::new(r"C:\ci\ws\.cargo\other.toml"),
            &clone,
            &files
        ));
        assert!(!inside_tagged_tree(
            &paths,
            Path::new(r"C:\home\.cargo\config.toml"),
            &clone,
            &files
        ));
    }

    #[test]
    fn no_probe_resolves_a_path_through_a_door_that_strips_the_prefix() {
        // `winpath::real` returns the plain spelling, the folders of the ledger are verbatim: comparing the two with Path methods
        // failed every Windows verify. Probes resolve through `PathOps` (or `real_verbatim` for a candidate list), nothing else.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/release_isolation");
        for name in ["config.rs", "links.rs", "verify.rs", "identity.rs", "crates.rs", "pathfind.rs"] {
            let text = std::fs::read_to_string(dir.join(name)).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap();
            assert!(
                !code.contains("winpath::real("),
                "{name} resolves with winpath::real: use PathOps::real"
            );
        }
    }
}
