//! What each caller may ask (ADR 0027 §2). The caller is the account the OS reports for the connection (`paddock-ipc`
//! `caller_of`: kernel credentials or the caller's own token, never a process id and never a program path), decided once
//! per connection; this module decides per request, purely. Anyone local may ask who the helper is; the rest needs the
//! owner (the account that turned the helper on) or an administrator. The request handlers (WP-3) widen the list by
//! adding rows here, not by changing the connection code.

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

/// Whether `caller` may make `request`.
///
/// # Errors
/// A [`Refusal`] saying who asked for what.
pub fn authorize(caller: &Caller, owner: &HelperUser, request: &HelperRequest) -> Result<(), Refusal> {
    let privileged = caller.is_admin() || is_owner(owner, caller.account());
    let allowed = match request {
        HelperRequest::Info => true,
        HelperRequest::Audit(_) | HelperRequest::Traffic | HelperRequest::Uninstall => privileged,
    };
    if allowed {
        Ok(())
    } else {
        Err(Refusal(format!(
            "{} may not make this request: it is for the owner or an administrator",
            caller.account()
        )))
    }
}

/// How much of an account's allowance the request uses.
pub fn weight(request: &HelperRequest) -> Weight {
    match request {
        HelperRequest::Traffic => Weight::Slow,
        HelperRequest::Info | HelperRequest::Audit(_) | HelperRequest::Uninstall => Weight::Light,
    }
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

    const ALL: [HelperRequest; 4] = [
        HelperRequest::Info,
        HelperRequest::Audit(5),
        HelperRequest::Traffic,
        HelperRequest::Uninstall,
    ];

    #[test]
    fn anyone_may_ask_who_the_helper_is_and_nothing_else() {
        let owner = HelperUser::Uid(501);
        let other = caller(AccountId::Uid(502), &[20]);
        assert_eq!(authorize(&other, &owner, &HelperRequest::Info), Ok(()));
        for r in &ALL[1..] {
            let e = authorize(&other, &owner, r).unwrap_err();
            assert!(
                e.to_string().contains("uid 502") && e.to_string().contains("owner or an administrator"),
                "{e}"
            );
        }
    }

    #[test]
    fn the_owner_and_administrators_may_make_every_request() {
        let owner = HelperUser::Uid(501);
        let me = caller(AccountId::Uid(501), &[20]);
        let admin = caller(AccountId::Uid(502), &[20, 80]);
        let root = caller(AccountId::Uid(0), &[]);
        for c in [&me, &admin, &root] {
            for r in &ALL {
                assert_eq!(authorize(c, &owner, r), Ok(()));
            }
        }
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
    fn only_traffic_is_a_slow_request() {
        assert_eq!(weight(&HelperRequest::Traffic), Weight::Slow);
        for r in [HelperRequest::Info, HelperRequest::Audit(1), HelperRequest::Uninstall] {
            assert_eq!(weight(&r), Weight::Light);
        }
    }
}
