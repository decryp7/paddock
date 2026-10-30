//! Background priority for download workers: a large download must not slow the work the user is doing.
//!
//! [`lower_current_thread`] asks the OS for a lowered but never starved level for the calling thread (QoS utility on
//! macOS, nice 10 on Linux, below-normal priority on Windows): the user waits for the download, so Windows' background
//! mode, which also drops the thread's disk and memory priority to the lowest, is not used (Paddock rule 52). It is best
//! effort: where the OS refuses or the platform has no such call, the thread keeps its priority and nothing is reported.

/// Lowers the calling thread's CPU priority.
pub fn lower_current_thread() {
    imp::lower();
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn lower() {
        // SAFETY: a plain call that only changes the calling thread's quality-of-service class.
        unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    pub fn lower() {
        // SAFETY: gettid and setpriority take and return plain integers; on Linux a priority of a thread id is per thread.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        unsafe {
            let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
            libc::setpriority(libc::PRIO_PROCESS, tid, 10);
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};

    pub fn lower() {
        // SAFETY: the pseudo handle of the calling thread is always valid; a refusal is ignored.
        unsafe {
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn lower() {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn lowering_a_thread_neither_panics_nor_blocks() {
        // The effect is the OS's business; what matters is that a thread that lowers itself still runs to completion.
        let done = std::thread::spawn(|| {
            super::lower_current_thread();
            super::lower_current_thread();
            7
        })
        .join();
        assert_eq!(done.ok(), Some(7));
    }

    /// A download the user waits for is lowered, never starved (Paddock rule 52): below normal, not Windows' background
    /// mode, which also drops the thread's disk and memory priority to the lowest.
    #[cfg(windows)]
    #[test]
    fn a_download_thread_is_below_normal_on_windows() {
        use windows::Win32::System::Threading::{GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
        let priority = std::thread::spawn(|| {
            super::lower_current_thread();
            // SAFETY: the pseudo handle of the calling thread is always valid.
            unsafe { GetThreadPriority(GetCurrentThread()) }
        })
        .join()
        .unwrap();
        assert_eq!(priority, THREAD_PRIORITY_BELOW_NORMAL.0);
    }

    /// The same contract on macOS: QoS utility, never background.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_download_thread_is_utility_on_macos() {
        let class = std::thread::spawn(|| {
            super::lower_current_thread();
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // SAFETY: reads the calling thread's own QoS class into two locals.
            assert_eq!(
                unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) },
                0
            );
            class
        })
        .join()
        .unwrap();
        assert_eq!(class as u32, libc::qos_class_t::QOS_CLASS_UTILITY as u32);
    }
}
