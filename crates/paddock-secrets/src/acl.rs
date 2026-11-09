//! The judgement of a Windows access list on a secret file or its folder, pure so it is tested on every OS (the Windows
//! adapter in `file_windows.rs` maps SIDs and ACEs to these values).
//!
//! A file is trusted when its owner is the user running the app, SYSTEM or Administrators, and no allow entry names anyone
//! else. A folder is trusted when its owner is one of those and nobody else may create, delete or change entries in it or
//! its access list. Read and list rights for others on the folder are fine: the file itself is owner-only.
#![cfg_attr(not(windows), allow(dead_code))]

/// Who an entry or owner names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Who {
    /// The user running the app.
    Me,
    /// `SYSTEM`.
    System,
    /// `BUILTIN\Administrators`.
    Admins,
    /// `OWNER RIGHTS`: the object's owner, who is checked separately.
    OwnerRights,
    /// Anyone else.
    Other,
}

/// The kind of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AceKind {
    /// `ACCESS_ALLOWED_ACE_TYPE`.
    Allow,
    /// Another allow type (compound, object, callback): never on a file of ours, so treated as letting others in.
    OtherAllow,
    /// A deny or audit entry: it only takes access away.
    Deny,
}

/// One entry of a DACL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ace {
    pub(crate) kind: AceKind,
    pub(crate) who: Who,
    pub(crate) mask: u32,
    /// Applies to children only (`INHERIT_ONLY_ACE`), not to this object.
    pub(crate) inherit_only: bool,
}

/// Why a file or folder is not trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Owned by another account.
    ForeignOwner,
    /// Others may open the file.
    Shared,
    /// Others may create, delete or change entries in the folder.
    OthersWrite,
}

impl Refusal {
    /// The reason as a sentence part, after the path.
    pub(crate) fn why(self) -> &'static str {
        match self {
            Refusal::ForeignOwner => {
                "is owned by another user, so it is not trusted; remove it and store the secret again"
            }
            Refusal::Shared => "can be opened by other users; remove it and store the secret again",
            Refusal::OthersWrite => {
                "lets other users create or change files in it, so secrets are not stored there; use a folder only you can write"
            }
        }
    }
}

/// `FILE_GENERIC_READ`: what an entry that only lets someone read grants (used by the tests).
#[cfg(test)]
pub(crate) const READ: u32 = 0x0012_0089;
pub(crate) const FULL: u32 = 0x001f_01ff;
pub(crate) const ADD_FILE: u32 = 0x0002;
pub(crate) const ADD_SUBDIRECTORY: u32 = 0x0004;
pub(crate) const DELETE_CHILD: u32 = 0x0040;
pub(crate) const DELETE: u32 = 0x0001_0000;
pub(crate) const WRITE_DAC: u32 = 0x0004_0000;
pub(crate) const WRITE_OWNER: u32 = 0x0008_0000;
pub(crate) const GENERIC_ALL: u32 = 0x1000_0000;
pub(crate) const GENERIC_WRITE: u32 = 0x4000_0000;
/// The rights that let someone put a file into a folder, swap one, or change who may.
const FOLDER_WRITE: u32 = ADD_FILE
    | ADD_SUBDIRECTORY
    | DELETE_CHILD
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_ALL
    | GENERIC_WRITE;

fn trusted_owner(owner: Who) -> bool {
    matches!(owner, Who::Me | Who::System | Who::Admins)
}

fn trusted(who: Who) -> bool {
    matches!(who, Who::Me | Who::System | Who::Admins | Who::OwnerRights)
}

/// Whether a secret file with this owner and DACL is trusted. A NULL DACL (everyone) is passed as one allow entry for
/// `Other` with every right.
pub(crate) fn file_refusal(owner: Who, aces: &[Ace]) -> Option<Refusal> {
    if !trusted_owner(owner) {
        return Some(Refusal::ForeignOwner);
    }
    let shared = aces
        .iter()
        .filter(|a| !a.inherit_only)
        .any(|a| match a.kind {
            AceKind::Allow => !trusted(a.who),
            AceKind::OtherAllow => true,
            AceKind::Deny => false,
        });
    shared.then_some(Refusal::Shared)
}

