//! `same-folder`: the reinstall marker's comparison (ADR 0053 section 2.4). The native glue names each folder with
//! `GetFinalPathNameByHandleW` (long names, so 8.3 short names and a trailing backslash do not matter); this module
//! compares the two names without case.

/// `true` when both folders were named and the names are the same without case, ignoring the `\\?\` prefix and trailing
/// backslashes. A folder that could not be opened (`None`) is "not the same".
pub fn same(a: Option<&str>, b: Option<&str>) -> bool {
    same_with(a, b, &|x, y| {
        x.chars()
            .flat_map(char::to_uppercase)
            .eq(y.chars().flat_map(char::to_uppercase))
    })
}

/// [`same`] with the comparison supplied: on Windows the glue passes `CompareStringOrdinal` with `ignoreCase`, which uses the
/// system's own case table.
pub fn same_with(
    a: Option<&str>,
    b: Option<&str>,
    equal_ignoring_case: &dyn Fn(&str, &str) -> bool,
) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    let (a, b) = (normalise(a), normalise(b));
    !a.is_empty() && equal_ignoring_case(&a, &b)
}

/// The name without the `\\?\` prefix and without trailing backslashes (`\\?\UNC\` becomes `\\`).
pub fn normalise(p: &str) -> String {
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
    fn the_comparison_is_the_one_the_caller_supplies() {
        let always = |_: &str, _: &str| true;
        let never = |_: &str, _: &str| false;
        assert!(same_with(Some("A"), Some("B"), &always));
        assert!(!same_with(Some("A"), Some("A"), &never));
        assert!(
            !same_with(None, Some("A"), &always),
            "an unopenable folder is never the same"
        );
        assert!(
            !same_with(Some(""), Some(""), &always),
            "an empty name is not a folder"
        );
        let seen = std::cell::RefCell::new(Vec::new());
        let record = |x: &str, y: &str| {
            seen.borrow_mut().push((x.to_string(), y.to_string()));
            true
        };
        same_with(Some(r"\\?\C:\A\"), Some(r"C:\B"), &record);
        assert_eq!(
            *seen.borrow(),
            [(r"C:\A".to_string(), r"C:\B".to_string())],
            "names are normalised before they are compared"
        );
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
