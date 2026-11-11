//! Who is on the other end of a system endpoint (ADR 0027 §2): the account and whether it administers the machine, taken
//! from facts the OS attaches to the connection itself.
//!
//! **A pid is never an identity** (rule 53: the process a pid names can be another by the time it is used). The platform
//! code reads, per connection: the audit token's uid (macOS), the `SO_PEERCRED` uid and `SO_PEERGROUPS` (Linux), or the
//! caller's own token through `ImpersonateNamedPipeClient` (Windows). The pid and the program path travel only in an
//! [`AuditHint`], which has no way into [`Caller::account`] or [`Caller::is_admin`]: [`identify`] never reads it.
//!
//! The decision is pure ([`identify`] over a [`RawPeer`]); the OS reads sit behind [`PeerSource`]. A fact that cannot be
//! read is a [`PeerError`] and the connection is refused (fail closed).

use std::fmt;

/// The account behind a connection, as the OS names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AccountId {
    /// A Unix user id.
    Uid(u32),
    /// A Windows security identifier in its string form (`S-1-5-18`).
    Sid(String),
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountId::Uid(uid) => write!(f, "uid {uid}"),
            AccountId::Sid(sid) => write!(f, "sid {sid}"),
        }
    }
}

/// What the audit line may say about the caller's process. Not an identity: it is read at most once, may already name
/// another process, and no decision uses it. The fields are private so a decision cannot reach for them by accident.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditHint {
    pid: Option<u32>,
    program: Option<String>,
}

impl AuditHint {
    /// A hint from what the OS reported (either may be missing).
    pub fn new(pid: Option<u32>, program: Option<String>) -> AuditHint {
        AuditHint { pid, program }
    }

    /// The pid at connect time, for the log only.
    pub fn pid_for_log(&self) -> Option<u32> {
        self.pid
    }

    /// The program path for the log only; `None` when the OS could not tell (older kernels, Windows).
    pub fn program_for_log(&self) -> Option<&str> {
        self.program.as_deref()
    }
}

/// How the OS says the caller administers the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminEvidence {
    /// Unix: the caller's group ids (`SO_PEERGROUPS`, or `getgrouplist` for the audit token's uid).
    Groups(Vec<u32>),
    /// Windows: membership of Administrators read from the caller's token. A UAC-filtered token of an administrator
    /// counts (the group is present, only "deny only"), by decision D9; [`TokenAdmin::Admin::elevated`] says which it was,
    /// so a policy for filtered tokens can be built.
    Token(TokenAdmin),
}

/// What a Windows token says about Administrators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAdmin {
    /// The token does not have the Administrators group.
    NotAdmin,
    /// The token has the Administrators group.
    Admin {
        /// True when the group is enabled (a full, elevated token); false when it is "deny only" (the UAC-filtered token of an
        /// administrator, `SE_GROUP_USE_FOR_DENY_ONLY`).
        elevated: bool,
    },
}

/// `SE_GROUP_ENABLED` (winnt.h): the group is enabled in the token.
const SE_GROUP_ENABLED: u32 = 0x0000_0004;
/// `SE_GROUP_USE_FOR_DENY_ONLY` (winnt.h): the group is in the token only to deny.
const SE_GROUP_USE_FOR_DENY_ONLY: u32 = 0x0000_0010;

/// Whether the Administrators group with these token attributes makes the token **elevated**: enabled, and not "deny only". A
/// UAC-filtered administrator's token carries the group deny-only (and not enabled); a group that is merely present proves
/// nothing. Pure, so the rule is tested on every platform; the Windows reader passes the attributes it found.
pub fn group_is_elevated(attributes: u32) -> bool {
    attributes & SE_GROUP_ENABLED != 0 && attributes & SE_GROUP_USE_FOR_DENY_ONLY == 0
}

/// The facts a platform read from one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPeer {
    /// The caller's account.
    pub account: AccountId,
    /// What says whether it administers the machine.
    pub admin: AdminEvidence,
    /// The pid and program, for the audit line only.
    pub audit: AuditHint,
}

/// Which Unix groups make an administrator. The helper resolves names to ids once at start (macOS `admin`, Linux `sudo`
/// and `wheel`/`admin` where they exist); the pure decision only compares numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminRule {
    admin_gids: Vec<u32>,
}

impl AdminRule {
    /// Root and members of any of `admin_gids`.
    pub fn new(admin_gids: impl IntoIterator<Item = u32>) -> AdminRule {
        AdminRule {
            admin_gids: admin_gids.into_iter().collect(),
        }
    }
}

