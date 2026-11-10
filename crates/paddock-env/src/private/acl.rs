//! The judgement of a Windows owner and access list, pure so it is tested on every OS (the Windows adapter in `windows.rs` maps
//! SIDs and ACEs to these values).
//!
//! A private folder or file is trusted when its owner is the account running the app and no allow entry names anyone but the
//! account, SYSTEM and Administrators. A folder's parent is trusted when nobody else may delete entries in it or change its owner
//! or access list (Dyno ADR 0022 section 2.1: another account could rename the private folder away and put its own in place).
#![cfg_attr(not(windows), allow(dead_code))]

use crate::folder::Refusal;

/// Who an entry or owner names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Who {
    /// The account running the app.
    Me,
    /// `SYSTEM`.
    System,
    /// `BUILTIN\Administrators`.
    Admins,
    /// `OWNER RIGHTS`: the object's owner, who is checked separately.
    OwnerRights,
    /// Anyone else, by SID text.
    Other(String),
}

impl Who {
    fn trusted(&self) -> bool {
        !matches!(self, Who::Other(_))
    }

    fn text(&self) -> String {
        match self {
            Who::Me => "this account".to_owned(),
            Who::System => "SYSTEM".to_owned(),
            Who::Admins => "Administrators".to_owned(),
            Who::OwnerRights => "OWNER RIGHTS".to_owned(),
            Who::Other(sid) => sid.clone(),
        }
    }
}

/// The kind of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AceKind {
    /// `ACCESS_ALLOWED_ACE_TYPE`.
    Allow,
    /// Another allow type (compound, object, callback): never on an object of ours, so treated as letting others in.
    OtherAllow,
    /// A deny or audit entry: it only takes access away.
    Deny,
}

/// One entry of a DACL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ace {
    pub(super) kind: AceKind,
    pub(super) who: Who,
    pub(super) mask: u32,
    /// Applies to children only (`INHERIT_ONLY_ACE`), not to this object.
    pub(super) inherit_only: bool,
}

pub(super) const ADD_FILE: u32 = 0x0002;
pub(super) const ADD_SUBDIRECTORY: u32 = 0x0004;
pub(super) const DELETE_CHILD: u32 = 0x0040;
pub(super) const DELETE: u32 = 0x0001_0000;
pub(super) const WRITE_DAC: u32 = 0x0004_0000;
pub(super) const WRITE_OWNER: u32 = 0x0008_0000;
pub(super) const GENERIC_ALL: u32 = 0x1000_0000;
pub(super) const GENERIC_WRITE: u32 = 0x4000_0000;

/// The rights that let someone put a file into the folder, swap one, or change who may.
const FOLDER_WRITE: u32 = ADD_FILE
    | ADD_SUBDIRECTORY
    | DELETE_CHILD
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_ALL
    | GENERIC_WRITE;
/// The rights on a parent that let someone rename or replace entries in it.
const PARENT_WRITE: u32 = DELETE_CHILD | WRITE_DAC | WRITE_OWNER | GENERIC_ALL | GENERIC_WRITE;

fn owner_check(owner: &Who) -> Result<(), Refusal> {
    match owner {
        Who::Me => Ok(()),
        other => Err(Refusal::OtherOwnerAccount { who: other.text() }),
    }
}

/// The first allow entry that names someone outside the account, SYSTEM and Administrators and carries a right in `rights`
/// (`u32::MAX`: any right). Entries for children only count when `children` is set: they decide what an object
/// created there would inherit.
fn intruder(aces: &[Ace], rights: u32, children: bool) -> Option<String> {
    aces.iter()
        .filter(|a| children || !a.inherit_only)
        .find_map(|a| match a.kind {
            AceKind::Allow if !a.who.trusted() && a.mask & rights != 0 => Some(a.who.text()),
            AceKind::OtherAllow => Some(a.who.text()),
            _ => None,
        })
}

/// Whether a folder with this owner and DACL is private: the account owns it and no one else may create, delete or change
/// entries in it or its access list. Read and list rights for others show no value and pass; the files inside are owner-only.
pub(super) fn folder_decision(owner: &Who, aces: &[Ace]) -> Result<(), Refusal> {
    owner_check(owner)?;
    match intruder(aces, FOLDER_WRITE, true) {
        Some(who) => Err(Refusal::OpenToOthersAcl { who }),
        None => Ok(()),
    }
}

