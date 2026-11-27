//! Ends a run's work on Ctrl+C or SIGTERM: the signal sets a flag that the run's loops read (`./dev check --pre-push`, `./dev ci run`),
//! so the running steps end, their tables print and the process leaves in order; a second signal leaves at once. Unix: `signal-hook`;
//! Windows: `ctrlc` (as `tools/throttle` does).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[cfg(windows)]
use std::sync::atomic::Ordering;

/// The exit code of a second Ctrl+C or SIGTERM: 128 + SIGINT.
#[cfg(unix)]
const SECOND_SIGNAL_EXIT: i32 = 130;

/// Ends the run on SIGINT and SIGTERM: the first sets `stop`, the second leaves with 130.
#[cfg(unix)]
pub fn stop_on_signals(stop: &Arc<AtomicBool>) {
    use signal_hook::consts::{SIGINT, SIGTERM};
    for sig in [SIGINT, SIGTERM] {
        // Registered first, so it sees the flag the second signal finds set: the first signal only sets the flag (the gates end,
        // the table prints), the second one leaves at once.
        let _ = signal_hook::flag::register_conditional_shutdown(sig, SECOND_SIGNAL_EXIT, Arc::clone(stop));
        let _ = signal_hook::flag::register(sig, Arc::clone(stop));
    }
}

/// Ends the run on Ctrl+C. Windows: Ctrl+C reaches the whole console group; the handler sets the same flag so the table prints and the gates end.
#[cfg(windows)]
pub fn stop_on_signals(stop: &Arc<AtomicBool>) {
    let flag = Arc::clone(stop);
    let _ = ctrlc::set_handler(move || {
        if flag.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    });
}
