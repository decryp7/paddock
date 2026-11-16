//! A diagnostic for ci.yml run 8161, not a test of behaviour: on the Windows runner the merge_check integration tests ran about
//! 5.7 times slower than in run 8138, and the test VM does not reproduce it (docs/benchmarks.md, "A child's `git` on Windows").
//! This measures on the runner itself what one git call costs a test: through `TestCommand` (its construction and its child's
//! curated environment, as merge_check's `World::cmd` starts git) against a plain `Command` of the same git, alone and with
//! eight threads at once (25 calls each); it prints `path_report()` of a merge_check-shaped command and what the runner says about Defender.
//! It always fails, so nextest prints its numbers. It lives on the branch diag/testkit-git-cost only and is never merged.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stderr)]

use paddock_testkit::TestCommand;
use std::path::{Path, PathBuf};
use std::time::Instant;

// Small enough to finish inside nextest's 60 s on the slow test VM (100 calls timed out there); each line is printed as soon as
// it is measured, so a timeout still shows what was.
const CALLS: usize = 25;
const THREADS: usize = 8;

/// merge_check's `World::cmd`: a TestCommand for `program` with git's config and identity pinned.
fn merge_check_cmd(program: &str) -> TestCommand {
    let mut c = TestCommand::new(program);
    c.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
    c
}

/// Milliseconds per call of `calls` runs of `call`.
fn per_call(calls: usize, call: &dyn Fn()) -> f64 {
    let t = Instant::now();
    for _ in 0..calls {
        call();
    }
    t.elapsed().as_secs_f64() * 1000.0 / calls as f64
}

/// Milliseconds per call, the median of `THREADS` threads each making `calls` calls at once.
fn per_call_concurrent(calls: usize, call: fn(&Path), git: &Path) -> f64 {
    let mut each: Vec<f64> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let git = git.to_path_buf();
                s.spawn(move || per_call(calls, &|| call(&git)))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    each.sort_by(f64::total_cmp);
    each[each.len() / 2]
}

fn through_test_command(_git: &Path) {
    let out = merge_check_cmd("git").arg("--version").output().unwrap();
    assert!(out.status.success());
}

#[allow(clippy::disallowed_methods)] // the plain baseline the diagnostic compares with, awaited here
fn through_plain_command(git: &Path) {
    let out = std::process::Command::new(git).arg("--version").output().unwrap();
    assert!(out.status.success());
}

/// Only the construction of the command, no child: what the kit itself costs per call.
fn construction_only(_git: &Path) {
    drop(merge_check_cmd("git"));
}

#[allow(clippy::disallowed_methods)] // a read of the runner's Defender settings, awaited here
fn defender() -> String {
    let script = "try { $p = Get-MpPreference -ErrorAction Stop; 'ExclusionPath: ' + (($p.ExclusionPath) -join '; '); \
                  'ExclusionProcess: ' + (($p.ExclusionProcess) -join '; ') } catch { 'Get-MpPreference: ' + $_.Exception.Message }; \
                  try { $s = Get-MpComputerStatus -ErrorAction Stop; 'RealTimeProtectionEnabled: ' + $s.RealTimeProtectionEnabled; \
                  'AMRunningMode: ' + $s.AMRunningMode } catch { 'Get-MpComputerStatus: ' + $_.Exception.Message }";
    match std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    {
        Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("powershell did not start: {e}"),
    }
}

#[test]
fn diagnostic_windows_git_cost_always_fails_to_print_its_numbers() {
    let probe = merge_check_cmd("git");
    let report = probe.path_report();
    let git = PathBuf::from(probe.get_program());
    let child_git = report.child.iter().map(|d| d.join("git.exe")).find(|p| p.is_file());
    // Warm both paths once (first-run checks of a fresh program are not what is measured).
    through_test_command(&git);
    through_plain_command(&git);
    eprintln!(
        "program a merge_check command starts: {}; first git.exe on its child's PATH: {child_git:?}",
        git.display()
    );
    let alone_kit = per_call(CALLS, &|| through_test_command(&git));
    eprintln!("alone, TestCommand + git: {alone_kit:.1} ms per call");
    let alone_plain = per_call(CALLS, &|| through_plain_command(&git));
    eprintln!("alone, plain Command + git: {alone_plain:.1} ms per call");
    let alone_build = per_call(CALLS, &|| construction_only(&git));
    eprintln!("alone, TestCommand construction only: {alone_build:.2} ms per call");
    let busy_kit = per_call_concurrent(CALLS, through_test_command, &git);
    eprintln!("{THREADS} threads, TestCommand + git: {busy_kit:.1} ms per call");
    let busy_plain = per_call_concurrent(CALLS, through_plain_command, &git);
    eprintln!("{THREADS} threads, plain Command + git: {busy_plain:.1} ms per call");
    let busy_build = per_call_concurrent(CALLS, construction_only, &git);
    eprintln!("{THREADS} threads, TestCommand construction only: {busy_build:.2} ms per call");
    // A program under test starts git by name through its own PATH, the curated one: the same calls from a shell started like that.
    let loop_ms = {
        let t = Instant::now();
        let out = merge_check_cmd("sh")
            .args([
                "-c",
                &format!("i=0; while [ $i -lt {CALLS} ]; do git --version >/dev/null || exit 1; i=$((i+1)); done"),
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        t.elapsed().as_secs_f64() * 1000.0 / CALLS as f64
    };
    let mut said = String::new();
    said.push_str(&format!(
        "git by name from a child on the curated PATH, ms per call ({CALLS} calls, alone): {loop_ms:.1}\n"
    ));
    said.push_str(&format!("program a merge_check command starts: {}\n", git.display()));
    said.push_str(&format!("first git.exe on its child's PATH: {child_git:?}\n"));
    said.push_str(&format!("{report}\n"));
    said.push_str(&format!(
        "alone, ms per call ({CALLS} calls): TestCommand + git {alone_kit:.1}, plain Command + git {alone_plain:.1}, \
         TestCommand construction only {alone_build:.2}\n"
    ));
    said.push_str(&format!(
        "{THREADS} threads at once, median ms per call: TestCommand + git {busy_kit:.1}, plain Command + git {busy_plain:.1}, \
         TestCommand construction only {busy_build:.2}\n"
    ));
    said.push_str(&format!("temp folder: {}\n", paddock_testkit::ambient().temp_dir.display()));
    said.push_str(&format!("Defender:\n{}", defender()));
    panic!("diagnostic for run 8161 (not a failure of the code):\n{said}");
}