/// Whether a folder with this owner and DACL may hold secrets. Entries for children only are counted too: they decide
/// what a file created there would inherit before its own DACL is set.
pub(crate) fn folder_refusal(owner: Who, aces: &[Ace]) -> Option<Refusal> {
    if !trusted_owner(owner) {
        return Some(Refusal::ForeignOwner);
    }
    let writable = aces.iter().any(|a| match a.kind {
        AceKind::Allow => !trusted(a.who) && a.mask & FOLDER_WRITE != 0,
        AceKind::OtherAllow => true,
        AceKind::Deny => false,
    });
    writable.then_some(Refusal::OthersWrite)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: Ace = Ace {
        kind: AceKind::Allow,
        who: Who::Me,
        mask: FULL,
        inherit_only: false,
    };
    const fn allow(who: Who, mask: u32) -> Ace {
        Ace {
            kind: AceKind::Allow,
            who,
            mask,
            inherit_only: false,
        }
    }

    #[test]
    fn a_file_of_ours_shared_only_with_system_and_administrators_is_trusted() {
        assert_eq!(file_refusal(Who::Me, &[ME]), None);
        assert_eq!(
            file_refusal(
                Who::Me,
                &[ME, allow(Who::System, FULL), allow(Who::Admins, FULL)]
            ),
            None
        );
        assert_eq!(
            file_refusal(Who::Admins, &[ME]),
            None,
            "an elevated write makes Administrators the owner"
        );
        assert_eq!(
            file_refusal(Who::Me, &[allow(Who::OwnerRights, FULL)]),
            None
        );
        // A deny entry or an entry for children only takes nothing away from the owner-only rule.
        let deny = Ace {
            kind: AceKind::Deny,
            who: Who::Other,
            mask: FULL,
            inherit_only: false,
        };
        let children = Ace {
            inherit_only: true,
            ..allow(Who::Other, FULL)
        };
        assert_eq!(file_refusal(Who::Me, &[ME, deny, children]), None);
    }

    /// Review P1-2: a key planted by another user, owned by them and readable by us, is not trusted.
    #[test]
    fn a_key_planted_by_another_user_is_refused() {
        let planted = [allow(Who::Other, FULL), allow(Who::Me, READ)];
        assert_eq!(
            file_refusal(Who::Other, &planted),
            Some(Refusal::ForeignOwner)
        );
        assert_eq!(
            file_refusal(Who::Other, &[ME]),
            Some(Refusal::ForeignOwner),
            "even with only our entry"
        );
    }

    #[test]
    fn a_file_others_can_open_is_refused() {
        assert_eq!(
            file_refusal(Who::Me, &[ME, allow(Who::Other, READ)]),
            Some(Refusal::Shared)
        );
        let object = Ace {
            kind: AceKind::OtherAllow,
            who: Who::Me,
            mask: READ,
            inherit_only: false,
        };
        assert_eq!(
            file_refusal(Who::Me, &[ME, object]),
            Some(Refusal::Shared),
            "an allow type a file of ours never has"
        );
        assert_eq!(
            file_refusal(Who::Me, &[]),
            None,
            "an empty DACL lets nobody in"
        );
    }

    /// Review P1-1: a folder where others can create or change files (a second NTFS drive gives Authenticated Users
    /// Modify) is refused, as Unix refuses a folder of another user.
    #[test]
    fn a_folder_others_can_write_in_is_refused_and_one_they_can_only_list_is_not() {
        assert_eq!(
            folder_refusal(
                Who::Me,
                &[ME, allow(Who::System, FULL), allow(Who::Admins, FULL)]
            ),
            None
        );
        assert_eq!(
            folder_refusal(Who::Me, &[ME, allow(Who::Other, READ)]),
            None,
            "listing shows no value"
        );
        for write in [
            ADD_FILE,
            ADD_SUBDIRECTORY,
            DELETE_CHILD,
            WRITE_DAC,
            WRITE_OWNER,
            GENERIC_WRITE,
            GENERIC_ALL,
            DELETE,
            0x0013_01bf,
        ] {
            assert_eq!(
                folder_refusal(Who::Me, &[ME, allow(Who::Other, write)]),
                Some(Refusal::OthersWrite),
                "{write:#x}"
            );
        }
        assert_eq!(
            folder_refusal(Who::Other, &[ME]),
            Some(Refusal::ForeignOwner)
        );
        let object = Ace {
            kind: AceKind::OtherAllow,
            who: Who::Other,
            mask: READ,
            inherit_only: false,
        };
        assert_eq!(
            folder_refusal(Who::Me, &[ME, object]),
            Some(Refusal::OthersWrite)
        );
    }

    #[test]
    fn refusals_say_what_to_do() {
        for r in [Refusal::ForeignOwner, Refusal::Shared, Refusal::OthersWrite] {
            assert!(!r.why().is_empty());
        }
    }
}
