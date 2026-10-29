//! Background priority for the calling thread (ADR 0024 rule 1): long reads such as the disk scan run at the OS's
//! background class, so they never compete with what the person is doing.
#![allow(unsafe_code)]

/// Puts the calling thread into the OS's background class (macOS `QOS_CLASS_BACKGROUND`, Linux nice 10, Windows
/// `THREAD_MODE_BACKGROUND_BEGIN`, which also lowers its I/O priority). Returns whether the OS accepted it. It stays
/// in effect until the thread ends, so call it only on threads made for background work.
pub fn background_current_thread() -> bool {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a plain call that sets the QoS class of the calling thread.
        unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0) == 0 }
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `gettid` takes no arguments; `setpriority` with a thread id lowers only that thread (Linux threads
        // have their own nice value).
        unsafe {
            let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
            libc::setpriority(libc::PRIO_PROCESS, tid, 10) == 0
        }
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
        // SAFETY: the pseudo handle of the calling thread is always valid.
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) }.is_ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_thread_can_enter_the_background_class() {
        // On its own thread: the class stays until the thread ends.
        assert!(std::thread::spawn(super::background_current_thread).join().unwrap());
    }
}
