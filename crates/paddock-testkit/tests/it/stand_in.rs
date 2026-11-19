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
fn a_program_name_with_a_dot_finds_its_script() {
    // `python3.12`: the script is `python3.12.stand-in.json`, never `python3.stand-in.json` (a file stem would cut at the dot).
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "python3.12")
        .stdout("3.12.1")
        .records_args()
        .install()
        .unwrap();
    assert_eq!(out(&run(p.program(), &["--version"])), "3.12.1\n");
    assert_eq!(p.calls().unwrap(), [["--version"]]);
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
    let text = std::fs::read_to_string(&pid_file).unwrap();
    // A complete record is a pid and a newline, what `paddock_testkit_start::recorded::read_pid_file` waits for.
    assert!(
        text.ends_with('\n'),
        "the pid record is not a complete line: {text:?}"
    );
    let pid: u32 = text.trim().parse().unwrap();
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

#[test]
fn an_exit_code_a_unix_status_cannot_hold_is_refused_at_install() {
    let s = Scratch::new();
    for bad in [256, -1, 1000] {
        let e = StandIn::new(&s.work(), "tool").exit(bad).install();
        let c = StandIn::new(&s.work(), "tool")
            .when_args(&["x"], Actions::new().exit(bad))
            .install();
        if cfg!(unix) {
            assert!(
                e.unwrap_err()
                    .to_string()
                    .contains("does not exist on Unix"),
                "{bad}"
            );
            assert!(c.is_err(), "{bad}: a case counts too");
        } else {
            assert!(e.is_ok() && c.is_ok());
        }
    }
    assert!(StandIn::new(&s.work(), "tool").exit(255).install().is_ok());
}

#[test]
fn echoing_stdin_in_a_case_reads_stdin_without_asking_for_it_twice() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "tee")
        .when_args(&["--echo"], Actions::new().echoes_stdin())
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program())
        .arg("--echo")
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
            .write_all(b"piped text")
            .unwrap();
    }
    assert_eq!(out(&child.wait_with_output().unwrap()), "piped text");
}

// What the shell-script fixtures needed (C130 conversion): output filled from the call, conditions, wrappers, pipes, installs.

use paddock_testkit::{Case, Exec};

#[test]
fn templates_fill_in_the_arguments_the_folder_the_environment_and_stdin() {
    let s = Scratch::new();
    let log = s.work().join("calls");
    let p = StandIn::new(&s.work(), "tool")
        .answers(
            Actions::new()
                .appends_with(
                    &log.to_string_lossy(),
                    "{name} {args}|{arg:2}|{arg:9}|{env:STAND_IN_T}|{env:STAND_IN_UNSET|unset}|{{literal}}\n",
                )
                .stdout_with("cwd={cwd} line=<{stdin_line}>"),
        )
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program())
        .args(["a b", "c"])
        .env("STAND_IN_T", "tv")
        .current_dir(s.work())
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
            .write_all(b"first\nsecond\n")
            .unwrap();
    }
    let o = child.wait_with_output().unwrap();
    assert!(out(&o).contains("line=<first>"), "{}", out(&o));
    assert!(out(&o).starts_with("cwd="), "{}", out(&o));
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().replace("\r\n", "\n"),
        "tool a b c|c||tv|unset|{literal}\n"
    );
}

#[test]
fn an_unknown_template_token_breaks_the_stand_in_instead_of_printing_it() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "tool")
        .answers(Actions::new().stdout_with("{nope}"))
        .install()
        .unwrap();
    let o = run(p.program(), &[]);
    assert_eq!(o.status.code(), Some(70));
    assert!(err(&o).contains("unknown template token"), "{}", err(&o));
}

