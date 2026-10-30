//! Lower priority for the calling thread (ADR 0024 rule 1): long reads such as the disk scan run below the person's own
//! work, so they never compete with it, but are never starved either: the person started the scan and watches it.
#![allow(unsafe_code)]

/// Lowers the calling thread gently (rule 52): macOS `QOS_CLASS_UTILITY`, Linux nice 10, Windows
/// `THREAD_PRIORITY_BELOW_NORMAL`, as Debrief's `debrief_core::threads::lower_priority`. Returns whether the OS accepted it.
/// It stays in effect until the thread ends, so call it only on threads made for this work.
///
/// Never an OS's starvable background level (macOS QoS `BACKGROUND`, Windows background mode): those run a thread only
/// when the machine has nothing else to do, and the Windows mode also drops its I/O priority to the lowest, so on a busy
/// machine work somebody waits for stalls (Debrief's report took 100 s instead of 3 s on a loaded Mac).
pub fn lower_current_thread() -> bool {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a plain call that sets the QoS class of the calling thread.
        unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0) == 0 }
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
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
        // SAFETY: the pseudo handle of the calling thread is always valid.
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) }.is_ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        false
    }
}

thread_local! {
    static LOWERED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// [`lower_current_thread`] once per thread: for work handed to a pool this crate does not start (a closure that runs on
/// another library's worker threads), which calls it for every item. New threads do not inherit the class of the thread
/// that starts them (macOS gives them the default class), so each worker lowers itself. Returns what the first call
/// returned.
pub fn lower_current_thread_once() -> bool {
    LOWERED.with(|l| match l.get() {
        Some(done) => done,
        None => {
            let done = lower_current_thread();
            l.set(Some(done));
            done
        }
    })
}

/// The calling thread's QoS class (`qos_class_t` as a number), for tests that check where work runs.
#[cfg(target_os = "macos")]
pub fn current_qos_class() -> u32 {
    let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut relative = 0;
    // SAFETY: reads the calling thread's own QoS class into two locals; on failure they keep UNSPECIFIED.
    unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) };
    class as u32
}

/// `QOS_CLASS_UTILITY` as a number, to compare with [`current_qos_class`].
#[cfg(target_os = "macos")]
pub const UTILITY: u32 = libc::qos_class_t::QOS_CLASS_UTILITY as u32;

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn a_pool_worker_lowers_itself_once() {
        let (first, again, class) = std::thread::spawn(|| {
            let first = super::lower_current_thread_once();
            let again = super::lower_current_thread_once();
            (first, again, super::current_qos_class())
        })
        .join()
        .unwrap();
        assert!(first && again);
        assert_eq!(class, super::UTILITY);
    }

    #[test]
    fn a_thread_can_be_lowered() {
        // On its own thread: the class stays until the thread ends.
        assert!(std::thread::spawn(super::lower_current_thread).join().unwrap());
    }

    /// The person waits for the disk scan (it is started from the Disk map and shows its progress), so a busy machine may
    /// slow it but must never starve it. macOS's `BACKGROUND` class keeps a thread on the efficiency cores and behind every
    /// other thread (Debrief's report waited 100 s instead of 3 s on a loaded Mac for it); `UTILITY` is family ADR 0020's
    /// class for work the user started and watches.
    #[cfg(target_os = "macos")]
    #[test]
    fn lowered_work_yields_to_the_user_but_is_never_starved_on_macos() {
        let class = std::thread::spawn(|| {
            assert!(super::lower_current_thread());
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // SAFETY: reads the calling thread's own QoS class into two locals.
            let rc = unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) };
            assert_eq!(rc, 0);
            class
        })
        .join()
        .unwrap();
        assert_eq!(
            class as u32,
            libc::qos_class_t::QOS_CLASS_UTILITY as u32,
            "lowered threads must not be in the starvable BACKGROUND class"
        );
    }
}
