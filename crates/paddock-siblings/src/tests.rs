//! The contract of a hand-off, over fakes: nothing here starts a process, searches the user's disk for an installed app or
//! reads the real environment (rule 7).

use crate::testing::{FakeFiles, FakeSpawner};
use crate::*;
use paddock_env::Os;
use paddock_window::launch::parse_link;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn env(k: &str) -> Option<String> {
    match k {
        "HOME" => Some("/Users/me".into()),
        "LOCALAPPDATA" => Some(r"C:\Users\me\AppData\Local".into()),
        "ProgramFiles" => Some(r"C:\Program Files".into()),
        _ => None,
    }
}

fn siblings(me: &str, os: Os, installed: &[&str]) -> (Siblings, Arc<FakeSpawner>) {
    let spawner = Arc::new(FakeSpawner::new());
    let files = Arc::new(FakeFiles::with(installed.iter().copied().chain([
        "/data/a.log",
        "/data/b.json",
        "/data/c.txt",
    ])));
    (
        Siblings::new(me, os, None, Box::new(env), files, spawner.clone()).unwrap(),
        spawner,
    )
}

fn files(list: &[&str]) -> FakeFiles {
    FakeFiles::with(list.iter().copied())
}

const MAC_DEBRIEF: &str = "/Applications/Debrief.app/Contents/MacOS/debrief";

// ---- the family ----------------------------------------------------------------------------------------------------

#[test]
fn only_the_family_is_known_and_the_contract_is_the_ecosystem_table() {
    assert_eq!(AppSpec::lookup("Debrief").unwrap().id, "debrief");
    for unknown in ["calculator", "", "debrief ", "../x"] {
        let e = AppSpec::lookup(unknown).unwrap_err();
        assert!(matches!(e, SiblingError::UnknownApp(_)));
        assert!(e.to_string().contains("not an app of the family"));
    }
    // A very long unknown name is shortened in the message.
    let SiblingError::UnknownApp(n) = AppSpec::lookup(&"x".repeat(500)).unwrap_err() else {
        panic!()
    };
    assert_eq!(n.len(), 40);
    // docs/ecosystem.md: job logs and issues to Debrief, installed software to Spotter, machine health to Pitcrew.
    assert_eq!(
        AppSpec::lookup("debrief").unwrap().import_extensions(),
        Some(&[][..])
    );
    assert_eq!(
        AppSpec::lookup("spotter").unwrap().import_extensions(),
        Some(&["json"][..])
    );
    assert_eq!(
        AppSpec::lookup("pitcrew").unwrap().show_kinds(),
        ["machine"]
    );
    assert_eq!(
        AppSpec::lookup("pitcrew").unwrap().import_extensions(),
        None
    );
    assert!(AppSpec::lookup("pitwall").unwrap().show_kinds().is_empty());
    // Ids and schemes are unique and match the program name's family.
    for (i, a) in FAMILY.iter().enumerate() {
        assert!(
            FAMILY[i + 1..]
                .iter()
                .all(|b| b.id != a.id && b.scheme != a.scheme)
        );
        assert_eq!(a.id, a.scheme);
    }
}

// ---- the typed hand-off --------------------------------------------------------------------------------------------

