//! The rule over fake security descriptors built from the default ACLs Windows ships (as described by `icacls` on a stock
//! install; the VM check confirms them on a real machine) and over fake Unix stats.

use super::super::trust_fake::*;
use super::*;
use std::collections::HashMap;

fn check(p: &mut FakePort) -> Result<(), Untrusted> {
    verify_chain(p, SMARTCTL, FileKind::File, Some(TOOLS), false)
}

#[test]
fn a_tools_folder_that_inherited_programdatas_create_rights_is_refused() {
    // The finding: created by create_dir_all, the tools tree inherits Users:(CI)(WD,AD,WEA) from C:\ProgramData.
    let mut p = windows_machine(|_| inherited_from_program_data());
    let e = check(&mut p).unwrap_err();
    assert!(
        matches!(e.reason, Reason::Grants { ref sid, .. } if sid == SID_USERS),
        "{e}"
    );
    assert!(e.to_string().contains("Repair the helper"), "{e}");
}

#[test]
fn a_locked_tree_under_stock_ancestors_is_trusted() {
    // Stock C:\ and C:\ProgramData let Users create things, and the tree is still trusted: ancestors only matter for
    // DELETE, FILE_DELETE_CHILD on the parent, WRITE_DAC and WRITE_OWNER.
    let mut p = windows_machine(|_| locked());
    check(&mut p).unwrap();
}

#[test]
fn the_first_unlocked_component_is_named_even_when_the_tools_folder_is_locked() {
    // Dyno\ and Dyno\Helper are made by create_dir_all too: they are ancestors of the root, so create rights there are fine,
    // but a delete or a permissions right is not.
    let mut p = windows_machine(|d| {
        if d.ends_with("Dyno") || d.ends_with("Helper") {
            inherited_from_program_data()
        } else {
            locked()
        }
    });
    check(&mut p).unwrap();
    let mut dyno = inherited_from_program_data();
    if let Security::Windows(d) = &mut dyno.security {
        d.dacl
            .as_mut()
            .unwrap()
            .push(Ace::allow(SID_USERS, right::WRITE_DAC));
    }
    p.put("C:\\ProgramData\\Dyno", dyno);
    let e = check(&mut p).unwrap_err();
    assert_eq!(e.path, "C:\\ProgramData\\Dyno");
    assert!(
        matches!(
            e.reason,
            Reason::Grants {
                right: "WRITE_DAC",
                ..
            }
        ),
        "{e}"
    );
}

#[test]
fn an_ancestor_that_lets_users_delete_the_folder_or_its_children_is_refused() {
    let mut p = windows_machine(|_| locked());
    let mut pd = stock_program_data();
    if let Security::Windows(d) = &mut pd.security {
        d.dacl
            .as_mut()
            .unwrap()
            .push(Ace::allow(SID_USERS, right::DELETE_CHILD));
    }
    // FILE_DELETE_CHILD on ProgramData lets a user delete Dyno and plant their own.
    p.put("C:\\ProgramData", pd);
    let e = check(&mut p).unwrap_err();
    assert_eq!(e.path, "C:\\ProgramData\\Dyno");
    assert!(
        matches!(e.reason, Reason::Grants { right, .. } if right.contains("DELETE_CHILD")),
        "{e}"
    );
}

#[test]
fn inherit_only_entries_do_not_count_on_the_object_they_sit_on() {
    // Stock C:\ has Authenticated Users:(OI)(CI)(IO)(M): MODIFY includes DELETE, but only for children.
    let mut p = windows_machine(|_| locked());
    check(&mut p).unwrap();
    let Security::Windows(d) = &p.nodes["c:"].security else {
        panic!()
    };
    // Authenticated Users: (A;OICIIO;SDGXGWGR;;;AU) holds DELETE (SD) for children only.
    assert!(
        d.dacl
            .as_ref()
            .unwrap()
            .iter()
            .any(|a| a.inherit_only && a.mask & right::DELETE != 0)
    );
}

#[test]
fn a_file_a_user_can_write_or_own_is_refused_even_in_a_locked_folder() {
    let mut p = windows_machine(|_| locked());
    let mut f = file(locked());
    if let Security::Windows(d) = &mut f.security {
        d.dacl
            .as_mut()
            .unwrap()
            .push(Ace::allow(SID_AUTHENTICATED_USERS, right::WRITE_DATA));
    }
    p.put(SMARTCTL, f);
    assert!(matches!(
        check(&mut p).unwrap_err().reason,
        Reason::Grants { .. }
    ));
    let mut owned = file(locked());
    if let Security::Windows(d) = &mut owned.security {
        d.owner = "S-1-5-21-1-2-3-1001".into();
    }
    p.put(SMARTCTL, owned);
    assert!(matches!(
        check(&mut p).unwrap_err().reason,
        Reason::UntrustedOwner(_)
    ));
}

