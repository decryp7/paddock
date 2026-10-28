//! Which per-process values were really read, as a pure decision over what the OS source returned.
//!
//! Some sources report `0` for a value the OS refused (another user's process, a protected one). A `0` is never passed on:
//! [`measure`] turns it into [`Measured::Unavailable`] with the reason, and only keeps the numbers that were read.

use dyno_core::process::{Measured, Owner, UnavailableReason};
use dyno_core::time::Timestamp;

/// The operating system family, as far as the readability rules differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Windows: a process that cannot be opened needs administrator rights.
    Windows,
    /// macOS: another user's process is refused by the OS.
    Macos,
    /// Linux: CPU and start time are public, memory and disk of other users' processes are not given.
    Linux,
}

impl Os {
    /// The family this build runs on.
    pub fn this() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }

    /// Why a refused process value was refused.
    fn refused(self) -> UnavailableReason {
        match self {
            Os::Windows => UnavailableReason::NeedsAdmin,
            Os::Macos | Os::Linux => UnavailableReason::NotPermitted,
        }
    }
}

/// The first user id of ordinary accounts on macOS: lower ids are system accounts.
pub const MACOS_FIRST_USER_UID: u32 = 500;
/// The default first user id of ordinary accounts on Linux when `/etc/login.defs` does not say.
pub const LINUX_DEFAULT_UID_MIN: u32 = 1000;

/// The first ordinary user id on this OS: ids below it are system accounts. `login_defs` is the text of
/// `/etc/login.defs` (Linux), when it could be read. Windows has no such range; this gives 0.
pub fn first_user_uid(os: Os, login_defs: Option<&str>) -> u32 {
    match os {
        Os::Macos => MACOS_FIRST_USER_UID,
        Os::Linux => login_defs.and_then(uid_min_from).unwrap_or(LINUX_DEFAULT_UID_MIN),
        Os::Windows => 0,
    }
}

/// `UID_MIN` from the text of `/etc/login.defs`.
pub fn uid_min_from(text: &str) -> Option<u32> {
    text.lines().find_map(|l| {
        let l = l.trim();
        if l.starts_with('#') {
            return None;
        }
        let mut words = l.split_whitespace();
        (words.next() == Some("UID_MIN")).then(|| words.next()?.parse().ok()).flatten()
    })
}

/// Whose a Unix process is: `uid` against Dyno's own and the first ordinary user id. A process with no readable owner is
/// [`Owner::Unknown`].
pub fn classify_uid(uid: Option<u32>, own_uid: Option<u32>, first_user: u32) -> Owner {
    match (uid, own_uid) {
        (Some(u), Some(own)) if u == own => Owner::Own,
        (Some(u), _) if u < first_user => Owner::System,
        (Some(_), Some(_)) => Owner::Other,
        _ => Owner::Unknown,
    }
}

/// Whose a Windows process is by its account SID: SYSTEM, LOCAL SERVICE and NETWORK SERVICE are the system; the same SID as
/// Dyno's is its own; any other is another user's.
pub fn classify_sid(sid: Option<&str>, own_sid: Option<&str>) -> Owner {
    match (sid, own_sid) {
        (Some("S-1-5-18" | "S-1-5-19" | "S-1-5-20"), _) => Owner::System,
        (Some(a), Some(b)) if a == b => Owner::Own,
        (Some(_), Some(_)) => Owner::Other,
        _ => Owner::Unknown,
    }
}

/// Whose a Windows process is by its session: session 0 is the system's, Dyno's own session is its own, any other session
/// belongs to another user.
pub fn classify_session(session: u32, own_session: u32) -> Owner {
    if session == 0 {
        Owner::System
    } else if session == own_session {
        Owner::Own
    } else {
        Owner::Other
    }
}