#[test]
fn an_item_is_two_plain_words() {
    let i = Item::new("machine", "Mac-mini.local:1").unwrap();
    assert_eq!((i.kind(), i.id()), ("machine", "Mac-mini.local:1"));
    for (k, id) in [
        ("", "x"),
        ("Machine", "x"),
        ("1m", "x"),
        ("a b", "x"),
        (&"k".repeat(MAX_KIND_LEN + 1), "x"),
        ("machine", ""),
        ("machine", "a/b"),
        ("machine", "a b"),
        ("machine", "a?b"),
        ("machine", &"i".repeat(MAX_ITEM_ID_LEN + 1)),
    ] {
        let e = Item::new(k, id).unwrap_err();
        assert!(matches!(e, SiblingError::BadItem(_)), "{k:?} {id:?}");
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn an_import_is_absolute_existing_files_only() {
    let fs = files(&["/data/a.log", "/data/b.log"]);
    let ok = Handoff::import(vec!["/data/a.log".into(), "/data/b.log".into()], &fs).unwrap();
    assert!(matches!(ok, Handoff::Import { ref files } if files.len() == 2));
    let bad = |list: Vec<&str>| {
        Handoff::import(list.into_iter().map(PathBuf::from).collect(), &fs).unwrap_err()
    };
    assert_eq!(bad(vec![]), SiblingError::NoFiles);
    assert!(
        matches!(bad(vec!["a.log"]), SiblingError::BadFile { why, .. } if why.contains("absolute"))
    );
    assert!(
        matches!(bad(vec!["/data/../data/a.log"]), SiblingError::BadFile { why, .. } if why.contains(".."))
    );
    assert!(
        matches!(bad(vec!["/data/a\u{7}.log"]), SiblingError::BadFile { why, .. } if why.contains("control"))
    );
    assert!(
        matches!(bad(vec!["/data/missing.log"]), SiblingError::BadFile { why, .. } if why.contains("no such file"))
    );
    let many: Vec<String> = (0..=MAX_FILES).map(|i| format!("/data/{i}.log")).collect();
    let e = Handoff::import(many.into_iter().map(PathBuf::from).collect(), &fs).unwrap_err();
    assert_eq!(e, SiblingError::TooManyFiles(MAX_FILES + 1));
    assert!(e.to_string().contains("too many"));
    // A very long path is shortened in the message.
    let long = format!("/{}", "d/".repeat(100));
    assert!(
        matches!(bad(vec![&long]), SiblingError::BadFile { file, .. } if file.starts_with("...") && file.chars().count() == 80)
    );
}

#[test]
fn an_app_takes_only_what_its_row_lists() {
    let fs = files(&["/data/a.log", "/data/b.JSON", "/data/c.txt"]);
    let debrief = AppSpec::lookup("debrief").unwrap();
    let spotter = AppSpec::lookup("spotter").unwrap();
    let pitcrew = AppSpec::lookup("pitcrew").unwrap();
    let pitwall = AppSpec::lookup("pitwall").unwrap();
    let import = |l: &[&str]| Handoff::import(l.iter().map(PathBuf::from).collect(), &fs).unwrap();

    assert!(
        import(&["/data/a.log", "/data/c.txt"])
            .check_accepted(debrief)
            .is_ok()
    );
    assert!(
        import(&["/data/b.JSON"]).check_accepted(spotter).is_ok(),
        "extensions are case-insensitive"
    );
    let e = import(&["/data/a.log"])
        .check_accepted(spotter)
        .unwrap_err();
    assert!(
        matches!(&e, SiblingError::BadFile { why, .. } if why.contains(".json")),
        "{e}"
    );
    let noext = Handoff::import(vec!["/data/c.txt".into()], &fs).unwrap();
    assert!(noext.check_accepted(spotter).is_err());
    let e = import(&["/data/a.log"])
        .check_accepted(pitcrew)
        .unwrap_err();
    assert_eq!(
        e,
        SiblingError::NotAccepted {
            app: "Pitcrew".into(),
            what: "files".into()
        }
    );
    assert!(e.to_string().contains("does not take files"));

    let machine = Handoff::show(Item::new("machine", "m1").unwrap());
    assert!(machine.check_accepted(pitcrew).is_ok());
    let e = machine.check_accepted(debrief).unwrap_err();
    assert!(matches!(e, SiblingError::NotAccepted { ref what, .. } if what == "machine items"));
    let issue = Handoff::show(Item::new("issue", "9").unwrap());
    assert!(issue.check_accepted(pitcrew).is_err() && issue.check_accepted(pitwall).is_err());

    for a in FAMILY {
        assert!(
            Handoff::Open.check_accepted(a).is_ok(),
            "every app can be opened"
        );
    }
}

#[test]
fn the_command_line_carries_paths_or_one_deep_link_and_the_receiver_reads_it_back() {
    let fs = files(&["/data/a.log", "/data/b.json"]);
    let debrief = AppSpec::lookup("debrief").unwrap();
    let pitcrew = AppSpec::lookup("pitcrew").unwrap();
    assert!(Handoff::Open.to_args(debrief).unwrap().is_empty());
    let import = Handoff::import(vec!["/data/a.log".into()], &fs).unwrap();
    assert_eq!(
        import.to_args(debrief).unwrap(),
        [std::ffi::OsString::from("/data/a.log")]
    );
    assert!(
        import.to_args(pitcrew).is_err(),
        "a refusal comes before anything is built"
    );

    let show = Handoff::show(Item::new("machine", "mac-mini").unwrap());
    let args = show.to_args(pitcrew).unwrap();
    assert_eq!(
        args,
        [std::ffi::OsString::from("pitcrew://show/machine/mac-mini")]
    );
    // The receiving side: paddock-window parses the link, this reads it as a hand-off.
    let link = parse_link(args[0].to_str().unwrap(), &["pitcrew"]).unwrap();
    assert_eq!(Handoff::from_link(&link, pitcrew), Some(show));
    // Anything else is not a hand-off: another host, extra parts, a query, another scheme, an unlisted kind, bad words.
    for text in [
        "pitcrew://open/machine/m1",
        "pitcrew://show/machine",
        "pitcrew://show/machine/m1/extra",
        "pitcrew://show/machine/m1?x=1",
        "pitcrew://show/issue/9",
        "pitcrew://show/Machine/m1",
        "pitcrew://show/",
    ] {
        let link = parse_link(text, &["pitcrew"]).unwrap();
        assert_eq!(Handoff::from_link(&link, pitcrew), None, "{text}");
    }
    let other = parse_link("debrief://show/machine/m1", &["debrief"]).unwrap();
    assert_eq!(Handoff::from_link(&other, pitcrew), None);

    // Paths: the app keeps those it takes, and nothing at all is no hand-off.
    let spotter = AppSpec::lookup("spotter").unwrap();
    let paths: Vec<PathBuf> = ["/data/a.log", "/data/b.json"]
        .iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(
        Handoff::from_paths(&paths, spotter, &fs),
        Some(Handoff::Import {
            files: vec!["/data/b.json".into()]
        })
    );
    assert_eq!(Handoff::from_paths(&paths, pitcrew, &fs), None);
    assert_eq!(Handoff::from_paths(&[], debrief, &fs), None);
}

// ---- discovery -----------------------------------------------------------------------------------------------------

#[test]
fn the_places_to_look_follow_the_os() {
    let debrief = AppSpec::lookup("debrief").unwrap();
    let mac = install_candidates(debrief, "debrief", Os::MacOs, &env);
    assert_eq!(mac[0], Path::new(MAC_DEBRIEF));
    assert!(mac.contains(&PathBuf::from(
        "/Users/me/Applications/Debrief.app/Contents/MacOS/debrief"
    )));
    assert!(
        mac.contains(&PathBuf::from("/Users/me/.cargo/bin/debrief"))
            && mac.contains(&PathBuf::from("/opt/homebrew/bin/debrief"))
    );
    let win = install_candidates(debrief, "debrief", Os::Windows, &env);
    assert!(
        win[0].to_string_lossy().ends_with("debrief.exe")
            && win[0].to_string_lossy().contains("Programs")
    );
    assert_eq!(win.len(), 4);
    let lin = install_candidates(debrief, "debrief", Os::Linux, &env);
    assert!(
        lin.contains(&PathBuf::from("/opt/Debrief/debrief"))
            && lin.contains(&PathBuf::from("/Users/me/.local/bin/debrief"))
    );
    // Without HOME or the Windows variables only the fixed places are left.
    assert!(install_candidates(debrief, "debrief", Os::Windows, &|_| None).is_empty());
    assert_eq!(
        install_candidates(debrief, "debrief", Os::MacOs, &|_| None).len(),
        3
    );
    assert_eq!(
        install_candidates(debrief, "debrief", Os::Linux, &|_| None).len(),
        3
    );
}

#[test]
fn discovery_prefers_path_then_the_install_places_and_names_the_old_program() {
    let debrief = AppSpec::lookup("debrief").unwrap();
    let on_path = files(&["/tools/debrief", MAC_DEBRIEF]);
    assert_eq!(
        discover(debrief, Some("/nowhere:/tools"), Os::MacOs, &env, &on_path),
        Found::Program("/tools/debrief".into())
    );
    let installed = files(&[MAC_DEBRIEF]);
    assert_eq!(
        discover(debrief, None, Os::MacOs, &env, &installed),
        Found::Program(MAC_DEBRIEF.into())
    );
    let old = files(&["/usr/local/bin/imt"]);
    assert_eq!(
        discover(debrief, None, Os::MacOs, &env, &old),
        Found::NeedsUpdate
    );
    assert_eq!(
        discover(debrief, None, Os::MacOs, &env, &files(&[])),
        Found::Missing
    );
    // An app without a legacy name is simply missing.
    let pitcrew = AppSpec::lookup("pitcrew").unwrap();
    assert_eq!(
        discover(pitcrew, None, Os::Linux, &env, &files(&["/usr/bin/imt"])),
        Found::Missing
    );
    // Windows programs end in .exe.
    let win = files(&[r"C:\Users\me\AppData\Local/Programs/Debrief/debrief.exe"]);
    assert!(matches!(
        discover(debrief, None, Os::Windows, &env, &win),
        Found::Program(_)
    ));
}

#[test]
fn the_disk_probe_sees_files_not_folders() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.log");
    std::fs::write(&file, "x").unwrap();
    assert!(DiskProbe.is_file(&file));
    assert!(!DiskProbe.is_file(dir.path()) && !DiskProbe.is_file(&dir.path().join("none")));
}

// ---- opening -------------------------------------------------------------------------------------------------------

#[test]
fn a_file_is_handed_to_an_installed_app_which_is_started_with_it() {
    let (s, spawner) = siblings("pitcrew", Os::MacOs, &[MAC_DEBRIEF]);
    let out = s.import("debrief", vec!["/data/a.log".into()]).unwrap();
    assert_eq!(
        out,
        Outcome::Opened {
            program: MAC_DEBRIEF.into()
        }
    );
    assert_eq!(
        spawner.launched(),
        [(PathBuf::from(MAC_DEBRIEF), vec!["/data/a.log".into()])]
    );
    // Import validates against the same file system.
    assert!(matches!(
        s.import("debrief", vec!["/data/none.log".into()]),
        Err(SiblingError::BadFile { .. })
    ));
    assert_eq!(spawner.launched().len(), 1);
}

#[test]
fn a_missing_app_is_a_plain_answer_with_words_for_the_user() {
    let (s, spawner) = siblings("pitcrew", Os::MacOs, &[]);
    let Outcome::NotInstalled { message } = s.open("debrief", &Handoff::Open).unwrap() else {
        panic!()
    };
    assert!(message.contains("Debrief was not found") && message.contains("`debrief`"));
    let (s, _) = siblings("pitcrew", Os::MacOs, &["/usr/local/bin/imt"]);
    let Outcome::NeedsUpdate { message } = s.open("debrief", &Handoff::Open).unwrap() else {
        panic!()
    };
    assert!(message.contains("Update Debrief") && message.contains("`imt`"));
    assert!(
        spawner.launched().is_empty(),
        "nothing is started for an app that is not there"
    );
}

#[test]
fn an_unknown_app_itself_and_a_refused_handoff_are_refused_before_anything_is_looked_for() {
    let (s, spawner) = siblings("dyno", Os::MacOs, &[MAC_DEBRIEF]);
    assert!(matches!(
        s.open("calculator", &Handoff::Open),
        Err(SiblingError::UnknownApp(_))
    ));
    let e = s.open("dyno", &Handoff::Open).unwrap_err();
    assert_eq!(e, SiblingError::ToSelf("Dyno".into()));
    assert!(e.to_string().contains("itself"));
    let issue = Handoff::show(Item::new("issue", "9").unwrap());
    assert!(matches!(
        s.open("debrief", &issue),
        Err(SiblingError::NotAccepted { .. })
    ));
    assert!(matches!(
        s.presence("calculator"),
        Err(SiblingError::UnknownApp(_))
    ));
    assert!(spawner.launched().is_empty());
    assert!(matches!(
        Siblings::new(
            "calculator",
            Os::Linux,
            None,
            Box::new(env),
            Arc::new(files(&[])),
            Arc::new(FakeSpawner::new())
        ),
        Err(SiblingError::UnknownApp(_))
    ));
}

#[test]
fn a_program_that_will_not_start_is_an_error_naming_the_app() {
    let (s, spawner) = siblings(
        "dyno",
        Os::MacOs,
        &["/Applications/Pitcrew.app/Contents/MacOS/pitcrew"],
    );
    spawner.fail_with(Some("permission denied"));
    let e = s
        .open(
            "pitcrew",
            &Handoff::show(Item::new("machine", "m").unwrap()),
        )
        .unwrap_err();
    assert_eq!(
        e,
        SiblingError::LaunchFailed {
            app: "Pitcrew".into(),
            why: "permission denied".into()
        }
    );
    assert!(e.to_string().contains("Pitcrew could not be started"));
    spawner.fail_with(None);
    assert!(
        s.open(
            "pitcrew",
            &Handoff::show(Item::new("machine", "m").unwrap())
        )
        .is_ok()
    );
}

#[test]
fn the_family_list_says_who_is_installed_and_leaves_out_the_caller() {
    let (s, _) = siblings("dyno", Os::MacOs, &[MAC_DEBRIEF, "/usr/local/bin/imt"]);
    let all = s.all();
    assert!(all.iter().all(|p| p.id != "dyno"));
    assert_eq!(all.len(), FAMILY.len() - 1);
    let debrief = all.iter().find(|p| p.id == "debrief").unwrap();
    assert!(
        debrief.installed
            && debrief.program.as_deref() == Some(MAC_DEBRIEF)
            && !debrief.needs_update
    );
    let spotter = all.iter().find(|p| p.id == "spotter").unwrap();
    assert!(!spotter.installed && spotter.program.is_none());
    let (s, _) = siblings("dyno", Os::MacOs, &["/usr/local/bin/imt"]);
    assert!(s.presence("debrief").unwrap().needs_update);
    // The wire shape is camelCase.
    let json = serde_json::to_string(&s.presence("debrief").unwrap()).unwrap();
    assert!(json.contains("needsUpdate"));
}

#[test]
fn the_native_siblings_use_the_real_environment_without_starting_anything() {
    assert!(Siblings::native("debrief").is_ok());
    assert!(matches!(
        Siblings::native("calculator"),
        Err(SiblingError::UnknownApp(_))
    ));
}

#[test]
fn every_error_has_a_sentence() {
    for e in [
        SiblingError::UnknownApp("x".into()),
        SiblingError::ToSelf("Dyno".into()),
        SiblingError::NotAccepted {
            app: "A".into(),
            what: "files".into(),
        },
        SiblingError::BadFile {
            file: "f".into(),
            why: "w".into(),
        },
        SiblingError::TooManyFiles(99),
        SiblingError::NoFiles,
        SiblingError::BadItem("w".into()),
        SiblingError::LaunchFailed {
            app: "A".into(),
            why: "w".into(),
        },
    ] {
        assert!(e.to_string().len() > 10, "{e:?}");
    }
}