#[test]
fn a_null_dacl_everyone_and_a_reparse_point_are_refused() {
    let mut p = windows_machine(|_| locked());
    let mut null = locked();
    if let Security::Windows(d) = &mut null.security {
        d.dacl = None;
    }
    p.put(TOOLS, null);
    assert_eq!(check(&mut p).unwrap_err().reason, Reason::NullDacl);

    let mut p = windows_machine(|_| locked());
    let mut every = locked();
    if let Security::Windows(d) = &mut every.security {
        d.dacl
            .as_mut()
            .unwrap()
            .push(Ace::allow(SID_EVERYONE, right::GENERIC_ALL));
    }
    p.put(
        "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5",
        every,
    );
    assert!(matches!(
        check(&mut p).unwrap_err().reason,
        Reason::Grants { .. }
    ));

    // A junction planted as the install folder: never followed.
    let mut p = windows_machine(|_| locked());
    let mut junction = locked();
    junction.kind = FileKind::Symlink;
    p.put(
        "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5",
        junction,
    );
    assert_eq!(check(&mut p).unwrap_err().reason, Reason::Link);
}

#[test]
fn a_target_outside_the_root_or_missing_is_refused() {
    let mut p = windows_machine(|_| locked());
    let e = verify_chain(
        &mut p,
        SMARTCTL,
        FileKind::File,
        Some("C:\\Elsewhere"),
        false,
    )
    .unwrap_err();
    assert_eq!(e.reason, Reason::OutsideRoot);
    p.nodes.remove(&norm(SMARTCTL, Style::Windows));
    assert_eq!(check(&mut p).unwrap_err().reason, Reason::Missing);
    // Case does not matter on Windows.
    let mut p = windows_machine(|_| locked());
    verify_chain(
        &mut p,
        SMARTCTL,
        FileKind::File,
        Some("c:\\programdata\\dyno\\helper\\TOOLS"),
        false,
    )
    .unwrap();
}

#[test]
fn paths_split_the_way_the_system_does() {
    assert_eq!(
        parent_of("C:\\ProgramData\\Dyno", Style::Windows).as_deref(),
        Some("C:\\ProgramData")
    );
    assert_eq!(
        parent_of("C:\\ProgramData", Style::Windows).as_deref(),
        Some("C:\\")
    );
    assert_eq!(parent_of("C:\\", Style::Windows), None);
    assert_eq!(
        parent_of("C:/ProgramData/", Style::Windows).as_deref(),
        Some("C:\\")
    );
    assert_eq!(
        parent_of("/Library/Application Support", Style::Unix).as_deref(),
        Some("/Library")
    );
    assert_eq!(parent_of("/Library", Style::Unix).as_deref(), Some("/"));
    assert_eq!(parent_of("/", Style::Unix), None);
}

fn unix(owner: u32, mode: u32, kind: FileKind) -> Node {
    Node {
        kind,
        security: Security::Unix {
            owner,
            mode,
            write_acl: false,
        },
    }
}

fn unix_machine() -> FakePort {
    let mut p = FakePort {
        style: Style::Unix,
        nodes: HashMap::new(),
        links: HashMap::new(),
        calls: vec![],
    };
    for d in [
        "/",
        "/var",
        "/var/lib",
        "/var/lib/dyno-helper",
        "/var/lib/dyno-helper/tools",
    ] {
        p.put(d, unix(0, 0o755, FileKind::Dir));
    }
    p.put(
        "/var/lib/dyno-helper/tools/smartctl",
        unix(0, 0o755, FileKind::File),
    );
    p
}

const UNIX_FILE: &str = "/var/lib/dyno-helper/tools/smartctl";

