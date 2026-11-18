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
                    &format!(
                        "{}/paddock-dev{}",
                        out_dir.display(),
                        std::env::consts::EXE_SUFFIX
                    ),
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
    let term = std::process::Command::new("kill")
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
        std::process::Command::new("kill")
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
