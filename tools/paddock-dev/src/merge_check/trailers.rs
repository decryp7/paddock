//! The reasons a merge message can give for a deliberate drop (ADR 0089: a reason is data, never prose the check guesses at).
//!
//! - `Tests-removed: <test name>: <reason>`
//! - `Hunks-dropped: <path>: <reason>` (a folder with a trailing slash covers the files under it)
//! - `Reverted: <path>: <reason>` (answers a backwards finding; `Hunks-dropped` does too)

/// The reason given for `subject` under `key` (`Tests-removed` or `Hunks-dropped`): the text after `<key>: <subject>: `, which
/// must not be empty. Looks at every line of the message, so a trailer block anywhere counts. For a path, `subject` may also
/// be a folder written with a trailing slash (`Hunks-dropped: docs/images/: ...`), which covers the files under it
/// ([`path_reason`]).
pub fn reason<'m>(message: &'m str, key: &str, subject: &str) -> Option<&'m str> {
    let prefix = format!("{key}: {subject}: ");
    message
        .lines()
        .filter_map(|l| l.trim().strip_prefix(prefix.as_str()))
        .map(str::trim)
        .find(|r| !r.is_empty())
}

/// [`reason`] for a file path: the path itself, or a folder prefix ending in `/` that contains it.
pub fn path_reason<'m>(message: &'m str, key: &str, path: &str) -> Option<&'m str> {
    if let Some(r) = reason(message, key, path) {
        return Some(r);
    }
    let prefix = format!("{key}: ");
    message
        .lines()
        .filter_map(|l| l.trim().strip_prefix(prefix.as_str()))
        .find_map(|rest| {
            let (dir, why) = rest.split_once(": ")?;
            (dir.ends_with('/') && path.starts_with(dir) && !why.trim().is_empty()).then_some(why.trim())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reason_is_needed_and_the_subject_must_match_exactly() {
        let m = "Merge x\n\nTests-removed: old_case: replaced by new_case\nTests-removed: bare: \nTests-removed: other_old_case: no\n";
        assert_eq!(reason(m, "Tests-removed", "old_case"), Some("replaced by new_case"));
        assert_eq!(reason(m, "Tests-removed", "bare"), None);
        assert_eq!(reason(m, "Tests-removed", "old"), None);
        assert_eq!(reason(m, "Hunks-dropped", "old_case"), None);
    }

    #[test]
    fn a_folder_with_a_trailing_slash_covers_its_files() {
        let m = "Hunks-dropped: docs/images/: the hero is redrawn\nHunks-dropped: src/: \n";
        assert!(path_reason(m, "Hunks-dropped", "docs/images/a/b.svg").is_some());
        assert!(path_reason(m, "Hunks-dropped", "docs/imagesx/b.svg").is_none());
        assert!(path_reason(m, "Hunks-dropped", "src/lib.rs").is_none());
    }
}
