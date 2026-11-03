//! What each OS gives the dialog layer beyond Tauri's events (ADR 0043 §3): whether it says "the user is resizing", so
//! a resize is told from the echo of our own `set_size` by the OS's signal and not by comparing sizes. An OS with no such
//! signal (X11, Wayland) infers it: an unexplained `Resized` with nothing queued is the user's.
//!
//! The per-OS observers (`WM_SIZING`, live resize) call [`crate::tauri::user_resize_started`].

/// Whether `os` (`std::env::consts::OS`) reports a user resize itself. False until its observer is installed.
pub const fn has_resize_signal(os: &str) -> bool {
    let _ = os;
    false
}
