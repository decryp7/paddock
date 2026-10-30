//! Small validated text types: a fix sentence, a reason, a calendar date.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Why a text was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    /// Nothing but white space.
    #[error("{what} is empty; write one sentence")]
    Empty {
        /// Which text.
        what: &'static str,
    },
    /// Too long.
    #[error("{what} is {len} bytes; at most {max} are accepted")]
    TooLong {
        /// Which text.
        what: &'static str,
        /// Its length.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// Contains a line break or a control character.
    #[error("{what} must be one line without control characters")]
    NotOneLine {
        /// Which text.
        what: &'static str,
    },
    /// Not a calendar date.
    #[error("{0:?} is not a date; write YYYY-MM-DD")]
    BadDate(String),
}

fn one_line(what: &'static str, text: &str, max: usize) -> Result<String, TextError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(TextError::Empty { what });
    }
    if text.len() > max {
        return Err(TextError::TooLong {
            what,
            len: text.len(),
            max,
        });
    }
    if text.chars().any(char::is_control) {
        return Err(TextError::NotOneLine { what });
    }
    Ok(text.to_owned())
}

/// One sentence that says what to do, at most 512 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FixSentence(String);

/// The longest fix sentence.
pub const MAX_FIX_BYTES: usize = 512;

impl FixSentence {
    /// A sentence that is known to be valid (a literal of this crate, covered by its tests).
    pub(super) fn known(text: &'static str) -> FixSentence {
        FixSentence(text.to_owned())
    }

    /// Parses a sentence.
    pub fn parse(text: &str) -> Result<FixSentence, TextError> {
        one_line("the fix", text, MAX_FIX_BYTES).map(FixSentence)
    }

    /// The first sentence of `text` (up to the first `. `, `! ` or `? ` or the end), cut to the limit on a character boundary. `None`
    /// when nothing usable is left.
    pub fn first_of(text: &str) -> Option<FixSentence> {
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let end = flat
            .char_indices()
            .find(|(i, c)| matches!(c, '.' | '!' | '?') && flat[i + 1..].starts_with(' '))
            .map_or(flat.len(), |(i, c)| i + c.len_utf8());
        let mut cut = end.min(MAX_FIX_BYTES);
        while !flat.is_char_boundary(cut) {
            cut -= 1;
        }
        FixSentence::parse(&flat[..cut]).ok()
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for FixSentence {
    type Error = TextError;
    fn try_from(v: String) -> Result<Self, TextError> {
        FixSentence::parse(&v)
    }
}

impl From<FixSentence> for String {
    fn from(v: FixSentence) -> String {
        v.0
    }
}

impl fmt::Display for FixSentence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a waiver or suppression exists, at most 500 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Reason(String);

impl Reason {
    /// A reason that is known to be valid (a literal of this crate, covered by its tests).
    pub(super) fn known(text: &'static str) -> Reason {
        Reason(text.to_owned())
    }

    /// Parses a reason; a blank one is refused.
    pub fn parse(text: &str) -> Result<Reason, TextError> {
        one_line("the reason", text, 500).map(Reason)
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Reason {
    type Error = TextError;
    fn try_from(v: String) -> Result<Self, TextError> {
        Reason::parse(&v)
    }
}

impl From<Reason> for String {
    fn from(v: Reason) -> String {
        v.0
    }
}

/// A calendar date, `YYYY-MM-DD`. Ordered by date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IsoDate {
    year: u16,
    month: u8,
    day: u8,
}

impl IsoDate {
    /// Parses `YYYY-MM-DD`, refusing dates that do not exist (`2026-02-30`).
    pub fn parse(text: &str) -> Result<IsoDate, TextError> {
        let bad = || TextError::BadDate(text.chars().take(20).collect());
        let b = text.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || ![0, 1, 2, 3, 5, 6, 8, 9].iter().all(|&i| b[i].is_ascii_digit()) {
            return Err(bad());
        }
        let year: u16 = text[0..4].parse().map_err(|_| bad())?;
        let month: u8 = text[5..7].parse().map_err(|_| bad())?;
        let day: u8 = text[8..10].parse().map_err(|_| bad())?;
        let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(bad()),
        };
        if year == 0 || day == 0 || day > days {
            return Err(bad());
        }
        Ok(IsoDate { year, month, day })
    }

    /// The UTC date of `days` days after 1970-01-01 (negative before). The caller passes the clock's days; this crate reads no clock.
    pub fn from_unix_days(days: i64) -> IsoDate {
        // civil-from-days (H. Hinnant)
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as u16;
        IsoDate { year, month, day }
    }
}

impl fmt::Display for IsoDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for IsoDate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for IsoDate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        IsoDate::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fix_sentence_is_one_bounded_line() {
        assert!(FixSentence::parse("  Rename it.  ").is_ok());
        assert_eq!(FixSentence::parse(" ").unwrap_err(), TextError::Empty { what: "the fix" });
        assert!(matches!(
            FixSentence::parse(&"a".repeat(513)),
            Err(TextError::TooLong { max: 512, .. })
        ));
        assert!(FixSentence::parse(&"a".repeat(512)).is_ok());
        assert_eq!(
            FixSentence::parse("two\nlines").unwrap_err(),
            TextError::NotOneLine { what: "the fix" }
        );
    }

    #[test]
    fn the_first_sentence_is_taken_and_cut_on_a_character_boundary() {
        assert_eq!(FixSentence::first_of("Use X.  Then do Y. More.").unwrap().as_str(), "Use X.");
        assert_eq!(FixSentence::first_of("no stop at all").unwrap().as_str(), "no stop at all");
        assert!(FixSentence::first_of("   ").is_none());
        let long = "\u{e9}".repeat(400);
        let cut = FixSentence::first_of(&long).unwrap();
        assert!(cut.as_str().len() <= 512 && cut.as_str().chars().all(|c| c == '\u{e9}'));
    }

    #[test]
    fn a_reason_must_say_something() {
        assert!(Reason::parse("the side panel is not a table row").is_ok());
        assert!(Reason::parse("   ").is_err());
        assert!(Reason::parse(&"r".repeat(501)).is_err());
    }

    #[test]
    fn dates_are_real_calendar_days_in_order() {
        assert!(IsoDate::parse("2026-10-30").is_ok());
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "2026-1-01",
            "20261030",
            "2026-10-3x",
            "0000-01-01",
            "",
        ] {
            assert!(IsoDate::parse(bad).is_err(), "{bad}");
        }
        assert!(IsoDate::parse("2024-02-29").is_ok() && IsoDate::parse("2025-02-29").is_err());
        assert!(IsoDate::parse("2026-10-30").unwrap() < IsoDate::parse("2026-11-01").unwrap());
        assert_eq!(IsoDate::from_unix_days(0).to_string(), "1970-01-01");
        assert_eq!(IsoDate::from_unix_days(19_782).to_string(), "2024-02-29");
    }
}
