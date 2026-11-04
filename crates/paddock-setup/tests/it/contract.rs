//! The command-line contract as the installer include and the app rely on it: arguments in, exit codes out.

use paddock_setup::apply::Refusal;
use paddock_setup::args::{Command, parse};
use paddock_setup::exit::Exit;

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(String::from).collect()
}

#[test]
fn the_include_can_call_every_command() {
    // The lines `paddock-per-machine.nsh` builds (ADR 0053 section 2); a change here is a change of the contract.
    for line in [
        r#"check-dir --dir C:\Program-Files\Dyno"#,
        "guard --product Dyno --main dyno.exe --update --restart",
        "stop-all --dir D --images dyno.exe,dyno-agent.exe",
        "same-folder --a A --b B",
        "clean-run --dir D --scope machine",
        "clean-run --dir D --scope all",
        "clean-staging --dir D",
        r"clean-staging --dir D --keep C:\a\updates\run-1-a",
        r"apply-update --file C:\t\u.exe --signature c2ln --version 1.2.3 --size 10 --restart",
        "hold-installing",
        "hold-installing --dir D",
        "self-test",
    ] {
        assert!(parse(&words(line)).is_ok(), "{line}");
    }
}

#[test]
fn the_apply_update_command_keeps_its_version_as_a_semver() {
    let Ok(Command::ApplyUpdate { version, .. }) = parse(&words(
        r"apply-update --file C:\t\u.exe --signature c2ln --version 1.2.3-beta.1 --restart",
    )) else {
        panic!("not parsed");
    };
    assert!(version < semver::Version::new(1, 2, 3));
}

#[test]
fn every_exit_code_the_app_may_see_has_a_sentence() {
    for r in Refusal::ALL {
        let exit = Exit::from_code(r.code()).expect("known code");
        assert_eq!(exit, Exit::Refused(r));
        assert!(!exit.sentence().is_empty());
    }
}

#[test]
fn documented_codes_never_change() {
    let table = [
        (Exit::Ok, 0),
        (Exit::NotSame, 1),
        (Exit::Blocked, 2),
        (Exit::BadDir, 3),
        (Exit::Usage, 4),
        (Exit::Failed, 5),
        (Exit::StillRunning, 11),
        (Exit::Refused(Refusal::NoKey), 20),
        (Exit::Refused(Refusal::Busy), 21),
        (Exit::Refused(Refusal::InstallDir), 22),
        (Exit::Refused(Refusal::SourceNotLocal), 23),
        (Exit::Refused(Refusal::TooLarge), 24),
        (Exit::Refused(Refusal::Io), 25),
        (Exit::Refused(Refusal::SignatureInvalid), 26),
        (Exit::Refused(Refusal::NotNewer), 27),
        (Exit::Refused(Refusal::StartFailed), 28),
        (Exit::Refused(Refusal::WrongProduct), 29),
        (Exit::Refused(Refusal::Squatted), 30),
    ];
    for (exit, code) in table {
        assert_eq!(exit.code(), code, "{exit:?}");
    }
}

/// Conformance C36: a parser never panics on any input.
#[test]
fn never_panics() {
    let inputs: Vec<Vec<String>> = vec![
        vec![],
        vec!["".into()],
        vec!["\u{0}".into(), "--dir".into(), "\u{ffff}".into()],
        vec![
            "apply-update".into(),
            "--version".into(),
            "99999999999999999999.0.0".into(),
        ],
        vec!["apply-update".into(), "--size".into(), "-1".into()],
        vec!["stop-all".into(), "--images".into(), ",,,,".into()],
        vec!["guard".into(), "--update".into(), "--update".into()],
        (0..500).map(|i| format!("--x{i}")).collect(),
    ];
    for i in inputs {
        let _ = parse(&i);
    }
}