#[test]
fn a_case_with_several_conditions_needs_all_of_them() {
    let s = Scratch::new();
    let flag = s.work().join("flag");
    let case = Case::new()
        .args_start_with(&["deny"])
        .file_exists(&flag)
        .has_none_of(&["licenses", "bans"]);
    let p = StandIn::new(&s.work(), "cargo")
        .exit(0)
        .when(&case, Actions::new().stderr("error: denied").exit(1))
        .when(
            &Case::new().has_arg("only-me").file_missing(&flag),
            Actions::new().stdout("no flag").exit(2),
        )
        .install()
        .unwrap();
    assert_eq!(
        run(p.program(), &["deny"]).status.code(),
        Some(0),
        "the file is not there"
    );
    assert_eq!(run(p.program(), &["only-me"]).status.code(), Some(2));
    std::fs::write(&flag, "").unwrap();
    assert_eq!(
        run(p.program(), &["only-me"]).status.code(),
        Some(0),
        "the flag exists now"
    );
    let o = run(p.program(), &["deny", "advisories"]);
    assert_eq!(
        (o.status.code(), err(&o).as_str()),
        (Some(1), "error: denied\n")
    );
    assert_eq!(
        run(p.program(), &["deny", "licenses"]).status.code(),
        Some(0),
        "an excluded argument"
    );
    assert_eq!(
        run(p.program(), &["check"]).status.code(),
        Some(0),
        "the prefix differs"
    );
}

#[test]
fn a_case_on_the_working_folder_reads_the_folder_the_call_ran_in() {
    let s = Scratch::new();
    let app = s.work().join("apps").join("dyno");
    std::fs::create_dir_all(&app).unwrap();
    let p = StandIn::new(&s.work(), "cargo")
        .when(
            &Case::new().cwd_ends_with("/apps/dyno"),
            Actions::new().exit(1),
        )
        .install()
        .unwrap();
    assert_eq!(
        TestCommand::new(p.program())
            .current_dir(&app)
            .status()
            .unwrap()
            .code(),
        Some(1)
    );
    assert_eq!(
        TestCommand::new(p.program())
            .current_dir(s.work())
            .status()
            .unwrap()
            .code(),
        Some(0)
    );
}

#[test]
fn exec_runs_another_program_with_the_call_arguments_and_its_exit_code() {
    let s = Scratch::new();
    let inner = StandIn::new(&s.work(), "inner")
        .records_args()
        .stdout("inner ran")
        .exit(4)
        .install()
        .unwrap();
    let wrapper = StandIn::new(&s.work(), "wrapper")
        .answers(
            Actions::new()
                .appends_with(&s.work().join("log").to_string_lossy(), "{args}\n")
                .execs(
                    &Exec::program(&inner.program().to_string_lossy())
                        .args(["first"])
                        .env("WRAPPED", "{name}"),
                ),
        )
        .install()
        .unwrap();
    let o = run(wrapper.program(), &["x", "y z"]);
    assert_eq!(o.status.code(), Some(4));
    assert_eq!(out(&o), "inner ran\n");
    assert_eq!(inner.calls().unwrap(), [["first", "x", "y z"]]);
}

#[test]
fn exec_of_the_call_arguments_and_exec_after_a_separator() {
    let s = Scratch::new();
    let inner = StandIn::new(&s.work(), "inner")
        .records_args()
        .install()
        .unwrap();
    let all = StandIn::new(&s.work(), "all")
        .answers(Actions::new().execs(&Exec::call_arguments()))
        .install()
        .unwrap();
    run(
        all.program(),
        &[&inner.program().to_string_lossy(), "p", "q"],
    );
    let after = StandIn::new(&s.work(), "after")
        .answers(Actions::new().execs(&Exec::call_arguments().after("--")))
        .install()
        .unwrap();
    run(
        after.program(),
        &["dropped", "--", &inner.program().to_string_lossy(), "r"],
    );
    assert_eq!(inner.calls().unwrap(), [vec!["p", "q"], vec!["r"]]);
    let none = run(after.program(), &["no separator"]);
    assert_eq!(none.status.code(), Some(70));
    assert!(err(&none).contains("no `--`"), "{}", err(&none));
}

