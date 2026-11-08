//! What each caller may ask (ADR 0027 §2). The caller is the account the OS reports for the connection (`paddock-ipc`
//! `caller_of`: kernel credentials or the caller's own token, never a process id and never a program path), decided once
//! per connection; this module decides per request, purely. Anyone local may ask who the helper is; the rest needs the
//! owner (the account that turned the helper on) or an administrator. The request handlers (WP-3) widen the list by
//! adding rows here, not by changing the connection code.

use crate::policy::HelperPolicy;
use dyno_core::helper_policy::{access, visibility, Access, Standing, Visibility};
use dyno_core::helper_wire::{HelperRequest, HelperUser};
use paddock_ipc::{AccountId, Caller, Weight};

/// Why a request was refused (recorded in the audit log, with the reason, never shown with more than this to the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `account` is the owner the helper was installed for.
pub fn is_owner(owner: &HelperUser, account: &AccountId) -> bool {
    match (owner, account) {
        (HelperUser::Uid(a), AccountId::Uid(b)) => a == b,
        (HelperUser::Sid(a), AccountId::Sid(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

/// How the caller stands with the helper: the owner, an administrator (also a UAC-filtered token, D9), or anyone else.
pub fn standing(caller: &Caller, owner: &HelperUser) -> Standing {
    Standing::of(is_owner(owner, caller.account()), caller.is_admin())
}

/// What `caller` gets from `request` (the restriction table of `dyno_core::helper_policy`).
///
/// # Errors
/// A [`Refusal`] saying who asked for what.
pub fn authorize(caller: &Caller, owner: &HelperUser, request: &HelperRequest) -> Result<Access, Refusal> {
    match access(request.kind(), standing(caller, owner)) {
        Access::Denied => Err(Refusal(format!(
            "{} may not make this request: it is for the owner or an administrator",
            caller.account()
        ))),
        granted => Ok(granted),
    }
}

/// How much of an account's allowance the request uses.
pub fn weight(request: &HelperRequest) -> Weight {
    if request.kind().is_slow() {
        Weight::Slow
    } else {
        Weight::Light
    }
}

/// What the caller sees of other accounts (the owner and administrators in full; others as the policy says; a UAC-filtered
/// administrator token as the policy says for it).
pub fn scope(caller: &Caller, owner: &HelperUser, policy: &HelperPolicy) -> Visibility {
    visibility(
        standing(caller, owner),
        caller.elevated().unwrap_or(true),
        policy.others,
        policy.filtered_as_total,
    )
}

#[cfg(test)]
pub(crate) mod testing {
    use paddock_ipc::identity::{AdminEvidence, RawPeer};
    use paddock_ipc::{AccountId, AdminRule, AuditHint, Caller};

    /// A caller as the identity code decides it from invented OS facts.
    pub fn caller(account: AccountId, groups: &[u32]) -> Caller {
        paddock_ipc::identity::identify(
            RawPeer {
                account,
                admin: AdminEvidence::Groups(groups.to_vec()),
                audit: AuditHint::new(Some(7), Some("/x".into())),
            },
            &AdminRule::new([80]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::caller;
    use super::*;
    use dyno_core::helper_policy::OthersPolicy;

    fn owner() -> HelperUser {
        HelperUser::Uid(501)
    }

    #[test]
    fn anyone_may_read_and_only_the_owner_and_administrators_may_see_the_audit_log_or_remove_the_helper() {
        let other = caller(AccountId::Uid(502), &[20]);
        for r in [
            HelperRequest::Info,
            HelperRequest::Traffic,
            HelperRequest::LoginItems,
            HelperRequest::MemoryModules,
        ] {
            assert_eq!(authorize(&other, &owner(), &r), Ok(Access::Full), "{r:?}");
        }
        assert_eq!(authorize(&other, &owner(), &HelperRequest::Firewall), Ok(Access::CountsOnly));
        for r in [HelperRequest::Audit(5), HelperRequest::LogTail(5), HelperRequest::Uninstall] {
            let e = authorize(&other, &owner(), &r).unwrap_err();
            assert!(
                e.to_string().contains("uid 502") && e.to_string().contains("owner or an administrator"),
                "{e}"
            );
        }
    }

    #[test]
    fn the_owner_and_administrators_may_make_every_request_in_full() {
        let me = caller(AccountId::Uid(501), &[20]);
        let admin = caller(AccountId::Uid(502), &[20, 80]);
        let root = caller(AccountId::Uid(0), &[]);
        for c in [&me, &admin, &root] {
            for r in [
                HelperRequest::Info,
                HelperRequest::Audit(5),
                HelperRequest::LogTail(5),
                HelperRequest::Uninstall,
                HelperRequest::Firewall,
            ] {
                assert_eq!(authorize(c, &owner(), &r), Ok(Access::Full));
            }
        }
        assert_eq!(standing(&me, &owner()), Standing::Owner);
        assert_eq!(standing(&admin, &owner()), Standing::Administrator);
    }

    #[test]
    fn the_scope_follows_the_standing_and_the_policy() {
        let policy = HelperPolicy {
            others: OthersPolicy::Detail,
            filtered_as_total: false,
        };
        assert_eq!(scope(&caller(AccountId::Uid(501), &[]), &owner(), &policy), Visibility::Full);
        assert_eq!(scope(&caller(AccountId::Uid(502), &[80]), &owner(), &policy), Visibility::Full);
        assert_eq!(
            scope(&caller(AccountId::Uid(502), &[]), &owner(), &policy),
            Visibility::Own(OthersPolicy::Detail)
        );
        assert_eq!(
            scope(&caller(AccountId::Uid(502), &[]), &owner(), &HelperPolicy::default()),
            Visibility::Own(OthersPolicy::Total)
        );
    }

    #[test]
    fn an_account_is_the_owner_only_by_kind_and_value() {
        assert!(is_owner(&HelperUser::Uid(5), &AccountId::Uid(5)));
        assert!(!is_owner(&HelperUser::Uid(5), &AccountId::Uid(6)));
        assert!(is_owner(
            &HelperUser::Sid("S-1-5-21-1".into()),
            &AccountId::Sid("s-1-5-21-1".into())
        ));
        assert!(!is_owner(&HelperUser::Sid("S-1-5-21-1".into()), &AccountId::Uid(1)));
        assert!(!is_owner(&HelperUser::Uid(1), &AccountId::Sid("S-1-5-21-1".into())));
    }

    #[test]
    fn the_slow_kinds_take_the_slow_slot() {
        assert_eq!(weight(&HelperRequest::LoginItems), Weight::Slow);
        assert_eq!(weight(&HelperRequest::Firewall), Weight::Slow);
        // Two sweeps of the process table per request: one at a time per account.
        assert_eq!(weight(&HelperRequest::Traffic), Weight::Slow);
        for r in [HelperRequest::Info, HelperRequest::Audit(1), HelperRequest::Uninstall] {
            assert_eq!(weight(&r), Weight::Light);
        }
    }
}