/// What the OS source returned for one process; a zero may mean "not read".
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    /// CPU, percent of the machine.
    pub cpu: f32,
    /// Resident memory in bytes.
    pub memory: u64,
    /// Bytes read per second.
    pub disk_read: f64,
    /// Bytes written per second.
    pub disk_write: f64,
    /// Start time in seconds since the Unix epoch.
    pub start_secs: u64,
    /// The user name, when the OS gave one.
    pub user: Option<String>,
}

/// The values of one process, each read or unavailable with a reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Measures {
    /// The user.
    pub user: Measured<String>,
    /// CPU.
    pub cpu: Measured<f32>,
    /// Memory.
    pub memory: Measured<u64>,
    /// Disk read rate.
    pub disk_read: Measured<f64>,
    /// Disk write rate.
    pub disk_write: Measured<f64>,
    /// Start time.
    pub started: Measured<Timestamp>,
}

/// Decides which of `raw` was read. `started` converts the start seconds to a timestamp.
pub fn measure(os: Os, owner: Owner, raw: &Raw, started: impl Fn(u64) -> Timestamp) -> Measures {
    let refused = os.refused();
    // Own processes are opened in full. Others on Windows and macOS are opened only when the OS allows it: a working set of 0
    // means it did not. On Linux the process file is public for CPU and start time, and never gives other users' memory or I/O.
    let opened = owner == Owner::Own || (os != Os::Linux && raw.memory > 0);
    let started_m = if raw.start_secs > 0 {
        Measured::Value(started(raw.start_secs))
    } else {
        Measured::unavailable(if owner == Owner::Own {
            UnavailableReason::NotSupported
        } else {
            refused
        })
    };
    let user = match &raw.user {
        Some(u) => Measured::Value(u.clone()),
        None => Measured::unavailable(if owner == Owner::Own {
            UnavailableReason::NotSupported
        } else {
            refused
        }),
    };
    if opened {
        return Measures {
            user,
            cpu: Measured::Value(raw.cpu),
            memory: Measured::Value(raw.memory),
            disk_read: Measured::Value(raw.disk_read),
            disk_write: Measured::Value(raw.disk_write),
            started: started_m,
        };
    }
    match os {
        Os::Linux => Measures {
            user,
            cpu: Measured::Value(raw.cpu),
            memory: Measured::unavailable(UnavailableReason::ResidentMemoryOnly),
            disk_read: Measured::unavailable(UnavailableReason::NotPermitted),
            disk_write: Measured::unavailable(UnavailableReason::NotPermitted),
            started: started_m,
        },
        Os::Windows | Os::Macos => Measures {
            user,
            cpu: Measured::unavailable(refused),
            memory: Measured::unavailable(refused),
            disk_read: Measured::unavailable(refused),
            disk_write: Measured::unavailable(refused),
            started: started_m,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(memory: u64) -> Raw {
        Raw {
            cpu: 3.0,
            memory,
            disk_read: 10.0,
            disk_write: 20.0,
            start_secs: 100,
            user: Some("other".into()),
        }
    }

    fn ts(s: u64) -> Timestamp {
        Timestamp(s as i64 * 1000)
    }

    #[test]
    fn own_processes_are_read_in_full_and_a_zero_stays_a_zero() {
        let m = measure(Os::Windows, Owner::Own, &raw(0), ts);
        assert_eq!(m.cpu, Measured::Value(3.0));
        assert_eq!(m.memory, Measured::Value(0), "an own process really using nothing is read as such");
        assert_eq!(m.started, Measured::Value(Timestamp(100_000)));
    }

    #[test]
    fn other_processes_the_os_would_not_open_need_admin_on_windows() {
        let m = measure(Os::Windows, Owner::Other, &raw(0), ts);
        for r in [m.cpu.reason(), m.memory.reason(), m.disk_read.reason(), m.disk_write.reason()] {
            assert_eq!(r, Some(UnavailableReason::NeedsAdmin));
        }
        assert_eq!(
            m.started,
            Measured::Value(Timestamp(100_000)),
            "the start time came from the process list"
        );
        // The same process opened (working set read) keeps its numbers.
        let open = measure(Os::Windows, Owner::Other, &raw(5), ts);
        assert_eq!(open.memory, Measured::Value(5));
        assert_eq!(open.cpu, Measured::Value(3.0));
    }

    #[test]
    fn other_processes_are_not_permitted_on_macos_and_missing_values_say_why() {
        let mut r = raw(0);
        r.start_secs = 0;
        r.user = None;
        let m = measure(Os::Macos, Owner::Unknown, &r, ts);
        assert_eq!(m.cpu.reason(), Some(UnavailableReason::NotPermitted));
        assert_eq!(m.started.reason(), Some(UnavailableReason::NotPermitted));
        assert_eq!(m.user.reason(), Some(UnavailableReason::NotPermitted));
        let own = measure(Os::Macos, Owner::Own, &r, ts);
        assert_eq!(own.started.reason(), Some(UnavailableReason::NotSupported));
        assert_eq!(own.user.reason(), Some(UnavailableReason::NotSupported));
    }

    #[test]
    fn linux_keeps_cpu_and_start_of_other_users_but_not_their_memory_or_disk() {
        let m = measure(Os::Linux, Owner::Other, &raw(4096), ts);
        assert_eq!(m.cpu, Measured::Value(3.0));
        assert_eq!(m.started, Measured::Value(Timestamp(100_000)));
        assert_eq!(m.memory.reason(), Some(UnavailableReason::ResidentMemoryOnly));
        assert_eq!(m.disk_read.reason(), Some(UnavailableReason::NotPermitted));
        assert_eq!(m.disk_write.reason(), Some(UnavailableReason::NotPermitted));
        assert_eq!(m.user, Measured::Value("other".into()));
    }

    #[test]
    fn owners_are_told_apart_by_uid_sid_and_session() {
        assert_eq!(classify_uid(Some(501), Some(501), 500), Owner::Own);
        assert_eq!(classify_uid(Some(0), Some(501), 500), Owner::System);
        assert_eq!(classify_uid(Some(499), Some(501), 500), Owner::System);
        assert_eq!(classify_uid(Some(502), Some(501), 500), Owner::Other);
        assert_eq!(classify_uid(None, Some(501), 500), Owner::Unknown);
        assert_eq!(classify_uid(Some(502), None, 500), Owner::Unknown);
        assert_eq!(classify_sid(Some("S-1-5-18"), Some("S-1-5-21-1")), Owner::System);
        assert_eq!(classify_sid(Some("S-1-5-20"), None), Owner::System);
        assert_eq!(classify_sid(Some("S-1-5-21-1"), Some("S-1-5-21-1")), Owner::Own);
        assert_eq!(classify_sid(Some("S-1-5-21-2"), Some("S-1-5-21-1")), Owner::Other);
        assert_eq!(classify_sid(None, Some("S-1-5-21-1")), Owner::Unknown);
        assert_eq!(classify_session(0, 2), Owner::System);
        assert_eq!(classify_session(2, 2), Owner::Own);
        assert_eq!(classify_session(3, 2), Owner::Other);
    }

    #[test]
    fn the_first_user_id_comes_from_login_defs_on_linux() {
        assert_eq!(first_user_uid(Os::Macos, None), 500);
        assert_eq!(first_user_uid(Os::Windows, None), 0);
        assert_eq!(first_user_uid(Os::Linux, None), 1000);
        assert_eq!(
            first_user_uid(
                Os::Linux,
                Some(
                    "# UID_MIN 5
UID_MIN	  1200
UID_MAX 60000
"
                )
            ),
            1200
        );
        assert_eq!(
            first_user_uid(
                Os::Linux,
                Some(
                    "UID_MIN abc
"
                )
            ),
            1000
        );
        assert_eq!(
            uid_min_from(
                "UID_MAX 5
"
            ),
            None
        );
    }

    #[test]
    fn this_host_is_one_of_the_families() {
        assert!(matches!(Os::this(), Os::Windows | Os::Macos | Os::Linux));
    }
}
