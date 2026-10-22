//! Reading an id, a version, a channel and extra fields out of a name with a regular expression, and the helper that
//! writes such a regular expression from an example. Everything here is pure; the engine calls it for checks and for
//! the add-watch wizard's live preview (no regular expression is evaluated in the UI).

use crate::user_regex;
use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Longest id kept (characters).
pub const MAX_ID_LEN: usize = 64;
/// Most extra (free-text) fields kept per name.
pub const MAX_FIELDS: usize = 16;
/// Longest extra field value kept (characters).
pub const MAX_FIELD_LEN: usize = 200;

/// The named groups that mean something to Spotter; every other named group is a display field.
pub const NUMERIC_GROUPS: [&str; 4] = ["major", "minor", "patch", "build"];
/// All reserved group names.
pub const RESERVED_GROUPS: [&str; 8] = ["id", "major", "minor", "patch", "build", "channel", "pre", "variant"];

/// Where a named group matched, in UTF-16 code units (what a JavaScript string indexes by).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSpan {
    /// The group's name.
    pub group: String,
    /// First code unit.
    pub start: usize,
    /// One past the last code unit.
    pub end: usize,
}

/// What a custom scheme read out of one name.
#[derive(Debug, Clone, PartialEq)]
pub struct Extracted {
    /// The product (`id` group), when the pattern has one.
    pub id: Option<String>,
    /// The version. Its text is the original matched text (letters and leading zeros kept).
    pub version: Version,
    /// The channel label as written (`beta`, `stable`, empty).
    pub channel: String,
    /// The `variant` group (`x64`, `lite`), empty when none.
    pub variant: String,
    /// Other named groups (display only).
    pub fields: BTreeMap<String, String>,
    /// Where each group matched.
    pub spans: Vec<GroupSpan>,
}

/// Why a name gave nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ignored {
    /// The pattern does not match the name.
    NoMatch,
    /// It matches, but the numbers are missing or too large to be versions.
    NoVersion,
    /// The pattern has an `id` group and it took no part (or is empty).
    NoId,
}

impl Ignored {
    /// The reason, for the user.
    pub fn reason(&self) -> &'static str {
        match self {
            Ignored::NoMatch => "no match: the pattern does not match this name",
            Ignored::NoVersion => "no version: the number groups are empty or too large",
            Ignored::NoId => "no id: the (?<id>...) group did not match",
        }
    }
}

/// Whether a channel label means "a normal release".
pub fn is_release_channel(channel: &str) -> bool {
    matches!(
        channel.trim().to_ascii_lowercase().as_str(),
        "" | "stable" | "release" | "ga" | "final" | "prod" | "production" | "rtm" | "lts"
    )
}

/// UTF-16 offset of byte offset `byte` in `text`.
pub fn utf16_offset(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |s| s.encode_utf16().count())
}

/// Byte offset of UTF-16 offset `units` in `text` (`None` when it falls inside a character or past the end).
pub fn byte_offset(text: &str, units: usize) -> Option<usize> {
    let mut seen = 0;
    for (i, c) in text.char_indices() {
        if seen == units {
            return Some(i);
        }
        seen += c.len_utf16();
        if seen > units {
            return None;
        }
    }
    (seen == units).then_some(text.len())
}

/// A part of an example name the user selected, for [`suggest_pattern`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Selection {
    /// `id`, `major`, `minor`, `patch`, `build`, `channel` or `variant`.
    pub role: String,
    /// First UTF-16 code unit.
    pub start: usize,
    /// One past the last.
    pub end: usize,
}

/// Writes a regular expression from `example` and the selected parts: the text around the parts is kept literally
/// (escaped), each part becomes a named group shaped like its content. The result is checked: it compiles within the
/// limits and reads the example back.
pub fn suggest_pattern(example: &str, selections: &[Selection]) -> Result<String, String> {
    if selections.is_empty() {
        return Err("Select at least one part of the example name (for example the major number).".into());
    }
    let mut parts: Vec<(usize, usize, &str)> = Vec::new();
    for s in selections {
        if !RESERVED_GROUPS.contains(&s.role.as_str()) || s.role == "pre" {
            return Err(format!(
                "\"{}\" is not a part Spotter can read (id, major, minor, patch, build, channel or variant).",
                s.role
            ));
        }
        let (Some(a), Some(b)) = (byte_offset(example, s.start), byte_offset(example, s.end)) else {
            return Err("The selection does not fit the example name.".into());
        };
        if a >= b {
            return Err(format!("Select some text for \"{}\".", s.role));
        }
        parts.push((a, b, s.role.as_str()));
    }
    parts.sort_by_key(|p| p.0);
    for w in parts.windows(2) {
        if w[0].1 > w[1].0 {
            return Err("Two selections overlap; select each part separately.".into());
        }
        if w[0].2 == w[1].2 {
            return Err(format!("\"{}\" is selected twice; each part can be used once.", w[0].2));
        }
    }
    let mut out = String::new();
    let mut at = 0;
    for (a, b, role) in &parts {
        out.push_str(&escape_literal(&example[at..*a]));
        out.push_str(&format!("(?<{role}>{})", shape(&example[*a..*b], role)));
        at = *b;
    }
    out.push_str(&escape_literal(&example[at..]));
    let scheme = crate::version::VersionScheme::custom(&out)?;
    if scheme.extract_full(example).is_err() {
        return Err("The generated pattern does not read the example back; adjust the selection.".into());
    }
    Ok(out)
}