#[test]
fn a_stand_in_that_waits_for_a_file_runs_on_only_once_it_exists() {
    let s = Scratch::new();
    let go = s.work().join("go");
    let p = StandIn::new(&s.work(), "gated")
        .answers(
            Actions::new()
                .waits_for_file(&go.to_string_lossy())
                .stdout("went"),
        )
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        child.try_wait().unwrap().is_none(),
        "it ran on without the file"
    );
    std::fs::write(&go, "").unwrap();
    assert_eq!(out(&child.wait_with_output().unwrap()), "went\n");
}

#[test]
fn the_exit_code_can_come_from_the_environment() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "npm")
        .answers(Actions::new().exits_with_env("FAKE_NPM_EXIT", 0))
        .install()
        .unwrap();
    assert_eq!(run(p.program(), &[]).status.code(), Some(0));
    assert_eq!(
        TestCommand::new(p.program())
            .env("FAKE_NPM_EXIT", "3")
            .status()
            .unwrap()
            .code(),
        Some(3)
    );
    let bad = TestCommand::new(p.program())
        .env("FAKE_NPM_EXIT", "x")
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(70));
}

#[test]
fn writes_atomically_and_makes_folders() {
    let s = Scratch::new();
    let f = s.work().join("deep").join("er").join("pid");
    let p = StandIn::new(&s.work(), "tool")
        .answers(
            Actions::new()
                .makes_dir(&s.work().join("deep").join("er").to_string_lossy())
                .writes_atomically(&f.to_string_lossy(), "{pid}\n"),
        )
        .install()
        .unwrap();
    run(p.program(), &[]);
    let pid = std::fs::read_to_string(&f).unwrap();
    assert!(pid.trim().parse::<u32>().is_ok(), "{pid:?}");
    assert!(
        !f.with_file_name("pid.part").exists(),
        "no half-written file is left"
    );
}

#[test]
fn installs_a_second_stand_in_that_answers_for_itself() {
    let s = Scratch::new();
    let out_dir = s.work().join("release");
    let built = StandIn::new(&s.work(), "built")
        .answers(Actions::new().stdout_with("embedded {env:STAND_IN_HASH}"))
        .when_args(&["--version"], Actions::new().stdout("0"))
        .when_args(
            &["args"],
            Actions::new().stdout_with("got {{args}} from {env:STAND_IN_HASH}"),
        );
    let builder = StandIn::new(&s.work(), "cargo")
        .answers(
            Actions::new()
                .makes_dir(&out_dir.to_string_lossy())
                .installs(
                    &out_dir
                        .join(format!("paddock-dev{}", std::env::consts::EXE_SUFFIX))
                        .to_string_lossy(),
                    &built,
                ),
        )
        .install()
        .unwrap();
    let o = TestCommand::new(builder.program())
        .env("STAND_IN_HASH", "abc")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let made = out_dir.join(format!("paddock-dev{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(out(&run(&made, &["--version"])), "0\n");
    assert_eq!(out(&run(&made, &["args", "k"])), "got args k from abc\n");
    assert_eq!(
        out(&run(&made, &[])),
        "embedded abc\n",
        "the token was filled when it was installed"
    );
    // One self-contained file: copied alone, to another name, it still answers (a wrapper that caches a built tool does this).
    let copy = s.work().join("copy");
    std::fs::create_dir_all(&copy).unwrap();
    let copied = copy.join("elsewhere");
    std::fs::copy(&made, &copied).unwrap();
    assert_eq!(out(&run(&copied, &["--version"])), "0\n");
    assert!(
        !out_dir.join("paddock-dev.stand-in.json").exists(),
        "no script is left beside it"
    );
}

#[cfg(unix)]
#[test]
fn a_named_pipe_hands_one_stand_in_to_another_and_the_reader_blocks_until_it_is_written() {
    let s = Scratch::new();
    let fifo = s.work().join("go");
    let maker = StandIn::new(&s.work(), "maker")
        .answers(
            Actions::new()
                .makes_fifo(&fifo.to_string_lossy())
                .reads_file(&fifo.to_string_lossy())
                .stdout("released")
                .exit(3),
        )
        .install()
        .unwrap();
    let mut child = TestCommand::new(maker.program())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_until("the named pipe exists", || fifo.exists());
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the reader did not block"
    );
    let writer = StandIn::new(&s.work(), "writer")
        .answers(Actions::new().writes(&fifo, "go\n"))
        .install()
        .unwrap();
    assert!(run(writer.program(), &[]).status.success());
    let o = child.wait_with_output().unwrap();
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(3), "released\n"));
}

