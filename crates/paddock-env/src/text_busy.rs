//! Starting a program that was written a moment ago: the `ETXTBSY` ("text file busy") retry.
//!
//! Linux (and the BSDs) refuse to execute a file that is open for writing, and to open for writing a file that is being
//! executed. Two things make that refusal outlive the program that caused it:
//!
//! - **A leaked write descriptor.** A thread that opens a file, writes a program into it and closes it can still lose to a
//!   concurrent `fork` in another thread: the child holds a copy of the write descriptor (even with `O_CLOEXEC`, which only acts
//!   at the child's `exec`) until it execs, so an `exec` of the new file in between fails with `ETXTBSY`. The kernel gives no
//!   way to wait for the child (Go issue 22315, Linux `__fput` thread of August 2025; Austin Group bug 1318 asks for
//!   `O_CLOFORK`).
//! - **A program that just ended.** The kernel lets go of the executed file when the dead process's address space is torn
//!   down, which can happen a moment after `wait` returned, so reopening the file for writing right away can fail too (CI run
//!   8174, `a_program_changed_after_install...`: no process holds the file in `/proc` when it fails).
//!
//! Neither has a condition to wait on, so established projects retry that one error for a bounded time: Go's `os/exec` and
//! `cmd/go` tests and its script runner (golang/go 22220, 56811, 62221: three attempts, 100 + 200 + 400 ms),
//! and the Go tool when it starts a binary it just built. [`retry`] is that, for the Paddock programs: it retries only
//! `ETXTBSY` (any other error returns at once), at most [`MAX_RETRIES`] times with a doubling pause (about 1.3 s in all),
//! counts and logs each retry, and returns the last error when the bound is reached. It never hides a product defect: a file
//! that stays busy (a program that really runs) fails after the bound with the same error.
//!
//! Measured (ER-12; Linux container, 4 CPU spinners, 2026-11-16): the write of a program right after its process was killed and
//! reaped needed one retry in 300 of 300 runs, and that one is the retry the test forces by writing while the program still
//! runs; none needed a second. The bound of 8 (about 1.3 s) is the Go tool's 3 tries (0.7 s) with room, not a measured need.
//!
//! The pause is real time on purpose. There is nothing to wait on, and a fake clock would turn the bound into an endless
//! loop (ER-12: the measured cause is recorded above).

use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// How many times [`retry`] tries again after the first failure.
pub const MAX_RETRIES: u32 = 8;
/// The pause before the first retry; it doubles each time (5, 10, 20, 40, 80, 160, 320, 640 ms: about 1.3 s in all).
pub const FIRST_PAUSE: Duration = Duration::from_millis(5);

static RETRIES: AtomicU64 = AtomicU64::new(0);
static BY_LABEL: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

/// Whether `error` is `ETXTBSY`: the file is being executed (or held open for writing by a child that has not exec'd yet).
pub fn is_text_busy(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::ExecutableFileBusy
}

/// How many retries this process has made so far, all operations together (a test that waits for its own retry uses
/// [`retries_for`]).
pub fn retries_so_far() -> u64 {
    RETRIES.load(Ordering::SeqCst)
}

/// How many retries were made for an operation named exactly `what` (the label [`retry`] logs): a test waits on its own label,
/// so another test's retry cannot stand in for it.
pub fn retries_for(what: &str) -> u64 {
    let by = BY_LABEL.lock().unwrap_or_else(|p| p.into_inner());
    by.iter().find(|(w, _)| w == what).map_or(0, |(_, n)| *n)
}

/// Runs `op`, again after a pause while it fails with `ETXTBSY` (see the module text), at most [`MAX_RETRIES`] times.
/// `what` names the operation in the log line of each retry.
///
/// # Errors
/// The first error that is not `ETXTBSY`, or the last `ETXTBSY` when the bound is reached.
pub fn retry<T>(what: &str, mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut pause = FIRST_PAUSE;
    for attempt in 1..=MAX_RETRIES {
        match op() {
            Err(e) if is_text_busy(&e) => {
                RETRIES.fetch_add(1, Ordering::SeqCst);
                {
                    let mut by = BY_LABEL.lock().unwrap_or_else(|p| p.into_inner());
                    match by.iter_mut().find(|(w, _)| w == what) {
                        Some((_, n)) => *n += 1,
                        None => by.push((what.to_string(), 1)),
                    }
                }
                log::warn!(
                    "{what}: text file busy ({e}); a program was written or ended a moment ago, trying again in {pause:?} (retry {attempt} of {MAX_RETRIES})"
                );
                std::thread::sleep(pause);
                pause = pause.saturating_mul(2);
            }
            other => return other,
        }
    }
    op()
}
