//! The slot locks as seen through the real binary (user decision, 2026-11-26, point 3 of the promises in the crate's
//! `tests/it/filelock.rs`): a holder that dies frees its slot at once, and the lock is not inherited by a child.

#[cfg(unix)]
mod real {
    use crate::procutil::*;
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
        let mut a = throttle(
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
        let (flag, done) = (w.flag(), w.path("bg.done"));
        // The background loop writes `bg.done` only after the flag appears, so a missing file while Q runs says it still lives.
        let script = concat!(
            "( ",
            crate::wait_flag_loop!("$0", "${0%/*}"),
            r#"; [ -e "$0" ] && : > "$1" ) >/dev/null 2>&1 &"#
        );
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
                done.to_str().unwrap(),
            ],
            &[],
        );
        assert_eq!(code, 0, "{e}");
        let t = Instant::now();
        let (code, _, e) = heavy_out(&w, &["run", "--no-gate", "Q", "--", "true"], &[]);
        assert_eq!(code, 0, "{e}");
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "Q waited {:?} for the background process",
            t.elapsed()
        );
        assert!(!done.exists(), "the background process was still running while Q ran");
        w.release();
        wait_until("the background process ends", || done.exists());
    }

    // (3) the same at the file: a lock taken here is not held by a child that outlives the handle.
    //
    // On macOS (and any fork+exec) a spawn gives its child a copy of the whole descriptor table, close-on-exec ones included, until
    // the child's exec closes them; the lock is on the open file description, so a handle dropped while another thread is
    // mid-spawn looks held for a moment (lessons.md, "A just-released lock can look held"). That is not inheritance: it clears
    // within moments, and inheritance never does. So the test overlaps a spawner thread on purpose (it failed with the other tests
    // as its accidental spawners) and judges the lock on the observation that it becomes free while the child still runs.
    #[test]
    fn a_child_started_while_the_lock_is_held_does_not_inherit_it() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        /// Spawns that must have overlapped the drops before the test may end, on a machine of any speed.
        const MIN_SPAWNS: usize = 50;
        let w = World::new();
        let lock = w.root().join("throttle-slot1.lock");
        let flag = w.flag();
        let stop = Arc::new(AtomicBool::new(false));
        let spawned = Arc::new(AtomicUsize::new(0));
        let spawner = {
            let (stop, spawned) = (stop.clone(), spawned.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let _ = paddock_testkit::TestCommand::new("true").status();
                    spawned.fetch_add(1, Ordering::SeqCst);
                }
            })
        };
        let held = paddock_throttle::slotfile::take(&lock).unwrap().expect("free");
        let mut child = Guard {
            child: paddock_testkit::TestCommand::new("sh")
                .args(["-c", crate::wait_flag_loop!("$0", "${0%/*}"), flag.to_str().unwrap()])
                .spawn()
                .expect("spawn sh"),
            label: "child that outlives the handle".into(),
        };
        paddock_testkit::lock::await_held("the handle holds the lock", || paddock_throttle::slotfile::is_held(&lock).unwrap());
        // Drop and take again while the spawner runs, until it has overlapped enough.
        drop(held);
        while spawned.load(Ordering::SeqCst) < MIN_SPAWNS {
            paddock_testkit::lock::await_free("the lock is free again", || paddock_throttle::slotfile::is_held(&lock).unwrap());
            let again = paddock_throttle::slotfile::take(&lock).unwrap();
            drop(again);
        }
        stop.store(true, Ordering::SeqCst);
        spawner.join().unwrap();
        assert!(child.child.try_wait().unwrap().is_none(), "the child is still running");
        paddock_testkit::lock::await_free("the lock went with the handle, not with the child", || {
            paddock_throttle::slotfile::is_held(&lock).unwrap()
        });
        w.release();
        assert!(child.finish().success());
    }

    // (ER-33) the probe above can see a real inheritance: a child that is given the locked description (as its stdin) keeps the lock
    // after the handle is gone, and frees it when it ends.
    #[test]
    fn a_child_given_the_locked_file_keeps_the_lock_until_it_ends() {
        let w = World::new();
        let lock = w.root().join("throttle-slot2.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock)
            .unwrap();
        file.try_lock().expect("free");
        let flag = w.flag();
        let mut child = Guard {
            child: paddock_testkit::TestCommand::new("sh")
                .args(["-c", r#"while [ ! -e "$0" ]; do sleep 0.1; done"#, flag.to_str().unwrap()])
                .stdin(file.try_clone().unwrap())
                .spawn()
                .expect("spawn sh"),
            label: "child that inherits the lock".into(),
        };
        drop(file);
        assert!(child.child.try_wait().unwrap().is_none(), "the child is still running");
        paddock_testkit::lock::await_held("the child holds the lock", || paddock_throttle::slotfile::is_held(&lock).unwrap());
        w.release();
        assert!(child.finish().success());
        paddock_testkit::lock::await_free("the child's end frees the lock", || {
            paddock_throttle::slotfile::is_held(&lock).unwrap()
        });
    }
}