/// A connection's caller, decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    account: AccountId,
    is_admin: bool,
    elevated: Option<bool>,
    audit: AuditHint,
}

impl Caller {
    /// The caller's account (the key of scope, limits and the per-account summary).
    pub fn account(&self) -> &AccountId {
        &self.account
    }

    /// Whether the caller administers the machine (root, an administrator group, Windows Administrators).
    pub fn is_admin(&self) -> bool {
        self.is_admin
    }

    /// Windows: whether an administrator's token is elevated (`Some(false)`: UAC-filtered); `None` for a caller that is not an
    /// administrator by token, and on Unix.
    pub fn elevated(&self) -> Option<bool> {
        self.elevated
    }

    /// The pid and program for the audit line.
    pub fn audit(&self) -> &AuditHint {
        &self.audit
    }
}

/// Why a caller could not be identified. Every variant refuses the connection.
#[derive(Debug)]
pub enum PeerError {
    /// The OS call that reports a fact failed or is not available here.
    Unreadable {
        /// What was being read (`"SO_PEERCRED"`, `"OpenThreadToken"`, ...).
        what: &'static str,
        /// The OS error text.
        cause: String,
    },
    /// Windows only: the OS refused to impersonate the client yet (documented to happen until the server has read from the
    /// pipe once; Windows 11 build 26300 does not need the read). Retry after the first read.
    NotReady,
}

impl fmt::Display for PeerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerError::Unreadable { what, cause } => write!(
                f,
                "the caller's identity could not be read ({what}: {cause}); the connection is refused"
            ),
            PeerError::NotReady => f.write_str(
                "the caller's identity can be read only after the first bytes arrived; the connection is refused",
            ),
        }
    }
}

impl std::error::Error for PeerError {}

/// Reads the facts of one connection from the OS.
pub trait PeerSource {
    /// The caller's account, administrator evidence and audit hint, or why they cannot be read.
    fn raw_peer(&self) -> Result<RawPeer, PeerError>;
}

/// Decides who the caller is from the facts. Pure; never reads the audit hint.
pub fn identify(raw: RawPeer, rule: &AdminRule) -> Caller {
    let is_admin = match (&raw.account, &raw.admin) {
        (AccountId::Uid(0), _) => true,
        (_, AdminEvidence::Groups(groups)) => groups.iter().any(|g| rule.admin_gids.contains(g)),
        (_, AdminEvidence::Token(token)) => matches!(token, TokenAdmin::Admin { .. }),
    };
    let elevated = match &raw.admin {
        AdminEvidence::Token(TokenAdmin::Admin { elevated }) => Some(*elevated),
        _ => None,
    };
    Caller {
        account: raw.account,
        is_admin,
        elevated,
        audit: raw.audit,
    }
}

