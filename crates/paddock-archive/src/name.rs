//! Entry names: parsed once into safe components, or refused.

use crate::limits;
use crate::error::{ArchiveError, Result};
use crate::options::{Layout, Limits};

fn unsafe_entry(entry: &str, reason: &str) -> ArchiveError {
    ArchiveError::Unsafe {
        entry: entry.chars().take(200).collect(),
        reason: reason.into(),
    }
}

/// Windows device names are refused everywhere: an archive unpacked on Linux is often copied to Windows.
fn reserved(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem != "COM0"
            && stem != "LPT0")
}

fn component_ok(raw: &str, c: &str) -> Result<()> {
    if c.chars().count() > 255 {
        return Err(unsafe_entry(raw, "has a name longer than 255 characters"));
    }
    if c.ends_with('.') || c.ends_with(' ') {
        return Err(unsafe_entry(raw, "has a name ending in a dot or space"));
    }
    if reserved(c) {
        return Err(unsafe_entry(raw, "uses a reserved device name"));
    }
    Ok(())
}

/// The components of an entry name: `/`-separated, no `..`, no absolute path, no drive letter, no backslash, no control
/// characters; empty parts and `.` are dropped. The depth and length limits apply.
pub(crate) fn components(raw: &str, limits: &Limits) -> Result<Vec<String>> {
    if raw.chars().any(|c| c.is_control() || c == '\\' || c == ':') {
        return Err(unsafe_entry(
            raw,
            "contains a backslash, a colon or a control character",
        ));
    }
    if raw.starts_with('/') {
        return Err(unsafe_entry(raw, "is an absolute path"));
    }
    if raw.chars().count() > limits.max_path_len {
        return Err(limits::path_length(limits.max_path_len as u64));
    }
    let mut out = Vec::new();
    for c in raw.split('/').filter(|c| !c.is_empty() && *c != ".") {
        if c == ".." {
            return Err(unsafe_entry(raw, "climbs out of the folder with .."));
        }
        component_ok(raw, c)?;
        out.push(c.to_string());
    }
    if out.len() > limits.max_depth {
        return Err(limits::depth(limits.max_depth as u64));
    }
    Ok(out)
}

/// The relative path an entry gets under `layout`, or `None` when the layout leaves it out.
pub(crate) fn apply_layout(comps: Vec<String>, layout: Layout) -> Option<Vec<String>> {
    match layout {
        Layout::Keep => (!comps.is_empty()).then_some(comps),
        Layout::Strip(n) => (comps.len() > n).then(|| comps[n..].to_vec()),
        Layout::Flat { max_depth } => (!comps.is_empty() && comps.len() <= max_depth)
            .then(|| vec![comps[comps.len() - 1].clone()]),
    }
}

/// Where a link points, as components relative to the tree root, resolved from the link's own folder. `..` may climb but never
/// out of the root; `keep_form` (a link that stays a link on disk) allows no `..` at all.
pub(crate) fn link_target(
    raw_link: &str,
    target: &str,
    link_dir: &[String],
    keep_form: bool,
    limits: &Limits,
) -> Result<Vec<String>> {
    if target.is_empty()
        || target.starts_with('/')
        || target
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
    {
        return Err(unsafe_entry(
            raw_link,
            "is a link to an absolute path or to a name that is not allowed",
        ));
    }
    let mut out: Vec<String> = link_dir.to_vec();
    for c in target.split('/').filter(|c| !c.is_empty() && *c != ".") {
        if c == ".." {
            if keep_form || out.pop().is_none() {
                return Err(unsafe_entry(
                    raw_link,
                    "is a link that points out of the folder",
                ));
            }
        } else {
            component_ok(raw_link, c)?;
            out.push(c.to_string());
        }
    }
    if out.is_empty() || out.len() > limits.max_depth {
        return Err(unsafe_entry(
            raw_link,
            "is a link to the folder itself or to a path that is too deep",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l() -> Limits {
        Limits::default()
    }

    #[test]
    fn accepts_plain_names_and_drops_dots() {
        assert_eq!(
            components("./a//b/./c.txt", &l()).unwrap(),
            ["a", "b", "c.txt"]
        );
        assert!(components("", &l()).unwrap().is_empty());
        assert!(components("dir/", &l()).unwrap() == ["dir"]);
    }

    #[test]
    fn refuses_everything_that_can_escape_or_confuse() {
        for bad in [
            "../x",
            "a/../../x",
            "a/..",
            "/etc/passwd",
            "C:x.dll",
            "a\\b",
            "a\0b",
            "a\nb",
            "CON",
            "dir/aux.txt",
            "COM1",
            "lpt9.log",
            "name.",
            "name ",
            &"a".repeat(256),
        ] {
            assert!(
                matches!(components(bad, &l()), Err(ArchiveError::Unsafe { .. })),
                "{bad:?}"
            );
        }
        assert!(
            components("COM0", &l()).is_ok()
                && components("COM10", &l()).is_ok()
                && components("console", &l()).is_ok()
        );
        let tight = Limits {
            max_depth: 2,
            max_path_len: 10,
            ..Limits::default()
        };
        assert!(matches!(
            components("a/b/c", &tight),
            Err(ArchiveError::Limit(l)) if l.id.as_str() == "archive.depth"
        ));
        assert!(matches!(
            components("aaaaaaaaaaaa", &tight),
            Err(ArchiveError::Limit(l)) if l.id.as_str() == "archive.path_length"
        ));
    }

    #[test]
    fn layouts() {
        let c = |s: &str| components(s, &l()).unwrap();
        assert_eq!(apply_layout(c("a/b"), Layout::Keep), Some(c("a/b")));
        assert_eq!(apply_layout(vec![], Layout::Keep), None);
        assert_eq!(apply_layout(c("top/a/b"), Layout::Strip(1)), Some(c("a/b")));
        assert_eq!(apply_layout(c("top"), Layout::Strip(1)), None);
        assert_eq!(
            apply_layout(c("top/bin/x"), Layout::Flat { max_depth: 3 }),
            Some(c("x"))
        );
        assert_eq!(
            apply_layout(c("a/b/c/x"), Layout::Flat { max_depth: 3 }),
            None
        );
        assert_eq!(apply_layout(vec![], Layout::Flat { max_depth: 3 }), None);
    }

    #[test]
    fn link_targets_are_resolved_inside_the_tree() {
        let dir = vec!["lib".to_string()];
        let t = |s: &str, keep| link_target("lib/l", s, &dir, keep, &l());
        assert_eq!(t("libx.so.1", false).unwrap(), ["lib", "libx.so.1"]);
        assert_eq!(t("../bin/x", false).unwrap(), ["bin", "x"]);
        assert!(t("../../x", false).is_err(), "out of the root");
        assert!(t("../x", true).is_err(), "no .. in a link kept on disk");
        for bad in ["", "/etc/x", "C:x", "a\\b", "..", "a/CON"] {
            assert!(t(bad, false).is_err(), "{bad:?}");
        }
        let shallow = Limits {
            max_depth: 1,
            ..Limits::default()
        };
        assert!(link_target("l", "a/b", &[], false, &shallow).is_err());
    }
}