#[cfg(unix)]
#[test]
fn a_stand_in_that_ignores_term_survives_it_and_dies_of_kill() {
    use std::os::unix::process::ExitStatusExt;
    let s = Scratch::new();
    let ready = s.work().join("ready");
    let p = StandIn::new(&s.work(), "stubborn")
        .answers(
            Actions::new()
                .ignores_term()
                .writes(&ready, "up")
                .sleeps(Duration::from_secs(60)),
        )
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program()).spawn().unwrap();
    wait_until("it is up", || ready.exists());
    // conformance: pid-kill — the pid is of a child this test spawned and still holds (not reaped), so it cannot have been handed on
    let term = paddock_env::command::command("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(term.success());
    std::thread::sleep(Duration::from_millis(200));
    assert!(child.try_wait().unwrap().is_none(), "SIGTERM ended it");
    child.kill().unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
}

#[test]
fn the_flag_token_is_what_follows_a_prefix_in_the_last_argument_that_has_it() {
    let s = Scratch::new();
    let f = s.work().join("port");
    let p = StandIn::new(&s.work(), "browser")
        .answers(Actions::new().writes_with(
            &f.to_string_lossy(),
            "dir=<{flag:--user-data-dir=}> none=<{flag:--nope=}>\n",
        ))
        .install()
        .unwrap();
    run(
        p.program(),
        &[
            "--headless",
            "--user-data-dir=/first",
            "--user-data-dir=/second dir",
        ],
    );
    assert_eq!(
        std::fs::read_to_string(&f).unwrap().replace("\r\n", "\n"),
        "dir=</second dir> none=<>\n"
    );
}

#[cfg(unix)]
#[test]
fn a_stand_in_that_stops_itself_goes_on_after_sigcont() {
    let s = Scratch::new();
    let before = s.work().join("before");
    let after = s.work().join("after");
    let p = StandIn::new(&s.work(), "stopper")
        .answers(
            Actions::new()
                .writes(&before, "x")
                .stops_itself()
                .writes(&after, "y"),
        )
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program()).spawn().unwrap();
    wait_until("it wrote before stopping", || before.exists());
    std::thread::sleep(Duration::from_millis(300));
    assert!(!after.exists(), "it did not stop");
    let id = child.id().to_string();
    assert!(
        // conformance: pid-kill — the pid is of a child this test spawned and still holds (not reaped), so it cannot have been handed on
        paddock_env::command::command("kill")
            .args(["-CONT", &id])
            .status()
            .unwrap()
            .success()
    );
    assert!(child.wait().unwrap().success());
    assert!(after.exists());
}

#[test]
fn files_are_the_program_and_its_script_to_pack_or_serve_and_they_run_where_they_are_put() {
    let s = Scratch::new();
    let f = StandIn::new(&s.work(), "Packed Tool")
        .stdout("from the archive")
        .exit(2)
        .files()
        .unwrap();
    assert_eq!(f.script_name, "Packed Tool.stand-in.json");
    let elsewhere = s.work().join("unpacked");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let program = elsewhere.join(&f.program_name);
    paddock_testkit::exe::write_program(&program, &f.program).unwrap();
    std::fs::write(elsewhere.join(&f.script_name), &f.script).unwrap();
    let o = run(&program, &[]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (Some(2), "from the archive\n")
    );
    let e = StandIn::new(&s.work(), "r")
        .records_args()
        .files()
        .unwrap_err();
    assert!(e.to_string().contains("cannot be packed"), "{e}");
}

