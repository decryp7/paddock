//! Who may ask the helper for what (ADR 0027 section 2): the per-request restriction table, the scope a caller sees and the
//! one-install-at-a-time rule. The per-account caps themselves are `paddock_ipc::AccountLimits` (the server's); this file only
//! says which kinds are slow. Pure: the caller's facts (account, Administrators membership, owner) arrive already read
//! from the connection by `paddock-ipc`; nothing here looks at a process id.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The kind of a request, without its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestKind {
    /// `Info`.
    Info,
    /// `Processes`.
    Processes,
    /// `Traffic`.
    Traffic,
    /// `SocketOwners`.
    SocketOwners,
    /// `FileHolders`.
    FileHolders,
    /// `LoginItems`.
    LoginItems,
    /// `Counters`.
    Counters,
    /// `MemoryModules`.
    MemoryModules,
    /// `Smart`.
    Smart,
    /// `InstallTool`.
    InstallTool,
    /// `Firewall`.
    Firewall,
    /// `Extras`.
    Extras,
    /// `Audit`.
    Audit,
    /// `LogTail`.
    LogTail,
    /// `Uninstall`.
    Uninstall,
    /// `ScheduleWake`.
    ScheduleWake,
    /// `CancelWake`.
    CancelWake,
    /// `ListWakes`.
    ListWakes,
    /// `WolState`.
    WolState,
}

impl RequestKind {
    /// Every kind, for tests and tables.
    pub const ALL: [RequestKind; 19] = [
        RequestKind::Info,
        RequestKind::Processes,
        RequestKind::Traffic,
        RequestKind::SocketOwners,
        RequestKind::FileHolders,
        RequestKind::LoginItems,
        RequestKind::Counters,
        RequestKind::MemoryModules,
        RequestKind::Smart,
        RequestKind::InstallTool,
        RequestKind::Firewall,
        RequestKind::Extras,
        RequestKind::Audit,
        RequestKind::LogTail,
        RequestKind::Uninstall,
        RequestKind::ScheduleWake,
        RequestKind::CancelWake,
        RequestKind::ListWakes,
        RequestKind::WolState,
    ];

    /// Requests that run an external tool for a long time, or sweep the whole process table twice (`Traffic`): the server gives them
    /// `Weight::Slow` (one per account at a time).
    pub fn is_slow(self) -> bool {
        matches!(
            self,
            RequestKind::Smart
                | RequestKind::Traffic
                | RequestKind::LoginItems
                | RequestKind::Firewall
                | RequestKind::Extras
                | RequestKind::InstallTool
                | RequestKind::ScheduleWake
                | RequestKind::CancelWake
                | RequestKind::ListWakes
        )
    }
}

/// How the caller stands with the helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Standing {
    /// The account that turned the helper on.
    Owner,
    /// A member of Administrators (also with a UAC-filtered token, ADR 0027 R3) or root.
    Administrator,
    /// Any other account.
    Other,
}

impl Standing {
    /// The owner counts first; then administrators; everyone else is `Other`.
    pub fn of(is_owner: bool, is_administrator: bool) -> Standing {
        if is_owner {
            Standing::Owner
        } else if is_administrator {
            Standing::Administrator
        } else {
            Standing::Other
        }
    }

    /// Owner or administrator.
    pub fn is_privileged(self) -> bool {
        self != Standing::Other
    }
}

/// What a standing gets from a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    /// The full answer (still scoped by [`Visibility`] where the data names other accounts).
    Full,
    /// Only states and counts.
    CountsOnly,
    /// Refused.
    Denied,
}

/// The restriction table: what `standing` gets from `kind`.
pub fn access(kind: RequestKind, standing: Standing) -> Access {
    use RequestKind as K;
    match (kind, standing.is_privileged()) {
        (K::InstallTool | K::Audit | K::LogTail | K::Uninstall | K::ScheduleWake | K::CancelWake | K::ListWakes, false) => Access::Denied,
        (K::Firewall | K::Extras, false) => Access::CountsOnly,
        _ => Access::Full,
    }
}

/// What the administrator-edited policy file says about other accounts' processes for callers without full detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OthersPolicy {
    /// Other accounts as one "Other users" total (the default).
    #[default]
    Total,
    /// Other accounts in detail.
    Detail,
    /// Other accounts not shown at all.
    Hidden,
}

/// What a caller sees of processes and traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Visibility {
    /// Every account's processes in detail.
    Full,
    /// Its own and system or service accounts in detail; other accounts as the policy says.
    Own(OthersPolicy),
}

