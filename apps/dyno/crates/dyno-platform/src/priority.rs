//! Lower priority for the calling thread (ADR 0024 rule 1): long reads such as the disk scan run below the person's own
//! work, so they never compete with it, but are never starved either: the person started the scan and watches it.
#![allow(unsafe_code)]

/// What a thread does, which decides its priority (rule 52).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadRole {
    /// Work someone is watching or waiting for now: the sampler behind the live view, a request from the app.
    Live,
    /// Bulk background work: walks, scans, compaction, exports, slow background reads.
    Bulk,
}

/// The operating system a level is chosen for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    MacOs,
    /// Linux.
    Linux,
    /// Windows.
    Windows,
    /// Anything else (nothing is changed).
    Other,
}

impl Os {
    /// The system this was built for.
    pub const CURRENT: Os = if cfg!(target_os = "macos") {
        Os::MacOs
    } else if cfg!(target_os = "linux") {
        Os::Linux
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Other
    };
}

/// A thread priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Left as the thread started (the normal level).
    Normal,
    /// macOS `QOS_CLASS_UTILITY`.
    Utility,
    /// Linux nice 10 for the thread.
    Nice10,
    /// Windows `THREAD_PRIORITY_BELOW_NORMAL`.
    BelowNormal,
}

/// The level for `role` on `os` (rule 52). Bulk work runs at the gentle level of each OS, never a starvable one. Live work
/// also runs gently where the scheduler shares the CPU by weight (macOS QoS, Linux nice), but stays at the normal level on
/// Windows: there a ready thread of a higher priority always runs before a lower one, and a lowered thread waiting behind
/// busy normal threads only gets a starvation boost after seconds, so the live view and the requests the user waits for
/// would stall exactly when the machine is busy (Windows CI runs 7166 and 7168).
/// <https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities>
pub const fn level(role: ThreadRole, os: Os) -> Level {
    match (os, role) {
        (Os::MacOs, _) => Level::Utility,
        (Os::Linux, _) => Level::Nice10,
        (Os::Windows, ThreadRole::Live) | (Os::Other, _) => Level::Normal,
        (Os::Windows, ThreadRole::Bulk) => Level::BelowNormal,
    }
}

/// The nice value of [`Level::Nice10`].
#[cfg(target_os = "linux")]
const NICE_BULK: i32 = 10;

/// Whether moving a thread from nice value `current` to `target` lowers its priority (a higher nice value is lower). An
/// unprivileged user may only do that; asking for a raise fails with EPERM.
#[cfg(target_os = "linux")]
const fn would_lower(current: i32, target: i32) -> bool {
    current < target
}

/// The nice value from a `getpriority` return value and errno: -1 is both a valid value and the error marker, so it counts
/// only when errno is 0.
#[cfg(target_os = "linux")]
const fn nice_result(raw: i32, errno: i32) -> Option<i32> {
    if raw == -1 && errno != 0 {
        None
    } else {
        Some(raw)
    }
}

/// The calling thread's nice value, `None` when the system does not say.
#[cfg(target_os = "linux")]
fn current_nice() -> Option<i32> {
    // SAFETY: `gettid` takes no arguments; `getpriority` with a thread id reads only that thread's nice value. -1 is a
    // valid value, so errno is cleared first and checked after.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        *libc::__errno_location() = 0;
        let nice = libc::getpriority(libc::PRIO_PROCESS, tid);
        nice_result(nice, *libc::__errno_location())
    }
}

/// Sets the calling thread's priority for `role` on this OS ([`level`]). Returns whether the OS accepted it (true when
/// nothing needs changing). It stays in effect until the thread ends, so call it only on threads made for this work.
pub fn set_current_thread(role: ThreadRole) -> bool {
    ROLE.with(|r| r.set(Some(role)));
    match level(role, Os::CURRENT) {
        Level::Normal => true,
        #[cfg(target_os = "macos")]
        Level::Utility => {
            // SAFETY: a plain call that sets the QoS class of the calling thread.
            unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0) == 0 }
        }
        #[cfg(target_os = "linux")]
        Level::Nice10 => {
            // SAFETY: `gettid` takes no arguments; `setpriority` with a thread id lowers only that thread (Linux threads
            // have their own nice value).
            unsafe {
                let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
                // A process started under `nice -n 19` is already lower than asked; setting 10 would be a raise (EPERM
                // for an unprivileged user), so leave it where it is.
                if let Some(current) = current_nice() {
                    if !would_lower(current, NICE_BULK) {
                        log::debug!(current, target = NICE_BULK; "thread priority: already at or below the bulk nice value, left as is");
                        return true;
                    }
                }
                libc::setpriority(libc::PRIO_PROCESS, tid, NICE_BULK) == 0
            }
        }
        #[cfg(windows)]
        Level::BelowNormal => {
            use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
            // SAFETY: the pseudo handle of the calling thread is always valid.
            unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) }.is_ok()
        }
        // A level of another OS is never chosen for this one.
        #[allow(unreachable_patterns)]
        _ => false,
    }
}

