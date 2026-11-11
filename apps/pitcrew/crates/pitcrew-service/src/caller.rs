//! Who is on the other end of a connection, and what that caller may ask (the Windows service runs as LocalSystem and its
//! pipe is open to every interactive user, ADR 0004).
//!
//! A request that changes something can make the service start a program with its own token, so it is accepted only from an
//! **elevated administrator**: the caller's own token, read from the pipe (`ImpersonateNamedPipeClient`, never a process id:
//! rule 53), must have the Administrators group enabled. An administrator's UAC-filtered token (the group is "deny only") is
//! an unelevated process of that user and does not count: it is exactly what any program the user runs has. A caller that
//! cannot be identified is refused (fail closed) for every request that changes anything, however small. Reads, and
//! operating an existing runner (start, stop, drain, ...) by an identified caller, stay open to the pipe's users. Every
//! request that changes something is logged with the caller's account.
//!
//! The decision is pure ([`classify`], [`authorize`]); only [`level_of_pipe`] reads the OS. On Unix the socket is the
//! user's own (mode 0600 in a 0700 folder) and the service runs as that user, so nothing is gained by asking: [`Caller::Local`].

use paddock_ipc::identity::{identify, AccountId, AdminEvidence, AdminRule, RawPeer, TokenAdmin};
use pitcrew_core::protocol::{ErrorCode, Request, ServiceError};

/// What the service knows about the caller of one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The transport itself limits who can connect to the service's own user (a Unix socket, tests): no more to ask.
    Local,
    /// An elevated administrator (the account's SID, for the log).
    Elevated(String),
    /// Anyone else the pipe lets in: a standard user, or an administrator's filtered token (the account's SID).
    Standard(String),
    /// The OS did not say who it is (the reason, for the log): it may read, never change.
    Unreadable(String),
}

impl Caller {
    /// Whether the caller may send requests that change what runs or where.
    pub fn may_change(&self) -> bool {
        matches!(self, Caller::Local | Caller::Elevated(_))
    }

    /// Whether the caller was identified (or needs no identity): anyone else may not even operate a runner.
    pub fn is_identified(&self) -> bool {
        !matches!(self, Caller::Unreadable(_))
    }

    /// A short phrase for the service log.
    pub fn describe(&self) -> String {
        match self {
            Caller::Local => "a local caller".into(),
            Caller::Elevated(sid) => format!("an elevated administrator ({sid})"),
            Caller::Standard(sid) => format!("a caller without elevated administrator rights ({sid})"),
            Caller::Unreadable(why) => format!("a caller that could not be identified ({why})"),
        }
    }
}

/// The caller the facts of the pipe's client describe. Pure.
pub fn classify(raw: RawPeer) -> Caller {
    let admin = matches!(raw.admin, AdminEvidence::Token(TokenAdmin::Admin { .. }));
    let caller = identify(raw, &AdminRule::default());
    let account = match caller.account() {
        AccountId::Sid(sid) => sid.clone(),
        other => other.to_string(),
    };
    if admin && caller.is_admin() && caller.elevated() == Some(true) {
        Caller::Elevated(account)
    } else {
        Caller::Standard(account)
    }
}

/// Refuses a request the caller may not send: `Ok` for a read, for operating a runner by an identified caller, and for an allowed
/// administrator.
///
/// # Errors
/// [`ErrorCode::PermissionDenied`] with a message that says what to do (the app answers it with the elevated request).
pub fn authorize(request: &Request, caller: &Caller) -> Result<(), ServiceError> {
    if !request.mutates() {
        return Ok(());
    }
    if !caller.is_identified() {
        return Err(ServiceError::new(
            ErrorCode::PermissionDenied,
            "the service could not tell who is asking, so it changes nothing; try again, and see the service log if it persists",
        ));
    }
    if request.requires_admin() && !caller.may_change() {
        return Err(ServiceError::new(
            ErrorCode::PermissionDenied,
            "Changing the runners needs administrator rights: ask again from an elevated Pitcrew (the app does this and Windows asks for permission)",
        ));
    }
    Ok(())
}

/// The caller on the server end of a connected pipe instance (the client has sent its first bytes). Windows only.
#[cfg(windows)]
pub fn level_of_pipe(pipe: std::os::windows::io::RawHandle) -> Caller {
    match paddock_ipc::endpoint::raw_peer_of_pipe(pipe) {
        Ok(raw) => classify(raw),
        Err(e) => Caller::Unreadable(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_ipc::identity::AuditHint;
    use pitcrew_core::names::RunnerName;

    const SID: &str = "S-1-5-21-1-2-3-1001";

    fn raw(token: TokenAdmin) -> RawPeer {
        RawPeer {
            account: AccountId::Sid(SID.into()),
            admin: AdminEvidence::Token(token),
            audit: AuditHint::default(),
        }
    }

    fn stop() -> Request {
        Request::Stop {
            runner: RunnerName::parse("a").unwrap(),
        }
    }

    #[test]
    fn only_an_elevated_administrator_token_may_change_things() {
        assert_eq!(classify(raw(TokenAdmin::Admin { elevated: true })), Caller::Elevated(SID.into()));
        assert_eq!(
            classify(raw(TokenAdmin::Admin { elevated: false })),
            Caller::Standard(SID.into()),
            "a UAC-filtered administrator is any program the user runs"
        );
        assert_eq!(classify(raw(TokenAdmin::NotAdmin)), Caller::Standard(SID.into()));
    }

    #[test]
    fn the_account_is_named_in_the_log_line() {
        assert!(Caller::Standard(SID.into()).describe().contains(SID));
        assert!(Caller::Elevated(SID.into()).describe().contains(SID));
    }

    #[test]
    fn a_caller_that_cannot_be_identified_cannot_change_anything() {
        let nobody = Caller::Unreadable("OpenThreadToken: denied".into());
        assert!(!nobody.may_change() && !nobody.is_identified());
        let e = authorize(&stop(), &nobody).unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied, "not even operating a runner");
        assert!(e.message.contains("could not tell who"), "{}", e.message);
        assert!(authorize(&Request::Status, &nobody).is_ok(), "reading needs no identity");
    }

    #[test]
    fn requests_that_change_what_runs_are_refused_for_everyone_else_and_operations_are_not() {
        let save = Request::SaveSettings {
            config: pitcrew_core::config::Config::default(),
        };
        let e = authorize(&save, &Caller::Standard(SID.into())).unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        assert!(e.message.contains("administrator"), "{}", e.message);
        assert!(authorize(&save, &Caller::Elevated(SID.into())).is_ok());
        assert!(authorize(&save, &Caller::Local).is_ok());
        assert!(
            authorize(&stop(), &Caller::Standard(SID.into())).is_ok(),
            "stopping a runner is open to the pipe's users"
        );
        assert!(authorize(&Request::Status, &Caller::Standard(SID.into())).is_ok());
    }
}
