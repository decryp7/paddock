//! The agent's session: normal rights, or administrator rights for this session only ("Run as administrator").
//!
//! An elevated agent is started by the person's own click after the OS prompt, keeps its data in its own folder, never
//! registers to start at login and ends when Dyno quits. These are the pure decisions around it: who may talk to the agent,
//! whether an agent's discovery file may be used by this user, and whether other users' processes are folded into one row.

use serde::{Deserialize, Serialize};

/// What the running agent was started as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// The agent runs with administrator rights.
    pub elevated: bool,
    /// The only Unix user id allowed to connect (the logged-in user's, also when the agent runs as root); `None` where the
    /// OS gives no peer id (Windows: the token and the private endpoint are the check).
    pub allowed_uid: Option<u32>,
    /// The logged-in user's Unix group id: files an elevated agent creates are handed to `allowed_uid:owner_gid`.
    pub owner_gid: Option<u32>,
    /// The key of the user the session belongs to (see [`user_key`]); written to the discovery file of an elevated agent.
    pub for_user: Option<String>,
}

impl Session {
    /// A normal agent of the user `uid` (`None` where the OS has no numeric user id).
    pub fn normal(allowed_uid: Option<u32>) -> Session {
        Session {
            elevated: false,
            allowed_uid,
            owner_gid: None,
            for_user: None,
        }
    }

    /// An elevated agent for the user `for_user` (whose Unix id is `allowed_uid`).
    pub fn elevated(for_user: String, allowed_uid: Option<u32>) -> Session {
        Session {
            elevated: true,
            allowed_uid,
            owner_gid: None,
            for_user: Some(for_user),
        }
    }

    /// The same session, handing the files it creates to this group.
    pub fn with_gid(mut self, gid: Option<u32>) -> Session {
        self.owner_gid = gid;
        self
    }

    /// The `(uid, gid)` the files of an elevated agent belong to, when the session has both.
    pub fn file_owner(&self) -> Option<(u32, u32)> {
        match (self.elevated, self.allowed_uid, self.owner_gid) {
            (true, Some(u), Some(g)) => Some((u, g)),
            _ => None,
        }
    }

    /// Whether other users' processes are folded into one "Other users" row. While elevated every process is shown in full.
    pub fn group_other_users(&self) -> bool {
        !self.elevated
    }

    /// Whether a connection from the Unix user `peer_uid` is accepted. Without a restriction everyone who has the token is;
    /// with one, a connection whose user the OS did not report is refused.
    pub fn caller_allowed(&self, peer_uid: Option<u32>) -> bool {
        match self.allowed_uid {
            None => true,
            Some(allowed) => peer_uid == Some(allowed),
        }
    }
}

/// The key that names a user in a discovery file: the numeric user id on Unix, the account name on Windows.
pub fn user_key(unix_uid: Option<u32>, windows_user: Option<&str>) -> String {
    match (unix_uid, windows_user) {
        (Some(uid), _) => format!("uid:{uid}"),
        (None, Some(name)) => format!("user:{}", name.to_lowercase()),
        (None, None) => "unknown".to_string(),
    }
}

/// Whether the agent a discovery file describes may be used by `current_user`: a normal agent always (the private folder
/// is the user's own); an elevated one only for the user it was started for, so another user's elevated agent is never
/// reused.
pub fn discovery_usable(elevated_for: Option<&str>, current_user: &str) -> bool {
    elevated_for.is_none_or(|u| u == current_user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_is_off_exactly_while_elevated() {
        assert!(Session::normal(Some(501)).group_other_users());
        assert!(!Session::elevated("uid:501".into(), Some(501)).group_other_users());
    }

    #[test]
    fn only_the_logged_in_user_may_connect_even_to_an_agent_running_as_root() {
        let s = Session::elevated("uid:501".into(), Some(501));
        assert!(s.caller_allowed(Some(501)));
        assert!(!s.caller_allowed(Some(502)));
        assert!(!s.caller_allowed(Some(0)), "root is not the logged-in user");
        assert!(!s.caller_allowed(None), "an unreported caller is refused when a user is required");
        let n = Session::normal(Some(501));
        assert!(n.caller_allowed(Some(501)) && !n.caller_allowed(Some(1)));
        let open = Session::normal(None);
        assert!(open.caller_allowed(None) && open.caller_allowed(Some(7)));
    }

    #[test]
    fn an_elevated_agent_is_never_reused_by_another_user() {
        assert!(discovery_usable(None, "uid:502"));
        assert!(discovery_usable(Some("uid:501"), "uid:501"));
        assert!(!discovery_usable(Some("uid:501"), "uid:502"));
        assert!(!discovery_usable(Some("user:alex"), "unknown"));
    }

    #[test]
    fn only_an_elevated_session_with_both_ids_hands_files_over() {
        assert_eq!(
            Session::elevated("u".into(), Some(501)).with_gid(Some(20)).file_owner(),
            Some((501, 20))
        );
        assert_eq!(Session::elevated("u".into(), Some(501)).file_owner(), None);
        assert_eq!(Session::elevated("u".into(), None).with_gid(Some(20)).file_owner(), None);
        assert_eq!(Session::normal(Some(501)).with_gid(Some(20)).file_owner(), None);
    }

    #[test]
    fn users_are_named_by_uid_or_account() {
        assert_eq!(user_key(Some(501), Some("Alex")), "uid:501");
        assert_eq!(user_key(None, Some("Alex")), "user:alex");
        assert_eq!(user_key(None, None), "unknown");
    }

    #[test]
    fn the_session_is_plain_data_on_the_wire() {
        let s = Session::elevated("uid:1".into(), Some(1));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["elevated"], true);
        assert_eq!(serde_json::from_value::<Session>(v).unwrap(), s);
    }
}