#[test]
fn unix_wants_root_owned_not_group_or_other_writable_components_all_the_way_up() {
    let mut p = unix_machine();
    verify_chain(
        &mut p,
        UNIX_FILE,
        FileKind::File,
        Some("/var/lib/dyno-helper/tools"),
        false,
    )
    .unwrap();
    for (path, node, want) in [
        (
            "/var/lib/dyno-helper/tools/smartctl",
            unix(0, 0o775, FileKind::File),
            "group or others",
        ),
        (
            "/var/lib/dyno-helper/tools/smartctl",
            unix(0, 0o757, FileKind::File),
            "group or others",
        ),
        (
            "/var/lib/dyno-helper/tools/smartctl",
            unix(501, 0o755, FileKind::File),
            "user id 501",
        ),
        ("/var/lib", unix(0, 0o777, FileKind::Dir), "group or others"),
        (
            "/var/lib/dyno-helper",
            unix(501, 0o755, FileKind::Dir),
            "user id 501",
        ),
        (
            "/var/lib/dyno-helper/tools",
            unix(0, 0o755, FileKind::Symlink),
            "link",
        ),
    ] {
        let mut p = unix_machine();
        p.put(path, node);
        let e = verify_chain(
            &mut p,
            UNIX_FILE,
            FileKind::File,
            Some("/var/lib/dyno-helper/tools"),
            false,
        )
        .unwrap_err();
        assert!(e.to_string().contains(want), "{path}: {e}");
    }
}

#[test]
fn a_unix_acl_entry_that_allows_a_write_is_refused() {
    let mut p = unix_machine();
    p.put(
        "/var/lib/dyno-helper/tools",
        Node {
            kind: FileKind::Dir,
            security: Security::Unix {
                owner: 0,
                mode: 0o755,
                write_acl: true,
            },
        },
    );
    assert_eq!(
        verify_chain(&mut p, UNIX_FILE, FileKind::File, None, false)
            .unwrap_err()
            .reason,
        Reason::AclWrite
    );
    let listing = "drwxr-xr-x+ 5 root  wheel  160 Nov  4 10:00 /x\n 0: user:_foo allow list,search,readattr\n 1: group:admin allow add_file,delete\n";
    assert!(acl_allows_write(listing));
    let read_only = "drwxr-xr-x+ 5 root  wheel  160 Nov  4 10:00 /x\n 0: group:everyone deny delete\n 1: user:_foo allow list,search,readattr\n";
    assert!(!acl_allows_write(read_only));
    assert!(!acl_allows_write(
        "drwxr-xr-x  5 root  wheel  160 Nov  4 10:00 /x\n"
    ));
}

#[test]
fn a_program_the_os_ships_is_checked_after_its_links_are_resolved() {
    // Merged /usr: /usr/sbin is a link to bin. Resolved first, the chain is root's; unresolved it is refused as a link.
    let mut p = FakePort {
        style: Style::Unix,
        nodes: HashMap::new(),
        links: HashMap::from([(
            "/usr/sbin/smartctl".to_string(),
            "/usr/bin/smartctl".to_string(),
        )]),
        calls: vec![],
    };
    for d in ["/", "/usr", "/usr/bin"] {
        p.put(d, unix(0, 0o755, FileKind::Dir));
    }
    p.put("/usr/bin/smartctl", unix(0, 0o755, FileKind::File));
    p.put("/usr/sbin", unix(0, 0o777, FileKind::Symlink));
    p.put("/usr/sbin/smartctl", unix(0, 0o755, FileKind::File));
    verify_chain(&mut p, "/usr/sbin/smartctl", FileKind::File, None, true).unwrap();
    assert_eq!(
        verify_chain(&mut p, "/usr/sbin/smartctl", FileKind::File, None, false)
            .unwrap_err()
            .reason,
        Reason::Link
    );
}

#[test]
fn a_junction_in_the_requested_path_is_caught_by_the_final_path_of_the_held_handle() {
    // The requested path walks through a junction (C:\ProgramData\Dyno\Helper\tools -> C:\Users\Eve\tools) that was
    // re-pointed after the walk; the held handle's final path (what GetFinalPathNameByHandle returns) differs.
    let requested = SMARTCTL;
    let final_path = "\\\\?\\C:\\Users\\Eve\\tools\\smartmontools-7.5\\bin\\smartctl.exe";
    let e = require_same_path(requested, final_path, Style::Windows).unwrap_err();
    assert!(
        matches!(e.reason, Reason::Redirected(ref to) if to.contains("Eve")),
        "{e}"
    );
    // The same file, spelled with the \\?\ prefix, other case and slashes, is the same path.
    require_same_path(
        requested,
        &format!("\\\\?\\{}", requested.to_uppercase()),
        Style::Windows,
    )
    .unwrap();
    require_same_path(
        "/Library/Dyno/Helper/tools/x",
        "/Library/Dyno/Helper/tools/x",
        Style::Unix,
    )
    .unwrap();
    // macOS: /var is /private/var; a requested path through it differs from the resolved one and is refused.
    assert!(require_same_path("/var/db/x", "/private/var/db/x", Style::Unix).is_err());
}

