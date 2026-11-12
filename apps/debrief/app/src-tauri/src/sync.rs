//! The shell's small concurrency helpers, in one place (they were copied into a dozen modules):
//! poison-recovering lock guards ([`lock`], [`read`], [`write`]) and [`blocking`], which runs work off the
//! async runtime and turns a crash of it into an error instead of taking the command down.
//!
//! A lock is never `unwrap()`ed: a panic while it was held leaves the data as it was, and the shell goes on
//! with it (AGENTS.md, Robustness).

use crate::error::{CmdError, CmdResult};
use crate::ErrorCode;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Locks `m`, recovering from poisoning.
pub fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Read-locks `l`, recovering from poisoning.
pub fn read<T: ?Sized>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|p| p.into_inner())
}

/// Write-locks `l`, recovering from poisoning.
pub fn write<T: ?Sized>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|p| p.into_inner())
}

/// Runs `f` on the blocking pool; a panic in it becomes an `internal` error naming `what`.
pub async fn blocking<T: Send + 'static, E: From<CmdError> + Send + 'static>(
    what: &str,
    f: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, E> {
    blocking_on(debrief_core::wait_log::global(), what, f).await
}

/// [`blocking`] with the set of watches that logs its phases (tests pass their own, with a short period).
///
/// Two phases, one correlation id: queued on the blocking pool (a wait), then running (an activity, long when healthy)
/// (ER-27). A request that never finishes shows in the log as the phase that stayed open, with the other open waits
/// beside it. The labels are formatted per request, which is nothing next to the pool hand-off; the hot path of the
/// store (`conn()`) uses static labels.
pub(crate) async fn blocking_on<T: Send + 'static, E: From<CmdError> + Send + 'static>(
    waits: &'static debrief_core::wait_log::Waits,
    what: &str,
    f: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> Result<T, E> {
    let queued = waits.watch(format!("{what}: queued on the blocking pool"));
    let id = queued.id();
    let what_run = what.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        drop(queued);
        let _running = waits.activity(format!("{what_run}: running (request #{id})"));
        f()
    })
    .await
    .map_err(|e| E::from(CmdError::new(ErrorCode::Internal, format!("{what} stopped unexpectedly ({e})"))))?
}

/// [`blocking`] for work that fails with `anyhow` (its whole chain becomes the message).
pub async fn blocking_anyhow<T: Send + 'static>(what: &str, f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> CmdResult<T> {
    blocking(what, move || f().map_err(|e| CmdError::failed(format!("{e:#}")))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_recover_from_poisoning_and_crashes_become_errors() {
        let m = std::sync::Arc::new(Mutex::new(1));
        let l = std::sync::Arc::new(RwLock::new(2));
        let (m2, l2) = (m.clone(), l.clone());
        let _ = std::thread::spawn(move || {
            let _a = m2.lock().unwrap();
            let _b = l2.write().unwrap();
            panic!("poison both");
        })
        .join();
        *lock(&m) += 1;
        *write(&l) += 1;
        assert_eq!((*lock(&m), *read(&l)), (2, 3));
        let ok: CmdResult<u8> = crate::test_support::block(blocking("work", || Ok(7)));
        assert_eq!(ok.unwrap(), 7);
        let crashed: CmdResult<u8> = crate::test_support::block(blocking("the import", || panic!("boom")));
        let e = crashed.unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal);
        assert!(e.contains("the import stopped unexpectedly"), "{e}");
        let failed = crate::test_support::block(blocking_anyhow("x", || -> anyhow::Result<u8> {
            Err(anyhow::anyhow!("inner").context("outer"))
        }));
        assert!(failed.unwrap_err().contains("outer: inner"));
    }

    /// A request that is long on the blocking pool leaves its phase in the log: `running` is an activity, so the watchdog
    /// reports it as "still open" at info with the request id, never at warn (the log has a daily byte budget), and its
    /// end is at debug (ER-27).
    #[test]
    fn a_long_request_logs_its_phase_at_info_and_ends_at_debug() {
        use debrief_core::wait_log::Waits;
        let (tx, lines) = std::sync::mpsc::channel::<(log::Level, String)>();
        let tx = Mutex::new(tx);
        // The period is the set's own, not a process-wide setting: nothing here depends on one test per process.
        let waits: &'static Waits = Box::leak(Box::new(Waits::new_tracing(
            std::time::Duration::from_millis(20),
            std::sync::Arc::new(move |l, s| {
                let _ = lock(&tx).send((l, s));
            }),
        )));
        let (held_tx, held_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let request = std::thread::spawn(move || {
            crate::test_support::block(blocking_on::<u8, CmdError>(
                waits,
                "reading the parsers of the sources",
                move || {
                    held_tx.send(()).unwrap();
                    // Held until the test has seen the line: the condition, bounded.
                    release_rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
                    Ok(1)
                },
            ))
        });
        held_rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        let next = |needle: &str| loop {
            let (level, text) = lines.recv_timeout(std::time::Duration::from_secs(30)).expect("the line never came");
            if text.contains(needle) {
                break (level, text);
            }
        };
        let (level, text) = next("still open");
        assert_eq!(level, log::Level::Info, "{text}");
        assert!(text.contains("reading the parsers of the sources: running (request #"), "{text}");
        release_tx.send(()).unwrap();
        assert_eq!(request.join().unwrap().unwrap(), 1);
        let (level, text) = next("running (request #");
        assert_eq!(level, log::Level::Debug, "{text}");
        assert!(text.contains("ended after"), "{text}");
    }
}
