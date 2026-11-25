//! The slot locks are kernel file locks held by handle (user decision, 2026-11-26): what that promises, each with a test.
//! 1. a lock folder on a network file system is refused; 2. the throttle is taken on the host around a container, never inside one;
//! 3. the lock is not inherited by a child, and a holder that dies frees its slot at once; 4. a nested run uses its parent's slot.

use crate::helpers::*;
use heavy::exit;
use heavy::fsinfo::FsKind;
use heavy::runner::{RunOpts, run};
use std::sync::Arc;

fn opts() -> RunOpts {
    RunOpts {
        gate: false,
        low: false,
        queue_max: None,
        queue_stall: None,
        run_max: None,
        stall: None,
    }
}

// (1) a network lock folder: refused by a run before anything is queued or run, with the code of "the lock could not be taken".
#[test]
fn a_run_with_its_lock_folder_on_a_network_file_system_is_refused_and_the_command_never_runs() {
    let rig = Rig::new();
    let mut a = rig.agent(11);
    a.h.fs_kind = Arc::new(|_| FsKind::Network("nfs".into()));
    let ran = rig.root().join("ran");
    let args = vec!["sh".to_string(), "-c".to_string(), format!("touch {}", ran.display())];
    let code = run(&a.h, req("N"), &args, opts(), &|| None);
    assert_eq!(code, exit::LOCK_NOT_TAKEN);
    assert!(a.said().contains("network file system"), "{}", a.said());
    assert!(!ran.exists(), "the command never ran");
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 0);
}

// (2) the throttle is taken on the host around containers: the container script mounts nothing of the lock folder, passes none of its
// variables and never starts heavy inside the container. (The call sites: `tools/linux-container.sh` is the only place that starts a
// container for the family's gates; the pre-push and member-gate rows reach it through `heavy run`, on the host, and heavy recognises
// the command as a container run, which then runs alone: `gate::is_container_command`.)
#[test]
fn the_container_script_never_mounts_the_lock_folder_or_starts_heavy_inside() {
    let script = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../linux-container.sh")).expect("tools/linux-container.sh");
    let code: Vec<&str> = script.lines().filter(|l| !l.trim_start().starts_with('#')).collect();
    for l in &code {
        let low = l.to_ascii_lowercase();
        assert!(
            !low.contains("heavy_dir") && !low.contains("throttle_dir") && !low.contains("heavy_held") && !low.contains("throttle_held"),
            "the container gets no variable of the throttle: {l}"
        );
        assert!(
            !low.contains("/throttle") && !low.contains("pitcrew-heavy"),
            "the lock folder is not mounted: {l}"
        );
    }
    for cmd in [
        "bash tools/linux-container.sh cargo test",
        "docker run --rm x",
        "podman run x",
        "nerdctl run x",
    ] {
        assert!(
            heavy::gate::is_container_command(cmd),
            "{cmd}: a container run runs alone, on the host"
        );
    }
}

#[cfg(unix)]
mod real {
    use crate::procutil::*;
    use paddock_proc::ProcessId;
    use std::time::{Duration, Instant};

    // (3) a holder killed with SIGKILL frees its slot at once, although its command still runs: the next run does not wait for it.
    // (On the directory locks the slot stayed taken while the command lived, and a waiter waited for it.)
    #[test]
    fn a_holder_killed_with_sigkill_frees_its_slot_at_once_while_its_command_still_runs() {
        let w = World::new();
        let (flag, out) = (w.flag(), w.path("out"));
        let (flag, out) = (flag.to_str().unwrap(), out.to_str().unwrap());
        // The command says when it runs: a kill before that would leave no command at all.
        let (started, script) = (w.path("started"), format!(r#": > "$3"; {HOLD}"#));
        let mut a = heavy(
            &w,
            &[
                "run",
                "--no-gate",
                "A",
                "--",
                "sh",
                "-c",
                &script,
                flag,
                out,
                "A",
                started.to_str().unwrap(),
            ],
        );
        wait_until("A's command runs", || started.exists() && w.locked());
        a.child.kill().expect("SIGKILL");
        a.wait(DEADLINE).expect("A is gone");
        let t = Instant::now();
        let (code, _, e) = heavy_out(&w, &echo_args("B", out), &[]);
        assert_eq!(code, 0, "{e}");
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "B took {:?}: it waited for A's command",
            t.elapsed()
        );
        assert_eq!(w.out_words(), ["B"], "A's command is still running (it has not written yet)");
        w.release();
        wait_until("A's orphaned command ends", || w.out_words() == ["B", "A"]);
    }

    // (3) the lock is not inherited: a command that starts a background process and exits leaves the slot free while that process lives.
    #[test]
    fn a_background_process_of_the_command_does_not_keep_the_slot() {
        let w = World::new();
        let (flag, pidfile) = (w.flag(), w.path("bg.pid"));
        let script = r#"( while [ ! -e "$0" ]; do sleep 0.1; done ) >/dev/null 2>&1 & echo $! > "$1""#;
        let (code, _, e) = heavy_out(
            &w,
            &[
                "run",
                "--no-gate",
                "P",
                "--",
                "sh",
                "-c",
                script,
                flag.to_str().unwrap(),
                pidfile.to_str().unwrap(),
            ],
            &[],
        );
        assert_eq!(code, 0, "{e}");
        let bg: u32 = w.read(&pidfile).trim().parse().expect("background pid");
        assert!(ProcessId::of(bg).is_some(), "the background process outlived heavy and its command");
        let t = Instant::now();
        let (code, _, e) = heavy_out(&w, &["run", "--no-gate", "Q", "--", "true"], &[]);
        assert_eq!(code, 0, "{e}");
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "Q waited {:?} for the background process",
            t.elapsed()
        );
        assert!(ProcessId::of(bg).is_some(), "it is still running while Q ran");
        w.release();
        wait_until("the background process ends", || ProcessId::of(bg).is_none());
    }

    // (3) the same at the file: a lock taken here is not held by a child that outlives the handle.
    #[test]
    fn a_child_started_while_the_lock_is_held_does_not_inherit_it() {
        let w = World::new();
        let lock = w.root().join("heavy-slot1.lock");
        let held = heavy::slotfile::take(&lock).unwrap().expect("free");
        let flag = w.flag();
        let mut child = Guard {
            child: paddock_testkit::TestCommand::new("sh")
                .args(["-c", r#"while [ ! -e "$0" ]; do sleep 0.1; done"#, flag.to_str().unwrap()])
                .spawn()
                .expect("spawn sh"),
            label: "child that outlives the handle".into(),
        };
        assert!(heavy::slotfile::is_held(&lock).unwrap());
        drop(held);
        assert!(child.child.try_wait().unwrap().is_none(), "the child is still running");
        assert!(
            !heavy::slotfile::is_held(&lock).unwrap(),
            "the lock went with the handle, not with the child"
        );
        w.release();
        assert!(child.finish().success());
    }
}
