//! Whether a client may trust the server it reached on a system endpoint (ADR 0027 §2, "pipe squatting"). While the real
//! server is not running, any local process can create the fixed name first and answer in its place. The client therefore
//! checks, before it sends anything, who owns what it is talking to. The decisions are pure; the OS reads (a pipe's owner
//! through `GetSecurityInfo`, a folder's `fstat`, the socket's peer uid) are thin and live in `endpoint`.

use std::fmt;

/// The SID of the LocalSystem account: the only acceptable owner of a system pipe.
pub const SYSTEM_SID: &str = "S-1-5-18";

/// Why a server is not trusted. Each variant is a refusal: the caller treats the service as missing and logs this text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Untrusted {
    /// Windows: the pipe's owner is not SYSTEM (a squatter, or a server whose descriptor lacks `O:SY`).
    PipeOwner {
        /// The owner's SID.
        owner: String,
    },
    /// Unix: the socket's folder is not owned by the account the server runs as (root).
    FolderOwner {
        /// The folder's owner uid.
        uid: u32,
    },
    /// Unix: the socket's folder can be changed by someone else (a group or world write bit).
    FolderWritable {
        /// The folder's permission bits.
        mode: u32,
    },
    /// Unix: the process holding the socket does not run as the expected account.
    ServerAccount {
        /// The listener's uid.
        uid: u32,
    },
    /// A fact needed for the check could not be read; the server is not trusted.
    Unreadable {
        /// What was being read.
        what: &'static str,
        /// The OS error text.
        cause: String,
    },
}

impl fmt::Display for Untrusted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Untrusted::PipeOwner { owner } => write!(
                f,
                "something other than the expected service answered on its pipe (owner {owner}, expected {SYSTEM_SID})"
            ),
            Untrusted::FolderOwner { uid } => write!(
                f,
                "the socket's folder is owned by uid {uid}, not by the service's account"
            ),
            Untrusted::FolderWritable { mode } => write!(
                f,
                "the socket's folder (mode {mode:04o}) can be changed by other accounts"
            ),
            Untrusted::ServerAccount { uid } => write!(
                f,
                "the socket is served by uid {uid}, not by the service's account"
            ),
            Untrusted::Unreadable { what, cause } => {
                write!(f, "the server could not be checked ({what}: {cause})")
            }
        }
    }
}

impl std::error::Error for Untrusted {}

impl Untrusted {
    /// A system endpoint that could not be created: the OS error, by name and number (231 on Windows means the name is
    /// taken, which is what a squatter that got there first looks like).
    pub(crate) fn create_failed(e: std::io::Error) -> Untrusted {
        Untrusted::Unreadable {
            what: "create the endpoint",
            cause: format!(
                "{e} ({:?} os error {})",
                e.kind(),
                e.raw_os_error().unwrap_or(0)
            ),
        }
    }
}

/// The pipe owner must be SYSTEM, compared as the SID string the OS returns (case of the `S` is fixed by the OS).
pub fn check_pipe_owner(owner_sid: &str) -> Result<(), Untrusted> {
    if owner_sid.eq_ignore_ascii_case(SYSTEM_SID) {
        Ok(())
    } else {
        Err(Untrusted::PipeOwner {
            owner: owner_sid.to_string(),
        })
    }
}

/// The socket's folder must belong to `expected_uid` (root for a system service) and have no group or world write bit. The
/// decision is the family's one folder-trust rule, `paddock_env::decide`.
pub fn check_socket_folder(owner_uid: u32, mode: u32, expected_uid: u32) -> Result<(), Untrusted> {
    paddock_env::decide(
        paddock_env::FolderFacts {
            uid: owner_uid,
            mode,
        },
        expected_uid,
    )
    .map_err(Untrusted::from)
}

impl From<paddock_env::Refusal> for Untrusted {
    fn from(r: paddock_env::Refusal) -> Untrusted {
        use paddock_env::Refusal;
        match r {
            Refusal::OtherOwner { uid } => Untrusted::FolderOwner { uid },
            Refusal::OpenToOthers { mode } => Untrusted::FolderWritable {
                mode: mode & 0o7777,
            },
            Refusal::Unreadable { what, cause } => Untrusted::Unreadable { what, cause },
            Refusal::Symlink => Untrusted::Unreadable {
                what: "open the socket's folder",
                cause: "it is a symbolic link".into(),
            },
            Refusal::NotAFolder => Untrusted::Unreadable {
                what: "open the socket's folder",
                cause: "it is not a folder".into(),
            },
        }
    }
}

/// The account holding the listening socket must be `expected_uid`.
pub fn check_server_account(uid: u32, expected_uid: u32) -> Result<(), Untrusted> {
    if uid == expected_uid {
        Ok(())
    } else {
        Err(Untrusted::ServerAccount { uid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_system_may_own_the_pipe() {
        assert!(check_pipe_owner("S-1-5-18").is_ok());
        assert!(check_pipe_owner("s-1-5-18").is_ok());
        for squatter in ["S-1-5-32-544", "S-1-5-21-1-2-3-1001", "S-1-5-180", ""] {
            let e = check_pipe_owner(squatter).unwrap_err();
            assert_eq!(
                e,
                Untrusted::PipeOwner {
                    owner: squatter.into()
                }
            );
            assert!(
                e.to_string()
                    .contains("something other than the expected service")
            );
            assert!(e.to_string().contains(squatter));
        }
    }

    #[test]
    fn the_socket_folder_must_be_the_services_and_closed() {
        assert!(check_socket_folder(0, 0o755, 0).is_ok());
        assert!(check_socket_folder(0, 0o1755, 0).is_ok());
        assert_eq!(
            check_socket_folder(501, 0o755, 0),
            Err(Untrusted::FolderOwner { uid: 501 })
        );
        assert_eq!(
            check_socket_folder(0, 0o775, 0),
            Err(Untrusted::FolderWritable { mode: 0o775 })
        );
        assert_eq!(
            check_socket_folder(0, 0o757, 0),
            Err(Untrusted::FolderWritable { mode: 0o757 })
        );
        assert!(
            check_socket_folder(0, 0o777, 0)
                .unwrap_err()
                .to_string()
                .contains("0777")
        );
        assert!(
            check_socket_folder(5, 0o700, 0)
                .unwrap_err()
                .to_string()
                .contains("uid 5")
        );
    }

    #[test]
    fn a_failed_create_names_the_os_error() {
        let e = Untrusted::create_failed(std::io::Error::from_raw_os_error(17));
        assert!(
            e.to_string().contains("create the endpoint") && e.to_string().contains("os error 17"),
            "{e}"
        );
        let e = Untrusted::create_failed(std::io::Error::other("no code"));
        assert!(e.to_string().contains("os error 0"), "{e}");
    }

    #[test]
    fn the_listener_must_run_as_the_services_account() {
        assert!(check_server_account(0, 0).is_ok());
        let e = check_server_account(501, 0).unwrap_err();
        assert!(e.to_string().contains("uid 501"));
        let u = Untrusted::Unreadable {
            what: "GetSecurityInfo",
            cause: "denied".into(),
        };
        assert!(u.to_string().contains("GetSecurityInfo") && u.to_string().contains("denied"));
    }
}
