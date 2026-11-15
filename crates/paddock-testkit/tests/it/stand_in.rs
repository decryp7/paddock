//! `StandIn`: every action, the record (arguments with spaces and non-ASCII), the defaults and the failures. Each test runs the
//! installed program for real, so a pass means it also runs on Windows, where a `#!/bin/sh` fixture could not.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::{Actions, Scratch, Spawn, StandIn, TestCommand, wait_until};
use std::path::Path;
use std::process::Output;
use std::time::Duration;

fn run(program: &Path, args: &[&str]) -> Output {
    TestCommand::new(program).args(args).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n")
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).replace("\r\n", "\n")
}

#[test]
fn prints_stdout_and_stderr_lines_and_exits_with_the_code() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "cargo")
        .stdout("added 3 packages")
        .stderr("warning: x")
        .stdout("done")
        .exit(7)
        .install()
        .unwrap();
    let o = run(p.program(), &["build"]);
    assert_eq!(o.status.code(), Some(7));
    assert_eq!(out(&o), "added 3 packages\ndone\n");
    assert_eq!(err(&o), "warning: x\n");
}

#[test]
fn installs_under_the_program_name_with_the_platform_suffix() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "smartctl").install().unwrap();
    assert_eq!(
        p.program(),
        s.work()
            .join(format!("smartctl{}", std::env::consts::EXE_SUFFIX))
    );
    assert_eq!(p.dir(), s.work());
    assert!(run(p.program(), &[]).status.success());
}

#[test]
fn writes_and_appends_files() {
    let s = Scratch::new();
    let f = s.work().join("made.txt");
    let p = StandIn::new(&s.work(), "tool")
        .writes(&f, "one\n")
        .appends(&f, "two\n")
        .install()
        .unwrap();
    run(p.program(), &[]);
    run(p.program(), &[]);
    // Each call truncates with `writes`, then appends: the second call leaves the same file.
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "one\ntwo\n");
}

#[test]
fn records_arguments_with_spaces_quotes_and_non_ascii() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "git")
        .records_args()
        .install()
        .unwrap();
    let args = [
        "commit",
        "-m",
        "two words",
        "say \"hi\"",
        "naïve 日本 😀",
        "back\\slash",
        "line\nbreak",
        "",
    ];
    run(p.program(), &args);
    run(p.program(), &["--version"]);
    let calls = p.calls().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], args.map(String::from));
    assert_eq!(calls[1], ["--version"]);
}

#[test]
fn records_the_environment_variables_asked_for_and_none_for_unset_ones() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "cargo")
        .records_env(&["STAND_IN_SET", "STAND_IN_UNSET"])
        .install()
        .unwrap();
    TestCommand::new(p.program())
        .env("STAND_IN_SET", "väl ue")
        .output()
        .unwrap();
    let calls = p.invocations().unwrap();
    assert_eq!(
        calls[0].env,
        [
            ("STAND_IN_SET".to_string(), Some("väl ue".to_string())),
            ("STAND_IN_UNSET".to_string(), None)
        ]
    );
    assert_eq!(calls[0].stdin, None);
}

#[test]
fn reads_stdin_records_it_and_echoes_it() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "tee")
        .reads_stdin()
        .echoes_stdin()
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all("héllo\nworld".as_bytes())
            .unwrap();
    }
    let o = child.wait_with_output().unwrap();
    assert_eq!(out(&o), "héllo\nworld");
    assert_eq!(
        p.invocations().unwrap()[0].stdin.as_deref(),
        Some("héllo\nworld")
    );
}

#[test]
fn sleeps_then_goes_on() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "slow")
        .sleeps(Duration::from_millis(20))
        .stdout("after")
        .install()
        .unwrap();
    assert_eq!(out(&run(p.program(), &[])), "after\n");
}

#[test]
fn starts_a_named_grandchild_beside_it_and_writes_its_pid() {
    let s = Scratch::new();
    let marker = s.work().join("child-ran");
    let pid_file = s.work().join("child.pid");
    StandIn::new(&s.work(), "child")
        .writes(&marker, "yes")
        .install()
        .unwrap();
    let parent = StandIn::new(&s.work(), "parent")
        .spawns(&Spawn::new("child").pid_file(&pid_file))
        .stdout("parent done")
        .install()
        .unwrap();
    assert_eq!(out(&run(parent.program(), &[])), "parent done\n");
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(pid > 0);
    wait_until("the grandchild wrote its marker", || marker.exists());
}

#[test]
fn the_first_matching_case_answers_and_a_later_rule_overrides_an_earlier_one() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "tool")
        .stdout("default")
        .when_args(&["--scan"], Actions::new().stdout("scan-1"))
        .when_args(&["--scan"], Actions::new().stdout("scan-2").exit(4))
        .when_args(&["info", "usb"], Actions::new().stderr("usb"))
        .install()
        .unwrap();
    let scan = run(p.program(), &["--scan", "extra"]);
    assert_eq!(
        (out(&scan), scan.status.code()),
        ("scan-2\n".to_string(), Some(4))
    );
    assert_eq!(err(&run(p.program(), &["info", "usb", "x"])), "usb\n");
    assert_eq!(out(&run(p.program(), &["info"])), "default\n");
    assert_eq!(out(&run(p.program(), &[])), "default\n");
}

