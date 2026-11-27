//! The shim through the real binary: `throttle` called as `cargo` finds a stand-in tool further along PATH, takes a slot, runs it with
//! the same arguments, streams and exit code, and records the run (user decision 77). Unix only: the links are symlinks and the
//! stand-in tools are shell scripts.
#![cfg(unix)]

use crate::procutil::*;
use paddock_testkit::TestCommand;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// A shims folder (written by `throttle shims install`), a folder with the stand-in `cargo`, and a project folder with a `.git`.
struct Rig {
    w: World,
    shims: PathBuf,
    proj: PathBuf,
}

/// The stand-in `cargo`: prints what it was given and what the shim told it, then waits for the flag file when `WAIT_FLAG` is set.
const STAND_IN: &str = r#"#!/bin/sh
echo "real-cargo $*"
echo "held=${THROTTLE_HELD:-none}"
if [ -n "$WAIT_FLAG" ]; then while [ ! -e "$WAIT_FLAG" ]; do sleep 0.1; done; fi
if [ -n "$STAND_SLEEP" ]; then exec sleep "$STAND_SLEEP"; fi
exit "${STAND_EXIT:-0}"
"#;

fn exe(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn rig() -> Rig {
    let w = World::new();
    let (shims, real, proj) = (w.path("shims"), w.path("real"), w.path("proj"));
    exe(&real.join("cargo"), STAND_IN);
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    let mut c = TestCommand::new(BIN);
    base_env(&mut c, &w);
    let out = c
        .args(["shims", "install", shims.to_str().unwrap()])
        .output()
        .expect("run shims install");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    Rig { w, shims, proj }
}

impl Rig {
    fn path_var(&self) -> String {
        format!("{}:{}:/usr/bin:/bin", self.shims.display(), self.w.path("real").display())
    }

    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> TestCommand {
        let mut c = TestCommand::new(self.shims.join("cargo"));
        base_env(&mut c, &self.w);
        c.current_dir(&self.proj)
            .env("PATH", self.path_var())
            // The load of the machine the test runs on never holds the shim at the gate.
            .env("THROTTLE_GATE_LOAD", "100000")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            c.env(k, v);
        }
        c
    }

    fn start(&self, args: &[&str], env: &[(&str, &str)]) -> Guard {
        Guard {
            child: self.command(args, env).spawn().expect("spawn the shim"),
            label: format!("cargo {args:?}"),
        }
    }

    /// Runs the shim to its end: (exit code, stdout, stderr).
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
        let mut g = self.start(args, env);
        let st = g.finish();
        let (mut o, mut e) = (String::new(), String::new());
        if let Some(mut s) = g.child.stdout.take() {
            let _ = s.read_to_string(&mut o);
        }
        if let Some(mut s) = g.child.stderr.take() {
            let _ = s.read_to_string(&mut e);
        }
        (st.code().unwrap_or(-1), o, e)
    }

    fn run_log(&self) -> String {
        let dir = self.w.path("dev-targets/runs/shim");
        let mut all = String::new();
        for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            all.push_str(&std::fs::read_to_string(f.path()).unwrap_or_default());
        }
        all
    }

    fn slot_lock(&self) -> PathBuf {
        self.w.root().join("throttle-slot1.lock")
    }
}

