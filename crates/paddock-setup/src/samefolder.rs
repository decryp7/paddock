//! `same-folder`: the reinstall marker's comparison (ADR 0053 section 2.4). The native glue names each folder with
//! `GetFinalPathNameByHandleW` (long names, so 8.3 short names and a trailing backslash do not matter); this module
//! compares the two names without case.

/// `true` when both folders were named and the names are the same without case, ignoring the `\\?\` prefix and trailing
/// backslashes. A folder that could not be opened (`None`) is "not the same".
pub fn same(a: Option<&str>, b: Option<&str>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    let (a, b) = (normalise(a), normalise(b));
    !a.is_empty()
        && a.chars()
            .flat_map(char::to_uppercase)
            .eq(b.chars().flat_map(char::to_uppercase))
}

/// The name without the `\\?\` prefix and without trailing backslashes (`\\?\UNC\` becomes `\\`).
fn normalise(p: &str) -> String {
    const UNC: &str = r"\\?\UNC\";
    let p = match p.get(..UNC.len()) {
        Some(head) if head.eq_ignore_ascii_case(UNC) => format!(r"\\{}", &p[UNC.len()..]),
        _ => p.strip_prefix(r"\\?\").unwrap_or(p).to_string(),
    };
    p.trim_end_matches('\\').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_without_case_and_trailing_backslash() {
        assert!(same(
            Some(r"C:\Program Files\Dyno"),
            Some(r"c:\program files\dyno\")
        ));
        assert!(same(
            Some(r"\\?\C:\Program Files\Dyno"),
            Some(r"C:\Program Files\Dyno")
        ));
        assert!(same(
            Some(r"C:\Program Files\Dyno\\"),
            Some(r"C:\PROGRAM FILES\DYNO")
        ));
    }

    #[test]
    fn different_folders_and_prefix_names() {
        assert!(!same(
            Some(r"C:\Program Files\Dyno"),
            Some(r"C:\Program Files\Dyno Helper")
        ));
        assert!(!same(
            Some(r"C:\Program Files\Dyno"),
            Some(r"C:\Users\x\AppData\Local\Temp\nsx1")
        ));
        assert!(!same(
            Some(r"C:\Program Files\Dyno"),
            Some(r"C:\Program Files\Dyno\sub")
        ));
    }

    #[test]
    fn unopenable_is_not_the_same() {
        assert!(!same(None, Some(r"C:\A")));
        assert!(!same(Some(r"C:\A"), None));
        assert!(!same(None, None));
    }

    #[test]
    fn unc_prefix_is_kept_distinct_from_a_drive() {
        assert!(!same(Some(r"\\?\UNC\srv\share\Dyno"), Some(r"C:\Dyno")));
        assert!(same(
            Some(r"\\?\UNC\srv\share\Dyno"),
            Some(r"\\?\unc\srv\SHARE\dyno\")
        ));
    }
}