#[test]
fn program_files_as_captured_on_a_real_machine_passes_for_a_program_two_levels_down() {
    // C:\Program Files grants CREATOR OWNER (OI)(CI)(IO)(GA) and TrustedInstaller (CI)(IO)(GA): children only. SYSTEM and
    // Administrators hold (M) on the folder itself, which is trusted. The installer's own folder is SDDL_PROGRAM_FILES_DYNO.
    let mut p = windows_machine(|_| locked());
    p.put(
        "C:\\Program Files",
        from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES),
    )
    .put(
        "C:\\Program Files\\x",
        from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES_DYNO),
    )
    .put(
        "C:\\Program Files\\x\\y.exe",
        from_sddl(FileKind::File, SDDL_PROGRAM_FILES_DYNO),
    );
    verify_chain(
        &mut p,
        "C:\\Program Files\\x\\y.exe",
        FileKind::File,
        Some("C:\\Program Files\\x"),
        false,
    )
    .unwrap();
    verify_chain(
        &mut p,
        "C:\\Program Files\\x\\y.exe",
        FileKind::File,
        None,
        false,
    )
    .unwrap();
}

#[test]
fn the_dyno_helper_folder_as_the_unfixed_helper_left_it_is_not_protected() {
    // Real SDDL: owner Administrators, but (A;CIID;DCLCRPCR;;;BU) inherited from ProgramData lets Users add files and
    // folders. The old start-up check said "protected=true" for this folder.
    let mut p = windows_machine(|_| locked());
    p.put(
        "C:\\ProgramData\\Dyno\\Helper",
        from_sddl(FileKind::Dir, SDDL_DYNO_HELPER_UNFIXED),
    );
    let e = verify_chain(
        &mut p,
        "C:\\ProgramData\\Dyno\\Helper",
        FileKind::Dir,
        Some("C:\\ProgramData\\Dyno\\Helper"),
        false,
    )
    .unwrap_err();
    assert!(
        matches!(e.reason, Reason::Grants { ref sid, right } if sid == SID_USERS && right.contains("add file")),
        "{e}"
    );
    // The same folder is fine as an ANCESTOR of a locked tools folder: creating a sibling cannot swap what the path means.
    let mut p = windows_machine(|_| locked());
    p.put(
        "C:\\ProgramData\\Dyno\\Helper",
        from_sddl(FileKind::Dir, SDDL_DYNO_HELPER_UNFIXED),
    );
    verify_chain(&mut p, SMARTCTL, FileKind::File, Some(TOOLS), false).unwrap();
}

#[test]
fn the_attack_a_user_pre_creates_the_tree_and_the_helper_installs_into_it() {
    // 1. A standard user pre-creates tools\smartmontools-7.5\bin, so they own it. 2. The helper installs into it: the files
    // it writes are Administrators', but the folders around them are the user's. 3. The user renames smartctl.exe and plants
    // another. The parent is user-controlled, so the Administrators-owned smartctl.exe is refused all the same.
    let user = "S-1-5-21-1-2-3-1001";
    let mut p = windows_machine(|_| locked());
    let mut bin = locked();
    if let Security::Windows(d) = &mut bin.security {
        d.owner = user.into();
    }
    p.put(
        "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5\\bin",
        bin,
    );
    let e = check(&mut p).unwrap_err();
    assert!(e.path.ends_with("\\bin"), "{e}");
    assert!(matches!(e.reason, Reason::UntrustedOwner(_)));
    // The same when the user gave themselves rights on a folder an Administrator owns.
    let mut p = windows_machine(|_| locked());
    let mut bin = locked();
    if let Security::Windows(d) = &mut bin.security {
        d.dacl
            .as_mut()
            .unwrap()
            .push(Ace::allow(user, right::WRITE_DATA | right::DELETE));
    }
    p.put(
        "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5\\bin",
        bin,
    );
    assert!(matches!(
        check(&mut p).unwrap_err().reason,
        Reason::Grants { .. }
    ));
}

#[test]
fn a_staging_folder_users_can_write_to_is_refused() {
    let setup = "C:\\ProgramData\\Dyno\\Helper\\staging\\smartmontools-7.5.win32-setup.exe";
    let staging = "C:\\ProgramData\\Dyno\\Helper\\staging";
    let mut p = windows_machine(|_| locked());
    p.put(staging, inherited_from_program_data())
        .put(setup, file(locked()));
    let e = verify_chain(&mut p, setup, FileKind::File, Some(staging), false).unwrap_err();
    assert_eq!(e.path, staging);
    assert!(
        matches!(e.reason, Reason::Grants { ref sid, .. } if sid == SID_USERS),
        "{e}"
    );
}
