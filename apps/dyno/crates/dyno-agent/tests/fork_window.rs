#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A forked child that has not exec'd yet holds a copy of the agent's lock. This test forks on purpose, so it lives in a binary
//! of its own: in `agent.rs` its child copied the descriptors of every agent the other tests had open at that moment (refusing
//! their restarts as "already running") and, on macOS, a fork while other threads sit in socket calls is the trigger of the
//! spurious EBADF on healthy sockets (docs/lessons.md). One test per binary means no neighbour to harm.

use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_platform::fake::{process, Fake};
use std::time::Duration;

fn start_on(dirs: &Dirs) -> Result<Running, String> {
    let fake = Fake::new();
    fake.add_process(process(100, "busy", 60.0));
    Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).map_err(|e| e.to_string())
}

/// Starts an agent on `dirs` once the lock is free: a refusal for "already running" is waited out.
fn start_when_the_folder_is_free(dirs: &Dirs) -> Running {
    let end = std::time::Instant::now() + Duration::from_secs(40); // hang guard of the harness only
    loop {
        match start_on(dirs) {
            Ok(running) => return running,
            Err(e) if e.contains("already running") && std::time::Instant::now() < end => std::thread::yield_now(),
            Err(e) => panic!("the folder never became free: {e}"),
        }
    }
}

/// A child between its fork and its exec holds a copy of every descriptor open at the fork, the lock's included (it is closed
/// at the exec, `O_CLOEXEC`). The agent starts programs on background threads (the hardware readers), so the OS can keep the data
/// folder locked for a while after the agent that held it is gone; a loaded macOS runner showed it (the third start of
/// `a_second_agent…` was refused). This holds a child in that state on purpose, without a clock: it blocks on a pipe the test
/// closes.
#[cfg(unix)]
#[test]
#[expect(
    unsafe_code,
    reason = "pipe(2), read(2) and write(2) in the child between fork and exec, async-signal-safe calls only"
)]
#[expect(clippy::disallowed_methods, reason = "the test starts a trivial program to hold a forked child")]
fn a_child_forked_before_it_execs_keeps_the_folder_locked_until_it_does_and_the_next_start_waits() {
    use std::os::unix::process::CommandExt;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let first = start_on(&dirs).unwrap();

    let mut ready = [0i32; 2];
    let mut release = [0i32; 2];
    // SAFETY: two fresh pipes; the descriptors are closed below.
    unsafe {
        assert_eq!(libc::pipe(ready.as_mut_ptr()), 0);
        assert_eq!(libc::pipe(release.as_mut_ptr()), 0);
    }
    let (ready_w, release_r, release_w) = (ready[1], release[0], release[1]);
    let forker = std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("true");
        // SAFETY: only `write` and `read` run in the child before the exec.
        unsafe {
            cmd.pre_exec(move || {
                let byte = 1u8;
                libc::write(ready_w, std::ptr::addr_of!(byte).cast(), 1);
                // (its own copy of the write end would keep the read from ever ending)
                libc::close(release_w);
                let mut b = 0u8;
                libc::read(release_r, std::ptr::addr_of_mut!(b).cast(), 1);
                Ok(())
            });
        }
        cmd.status().unwrap();
    });
    // The child has forked (it says so) and now waits, holding the descriptors it was born with.
    let mut b = 0u8;
    // SAFETY: reads one byte from our own pipe.
    assert_eq!(unsafe { libc::read(ready[0], std::ptr::addr_of_mut!(b).cast(), 1) }, 1);

    first.stop();
    assert!(
        dyno_agent::instance::is_held(&dirs),
        "the stopped agent's lock should still be held by the forked child's copy of the descriptor"
    );
    match start_on(&dirs) {
        Ok(r) => {
            r.stop();
            panic!("an agent started while a forked child still carried the lock");
        }
        Err(refused) => assert!(refused.contains("already running"), "{refused}"),
    }

    // The child execs (the read ends), its copy closes, the lock is free: the waiting start succeeds.
    // SAFETY: closes our write end of the release pipe, which unblocks the child's read.
    unsafe {
        libc::close(release[1]);
    }
    forker.join().unwrap();
    start_when_the_folder_is_free(&dirs).stop();
    // SAFETY: closes the remaining ends.
    unsafe {
        libc::close(ready[0]);
        libc::close(ready[1]);
        libc::close(release[0]);
    }
}