#[test]
fn a_stand_in_tool_is_found_past_the_shim_folder_and_gets_the_same_arguments() {
    let r = rig();
    let (code, out, err) = r.run(&["build", "--release", "-p", "x y"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("real-cargo build --release -p x y"), "{out}");
    assert!(
        err.is_empty(),
        "the shim adds nothing to the tool's standard error when nothing waits: {err}"
    );
}

#[test]
fn the_exit_code_of_the_tool_is_the_exit_code_of_the_shim() {
    let r = rig();
    for want in [0, 1, 7, 101] {
        let (code, _, err) = r.run(&["test"], &[("STAND_EXIT", &want.to_string())]);
        assert_eq!(code, want, "{err}");
    }
}

#[test]
fn standard_input_reaches_the_tool_untouched() {
    let r = rig();
    exe(&r.w.path("real/cargo"), "#!/bin/sh\ncat\n");
    let mut c = r.command(&["x"], &[]);
    c.stdin(Stdio::piped());
    let mut g = Guard {
        child: c.spawn().expect("spawn"),
        label: "cargo with stdin".into(),
    };
    g.child.stdin.take().unwrap().write_all(b"typed input\n").unwrap();
    assert!(g.finish().success());
    let mut o = String::new();
    g.child.stdout.take().unwrap().read_to_string(&mut o).unwrap();
    assert_eq!(o, "typed input\n");
}

#[test]
fn no_real_tool_on_the_path_is_said_with_the_next_step_and_exit_127() {
    let r = rig();
    let (code, _, err) = r.run(&["x"], &[("PATH", &r.shims.display().to_string())]);
    assert_eq!(code, 127, "{err}");
    assert!(err.contains("no real `cargo`") && err.contains("Next:"), "{err}");
}

#[test]
fn the_shim_holds_a_slot_while_the_tool_runs_and_hands_the_tool_the_held_marker() {
    let r = rig();
    let flag = r.w.flag();
    let mut g = r.start(&["build"], &[("WAIT_FLAG", flag.to_str().unwrap())]);
    wait_until("the shim holds the slot", || r.w.locked());
    r.w.release();
    assert!(g.finish().success());
    let mut o = String::new();
    g.child.stdout.take().unwrap().read_to_string(&mut o).unwrap();
    let held = o.lines().find_map(|l| l.strip_prefix("held=")).unwrap_or("");
    assert!(held.ends_with(&format!(":{}", g.pid())), "THROTTLE_HELD names the shim's pid: {o}");
    assert!(
        !paddock_throttle::slotfile::is_held(&r.slot_lock()).unwrap(),
        "the slot is free when the tool ended"
    );
}

#[test]
fn a_nested_shim_runs_at_once_in_the_slot_its_holder_has_and_records_nothing() {
    let r = rig();
    // Another run holds the only slot.
    let (flag, out) = (r.w.flag(), r.w.path("out"));
    let hold = heavy_env(&r.w, &hold_args("H", flag.to_str().unwrap(), out.to_str().unwrap()), &[]);
    wait_until("H holds the slot", || r.w.locked());
    let marker = format!("run-1:{}", hold.pid());
    let (code, o, err) = r.run(&["check"], &[("THROTTLE_HELD", &marker)]);
    assert_eq!(code, 0, "{err}");
    assert!(o.contains("real-cargo check"), "{o}");
    assert!(o.contains(&format!("held={marker}")), "the marker passes through unchanged: {o}");
    assert_eq!(r.run_log(), "", "a nested run is part of its holder's run");
    r.w.release();
}

#[test]
fn a_shim_killed_with_sigkill_frees_its_slot_at_once_while_the_tool_still_runs() {
    let r = rig();
    let flag = r.w.flag();
    let mut g = r.start(&["build"], &[("WAIT_FLAG", flag.to_str().unwrap())]);
    wait_until("the shim holds the slot", || {
        paddock_throttle::slotfile::is_held(&r.slot_lock()).unwrap_or(false)
    });
    g.child.kill().expect("SIGKILL");
    g.wait(DEADLINE).expect("the shim is gone");
    assert!(
        !paddock_throttle::slotfile::is_held(&r.slot_lock()).unwrap(),
        "the kernel freed the lock, the tool did not keep it"
    );
    // The next run does not wait for the orphaned tool.
    let (code, _, err) = r.run(&["next"], &[]);
    assert_eq!(code, 0, "{err}");
    r.w.release();
}

#[test]
fn each_run_is_one_line_in_the_run_log_without_its_arguments() {
    let r = rig();
    let (code, _, err) = r.run(&["publish", "--token", "SECRET-VALUE"], &[("STAND_EXIT", "3")]);
    assert_eq!(code, 3, "{err}");
    let log = r.run_log();
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains(" shim tool=cargo project=proj start="), "{log}");
    assert!(log.contains("exit=3") && log.contains("duration_ms="), "{log}");
    assert!(
        !log.contains("SECRET") && !log.contains("publish"),
        "the command line is not recorded (ER-27): {log}"
    );
}

#[test]
fn a_stalled_tool_is_reported_and_logged_and_not_ended_by_default() {
    let r = rig();
    let (code, _, err) = r.run(&["slow"], &[("PADDOCK_SHIM_STALL", "1"), ("STAND_SLEEP", "4")]);
    assert_eq!(code, 0, "observe-by-default never ends the tool: {err}");
    assert!(err.contains("may be stuck") && err.contains("not ending it"), "{err}");
    let log = r.run_log();
    assert!(
        log.contains("shim-stall tool=cargo project=proj") && log.contains("ends=false"),
        "{log}"
    );
    assert!(log.contains("exit=0"), "{log}");
}

#[test]
fn a_stalled_tool_is_ended_only_when_the_action_says_so() {
    let r = rig();
    let (code, _, err) = r.run(
        &["slow"],
        &[
            ("PADDOCK_SHIM_STALL", "1"),
            ("PADDOCK_SHIM_STALL_ACTION", "end"),
            ("STAND_SLEEP", "60"),
        ],
    );
    assert_eq!(code, 81, "{err}");
    assert!(r.run_log().contains("ends=true"), "{}", r.run_log());
}

#[test]
fn a_project_marker_or_the_switch_runs_the_tool_with_no_slot_and_no_record() {
    let r = rig();
    let (flag, out) = (r.w.flag(), r.w.path("out"));
    let hold = heavy_env(&r.w, &hold_args("H", flag.to_str().unwrap(), out.to_str().unwrap()), &[]);
    wait_until("H holds the slot", || r.w.locked());
    // The switch: the tool runs although the only slot is taken.
    let (code, o, err) = r.run(&["a"], &[("PADDOCK_SHIM", "off")]);
    assert_eq!((code, o.contains("held=none")), (0, true), "{err}\n{o}");
    // The marker in the project folder.
    std::fs::write(r.proj.join(".paddock-shim-off"), "").unwrap();
    let (code, _, err) = r.run(&["b"], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(r.run_log(), "");
    r.w.release();
    drop(hold);
}

#[test]
fn the_installer_never_replaces_a_file_it_did_not_write() {
    let w = World::new();
    let dir = w.path("shims");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("npm"), "mine").unwrap();
    let mut c = TestCommand::new(BIN);
    base_env(&mut c, &w);
    let out = c.args(["shims", "install", dir.to_str().unwrap()]).output().expect("run");
    assert_eq!(out.status.code(), Some(78), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(dir.join("npm")).unwrap(), "mine");
    for tool in ["cargo", "npx", "node", "playwright", "ng"] {
        assert!(
            std::fs::symlink_metadata(dir.join(tool)).unwrap().file_type().is_symlink(),
            "{tool}"
        );
    }
    let mut c = TestCommand::new(BIN);
    base_env(&mut c, &w);
    let again = c.args(["shims", "install", dir.to_str().unwrap()]).output().expect("run");
    assert!(
        String::from_utf8_lossy(&again.stdout).contains("cargo: already"),
        "{}",
        String::from_utf8_lossy(&again.stdout)
    );
}