/// The scope of a caller. `elevated` is false for an Administrators member whose token UAC filtered (R3, D9; paddock-ipc's
/// `Admin { elevated }`): it gets the administrator scope unless the policy file asked for `Total` for filtered tokens.
pub fn visibility(standing: Standing, elevated: bool, others: OthersPolicy, filtered_as_total: bool) -> Visibility {
    match standing {
        Standing::Owner => Visibility::Full,
        Standing::Administrator if !elevated && filtered_as_total => Visibility::Own(OthersPolicy::Total),
        Standing::Administrator => Visibility::Full,
        Standing::Other => Visibility::Own(others),
    }
}

/// The rule that only one tool install runs at a time, whoever asked. Cheap to clone; clones share the flag.
#[derive(Clone, Default)]
pub struct InstallGuard(Arc<AtomicBool>);

impl InstallGuard {
    /// A guard with no install running.
    pub fn new() -> InstallGuard {
        InstallGuard::default()
    }

    /// Takes the install slot, or `None` while another install runs. Dropping the ticket frees it.
    pub fn try_begin(&self) -> Option<InstallTicket> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| InstallTicket(self.0.clone()))
    }
}

/// The running install's slot.
#[must_use = "dropping the ticket frees the install slot"]
pub struct InstallTicket(Arc<AtomicBool>);

impl Drop for InstallTicket {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RequestKind as K;

    #[test]
    fn the_table_restricts_each_request_by_standing() {
        for s in [Standing::Owner, Standing::Administrator] {
            for k in K::ALL {
                assert_eq!(access(k, s), Access::Full, "{k:?} {s:?}");
            }
        }
        let other = |k| access(k, Standing::Other);
        for k in [
            K::InstallTool,
            K::Audit,
            K::LogTail,
            K::Uninstall,
            K::ScheduleWake,
            K::CancelWake,
            K::ListWakes,
        ] {
            assert_eq!(other(k), Access::Denied, "{k:?}");
        }
        for k in [K::Firewall, K::Extras] {
            assert_eq!(other(k), Access::CountsOnly, "{k:?}");
        }
        for k in [
            K::Info,
            K::Processes,
            K::Traffic,
            K::SocketOwners,
            K::FileHolders,
            K::LoginItems,
            K::Counters,
            K::MemoryModules,
            K::WolState,
            K::Smart,
        ] {
            assert_eq!(other(k), Access::Full, "{k:?}");
        }
    }

    #[test]
    fn the_slow_kinds_are_the_tool_runs() {
        let slow: Vec<_> = K::ALL.into_iter().filter(|k| k.is_slow()).collect();
        assert_eq!(
            slow,
            [
                K::Traffic,
                K::LoginItems,
                K::Smart,
                K::InstallTool,
                K::Firewall,
                K::Extras,
                K::ScheduleWake,
                K::CancelWake,
                K::ListWakes
            ]
        );
    }

    #[test]
    fn owner_wins_over_administrator() {
        assert_eq!(Standing::of(true, true), Standing::Owner);
        assert_eq!(Standing::of(true, false), Standing::Owner);
        assert_eq!(Standing::of(false, true), Standing::Administrator);
        assert_eq!(Standing::of(false, false), Standing::Other);
    }

    #[test]
    fn scope_follows_standing_and_the_policy_file() {
        use OthersPolicy as P;
        assert_eq!(visibility(Standing::Owner, false, P::Hidden, true), Visibility::Full);
        assert_eq!(visibility(Standing::Administrator, true, P::Total, true), Visibility::Full);
        // R3/D9: a filtered administrator token is an administrator unless the policy file says otherwise.
        assert_eq!(visibility(Standing::Administrator, false, P::Total, false), Visibility::Full);
        assert_eq!(
            visibility(Standing::Administrator, false, P::Detail, true),
            Visibility::Own(P::Total)
        );
        for p in [P::Total, P::Detail, P::Hidden] {
            assert_eq!(visibility(Standing::Other, true, p, false), Visibility::Own(p));
        }
        assert_eq!(OthersPolicy::default(), P::Total);
    }

    #[test]
    fn one_install_at_a_time() {
        let g = InstallGuard::new();
        let first = g.try_begin().unwrap();
        assert!(g.clone().try_begin().is_none());
        drop(first);
        assert!(g.try_begin().is_some());
    }
}
