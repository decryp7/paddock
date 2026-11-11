//! Who is on the other end of a connection, and what that caller may ask (the Windows service runs as LocalSystem and its
//! pipe is open to every interactive user, ADR 0004).
//!
//! A request that changes something can make the service start a program with its own token, so it is accepted only from an
//! **elevated administrator**: the caller's own token, read from the pipe (`ImpersonateNamedPipeClient`, never a process id:
//! rule 53), must have the Administrators group enabled. An administrator's UAC-filtered token (the group is "deny only") is
//! an unelevated process of that user and does not count: it is exactly what any program the user runs has. A caller that
//! cannot be identified is refused (fail closed). Requests that only read stay open to the pipe's users.
//!
//! The decision is pure ([`classify`], [`authorize`]); only [`level_of_pipe`] reads the OS. On Unix the socket is the
//! user's own (mode 0600 in a 0700 folder) and the service runs as that user, so nothing is gained by asking: [`Caller::Local`].

use paddock_ipc::identity::{identify, AdminEvidence, AdminRule, RawPeer, TokenAdmin};
use pitcrew_core::protocol::{ErrorCode, Request, ServiceError};

/// What the service knows about the caller of one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The transport itself limits who can connect to the service's own user (a Unix socket, tests): no more to ask.
    Local,
    /// An elevated administrator.
    Elevated,
    /// Anyone else the pipe lets in: a standard user, or an administrator's filtered token.
    Standard,
    /// The OS did not say who it is (the reason, for the log): treated as [`Caller::Standard`].
    Unreadable(String),
}

impl Caller {
    /// Whether the caller may send requests that change something.
    pub fn may_change(&self) -> bool {
        matches!(self, Caller::Local | Caller::Elevated)
    }

    /// A short phrase for the service log.
    pub fn describe(&self) -> String {
        match self {
            Caller::Local => "a local caller".into(),
            Caller::Elevated => "an elevated administrator".into(),
            Caller::Standard => "a caller without elevated administrator rights".into(),
            Caller::Unreadable(why) => format!("a caller that could not be identified ({why})"),
        }
    }
}

/// The caller the facts of the pipe's client describe. Pure.
pub fn classify(raw: RawPeer) -> Caller {
    let admin = matches!(raw.admin, AdminEvidence::Token(TokenAdmin::Admin { .. }));
    let caller = identify(raw, &AdminRule::default());
    if admin && caller.is_admin() && caller.elevated() == Some(true) {
        Caller::Elevated
    } else {
        Caller::Standard
    }
}

/// Refuses a request the caller may not send: `Ok` for a read-only request or an allowed caller.
///
/// # Errors
/// [`ErrorCode::PermissionDenied`] with a message that says what to do (the app answers it with the elevated request).
pub fn authorize(request: &Request, caller: &Caller) -> Result<(), ServiceError> {
    if !request.requires_admin() || caller.may_change() {
        return Ok(());
    }
    Err(ServiceError::new(
        ErrorCode::PermissionDenied,
        "Changing the runners needs administrator rights: ask again from an elevated Pitcrew (the app does this and Windows asks for permission)",
    ))
}

/// The name of a request for the log: its variant, never its fields (a registration token is one of them).
pub fn kind_of(request: &Request) -> String {
    format!("{request:?}")
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
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
    use paddock_ipc::identity::{AccountId, AuditHint};
    use pitcrew_core::names::RunnerName;

    fn raw(token: TokenAdmin) -> RawPeer {
        RawPeer {
            account: AccountId::Sid("S-1-5-21-1-2-3-1001".into()),
            admin: AdminEvidence::Token(token),
            audit: AuditHint::default(),
        }
    }

    #[test]
    fn only_an_elevated_administrator_token_may_change_things() {
        assert_eq!(classify(raw(TokenAdmin::Admin { elevated: true })), Caller::Elevated);
        assert_eq!(
            classify(raw(TokenAdmin::Admin { elevated: false })),
            Caller::Standard,
            "a UAC-filtered administrator is any program the user runs"
        );
        assert_eq!(classify(raw(TokenAdmin::NotAdmin)), Caller::Standard);
    }

    #[test]
    fn a_caller_that_cannot_be_identified_cannot_change_things() {
        assert!(!Caller::Unreadable("OpenThreadToken: denied".into()).may_change());
        assert!(!Caller::Standard.may_change());
        assert!(Caller::Elevated.may_change() && Caller::Local.may_change());
    }

    #[test]
    fn requests_that_change_what_runs_are_refused_for_everyone_else_and_operations_are_not() {
        let save = Request::SaveSettings {
            config: pitcrew_core::config::Config::default(),
        };
        let e = authorize(&save, &Caller::Standard).unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        assert!(e.message.contains("administrator"), "{}", e.message);
        assert!(authorize(&save, &Caller::Elevated).is_ok());
        let stop = Request::Stop {
            runner: RunnerName::parse("a").unwrap(),
        };
        assert!(
            authorize(&stop, &Caller::Standard).is_ok(),
            "stopping a runner is open to the pipe's users"
        );
        assert!(authorize(&Request::Status, &Caller::Standard).is_ok());
        assert!(authorize(&Request::GetSettings, &Caller::Unreadable("x".into())).is_ok());
    }

    #[test]
    fn the_log_names_the_request_and_never_its_fields() {
        let r = Request::Drain {
            runner: RunnerName::parse("secret-looking-name").unwrap(),
        };
        assert_eq!(kind_of(&r), "Drain");
        assert_eq!(kind_of(&Request::Status), "Status");
    }
}
