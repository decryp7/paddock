//! The ACL of the helper's Program Files folder (ADR 0027 §1): SYSTEM and Administrators Full control, Users Read and
//! execute, no inheritance. Set with `icacls` by well-known SID (so the language of the system does not matter) and
//! checked afterwards from `icacls`'s own listing by the shape of the rights, which are abbreviations in every language.

use std::path::Path;

/// `icacls`.
pub const ICACLS: &str = "icacls";

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