#[test]
fn a_case_can_match_any_argument_that_contains_a_text_and_the_latest_rule_wins() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "smartctl")
        .stdout("ok")
        .when_arg_contains("usb-", Actions::new().stderr("cannot open").exit(2))
        .install()
        .unwrap();
    let usb = run(p.program(), &["-a", "-j", "IOService:/x/usb-drd1@B"]);
    assert_eq!(
        (usb.status.code(), err(&usb)),
        (Some(2), "cannot open\n".to_string())
    );
    assert_eq!(out(&run(p.program(), &["-a", "-j", "/dev/nvme0"])), "ok\n");
}

#[test]
fn a_stand_in_that_does_not_record_says_so() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "quiet").install().unwrap();
    run(p.program(), &[]);
    assert!(p.calls().unwrap_err().to_string().contains("records_args"));
}

#[test]
fn a_recording_stand_in_that_was_never_called_has_no_calls() {
    let s = Scratch::new();
    assert!(
        StandIn::new(&s.work(), "idle")
            .records_args()
            .install()
            .unwrap()
            .calls()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn installing_again_replaces_the_script_and_empties_the_record() {
    let s = Scratch::new();
    let first = StandIn::new(&s.work(), "tool")
        .records_args()
        .stdout("one")
        .install()
        .unwrap();
    run(first.program(), &[]);
    let second = StandIn::new(&s.work(), "tool")
        .records_args()
        .stdout("two")
        .install()
        .unwrap();
    assert!(second.calls().unwrap().is_empty());
    assert_eq!(out(&run(second.program(), &[])), "two\n");
}

#[test]
fn a_broken_script_exits_70_and_says_why() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "tool").install().unwrap();
    std::fs::write(s.work().join("tool.stand-in.json"), "{ not json").unwrap();
    let o = run(p.program(), &[]);
    assert_eq!(o.status.code(), Some(70));
    assert!(
        err(&o).contains("paddock-stand-in:") && err(&o).contains("tool.stand-in.json"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_missing_script_exits_70_and_names_the_file() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "tool").install().unwrap();
    std::fs::remove_file(s.work().join("tool.stand-in.json")).unwrap();
    let o = run(p.program(), &[]);
    assert_eq!(o.status.code(), Some(70));
    assert!(err(&o).contains("cannot read its script"), "{}", err(&o));
}

#[test]
fn an_unknown_step_is_refused_not_ignored() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "tool").install().unwrap();
    std::fs::write(
        s.work().join("tool.stand-in.json"),
        r#"{"actions":{"steps":[{"explode":1}]}}"#,
    )
    .unwrap();
    let o = run(p.program(), &[]);
    assert_eq!(o.status.code(), Some(70));
    assert!(err(&o).contains("unknown step `explode`"), "{}", err(&o));
}

#[test]
fn install_refuses_names_that_are_not_a_bare_program_name() {
    let s = Scratch::new();
    for bad in ["", "a/b", "a\\b", "git.exe", "GIT.EXE"] {
        assert!(StandIn::new(&s.work(), bad).install().is_err(), "{bad:?}");
    }
}

#[test]
fn install_into_a_missing_folder_is_an_error_naming_it() {
    let s = Scratch::new();
    let e = StandIn::new(&s.work().join("missing"), "tool")
        .install()
        .unwrap_err();
    assert!(e.to_string().contains("missing"), "{e}");
}

#[test]
fn npm_like_answers_the_common_calls_as_npm_prints_them() {
    let s = Scratch::new();
    let p = StandIn::npm_like(&s.work()).install().unwrap();
    assert_eq!(out(&run(p.program(), &["--version"])), "10.9.2\n");
    let ci = run(p.program(), &["ci"]);
    assert!(
        out(&ci).contains("added 3 packages, and audited 4 packages in 1s") && ci.status.success()
    );
    assert!(out(&run(p.program(), &["ls", "--json"])).starts_with("{\n  \"name\": \"fixture\""));
    let unknown = run(p.program(), &["frobnicate"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(err(&unknown).contains("npm error"));
    assert_eq!(p.calls().unwrap().len(), 4);
}

#[test]
fn git_like_answers_the_common_calls_and_a_test_can_override_one() {
    let s = Scratch::new();
    let p = StandIn::git_like(&s.work())
        .when_args(&["status"], Actions::new().stdout(" M a.txt"))
        .install()
        .unwrap();
    assert_eq!(
        out(&run(p.program(), &["--version"])),
        "git version 2.47.1\n"
    );
    assert_eq!(
        out(&run(p.program(), &["rev-parse", "HEAD"])).trim().len(),
        40
    );
    assert_eq!(
        out(&run(p.program(), &["status", "--porcelain"])),
        " M a.txt\n"
    );
    assert_eq!(run(p.program(), &["nope"]).status.code(), Some(1));
}
