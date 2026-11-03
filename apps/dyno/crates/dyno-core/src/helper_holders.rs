//! The helper's `SocketOwners` and `FileHolders` answers (ADR 0027 WP-10, Linux): which program holds a socket or a file. The
//! kernel shows `/proc/<pid>/fd` of other accounts only to root, so this is the helper's reading. Scoped like the process list
//! ([`crate::helper_processes`]): the caller's own, system and service programs always; other accounts' by the policy
//! (in detail, as a count, or not at all). The helper never opens the file it is asked about.

use crate::helper_policy::{OthersPolicy, Visibility};
use crate::helper_processes::{OwnerKind, RawOwner};
use crate::helper_wire::HelperUser;
use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};

/// The most sockets one answer carries.
pub const MAX_SOCKETS: usize = 65_536;
/// The most processes one sweep looks through.
pub const MAX_SWEEP: usize = 16_384;
/// The most holders of one file.
pub const MAX_HOLDERS: usize = 256;

/// A program as an answer names it (pid plus start time identifies it across time, rule 53).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holder {
    /// Process id.
    pub pid: u32,
    /// Start time, seconds since 1970.
    pub started: Option<i64>,
    /// Program name.
    pub name: String,
    /// Whose it is.
    pub owner: OwnerKind,
}

/// One socket and the program that holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocketOwner {
    /// The socket's inode, as `/proc/net/*` lists it.
    pub inode: u64,
    /// The program holding it.
    pub holder: Holder,
}

/// The answer to `SocketOwners`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocketOwners {
    /// The sockets of programs the caller may see.
    pub owners: Vec<SocketOwner>,
    /// How many programs of other accounts hold sockets that are not listed (as a count only when the policy says total).
    pub others: u32,
    /// The sweep or the list was cut at its limit.
    pub truncated: bool,
}

/// The answer to `FileHolders`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHolders {
    /// The programs holding the file that the caller may see.
    pub holders: Vec<Holder>,
    /// How many programs of other accounts hold it, not listed.
    pub others: u32,
    /// The sweep was cut at its limit.
    pub truncated: bool,
}

/// What the OS read for one program with sockets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHolder {
    /// Process id.
    pub pid: u32,
    /// Start time.
    pub started: Option<i64>,
    /// Name.
    pub name: String,
    /// Owner.
    pub owner: RawOwner,
    /// The socket inodes it holds (empty for a file holder).
    pub sockets: Vec<u64>,
}

/// What one scan found.
#[derive(Debug, Clone, Default)]
pub struct HolderScan {
    /// The programs.
    pub holders: Vec<RawHolder>,
    /// The scan stopped at a limit.
    pub truncated: bool,
}

/// The helper's reader of who holds what.
pub trait HolderSource: Send + Sync {
    /// Every program that holds a socket.
    fn sockets(&self) -> Result<HolderScan, PlatformError>;
    /// Every program that holds the file `dev`/`ino` (open descriptor or lock); never opens the file.
    fn file(&self, dev: u64, ino: u64) -> Result<HolderScan, PlatformError>;
}

/// What a system without this reading says.
pub struct NoHolders;

impl HolderSource for NoHolders {
    fn sockets(&self) -> Result<HolderScan, PlatformError> {
        Err(PlatformError::Unsupported(
            "Socket owners are read through the helper on Linux only.".into(),
        ))
    }
    fn file(&self, _: u64, _: u64) -> Result<HolderScan, PlatformError> {
        Err(PlatformError::Unsupported(
            "File holders are read through the helper on Linux only.".into(),
        ))
    }
}

enum Seen {
    Show(OwnerKind),
    Count,
    Hide,
}

fn seen(owner: &RawOwner, caller: &HelperUser, scope: Visibility) -> Seen {
    match owner {
        RawOwner::System => Seen::Show(OwnerKind::System),
        RawOwner::Service(_, Some(id)) if id == caller => Seen::Show(OwnerKind::Caller),
        RawOwner::Service(n, _) => Seen::Show(OwnerKind::Service(n.clone())),
        RawOwner::Account { id, .. } if id == caller => Seen::Show(OwnerKind::Caller),
        RawOwner::Account { .. } => match scope {
            Visibility::Full | Visibility::Own(OthersPolicy::Detail) => Seen::Show(OwnerKind::OtherUser),
            Visibility::Own(OthersPolicy::Total) => Seen::Count,
            Visibility::Own(OthersPolicy::Hidden) => Seen::Hide,
        },
    }
}