#[test]
fn a_case_on_the_joined_arguments_and_on_file_contents_answers_only_when_they_hold() {
    let s = Scratch::new();
    let work = s.work();
    std::fs::write(work.join("a.toml"), "x = base64\n").unwrap();
    std::fs::write(work.join("b.toml"), "y = serde\n").unwrap();
    let p = StandIn::new(&work, "tool")
        .when(
            &Case::new()
                .args_text_contains(" -C x rev-parse HEAD ")
                .file_contains(&work.join("a.toml"), "base64")
                .file_lacks(&work.join("b.toml"), "base64"),
            Actions::new().exit(3),
        )
        .install()
        .unwrap();
    let call = |a: &[&str]| run(p.program(), a).status.code();
    assert_eq!(call(&["-C", "x", "rev-parse", "HEAD"]), Some(3));
    // Each condition alone can fail it: the joined text is a whole-word match, the files are read at the call.
    assert_eq!(call(&["-C", "x", "rev-parse", "HEAD2"]), Some(0));
    assert_eq!(call(&["-C", "xy", "rev-parse", "HEAD"]), Some(0));
    std::fs::write(work.join("b.toml"), "y = base64\n").unwrap();
    assert_eq!(
        call(&["-C", "x", "rev-parse", "HEAD"]),
        Some(0),
        "file_lacks"
    );
    std::fs::write(work.join("b.toml"), "y = serde\n").unwrap();
    std::fs::write(work.join("a.toml"), "x = serde\n").unwrap();
    assert_eq!(
        call(&["-C", "x", "rev-parse", "HEAD"]),
        Some(0),
        "file_contains"
    );
    // A missing file holds nothing: file_lacks is true.
    std::fs::write(work.join("a.toml"), "x = base64\n").unwrap();
    std::fs::remove_file(work.join("b.toml")).unwrap();
    assert_eq!(call(&["-C", "x", "rev-parse", "HEAD"]), Some(3));
}

