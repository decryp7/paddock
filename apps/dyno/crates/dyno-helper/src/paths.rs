//! The helper's places (shared with the agent through `dyno_core::helper_paths`) and who may open its endpoint.

pub use dyno_core::helper_paths::*;

use paddock_ipc::Access;

/// Who may open the endpoint: any local user may connect to the socket (the daemon refuses all but the authorised
/// uid after the kernel tells it who connected); on Windows only `SYSTEM` and the authorised account's SID may open
/// the pipe at all.
pub fn endpoint_access(user: &dyno_core::helper_wire::HelperUser) -> Access {
    let sddl = match user {
        dyno_core::helper_wire::HelperUser::Sid(sid) => format!("D:P(A;;GA;;;SY)(A;;GRGW;;;{sid})"),
        // A Unix user on Windows cannot happen; the descriptor then admits SYSTEM only.
        dyno_core::helper_wire::HelperUser::Uid(_) => "D:P(A;;GA;;;SY)".to_string(),
    };
    Access { mode: 0o666, sddl }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper_wire::HelperUser;

    #[test]
    fn the_pipe_admits_system_and_the_one_account() {
        let a = endpoint_access(&HelperUser::Sid("S-1-5-21-1-2-3-1001".into()));
        assert_eq!(a.sddl, "D:P(A;;GA;;;SY)(A;;GRGW;;;S-1-5-21-1-2-3-1001)");
        assert_eq!(a.mode, 0o666);
        assert_eq!(endpoint_access(&HelperUser::Uid(5)).sddl, "D:P(A;;GA;;;SY)");
    }
}