/// Escapes literal text for a pattern, leaving `-` readable (it is only special inside a class).
fn escape_literal(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c == '-' {
            out.push('-');
        } else {
            out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4])));
        }
    }
    out
}

/// A character class shaped like `sample`.
fn shape(sample: &str, role: &str) -> String {
    if NUMERIC_GROUPS.contains(&role) {
        return r"\d+".into();
    }
    let digits = sample.chars().any(|c| c.is_ascii_digit());
    let lower = sample.chars().any(|c| c.is_ascii_lowercase());
    let upper = sample.chars().any(|c| c.is_ascii_uppercase());
    let other = sample.chars().any(|c| !c.is_ascii_alphanumeric());
    match (upper, lower, digits, other) {
        (true, false, false, false) => "[A-Z]+".into(),
        (false, true, false, false) => "[a-z]+".into(),
        (_, _, _, false) if !digits => "[A-Za-z]+".into(),
        (_, _, _, false) => "[A-Za-z0-9]+".into(),
        _ => "[^/\\\\]+?".into(),
    }
}

/// Checks a pattern the way the add-watch form does and says what is wrong, for the live validation.
pub fn check_pattern(pattern: &str) -> Result<(), String> {
    crate::version::VersionScheme::custom(pattern).map(|_| ())
}

/// Compiles a pattern for prefiltering names (no scheme rules).
pub fn compile_name_regex(pattern: &str) -> Result<regex::Regex, String> {
    user_regex::compile(pattern, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(role: &str, s: usize, e: usize) -> Selection {
        Selection {
            role: role.into(),
            start: s,
            end: e,
        }
    }

    #[test]
    fn offsets_convert_between_utf16_and_bytes() {
        let t = "aé😀b";
        assert_eq!(utf16_offset(t, 0), 0);
        assert_eq!(utf16_offset(t, t.len()), 5);
        assert_eq!(byte_offset(t, 1), Some(1));
        assert_eq!(byte_offset(t, 2), Some(3));
        assert_eq!(byte_offset(t, 3), None, "inside a surrogate pair");
        assert_eq!(byte_offset(t, 5), Some(t.len()));
        assert_eq!(byte_offset(t, 6), None);
        assert_eq!(utf16_offset(t, 2), 0, "not a boundary");
    }

    #[test]
    fn channels_that_mean_release() {
        assert!(is_release_channel("") && is_release_channel(" Stable ") && is_release_channel("GA"));
        assert!(!is_release_channel("beta") && !is_release_channel("nightly"));
    }

    #[test]
    fn a_pattern_is_written_from_an_example() {
        let name = "Acme-PRO-R3.10.00T32.zip";
        let p = suggest_pattern(
            name,
            &[
                sel("id", 5, 8),
                sel("major", 10, 11),
                sel("minor", 12, 14),
                sel("patch", 15, 17),
                sel("build", 18, 20),
            ],
        )
        .unwrap();
        assert_eq!(
            p,
            r"Acme-(?<id>[A-Z]+)-R(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)T(?<build>\d+)\.zip"
        );
        let s = crate::version::VersionScheme::custom(&p).unwrap();
        let x = s.extract_full("Acme-QA-R4.01.02T7.zip").unwrap();
        assert_eq!((x.id.as_deref(), x.version.text.as_str()), (Some("QA"), "R4.01.02T7"));
    }

    #[test]
    fn bad_selections_say_what_to_fix() {
        let n = "app-1.2.zip";
        assert!(suggest_pattern(n, &[]).unwrap_err().contains("Select at least one"));
        assert!(suggest_pattern(n, &[sel("colour", 4, 5)]).unwrap_err().contains("not a part"));
        assert!(suggest_pattern(n, &[sel("pre", 4, 5)]).unwrap_err().contains("not a part"));
        assert!(suggest_pattern(n, &[sel("major", 4, 4)]).unwrap_err().contains("Select some text"));
        assert!(suggest_pattern(n, &[sel("major", 4, 99)]).unwrap_err().contains("does not fit"));
        assert!(suggest_pattern(n, &[sel("major", 4, 6), sel("minor", 5, 7)])
            .unwrap_err()
            .contains("overlap"));
        assert!(suggest_pattern(n, &[sel("major", 4, 5), sel("major", 6, 7)])
            .unwrap_err()
            .contains("twice"));
        assert!(suggest_pattern(n, &[sel("channel", 0, 3)]).unwrap_err().contains("named groups"));
    }

    #[test]
    fn shapes_follow_the_content() {
        assert_eq!(shape("PRO", "id"), "[A-Z]+");
        assert_eq!(shape("pro", "id"), "[a-z]+");
        assert_eq!(shape("Pro", "id"), "[A-Za-z]+");
        assert_eq!(shape("P2", "id"), "[A-Za-z0-9]+");
        assert_eq!(shape("x-64", "variant"), "[^/\\\\]+?");
        assert_eq!(shape("12", "major"), r"\d+");
    }

    #[test]
    fn patterns_are_checked_with_limits() {
        assert!(check_pattern(r"(?<id>\w+)-(?<major>\d+)").is_ok());
        assert!(check_pattern("(").unwrap_err().contains("not a valid regular expression"));
        assert!(compile_name_regex("a+").unwrap().is_match("AA"));
        assert_eq!(Ignored::NoMatch.reason().split(':').next(), Some("no match"));
        assert!(Ignored::NoVersion.reason().starts_with("no version") && Ignored::NoId.reason().starts_with("no id"));
    }
}