/// Lowers the calling thread for bulk work: [`set_current_thread`] with [`ThreadRole::Bulk`] (macOS `QOS_CLASS_UTILITY`,
/// Linux nice 10, Windows `THREAD_PRIORITY_BELOW_NORMAL`, as Debrief's `debrief_core::threads::lower_priority`).
///
/// Never an OS's starvable background level (macOS QoS `BACKGROUND`, Windows background mode): those run a thread only
/// when the machine has nothing else to do, and the Windows mode also drops its I/O priority to the lowest, so on a busy
/// machine work somebody waits for stalls (Debrief's report took 100 s instead of 3 s on a loaded Mac).
pub fn lower_current_thread() -> bool {
    set_current_thread(ThreadRole::Bulk)
}

thread_local! {
    static ROLE: std::cell::Cell<Option<ThreadRole>> = const { std::cell::Cell::new(None) };
}

/// The role the calling thread was given ([`set_current_thread`]), `None` when none was: for tests that check where work
/// runs, on every OS.
pub fn current_role() -> Option<ThreadRole> {
    ROLE.with(std::cell::Cell::get)
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
    use super::{level, Level, Os, ThreadRole};

    /// Rule 52 on Windows (coordinator, 2026-10-30): Windows runs a ready thread of a higher priority before any lower
    /// one, so a lowered live thread (the sampler, a request the user waits on) stalls while normal-priority work fills
    /// every core (Windows CI runs 7166 and 7168). Live work stays at the normal level there; bulk work is lowered on
    /// every OS, and live work keeps the gentle level where the scheduler shares the CPU by weight (macOS, Linux).
    #[test]
    fn live_threads_stay_normal_on_windows_and_bulk_threads_are_lowered_everywhere() {
        assert_eq!(level(ThreadRole::Live, Os::Windows), Level::Normal);
        assert_eq!(level(ThreadRole::Bulk, Os::Windows), Level::BelowNormal);
        assert_eq!(level(ThreadRole::Live, Os::MacOs), Level::Utility);
        assert_eq!(level(ThreadRole::Bulk, Os::MacOs), Level::Utility);
        assert_eq!(level(ThreadRole::Live, Os::Linux), Level::Nice10);
        assert_eq!(level(ThreadRole::Bulk, Os::Linux), Level::Nice10);
        assert_eq!(level(ThreadRole::Live, Os::Other), Level::Normal);
        assert_eq!(level(ThreadRole::Bulk, Os::Other), Level::Normal);
    }

    #[test]
    fn a_thread_knows_the_role_it_was_given() {
        let roles = std::thread::spawn(|| {
            let before = super::current_role();
            super::set_current_thread(ThreadRole::Live);
            let live = super::current_role();
            super::lower_current_thread();
            (before, live, super::current_role())
        })
        .join()
        .unwrap();
        assert_eq!(roles, (None, Some(ThreadRole::Live), Some(ThreadRole::Bulk)));
    }

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

    /// A process started under `nice -n 19` (or a thread already lowered) is below the bulk level; asking for nice 10 would
    /// be a raise, which an unprivileged user may not do (EPERM), yet the thread is lower than asked, so it counts as lowered.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_thread_that_is_already_lower_than_the_bulk_level_counts_as_lowered() {
        let (lowered, nice) = std::thread::spawn(|| {
            // SAFETY: `gettid` takes no arguments; `setpriority` with a thread id changes only that thread (lowering is
            // allowed without privileges).
            let tid = unsafe { libc::syscall(libc::SYS_gettid) as libc::id_t };
            assert_eq!(unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, 19) }, 0);
            (super::lower_current_thread(), super::current_nice())
        })
        .join()
        .unwrap();
        assert!(lowered, "a thread at nice 19 is already lower than the bulk level");
        assert_eq!(nice, Some(19), "it must stay at 19, never be raised to 10");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_normal_thread_is_lowered_to_the_bulk_level_on_linux() {
        let nice = std::thread::spawn(|| {
            // Start from the normal level whatever the test process was started with.
            let before = super::current_nice().unwrap();
            let lowered = super::lower_current_thread();
            (before, lowered, super::current_nice().unwrap())
        })
        .join()
        .unwrap();
        assert!(nice.1);
        assert_eq!(
            nice.2,
            nice.0.max(10),
            "the thread ends at nice 10, or where it already was when lower"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn minus_one_is_a_nice_value_only_without_an_error() {
        assert_eq!(super::nice_result(-1, 0), Some(-1));
        assert_eq!(super::nice_result(-1, 1), None);
        assert_eq!(super::nice_result(5, 1), Some(5));
        assert_eq!(super::nice_result(0, 0), Some(0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_level_is_set_only_when_it_would_lower_the_thread() {
        assert!(super::would_lower(0, 10));
        assert!(super::would_lower(9, 10));
        assert!(!super::would_lower(10, 10));
        assert!(!super::would_lower(19, 10));
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