/// Whether a file with this owner and DACL is private: the account owns it and no allow entry names anyone else with any right.
/// A NULL DACL (everyone) is passed as one allow entry for `Other` with every right.
pub(super) fn file_decision(owner: &Who, aces: &[Ace]) -> Result<(), Refusal> {
    owner_check(owner)?;
    match intruder(aces, u32::MAX, false) {
        Some(who) => Err(Refusal::OpenToOthersAcl { who }),
        None => Ok(()),
    }
}

/// Whether a folder's parent cannot be used to rename or replace the folder: Administrators, SYSTEM or the account own it and no
/// one else may delete entries in it or change its owner or access list.
pub(super) fn parent_decision(owner: &Who, aces: &[Ace]) -> Result<(), Refusal> {
    let inner = if owner.trusted() && !matches!(owner, Who::OwnerRights) {
        intruder(aces, PARENT_WRITE, false).map(|who| Refusal::OpenToOthersAcl { who })
    } else {
        Some(Refusal::OtherOwnerAccount { who: owner.text() })
    };
    match inner {
        Some(r) => Err(Refusal::Parent(Box::new(r))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: u32 = 0x001f_01ff;
    const READ: u32 = 0x0012_0089;
    const EVERYONE: &str = "S-1-1-0";

    const fn allow(who: Who, mask: u32) -> Ace {
        Ace {
            kind: AceKind::Allow,
            who,
            mask,
            inherit_only: false,
        }
    }
    fn other() -> Who {
        Who::Other(EVERYONE.to_owned())
    }
    fn open_acl(r: Result<(), Refusal>) -> bool {
        matches!(r, Err(Refusal::OpenToOthersAcl { .. }))
    }
    fn mine() -> Vec<Ace> {
        vec![allow(Who::Me, FULL), allow(Who::System, FULL)]
    }

    #[test]
    fn a_folder_with_only_the_account_and_system_is_private() {
        assert_eq!(folder_decision(&Who::Me, &mine()), Ok(()));
        let mut with_admins = mine();
        with_admins.push(allow(Who::Admins, FULL));
        with_admins.push(allow(Who::OwnerRights, FULL));
        assert_eq!(folder_decision(&Who::Me, &with_admins), Ok(()));
        assert_eq!(
            folder_decision(&Who::Me, &[]),
            Ok(()),
            "an empty DACL lets nobody in"
        );
    }

    /// Failing test 1 (ADR 0092): `Everyone` write on the folder passes today's check; it must be refused with who named.
    #[test]
    fn a_folder_everyone_can_write_in_is_refused_naming_who() {
        let mut aces = mine();
        aces.push(allow(other(), FULL));
        assert_eq!(
            folder_decision(&Who::Me, &aces),
            Err(Refusal::OpenToOthersAcl {
                who: EVERYONE.to_owned()
            })
        );
        for write in [
            ADD_FILE,
            ADD_SUBDIRECTORY,
            DELETE_CHILD,
            DELETE,
            WRITE_DAC,
            WRITE_OWNER,
            GENERIC_WRITE,
            GENERIC_ALL,
            0x0013_01bf,
        ] {
            let aces = [allow(Who::Me, FULL), allow(other(), write)];
            assert!(open_acl(folder_decision(&Who::Me, &aces)), "{write:#x}");
        }
    }

    #[test]
    fn a_folder_others_can_only_list_or_deny_is_not_refused_but_children_entries_count() {
        assert_eq!(
            folder_decision(&Who::Me, &[allow(Who::Me, FULL), allow(other(), READ)]),
            Ok(())
        );
        let deny = Ace {
            kind: AceKind::Deny,
            who: other(),
            mask: FULL,
            inherit_only: false,
        };
        assert_eq!(
            folder_decision(&Who::Me, &[allow(Who::Me, FULL), deny]),
            Ok(())
        );
        let children = Ace {
            inherit_only: true,
            ..allow(other(), FULL)
        };
        let seen = folder_decision(&Who::Me, &[allow(Who::Me, FULL), children]);
        assert!(open_acl(seen), "what a file made there would inherit");
        let object = Ace {
            kind: AceKind::OtherAllow,
            who: other(),
            mask: READ,
            inherit_only: false,
        };
        assert!(matches!(
            folder_decision(&Who::Me, &[object]),
            Err(Refusal::OpenToOthersAcl { .. })
        ));
    }

    #[test]
    fn an_owner_other_than_the_account_is_refused() {
        for owner in [other(), Who::Admins, Who::System, Who::OwnerRights] {
            assert!(matches!(
                folder_decision(&owner, &mine()),
                Err(Refusal::OtherOwnerAccount { .. })
            ));
            assert!(matches!(
                file_decision(&owner, &mine()),
                Err(Refusal::OtherOwnerAccount { .. })
            ));
        }
        let planted = [allow(other(), FULL), allow(Who::Me, READ)];
        assert_eq!(
            file_decision(&other(), &planted),
            Err(Refusal::OtherOwnerAccount {
                who: EVERYONE.to_owned()
            }),
            "a key planted by another user and readable by us"
        );
    }

    #[test]
    fn a_file_is_private_only_without_any_right_for_others() {
        assert_eq!(file_decision(&Who::Me, &mine()), Ok(()));
        assert!(matches!(
            file_decision(&Who::Me, &[allow(Who::Me, FULL), allow(other(), READ)]),
            Err(Refusal::OpenToOthersAcl { .. })
        ));
        let children = Ace {
            inherit_only: true,
            ..allow(other(), FULL)
        };
        let deny = Ace {
            kind: AceKind::Deny,
            who: other(),
            mask: FULL,
            inherit_only: false,
        };
        assert_eq!(
            file_decision(&Who::Me, &[allow(Who::Me, FULL), children, deny]),
            Ok(()),
            "children-only and deny take nothing"
        );
        let object = Ace {
            kind: AceKind::OtherAllow,
            who: Who::Me,
            mask: READ,
            inherit_only: false,
        };
        assert!(matches!(
            file_decision(&Who::Me, &[object]),
            Err(Refusal::OpenToOthersAcl { .. })
        ));
        assert!(
            matches!(file_decision(&Who::Me, &[allow(other(), 0)]), Ok(())),
            "an entry with no right lets nobody in"
        );
    }

    #[test]
    fn a_parent_where_others_can_delete_entries_or_change_its_acl_is_refused() {
        let parent = |aces: &[Ace]| parent_decision(&Who::Me, aces);
        assert_eq!(parent(&mine()), Ok(()));
        assert_eq!(
            parent(&[allow(Who::Me, FULL), allow(other(), READ)]),
            Ok(()),
            "reading is fine"
        );
        assert_eq!(
            parent(&[
                allow(Who::Me, FULL),
                allow(other(), ADD_FILE | ADD_SUBDIRECTORY)
            ]),
            Ok(()),
            "creating alone cannot replace ours"
        );
        for right in [
            DELETE_CHILD,
            WRITE_DAC,
            WRITE_OWNER,
            GENERIC_WRITE,
            GENERIC_ALL,
        ] {
            let r = parent(&[allow(Who::Me, FULL), allow(other(), right)]).unwrap_err();
            assert!(
                matches!(&r, Refusal::Parent(inner) if matches!(**inner, Refusal::OpenToOthersAcl { .. })),
                "{right:#x}"
            );
        }
        let children = Ace {
            inherit_only: true,
            ..allow(other(), FULL)
        };
        assert_eq!(
            parent(&[allow(Who::Me, FULL), children]),
            Ok(()),
            "children-only does not touch the parent itself"
        );
        for owner in [Who::Me, Who::System, Who::Admins] {
            assert_eq!(parent_decision(&owner, &mine()), Ok(()));
        }
        for owner in [other(), Who::OwnerRights] {
            assert!(matches!(
                parent_decision(&owner, &mine()),
                Err(Refusal::Parent(_))
            ));
        }
    }

    #[test]
    fn every_who_has_a_text() {
        for who in [Who::Me, Who::System, Who::Admins, Who::OwnerRights, other()] {
            assert!(!who.text().is_empty());
        }
    }
}