/// Reads the connection's facts and decides: [`PeerSource::raw_peer`] then [`identify`].
pub fn caller_of(source: &dyn PeerSource, rule: &AdminRule) -> Result<Caller, PeerError> {
    source.raw_peer().map(|raw| identify(raw, rule))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn raw(account: AccountId, admin: AdminEvidence, pid: Option<u32>) -> RawPeer {
        RawPeer {
            account,
            admin,
            audit: AuditHint::new(pid, Some("/usr/bin/example".into())),
        }
    }

    #[test]
    fn a_group_is_elevated_only_when_enabled_and_not_deny_only() {
        assert!(
            group_is_elevated(0x7),
            "mandatory, enabled by default, enabled"
        );
        assert!(
            !group_is_elevated(0x10),
            "deny only, as in a filtered token"
        );
        assert!(
            !group_is_elevated(0x14),
            "enabled and deny only is still a filtered token"
        );
        assert!(!group_is_elevated(0x0), "present but not enabled");
        assert!(
            !group_is_elevated(0x3),
            "mandatory and default but not enabled"
        );
    }

    #[test]
    fn root_is_an_administrator_whatever_the_groups() {
        let c = identify(
            raw(AccountId::Uid(0), AdminEvidence::Groups(vec![]), None),
            &AdminRule::default(),
        );
        assert!(c.is_admin());
    }

    #[test]
    fn a_member_of_an_admin_group_is_an_administrator_and_others_are_not() {
        let rule = AdminRule::new([80, 27]);
        let admin = identify(
            raw(
                AccountId::Uid(501),
                AdminEvidence::Groups(vec![20, 80]),
                None,
            ),
            &rule,
        );
        let plain = identify(
            raw(AccountId::Uid(502), AdminEvidence::Groups(vec![20]), None),
            &rule,
        );
        let none = identify(
            raw(AccountId::Uid(503), AdminEvidence::Groups(vec![]), None),
            &AdminRule::default(),
        );
        assert!(admin.is_admin() && !plain.is_admin() && !none.is_admin());
        assert_eq!(admin.account(), &AccountId::Uid(501));
    }

    #[test]
    fn a_windows_token_decides_and_a_filtered_administrator_counts() {
        let sid = || AccountId::Sid("S-1-5-21-1-2-3-1001".into());
        let rule = AdminRule::default();
        let full = identify(
            raw(
                sid(),
                AdminEvidence::Token(TokenAdmin::Admin { elevated: true }),
                None,
            ),
            &rule,
        );
        let filtered = identify(
            raw(
                sid(),
                AdminEvidence::Token(TokenAdmin::Admin { elevated: false }),
                None,
            ),
            &rule,
        );
        let user = identify(
            raw(sid(), AdminEvidence::Token(TokenAdmin::NotAdmin), None),
            &rule,
        );
        assert!(full.is_admin() && filtered.is_admin() && !user.is_admin());
        assert_eq!(
            (full.elevated(), filtered.elevated(), user.elevated()),
            (Some(true), Some(false), None)
        );
        let unix = identify(
            raw(AccountId::Uid(1), AdminEvidence::Groups(vec![]), None),
            &rule,
        );
        assert_eq!(unix.elevated(), None);
    }

    /// A fake connection that says uid 501 while its pid, in the fake's own process table, belongs to root.
    struct LyingPids {
        lookups: Cell<u32>,
    }

    impl LyingPids {
        /// What a pid-based lookup would answer: pid 1 is root's. Counting proves nobody asks.
        fn owner_of_pid(&self, _pid: u32) -> AccountId {
            self.lookups.set(self.lookups.get() + 1);
            AccountId::Uid(0)
        }
    }

    impl PeerSource for LyingPids {
        fn raw_peer(&self) -> Result<RawPeer, PeerError> {
            Ok(raw(
                AccountId::Uid(501),
                AdminEvidence::Groups(vec![20]),
                Some(1),
            ))
        }
    }

    #[test]
    fn the_pid_is_never_used_for_the_identity() {
        let source = LyingPids {
            lookups: Cell::new(0),
        };
        let caller = caller_of(&source, &AdminRule::new([80])).unwrap();
        assert_eq!(caller.account(), &AccountId::Uid(501));
        assert!(!caller.is_admin());
        assert_eq!(caller.audit().pid_for_log(), Some(1));
        assert_eq!(caller.audit().program_for_log(), Some("/usr/bin/example"));
        assert_eq!(
            source.lookups.get(),
            0,
            "no pid-based owner lookup happened"
        );
        assert_eq!(source.owner_of_pid(1), AccountId::Uid(0));
        // Whatever pid the audit hint carries, the decision is the same.
        for pid in [None, Some(0), Some(1), Some(4242), Some(u32::MAX)] {
            let a = identify(
                raw(AccountId::Uid(501), AdminEvidence::Groups(vec![20]), pid),
                &AdminRule::new([80]),
            );
            assert_eq!((a.account(), a.is_admin()), (&AccountId::Uid(501), false));
        }
    }

    #[test]
    fn unreadable_facts_refuse_with_the_cause() {
        struct Broken;
        impl PeerSource for Broken {
            fn raw_peer(&self) -> Result<RawPeer, PeerError> {
                Err(PeerError::Unreadable {
                    what: "SO_PEERCRED",
                    cause: "bad descriptor".into(),
                })
            }
        }
        let e = caller_of(&Broken, &AdminRule::default()).unwrap_err();
        assert!(e.to_string().contains("SO_PEERCRED") && e.to_string().contains("refused"));
        assert!(PeerError::NotReady.to_string().contains("first bytes"));
    }

    #[test]
    fn accounts_print_for_logs_and_hint_defaults_are_empty() {
        assert_eq!(AccountId::Uid(7).to_string(), "uid 7");
        assert_eq!(
            AccountId::Sid("S-1-5-18".into()).to_string(),
            "sid S-1-5-18"
        );
        assert_eq!(AuditHint::default().pid_for_log(), None);
        assert_eq!(AuditHint::default().program_for_log(), None);
    }
}