fn holder(h: &RawHolder, owner: OwnerKind) -> Holder {
    Holder {
        pid: h.pid,
        started: h.started,
        name: h.name.chars().take(crate::helper_processes::MAX_NAME).collect(),
        owner,
    }
}

/// The socket answer for `caller`: each socket once per holding program, in inode then pid order.
pub fn socket_answer(scan: &HolderScan, caller: &HelperUser, scope: Visibility) -> SocketOwners {
    let mut out = SocketOwners {
        truncated: scan.truncated,
        ..SocketOwners::default()
    };
    for h in scan.holders.iter().take(MAX_SWEEP) {
        match seen(&h.owner, caller, scope) {
            Seen::Show(kind) => {
                for inode in &h.sockets {
                    if out.owners.len() >= MAX_SOCKETS {
                        out.truncated = true;
                        break;
                    }
                    out.owners.push(SocketOwner {
                        inode: *inode,
                        holder: holder(h, kind.clone()),
                    });
                }
            }
            Seen::Count if !h.sockets.is_empty() => out.others += 1,
            _ => {}
        }
    }
    out.owners.sort_by_key(|o| (o.inode, o.holder.pid));
    out
}

/// The file answer for `caller`: holders in pid order, at most [`MAX_HOLDERS`].
pub fn file_answer(scan: &HolderScan, caller: &HelperUser, scope: Visibility) -> FileHolders {
    let mut out = FileHolders {
        truncated: scan.truncated,
        ..FileHolders::default()
    };
    for h in scan.holders.iter().take(MAX_SWEEP) {
        match seen(&h.owner, caller, scope) {
            Seen::Show(kind) if out.holders.len() < MAX_HOLDERS => out.holders.push(holder(h, kind)),
            Seen::Show(_) => out.truncated = true,
            Seen::Count => out.others += 1,
            Seen::Hide => {}
        }
    }
    out.holders.sort_by_key(|h| h.pid);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(pid: u32, owner: RawOwner, sockets: &[u64]) -> RawHolder {
        RawHolder {
            pid,
            started: Some(5),
            name: format!("p{pid}"),
            owner,
            sockets: sockets.to_vec(),
        }
    }

    fn scan() -> HolderScan {
        let acct = |uid: u32| RawOwner::Account {
            id: HelperUser::Uid(uid),
            name: None,
        };
        HolderScan {
            holders: vec![
                raw(30, acct(1002), &[300]),
                raw(10, acct(1001), &[100, 101]),
                raw(20, RawOwner::System, &[200]),
                raw(40, acct(1002), &[]),
            ],
            truncated: false,
        }
    }

    #[test]
    fn sockets_are_scoped_to_the_caller_the_system_and_what_the_policy_shows() {
        let me = HelperUser::Uid(1001);
        let a = socket_answer(&scan(), &me, Visibility::Own(OthersPolicy::Total));
        assert_eq!(
            a.owners.iter().map(|o| (o.inode, o.holder.pid)).collect::<Vec<_>>(),
            [(100, 10), (101, 10), (200, 20)]
        );
        assert_eq!(a.others, 1, "a program without sockets is not counted");
        assert_eq!(socket_answer(&scan(), &me, Visibility::Own(OthersPolicy::Hidden)).others, 0);
        let all = socket_answer(&scan(), &me, Visibility::Full);
        assert_eq!((all.owners.len(), all.others), (4, 0));
        assert_eq!(all.owners[3].holder.owner, OwnerKind::OtherUser);
    }

    #[test]
    fn file_holders_are_scoped_sorted_and_capped() {
        let me = HelperUser::Uid(1001);
        let a = file_answer(&scan(), &me, Visibility::Own(OthersPolicy::Total));
        assert_eq!((a.holders.iter().map(|h| h.pid).collect::<Vec<_>>(), a.others), (vec![10, 20], 2));
        let many = HolderScan {
            holders: (0..300).map(|p| raw(p, RawOwner::System, &[])).collect(),
            truncated: false,
        };
        let capped = file_answer(&many, &me, Visibility::Full);
        assert_eq!((capped.holders.len(), capped.truncated), (MAX_HOLDERS, true));
    }

    #[test]
    fn the_answers_cross_the_wire() {
        let a = socket_answer(&scan(), &HelperUser::Uid(1001), Visibility::Full);
        let back: SocketOwners = postcard::from_bytes(&postcard::to_stdvec(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }
}