#[test]
fn the_run_token_is_what_a_program_prints_in_the_working_folder() {
    let s = Scratch::new();
    let work = s.work();
    let p = StandIn::new(&work, "gates")
        .answers(Actions::new().stdout_with(
            "inside=<{run:git|rev-parse|--is-inside-work-tree}> bad=<{run:git|no-such-subcommand}>",
        ))
        .install()
        .unwrap();
    let repo = work.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    assert!(
        TestCommand::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    let o = TestCommand::new(p.program())
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(out(&o), "inside=<true> bad=<>\n");
}

#[test]
fn an_exact_name_may_end_in_exe_on_any_host_and_answers_for_itself() {
    let s = Scratch::new();
    let work = s.work();
    assert!(
        StandIn::new(&work, "sccache.exe").install().is_err(),
        "a plain name still refuses `.exe`"
    );
    let p = StandIn::new(&work, "sccache.exe")
        .exact_name()
        .stdout("sccache 0.19.0")
        .install()
        .unwrap();
    assert!(p.program().ends_with("sccache.exe"));
    assert_eq!(out(&run(p.program(), &[])), "sccache 0.19.0\n");
    let files = StandIn::new(&work, "sccache.exe")
        .exact_name()
        .files()
        .unwrap();
    assert!(
        files.program_name.ends_with("sccache.exe")
            && files.script_name.ends_with(".stand-in.json")
    );
}

#[test]
fn the_environ_token_lists_every_variable_the_call_was_given() {
    let s = Scratch::new();
    let log = s.work().join("env");
    let p = StandIn::new(&s.work(), "tool")
        .answers(Actions::new().appends_with(&log.to_string_lossy(), "{environ}\n"))
        .install()
        .unwrap();
    TestCommand::new(p.program())
        .env("PADDOCK_STAND_IN_CANARY", "a b")
        .status()
        .unwrap();
    let seen = std::fs::read_to_string(&log).unwrap();
    assert!(
        seen.lines().any(|l| l == "PADDOCK_STAND_IN_CANARY=a b"),
        "{seen}"
    );
}

#[test]
fn runs_starts_a_program_and_goes_on_with_the_next_step() {
    let s = Scratch::new();
    let work = s.work();
    let helper = StandIn::new(&work, "helper")
        .answers(Actions::new().stdout_with("helper {args}").exit(9))
        .install()
        .unwrap();
    let p = StandIn::new(&work, "tool")
        .answers(
            Actions::new()
                .runs(
                    &Exec::program(&helper.program().to_string_lossy())
                        .args(["first"])
                        .without_call_arguments(),
                )
                .stdout("after")
                .exit(4),
        )
        .install()
        .unwrap();
    let o = run(p.program(), &["x"]);
    assert_eq!(
        o.status.code(),
        Some(4),
        "the helper's exit code 9 is not the stand-in's"
    );
    // The helper's output goes to the stand-in's own stdout, before the next step's line.
    assert_eq!(out(&o), "helper first\nafter\n");
}

#[test]
fn an_env_condition_a_templated_file_condition_lines_and_a_flag_suffix_read_the_call() {
    let s = Scratch::new();
    let work = s.work();
    let count = work.join("runs");
    let p = StandIn::new(&work, "cargo")
        .answers(
            Actions::new()
                .appends_with(&count.to_string_lossy(), "run\n")
                .stdout_with("target={flag:build.target-dir='|'} runs={lines:%COUNT%|2} none={lines:%COUNT%.missing}"
                    .replace("%COUNT%", &count.to_string_lossy())
                    .as_str()),
        )
        .when(
            &Case::new().env_is_set("STAND_IN_FORBIDDEN"),
            Actions::new().stderr("forbidden").exit(97),
        )
        .when(
            &Case::new().file_missing_with("{flag:build.target-dir='|'}/marker"),
            Actions::new().stdout("cold").exit(3),
        )
        .install()
        .unwrap();
    let marker_dir = work.join("t");
    std::fs::create_dir_all(&marker_dir).unwrap();
    let arg = format!("build.target-dir='{}'", marker_dir.display());
    let call = |env: Option<(&str, &str)>| {
        let mut c = TestCommand::new(p.program());
        c.args(["--config", &arg]);
        if let Some((k, v)) = env {
            c.env(k, v);
        }
        c.output().unwrap()
    };
    let cold = call(None);
    assert_eq!(
        (cold.status.code(), out(&cold)),
        (Some(3), "cold\n".to_string()),
        "no marker yet"
    );
    std::fs::write(marker_dir.join("marker"), "").unwrap();
    for runs in 1..=3 {
        let o = call(None);
        let want = format!(
            "target={} runs={} none=0\n",
            marker_dir.display(),
            runs.min(2)
        );
        assert_eq!(
            out(&o),
            want,
            "the suffix quote is dropped and the count is capped"
        );
    }
    let o = call(Some(("STAND_IN_FORBIDDEN", "1")));
    assert_eq!(
        (o.status.code(), err(&o)),
        (Some(97), "forbidden\n".to_string())
    );
    let o = call(Some(("STAND_IN_FORBIDDEN", "")));
    assert_eq!(o.status.code(), Some(0), "an empty variable is not set");
}

#[test]
fn a_single_file_stand_in_answers_from_its_own_bytes_when_copied_alone() {
    let s = Scratch::new();
    let work = s.work();
    let tool = StandIn::new(&work, "ignored")
        .answers(Actions::new().stdout_with("name={name} args={args}"))
        .exit(5);
    let first = work.join("one");
    tool.write_single_file(&first).unwrap();
    let elsewhere = work.join("sub");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let second = elsewhere.join("renamed-tool");
    std::fs::copy(&first, &second).unwrap();
    let o = run(&second, &["a", "b"]);
    assert_eq!(
        (o.status.code(), out(&o)),
        (Some(5), "name=renamed-tool args=a b\n".to_string())
    );
    assert_eq!(
        std::fs::read(&first).unwrap(),
        tool.single_file_bytes().unwrap()
    );
}

#[test]
fn copies_a_binary_file_to_the_place_the_call_names() {
    let s = Scratch::new();
    let src = s.work().join("archive.bin");
    let bytes: Vec<u8> = vec![0, 159, 146, 150, 255, 10, 0];
    std::fs::write(&src, &bytes).unwrap();
    let p = StandIn::new(&s.work(), "curl")
        .answers(Actions::new().copies_file(&src.to_string_lossy(), "{arg:2}"))
        .install()
        .unwrap();
    let dest = s.work().join("copy.bin");
    let o = run(p.program(), &["-o", &dest.to_string_lossy()]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    // A source that is missing is a broken stand-in, not a silent success.
    std::fs::remove_file(&src).unwrap();
    assert!(
        !run(p.program(), &["-o", &dest.to_string_lossy()])
            .status
            .success()
    );
}

#[test]
fn a_bundled_program_finds_its_script_at_its_own_end_and_a_plain_one_alone_is_broken() {
    let s = Scratch::new();
    let f = StandIn::new(&s.work(), "tool")
        .stdout("alone")
        .exit(4)
        .files()
        .unwrap();
    let bundled = s.work().join("one").join(&f.program_name);
    std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
    paddock_testkit::exe::write_program(&bundled, f.bundled()).unwrap();
    let o = run(&bundled, &[]);
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(4), "alone\n"));
    // Control: the program without its script (nothing beside it, nothing at its end) is a broken stand-in.
    let plain = s.work().join("two").join(&f.program_name);
    std::fs::create_dir_all(plain.parent().unwrap()).unwrap();
    paddock_testkit::exe::write_program(&plain, &f.program).unwrap();
    let o = run(&plain, &[]);
    assert_eq!(o.status.code(), Some(70), "{}", err(&o));
}

