//! The run log (`<dev-targets>/runs/<agent>/...log`) and the hang detector (`--stall`) of the real binary with real processes.
//! The commands are testkit stand-ins (no shell scripts); every wait is bounded by the 40 s DEADLINE.
#![cfg(unix)]

use crate::procutil::*;
use paddock_proc::ProcessId;
use paddock_testkit::{Actions, Spawn, StandIn};
use paddock_throttle::exit;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The longest a stand-in naps: under the kit's cap, and far longer than any test here waits.
const NAP: Duration = Duration::from_secs(110);

fn dev_targets(w: &World) -> PathBuf {
    w.path("dev-targets")
}

fn env(w: &World) -> Vec<(String, String)> {
    vec![("PADDOCK_DEV_TARGETS".into(), dev_targets(w).to_string_lossy().into_owned())]
}

fn run_logs(w: &World, agent: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dev_targets(w).join("runs").join(agent))
        .map(|r| r.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn bin(w: &World) -> PathBuf {
    let d = w.path("bin");
    std::fs::create_dir_all(&d).expect("bin folder");
    d
}

fn heavy_with(w: &World, args: &[&str]) -> Guard {
    let e = env(w);
    let e: Vec<(&str, &str)> = e.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    heavy_env(w, args, &e)
}

fn finish(mut g: Guard) -> (i32, String) {
    let st = g.finish();
    let mut e = String::new();
    if let Some(mut s) = g.child.stderr.take() {
        use std::io::Read;
        let _ = s.read_to_string(&mut e);
    }
    (st.code().unwrap_or(-1), e)
}

/// The identity of the process that wrote `file`, read while it runs.
fn running_from(file: &Path) -> ProcessId {
    paddock_testkit_start::recorded::read_pid_file(file, "the stand-in")
        .running()
        .expect("the stand-in runs")
}

#[test]
fn the_commands_output_lands_in_the_run_log_and_the_path_and_a_summary_are_printed() {
    let w = World::new();
    let talker = StandIn::new(&bin(&w), "talker")
        .stdout("out-line-1")
        .stderr("err-line-2")
        .exit(3)
        .install()
        .expect("stand-in");
    let prog = talker.program().to_string_lossy().into_owned();
    let (code, e) = finish(heavy_with(&w, &["run", "--no-gate", "impl:RT/1", "--", &prog, "build"]));
    assert_eq!(code, 3, "{e}");
    let logs = run_logs(&w, "impl-RT-1");
    assert_eq!(logs.len(), 1, "one log in the agent's folder: {logs:?}\n{e}");
    let name = logs[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        paddock_throttle::capture::is_run_log(&name) && name.ends_with("-talker-build.log"),
        "{name}"
    );
    let text = std::fs::read_to_string(&logs[0]).unwrap();
    assert!(text.contains("out-line-1") && text.contains("err-line-2"), "{text}");
    assert!(e.contains(&format!("run log {}", logs[0].display())), "the path is printed: {e}");
    let summary = e
        .lines()
        .find(|l| l.contains("summary:"))
        .unwrap_or_else(|| panic!("a summary line: {e}"));
    assert!(
        summary.contains("exit 3") && summary.contains("ran") && !summary.contains("hung"),
        "{summary}"
    );
    assert!(text.contains("summary:"), "the log ends with the summary: {text}");
    // The live output still reaches the caller.
    assert!(e.contains("err-line-2"), "{e}");
}

#[test]
fn a_silent_idle_command_is_ended_as_hung_with_its_whole_tree_and_a_namesake_is_left_alone() {
    let w = World::new();
    let dir = bin(&w);
    let kid_pid = w.path("kid.pid");
    let top_pid = w.path("top.pid");
    let kid = StandIn::new(&dir, "napkid")
        .answers(Actions::new().writes_atomically(&kid_pid.to_string_lossy(), "{pid}\n").sleeps(NAP))
        .install()
        .expect("stand-in");
    let top = StandIn::new(&dir, "napper")
        .answers(
            Actions::new()
                .writes_atomically(&top_pid.to_string_lossy(), "{pid}\n")
                .spawns(&Spawn::new("napkid"))
                .sleeps(NAP),
        )
        .install()
        .expect("stand-in");
    // A process of the same name that throttle did not start (rule 53: never by name).
    let other_pid = w.path("other.pid");
    let other_kid = StandIn::new(&w.path("elsewhere"), "napkid");
    std::fs::create_dir_all(w.path("elsewhere")).unwrap();
    let other_kid = other_kid
        .answers(
            Actions::new()
                .writes_atomically(&other_pid.to_string_lossy(), "{pid}\n")
                .sleeps(NAP),
        )
        .install()
        .expect("stand-in");
    let mut namesake = Guard {
        child: paddock_testkit::TestCommand::new(other_kid.program()).spawn().expect("namesake"),
        label: "namesake".into(),
    };
    let namesake_id = running_from(&other_pid);

    let prog = top.program().to_string_lossy().into_owned();
    let g = heavy_with(&w, &["run", "--no-gate", "--stall", "1", "--run-max", "60", "H", "--", &prog]);
    let top_id = running_from(&top_pid);
    let kid_id = running_from(&kid_pid);
    let _ = &kid;
    let (code, e) = finish(g);
    assert_eq!(code, exit::HUNG, "{e}");
    assert!(e.contains("hung: no output and no CPU for 1 s"), "{e}");
    let summary = e
        .lines()
        .find(|l| l.contains("summary:"))
        .unwrap_or_else(|| panic!("a summary line: {e}"));
    assert!(summary.contains("exit 81") && summary.contains("hung"), "{summary}");
    wait_until("the hung command and its child to end", || top_id.has_ended() && kid_id.has_ended());
    assert!(
        namesake.child.try_wait().ok().flatten().is_none() && namesake_id.confirmed_running(),
        "a process of the same name that throttle did not start is never touched"
    );
    let _ = namesake.child.kill();
}

#[test]
fn a_silent_but_busy_command_is_not_hung() {
    let w = World::new();
    let spinner = StandIn::new(&bin(&w), "spinner")
        .answers(Actions::new().burns_cpu())
        .install()
        .expect("stand-in");
    let prog = spinner.program().to_string_lossy().into_owned();
    // The same --stall as the hung test (its positive control); the run limit ends the spinner, so the test is bounded.
    let (code, e) = finish(heavy_with(
        &w,
        &["run", "--no-gate", "--stall", "1", "--run-max", "5", "S", "--", &prog],
    ));
    assert_eq!(code, exit::RUN_LIMIT, "ended by the run limit, not as hung: {e}");
    assert!(!e.contains("hung:"), "{e}");
}
