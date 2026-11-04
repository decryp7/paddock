//! The ACL of the helper's Program Files folder (ADR 0027 §1): SYSTEM and Administrators Full control, Users Read and
//! execute, no inheritance. Set with `icacls` by well-known SID (so the language of the system does not matter) and
//! checked afterwards from `icacls`'s own listing by the shape of the rights, which are abbreviations in every language.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use std::path::Path;

/// `icacls`.
pub const ICACLS: &str = "icacls";

/// The SDDL of a protected folder, the same ACL as [`acl_command`] but in the form `SetSecurityInfo` and
/// `CreateDirectoryW` take: owner Administrators, the DACL protected (no inheritance from above), SYSTEM and Administrators Full
/// control and Users Read and execute, all inherited by what the folder holds. A test keeps the two in step.
pub const PROTECTED_FOLDER_SDDL: &str =
    "O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";

/// A bare name of a Windows system tool as its absolute path under `%SystemRoot%\System32` (so a program planted in the
/// working folder or on `PATH` is never the one a SYSTEM process runs). Other programs, and every program on another OS, are
/// returned as given.
pub fn system_tool(program: &str) -> String {
    if cfg!(windows) && program.eq_ignore_ascii_case("icacls") {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        return format!("{}\\System32\\icacls.exe", root.trim_end_matches('\\'));
    }
    program.to_string()
}

/// The `icacls` call that replaces the folder's ACL: inheritance removed, then SYSTEM (`S-1-5-18`) and Administrators
/// (`S-1-5-32-544`) Full control and Users (`S-1-5-32-545`) Read and execute, all inherited by what the folder holds.
pub fn acl_command(folder: &Path) -> Vec<String> {
    [
        folder.to_string_lossy().as_ref(),
        "/inheritance:r",
        "/grant:r",
        "*S-1-5-18:(OI)(CI)F",
        "/grant:r",
        "*S-1-5-32-544:(OI)(CI)F",
        "/grant:r",
        "*S-1-5-32-545:(OI)(CI)RX",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// The rights words of one `icacls` entry (`BUILTIN\Users:(OI)(CI)(RX)` gives `OI`, `CI`, `RX`).
fn rights(entry: &str) -> Vec<String> {
    entry
        .split('(')
        .skip(1)
        .filter_map(|part| part.split(')').next())
        .map(|w| w.trim().to_string())
        .collect()
}

/// Whether a rights word grants only reading and running (inheritance flags are not rights).
fn read_only_word(word: &str) -> bool {
    matches!(
        word,
        "OI" | "CI"
            | "IO"
            | "NP"
            | "I"
            | "RX"
            | "R"
            | "GR"
            | "GE"
            | "RD"
            | "RA"
            | "REA"
            | "X"
            | "S"
            | "RC"
    )
}

/// Checks `icacls <folder>` output: exactly three entries, exactly two with Full control (`F`) and one that only reads
/// and runs.
///
/// # Errors
/// What differs from the ADR's ACL, in words.
pub fn acl_shape_ok(listing: &str) -> Result<(), String> {
    let entries: Vec<Vec<String>> = listing
        .lines()
        .filter(|l| l.contains(":("))
        .map(rights)
        .filter(|r| !r.is_empty())
        .collect();
    if entries.len() != 3 {
        return Err(format!(
            "the folder has {} access entries, not the 3 the install sets",
            entries.len()
        ));
    }
    let full = entries
        .iter()
        .filter(|r| r.iter().any(|w| w == "F"))
        .count();
    let readers = entries
        .iter()
        .filter(|r| !r.iter().any(|w| w == "F") && r.iter().all(|w| read_only_word(w)))
        .count();
    if full != 2 || readers != 1 {
        return Err("the folder must give Full control to two accounts and only Read and execute to the third".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sddl_and_the_icacls_call_describe_the_same_acl() {
        // SYSTEM Full, Administrators Full, Users Read and execute, both inherited by folders and files, owner Administrators.
        let c = acl_command(Path::new("C:\\x"));
        for (sid, sddl_sid, sddl_rights, icacls_rights) in [
            ("S-1-5-18", "SY", "FA", "F"),
            ("S-1-5-32-544", "BA", "FA", "F"),
            ("S-1-5-32-545", "BU", "0x1200a9", "RX"),
        ] {
            assert!(
                PROTECTED_FOLDER_SDDL.contains(&format!("(A;OICI;{sddl_rights};;;{sddl_sid})")),
                "{sid}"
            );
            assert!(
                c.contains(&format!("*{sid}:(OI)(CI){icacls_rights}")),
                "{sid}"
            );
        }
        assert!(PROTECTED_FOLDER_SDDL.starts_with("O:BAD:P"));
    }

    #[test]
    fn a_system_tool_is_run_by_its_absolute_path_on_windows_only() {
        let p = system_tool("icacls");
        if cfg!(windows) {
            assert!(
                p.to_ascii_lowercase().ends_with("\\system32\\icacls.exe"),
                "{p}"
            );
        } else {
            assert_eq!(p, "icacls");
        }
        assert_eq!(system_tool("other"), "other");
    }

    #[test]
    fn the_acl_command_names_the_well_known_sids() {
        let c = acl_command(Path::new("C:\\Program Files\\Dyno Helper"));
        assert_eq!(c[0], "C:\\Program Files\\Dyno Helper");
        assert_eq!(c[1], "/inheritance:r");
        assert_eq!(c.iter().filter(|a| a.as_str() == "/grant:r").count(), 3);
        assert!(c.contains(&"*S-1-5-32-545:(OI)(CI)RX".to_string()));
    }

    const GOOD: &str = "C:\\Program Files\\Dyno Helper NT AUTHORITY\\SYSTEM:(OI)(CI)(F)\r\n                           BUILTIN\\Administrators:(OI)(CI)(F)\r\n                           BUILTIN\\Users:(OI)(CI)(RX)\r\n\r\nSuccessfully processed 1 files; Failed processing 0 files\r\n";

    #[test]
    fn only_the_adrs_acl_passes() {
        acl_shape_ok(GOOD).unwrap();
        // The same shape in another language passes: the rights are abbreviations.
        acl_shape_ok(
            &GOOD
                .replace("BUILTIN\\Users", "VORDEFINIERT\\Benutzer")
                .replace("Administrators", "Administratoren"),
        )
        .unwrap();
        let writable = GOOD.replace("(RX)", "(M)");
        assert!(
            acl_shape_ok(&writable)
                .unwrap_err()
                .contains("Full control")
        );
        let everyone = format!("{GOOD}                           Everyone:(OI)(CI)(RX)\r\n");
        assert!(
            acl_shape_ok(&everyone)
                .unwrap_err()
                .contains("4 access entries")
        );
        let three_full = GOOD.replace("(RX)", "(F)");
        assert!(acl_shape_ok(&three_full).is_err());
        let wd = GOOD.replace("(RX)", "(RX,WD)");
        assert!(acl_shape_ok(&wd).is_err());
        assert!(acl_shape_ok("").unwrap_err().contains("0 access entries"));
    }
}
