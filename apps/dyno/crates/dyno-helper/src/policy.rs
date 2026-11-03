//! The administrator-edited policy file (ADR 0027 D4/D9, ADR 0022 §5): what non-owner accounts see of other accounts, and
//! whether a UAC-filtered administrator token counts as a full administrator. Parsed once from text with limits (a pure
//! function), read through the protected-location rule, and replaced by the defaults, with a note naming the line and the
//! field, when anything is wrong: a bad file can only narrow what is shown, never widen it.
//!
//! ```toml
//! others = "total"            # "total" (default) | "detail" | "hidden"
//! filtered_tokens = "full"    # "full" (default) | "total": what a UAC-filtered administrator token sees
//! ```

use dyno_core::helper_policy::OthersPolicy;
use paddock_service::privileged::{verify_protected, FileKind, ProtectedFiles};
use std::path::Path;

/// The largest policy file read.
pub const MAX_POLICY_BYTES: u64 = 4 * 1024;

/// The policy in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelperPolicy {
    /// What accounts without full detail see of other accounts.
    pub others: OthersPolicy,
    /// A UAC-filtered administrator token gets the "total" view instead of the administrator view.
    pub filtered_as_total: bool,
}

impl Default for HelperPolicy {
    fn default() -> Self {
        HelperPolicy {
            others: OthersPolicy::Total,
            filtered_as_total: false,
        }
    }
}

/// A policy and what was said about the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// The policy in force.
    pub policy: HelperPolicy,
    /// Why the defaults are in force instead of the file, or what the file was (for the audit line and the log).
    pub note: Option<String>,
}

fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset.min(text.len())).map_or(1, |head| head.matches('\n').count() + 1)
}

/// Parses the text. `Err` names the line and field and means "use the defaults".
///
/// # Errors
/// The first problem, in words.
pub fn parse(text: &str) -> Result<HelperPolicy, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| {
        let line = e.span().map_or(1, |s| line_of(text, s.start));
        format!("line {line}: {}", e.message())
    })?;
    let mut policy = HelperPolicy::default();
    for (key, value) in &table {
        let line = text
            .lines()
            .position(|l| l.trim_start().starts_with(key.as_str()))
            .map_or(1, |i| i + 1);
        let word = value
            .as_str()
            .ok_or_else(|| format!("line {line}: {key} must be a word in quotes"))?;
        match (key.as_str(), word) {
            ("others", "total") => policy.others = OthersPolicy::Total,
            ("others", "detail") => policy.others = OthersPolicy::Detail,
            ("others", "hidden") => policy.others = OthersPolicy::Hidden,
            ("others", _) => return Err(format!("line {line}: others must be \"total\", \"detail\" or \"hidden\"")),
            ("filtered_tokens", "full") => policy.filtered_as_total = false,
            ("filtered_tokens", "total") => policy.filtered_as_total = true,
            ("filtered_tokens", _) => return Err(format!("line {line}: filtered_tokens must be \"full\" or \"total\"")),
            (other, _) => return Err(format!("line {line}: {other} is not a setting of the helper")),
        }
    }
    Ok(policy)
}

/// Reads the policy at `path` by handle, applies the protected-location rule to what was opened and parses it. A missing
/// file is the defaults without a note; a file that is not protected, too large, unreadable or invalid is the defaults with
/// a note.
pub fn load(files: &mut dyn ProtectedFiles, path: &Path) -> Loaded {
    let defaults = |note: Option<String>| Loaded {
        policy: HelperPolicy::default(),
        note,
    };
    let (facts, text) = match files.read_checked(path, MAX_POLICY_BYTES) {
        Ok(Some(found)) => found,
        Ok(None) => return defaults(None),
        Err(e) => {
            return defaults(Some(format!(
                "{} could not be read ({e}); the defaults are in force",
                path.display()
            )))
        }
    };
    if let Err(v) = verify_protected(Some(&facts), FileKind::File) {
        return defaults(Some(format!("{} is ignored: {v}; the defaults are in force", path.display())));
    }
    match parse(&text) {
        Ok(policy) => Loaded {
            policy,
            note: Some(format!("{} applied", path.display())),
        },
        Err(why) => defaults(Some(format!("{} is ignored, {why}; the defaults are in force", path.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::testing::FakeFiles;

    #[test]
    fn the_settings_are_read_and_the_default_is_the_total() {
        assert_eq!(parse("").unwrap(), HelperPolicy::default());
        assert_eq!(
            parse("others = \"detail\"\nfiltered_tokens = \"total\"\n").unwrap().others,
            OthersPolicy::Detail
        );
        assert!(parse("filtered_tokens = \"total\"").unwrap().filtered_as_total);
        assert_eq!(parse("others = \"hidden\"").unwrap().others, OthersPolicy::Hidden);
        assert_eq!(parse("# nothing\n").unwrap(), HelperPolicy::default());
    }

    #[test]
    fn every_problem_names_its_line_and_means_the_defaults() {
        for (text, needle) in [
            ("others = \"all\"", "line 1: others must be"),
            ("\n\nothers = 3", "line 3: others must be a word"),
            ("others = \"total\"\nfiltered_tokens = \"x\"", "line 2: filtered_tokens must be"),
            ("color = \"red\"", "color is not a setting"),
            ("others = ", "line 1"),
            ("[others]", "line"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(needle), "{text:?}: {e}");
        }
    }

    #[test]
    fn the_file_is_read_by_handle_and_must_be_protected() {
        let path = Path::new("/etc/dyno/helper.toml");
        let mut disk = FakeFiles::new();
        assert_eq!(
            load(&mut disk, path),
            Loaded {
                policy: HelperPolicy::default(),
                note: None
            }
        );
        disk.put_file(path, b"others = \"detail\"", 0, 0o644);
        let l = load(&mut disk, path);
        assert_eq!(l.policy.others, OthersPolicy::Detail);
        assert!(l.note.unwrap().contains("applied"));
        // A file anyone could have written is ignored.
        disk.put_file(path, b"others = \"detail\"", 501, 0o644);
        let l = load(&mut disk, path);
        assert_eq!(l.policy, HelperPolicy::default());
        assert!(l.note.unwrap().contains("not root"));
        disk.put_file(path, b"others = \"detail\"", 0, 0o666);
        assert!(load(&mut disk, path).note.unwrap().contains("lets the group or others"));
        disk.put_symlink(path);
        assert!(load(&mut disk, path).note.unwrap().contains("ignored"));
        disk.put_file(path, b"others = \"nope\"", 0, 0o644);
        assert!(load(&mut disk, path).note.unwrap().contains("line 1"));
        disk.put_file(path, &vec![b'#'; 5000], 0, 0o644);
        assert!(load(&mut disk, path).note.unwrap().contains("could not be read"));
    }
}
