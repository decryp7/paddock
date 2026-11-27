//! Waits on the run state of a process, for tests (feature `test-support`, rule 99; re-exported by `paddock_testkit_start`).
//!
//! A process's state is read from the OS at one instant, and what a test knows about when it changes is only the order of the events
//! it caused: a state read right after `kill`, `wait` or a signal races whatever the OS and the other process do next (CI runs 6603,
//! 7448, 9168 and 9201). So a test never reads [`run_state`](crate::run_state) directly (a Clippy ban in `crates/clippy.toml`):
//!
//! Windows does not read a live process as [`RunState::Running`] (its state is [`RunState::Unknown`] until `IsFrozen` is read, ADR 0031
//! section 4; run 9225 waited 40 s for `Running`): a test that must work there waits for `Exited`, or for a state that is not
//! `Exited` (`|s| s != RunState::Exited`), and keeps `Running` and `Stopped` for Unix tests.
//!
//! - [`wait_state`] / [`wait_exited`]: the state the test expects arrives, polled on the condition and bounded (no sleep decides
//!   anything, ER-12);
//! - [`state_after`] / [`still_running_after`]: a state that must hold ("it stays running") is read once, after the event that would
//!   have changed it if the code were wrong (the closure waits for it: the guardian acted or logged, the owner was reaped, the kill
//!   returned). The event is named by the closure, so the reader sees what the read follows.

use crate::{ProcessId, RunState};
use std::time::{Duration, Instant};

/// The first poll interval and the longest one (the pace of `paddock_testkit::wait`, which this crate cannot depend on: its tests
/// use that crate, and a dependency here would add the kit to every lockfile that holds `paddock-proc`).
const FIRST: Duration = Duration::from_micros(200);
const LAST: Duration = Duration::from_millis(20);

/// The one read of the OS state in test code, so every other test goes through a wait.
fn read(id: ProcessId) -> RunState {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one read the waits are built on (statewait)"
    )]
    crate::run_state(id)
}

/// Polls until `pred` holds for the state of `id`, for at most `max`; whether it did (it does not mean the state stays). The state is
/// identity-checked like [`run_state`](crate::run_state): a pid that was handed on is [`RunState::Exited`].
#[must_use]
pub fn wait_state(id: ProcessId, max: Duration, pred: impl Fn(RunState) -> bool) -> bool {
    poll(max, || pred(read(id)))
}

/// Polls `done` until it holds or `max` has passed; whether it held.
fn poll(max: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + max;
    let mut pause = FIRST;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        // conformance: poll-pace — a wait on the condition; the interval only paces the polls, no pause decides the verdict
        std::thread::sleep(pause);
        pause = (pause * 2).min(LAST);
    }
}

/// Whether the process has ended (gone, a zombie, or its pid handed on). On Windows that is its handle being signaled, never "no longer
/// in the process table": an ended process stays listed, and reads [`RunState::Unknown`], while any handle to it is open (a
/// monitor's, the runner's, the test's own `Child`), so a table read there can hold off for ever (trial run 9253).
fn ended(id: ProcessId) -> bool {
    #[cfg(windows)]
    {
        crate::sys::has_ended_by_handle(id)
    }
    #[cfg(not(windows))]
    {
        read(id) == RunState::Exited
    }
}

/// Polls until the process has ended (see [`ended`]), for at most `max`; whether it had. The state is identity-checked.
#[must_use]
pub fn wait_exited(id: ProcessId, max: Duration) -> bool {
    poll(max, || ended(id))
}

/// The state of `id` read once, after `event` returned: `event` blocks until the thing that would change the state, if the code were
/// wrong, has happened (name it in the closure).
pub fn state_after(id: ProcessId, event: impl FnOnce()) -> RunState {
    event();
    read(id)
}

/// Whether `id` is still running once `event` has happened (see [`state_after`]).
#[must_use]
pub fn still_running_after(id: ProcessId, event: impl FnOnce()) -> bool {
    state_after(id, event) == RunState::Running
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testchild::{Mode, child};
    use paddock_testkit::HANG_GUARD;

    /// A second handle to the child, held open on purpose (a monitor's, the runner's): it keeps the ended process listed.
    #[cfg(windows)]
    struct OtherHolder(windows_sys::Win32::Foundation::HANDLE);

    #[cfg(windows)]
    impl OtherHolder {
        fn of(id: ProcessId) -> OtherHolder {
            use windows_sys::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            };
            // SAFETY: OpenProcess takes plain values; the handle is closed in `drop`.
            let h = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    id.pid(),
                )
            };
            assert!(!h.is_null(), "the child is there to be opened");
            OtherHolder(h)
        }
    }

    #[cfg(windows)]
    impl Drop for OtherHolder {
        fn drop(&mut self) {
            // SAFETY: the handle was opened in `of` and is closed once.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }

    /// Windows lists an ended process while any handle to it is open and reads it as `Unknown`: the wait for the end must not depend
    /// on the process table. The positive control is the table read: it still says "not exited" while the handle is held.
    #[cfg(windows)]
    #[test]
    fn an_ended_child_is_ended_while_another_handle_to_it_is_open() {
        let mut c = crate::Supervised::spawn(child(Mode::Sleep)).unwrap();
        let id = ProcessId::of(c.id().unwrap()).unwrap();
        let other = OtherHolder::of(id);
        crate::kill_tree(id);
        let _ = c.wait();
        // `c` is not dropped either: two handles are open.
        assert!(wait_exited(id, HANG_GUARD), "the handle is signaled");
        assert_ne!(
            state_after(id, || {}),
            RunState::Exited,
            "the table read cannot see the end (the probe sees the thing)"
        );
        drop(other);
    }

    #[test]
    fn a_running_process_is_waited_for_until_it_ends_and_the_ending_is_seen() {
        let mut c = crate::Supervised::spawn(child(Mode::Sleep)).unwrap();
        let id = ProcessId::of(c.id().unwrap()).unwrap();
        // Alive, not `Running`: a live process reads `Unknown` on Windows (run 9225).
        assert!(wait_state(id, HANG_GUARD, |s| s != RunState::Exited));
        assert_ne!(state_after(id, || {}), RunState::Exited);
        // `Running` is a Unix reading (see the module text).
        #[cfg(unix)]
        assert!(still_running_after(id, || {}));
        // A bound that passes with the state not reached is a `false`, not a hang or a panic.
        assert!(!wait_exited(id, Duration::from_millis(20)));
        crate::kill_tree(id);
        let _ = c.wait();
        // Windows keeps an ended process readable while any handle to it is open, and its state is then `Unknown`, never `Exited`
        // (trial run 9253 waited 40 s here): the handle this test holds closes before it waits for the ending.
        drop(c);
        assert!(wait_exited(id, HANG_GUARD));
        assert_eq!(state_after(id, || {}), RunState::Exited);
    }
}