#[test]
fn the_unquoted_flag_token_drops_one_pair_of_single_quotes_around_the_value() {
    let s = Scratch::new();
    let f = s.work().join("dir");
    let p = StandIn::new(&s.work(), "cargo")
        .answers(Actions::new().writes_with(
            &f.to_string_lossy(),
            "<{flag_unquoted:build.target-dir=}> <{flag_unquoted:other=}> <{flag_unquoted:none=}>\n",
        ))
        .install()
        .unwrap();
    run(
        p.program(),
        &["--config", "build.target-dir='/t dir'", "other=plain"],
    );
    assert_eq!(
        std::fs::read_to_string(&f).unwrap().replace("\r\n", "\n"),
        "</t dir> <plain> <>\n"
    );
}

#[test]
fn a_bundled_install_runs_under_any_name_it_is_copied_to() {
    let s = Scratch::new();
    let built = StandIn::new(&s.work(), "made").answers(
        Actions::new()
            .stdout_with("built {{args}}")
            .stdout("none")
            .exit(5),
    );
    let path = s
        .work()
        .join(format!("made{}", std::env::consts::EXE_SUFFIX));
    let maker = StandIn::new(&s.work(), "maker")
        .answers(Actions::new().installs(&path.to_string_lossy(), &built))
        .install()
        .unwrap();
    assert!(run(maker.program(), &[]).status.success());
    // No script beside it, and a copy under another name: it still answers.
    assert!(!s.work().join("made.stand-in.json").exists());
    let other = s
        .work()
        .join(format!("other{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(&path, &other).unwrap();
    let o = run(&other, &["a", "b"]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (Some(5), "built a b\nnone\n")
    );
}

#[test]
fn reads_one_stdin_line_without_waiting_for_the_end_of_stdin() {
    use std::io::{BufRead, Write};
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "runner")
        .answers(
            Actions::new()
                .reads_stdin_line()
                .stdout_with("got <{stdin_line}>"),
        )
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"go\nmore").unwrap();
    // stdin stays open: the line alone is enough.
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim_end(), "got <go>");
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn repeats_its_steps_until_it_is_ended_and_burns_cpu_until_it_is_ended() {
    use std::io::BufRead;
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "ticker")
        .answers(
            Actions::new().repeats(
                &Actions::new()
                    .stdout("tick")
                    .sleeps(Duration::from_millis(10)),
            ),
        )
        .install()
        .unwrap();
    let mut child = TestCommand::new(p.program())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
    for _ in 0..3 {
        assert_eq!(lines.next().unwrap().unwrap(), "tick");
    }
    child.kill().unwrap();
    child.wait().unwrap();

    let b = StandIn::new(&s.work(), "burner")
        .answers(Actions::new().burns_cpu())
        .install()
        .unwrap();
    let mut burner = TestCommand::new(b.program()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(burner.try_wait().unwrap().is_none(), "it ended by itself");
    burner.kill().unwrap();
    burner.wait().unwrap();
}

#[test]
fn the_nice_token_is_a_number_and_the_cgroup_token_falls_back_when_the_file_is_not_there() {
    let s = Scratch::new();
    let p = StandIn::new(&s.work(), "probe")
        .answers(Actions::new().stdout_with("ni={nice} mh={cgroup:no-such-file.here|none}"))
        .install()
        .unwrap();
    let o = run(p.program(), &[]);
    let text = out(&o);
    let ni = text
        .trim_end()
        .strip_prefix("ni=")
        .and_then(|t| t.strip_suffix(" mh=none"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(ni.parse::<i32>().is_ok(), "{text}");
}

#[test]
fn a_sleep_over_the_cap_is_refused_at_install_wherever_it_hides() {
    use paddock_testkit::MAX_SLEEP;
    let s = Scratch::new();
    let long = MAX_SLEEP + Duration::from_secs(1);
    assert!(
        StandIn::new(&s.work(), "ok")
            .sleeps(MAX_SLEEP)
            .install()
            .is_ok(),
        "the cap itself is allowed"
    );
    let default = StandIn::new(&s.work(), "a")
        .sleeps(long)
        .install()
        .unwrap_err();
    assert!(default.to_string().contains("over the cap"), "{default}");
    let in_case = StandIn::new(&s.work(), "b")
        .when_args(&["x"], Actions::new().sleeps(long))
        .install();
    assert!(in_case.is_err(), "a case counts");
    let repeated = StandIn::new(&s.work(), "c")
        .answers(Actions::new().repeats(&Actions::new().sleeps(long)))
        .install();
    assert!(repeated.is_err(), "a repeat counts");
    let nested = StandIn::new(&s.work(), "d")
        .answers(Actions::new().installs("x", &StandIn::new(&s.work(), "inner").sleeps(long)))
        .files();
    assert!(nested.is_err(), "an installed program counts");
}

#[test]
fn burn_and_repeat_end_at_the_cap_and_say_so() {
    let s = Scratch::new();
    let burn = StandIn::new(&s.work(), "burn")
        .answers(Actions::new().burns_cpu())
        .install()
        .unwrap();
    let repeat = StandIn::new(&s.work(), "rep")
        .answers(Actions::new().repeats(&Actions::new().sleeps(Duration::from_millis(20))))
        .install()
        .unwrap();
    for p in [&burn, &repeat] {
        // The cap is two minutes; the variable lowers it so the control can run.
        let o = TestCommand::new(p.program())
            .env("PADDOCK_STAND_IN_CAP_MS", "300")
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(70), "{}", err(&o));
        assert!(err(&o).contains("cap of two minutes"), "{}", err(&o));
    }
}

#[test]
fn a_single_file_program_written_to_a_path_runs_and_replaces_one_that_is_there() {
    let s = Scratch::new();
    let path = s
        .work()
        .join(format!("tool{}", std::env::consts::EXE_SUFFIX));
    StandIn::new(&s.work(), "tool")
        .stdout("one")
        .write_single_file(&path)
        .unwrap();
    assert_eq!(out(&run(&path, &[])), "one\n");
    StandIn::new(&s.work(), "tool")
        .stdout("two")
        .write_single_file(&path)
        .unwrap();
    assert_eq!(out(&run(&path, &[])), "two\n");
}

#[test]
fn a_literal_with_braces_reaches_the_output_unchanged_through_a_template_method() {
    let s = Scratch::new();
    let text = r#"{"path":"C:\{x}\y"}"#;
    let p = StandIn::new(&s.work(), "tool")
        .answers(Actions::new().stdout_with(&paddock_testkit::template_literal(text)))
        .install()
        .unwrap();
    assert_eq!(out(&run(p.program(), &[])), format!("{text}\n"));
}
