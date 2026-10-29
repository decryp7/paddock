//! Secret masking. A secret is **never** shown as HUD text, only as `•••• secret NAME`.
//!
//! The guarantee is structural: the HUD's only text type, [`HudText`], can hold a literal or a secret's *name*, and no
//! function turns a secret's value into one. A [`Secret`] has no `Display`, no `Serialize`, and a `Debug` that prints the
//! marker, so a stray `format!("{secret:?}")` cannot leak either. [`mask`] replaces secret values found inside free
//! text (typed keys, OCR) by the marker.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Longest secret name.
pub const MAX_SECRET_NAME: usize = 64;
/// Longest text the HUD keeps; longer text is cut and ends in an ellipsis.
pub const MAX_HUD_TEXT: usize = 512;

/// Why a name or secret was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MaskError {
    /// A secret name was empty, too long or held characters outside `A-Z a-z 0-9 _ . -`.
    #[error("a secret name must be 1 to {MAX_SECRET_NAME} characters from A-Z, a-z, 0-9, '_', '.' or '-'")]
    BadName,
    /// A secret with an empty value would match everywhere.
    #[error("a secret value must not be empty")]
    EmptyValue,
}

/// The name of a secret (never its value).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretName(String);

impl SecretName {
    /// Validates a name.
    pub fn new(name: &str) -> Result<Self, MaskError> {
        let ok = !name.is_empty()
            && name.chars().count() <= MAX_SECRET_NAME
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if ok { Ok(Self(name.to_owned())) } else { Err(MaskError::BadName) }
    }
    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for SecretName {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for SecretName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        SecretName::new(&s).map_err(D::Error::custom)
    }
}

/// A secret known to the run: a name and a value. The value can only be compared against text, never read back.
#[derive(Clone)]
pub struct Secret {
    name: SecretName,
    value: String,
}

impl Secret {
    /// Builds a secret; the value must not be empty.
    pub fn new(name: SecretName, value: String) -> Result<Self, MaskError> {
        if value.is_empty() {
            Err(MaskError::EmptyValue)
        } else {
            Ok(Self { name, value })
        }
    }
    /// The secret's name.
    pub fn name(&self) -> &SecretName {
        &self.name
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", marker(&self.name))
    }
}

/// The only way a secret appears: `•••• secret NAME`.
pub fn marker(name: &SecretName) -> String {
    format!("•••• secret {}", name.as_str())
}

/// Text the HUD may show: a literal that went through [`mask`] or a secret reference. The HUD, the trace and the sidecar
/// carry nothing else, and the only public way to make a literal is [`mask`] (with no secrets it is a plain cut), so a raw
/// secret has no path to the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudText(Repr);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Repr {
    /// At most [`MAX_HUD_TEXT`] characters, cut when built.
    Plain(String),
    /// A secret, shown as its marker.
    Secret(SecretName),
}

impl HudText {
    /// Plain text, cut to [`MAX_HUD_TEXT`] characters. Crate-private: it does **not** mask. Callers outside the crate go
    /// through [`mask`].
    pub(crate) fn plain(s: &str) -> Self {
        Self(Repr::Plain(cut(s)))
    }
    /// A secret reference.
    pub fn secret(name: SecretName) -> Self {
        Self(Repr::Secret(name))
    }
    /// What to draw.
    pub fn render(&self) -> String {
        match &self.0 {
            Repr::Plain(s) => s.clone(),
            Repr::Secret(n) => marker(n),
        }
    }
}

impl From<&Secret> for HudText {
    fn from(s: &Secret) -> Self {
        Self::secret(s.name.clone())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Wire {
    Plain(String),
    Secret(SecretName),
}

impl Serialize for HudText {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            Repr::Plain(t) => Wire::Plain(t.clone()),
            Repr::Secret(n) => Wire::Secret(n.clone()),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for HudText {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Wire::deserialize(d)? {
            Wire::Plain(t) => HudText::plain(&t),
            Wire::Secret(n) => HudText::secret(n),
        })
    }
}

fn cut(s: &str) -> String {
    if s.chars().count() <= MAX_HUD_TEXT {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(MAX_HUD_TEXT - 1).collect();
    out.push('…');
    out
}

/// Replaces every occurrence of every secret's value in `text` with a marker. All match ranges of all secrets are found in
/// the **original** text (overlapping and nested matches included), overlapping ranges are merged, and each merged range
/// becomes one marker naming every secret it covers (`•••• secret A, B`, sorted by name), so no readable part of any secret
/// survives, however the secrets overlap. Replaced text is never rescanned. Matching is exact (case-sensitive): a secret
/// the screen shows in another form (encoded, re-cased) is the job of the redaction regions, not of this function.
pub fn mask(text: &str, secrets: &[Secret]) -> HudText {
    // (start, end, secret index) for every occurrence, found on character boundaries of the original text.
    let mut ranges: Vec<(usize, usize, usize)> = Vec::new();
    for (i, _) in text.char_indices() {
        let rest = &text[i..];
        for (k, s) in secrets.iter().enumerate() {
            if rest.starts_with(s.value.as_str()) {
                ranges.push((i, i + s.value.len(), k));
            }
        }
    }
    // `ranges` is sorted by start already; merge the overlapping ones.
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut k = 0;
    while k < ranges.len() {
        let (start, mut end, first) = ranges[k];
        let mut names = vec![first];
        k += 1;
        while k < ranges.len() && ranges[k].0 < end {
            end = end.max(ranges[k].1);
            names.push(ranges[k].2);
            k += 1;
        }
        out.push_str(&text[pos..start]);
        let mut list: Vec<&str> = names.iter().map(|&n| secrets[n].name.as_str()).collect();
        list.sort_unstable();
        list.dedup();
        out.push_str(&format!("•••• secret {}", list.join(", ")));
        pos = end;
    }
    out.push_str(&text[pos..]);
    HudText::plain(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn name(s: &str) -> SecretName {
        SecretName::new(s).unwrap()
    }
    fn secret(n: &str, v: &str) -> Secret {
        Secret::new(name(n), v.to_owned()).unwrap()
    }

    #[test]
    fn a_secret_shows_only_as_its_marker() {
        let s = secret("DB_PASSWORD", "hunter2");
        assert_eq!(HudText::from(&s).render(), "•••• secret DB_PASSWORD");
        assert_eq!(format!("{s:?}"), "•••• secret DB_PASSWORD");
        assert_eq!(
            mask("type hunter2 now", std::slice::from_ref(&s)).render(),
            "type •••• secret DB_PASSWORD now"
        );
        assert_eq!(
            mask("hunter2hunter2", std::slice::from_ref(&s)).render(),
            "•••• secret DB_PASSWORD•••• secret DB_PASSWORD"
        );
        assert_eq!(s.name().as_str(), "DB_PASSWORD");
    }

    #[test]
    fn overlapping_secrets_leave_nothing_readable() {
        // Regression: the greedy scan masked "xab" and then leaked "cdef" of the longer secret.
        let secrets = [secret("A", "xab"), secret("B", "abcdef")];
        assert_eq!(mask("xabcdef", &secrets).render(), "•••• secret A, B");
        // Nested: one secret inside another.
        let nested = [secret("SHORT", "abc"), secret("LONG", "xabcdefx")];
        assert_eq!(mask("1xabcdefx2", &nested).render(), "1•••• secret LONG, SHORT2");
        // Touching but not overlapping secrets keep their own markers.
        let touching = [secret("A", "ab"), secret("B", "cd")];
        assert_eq!(mask("abcd", &touching).render(), "•••• secret A•••• secret B");
        // A secret that overlaps itself ("aaa" in "aaaaa") is one range.
        assert_eq!(mask("aaaaa!", &[secret("S", "aaa")]).render(), "•••• secret S!");
        assert_eq!(mask("no secret here", &[]).render(), "no secret here");
    }

    #[test]
    fn names_and_values_are_validated() {
        for bad in ["", "has space", "ünï", &"x".repeat(MAX_SECRET_NAME + 1)] {
            assert_eq!(SecretName::new(bad), Err(MaskError::BadName));
        }
        assert!(SecretName::new("a.b-c_D9").is_ok());
        assert_eq!(Secret::new(name("A"), String::new()).map(|_| ()), Err(MaskError::EmptyValue));
    }

    #[test]
    fn long_text_is_cut_after_masking() {
        let s = secret("K", "SECRETVALUE");
        let text = format!("{}SECRETVALUE", "a".repeat(MAX_HUD_TEXT - 3));
        let out = mask(&text, &[s]).render();
        assert!(!out.contains("SECRETVALUE") && out.chars().count() == MAX_HUD_TEXT && out.ends_with('…'));
    }

    #[test]
    fn serde_never_carries_a_value() {
        let t = HudText::secret(name("TOKEN"));
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"secret":"TOKEN"}"#);
        assert_eq!(serde_json::from_str::<HudText>(&json).unwrap(), t);
        assert!(serde_json::from_str::<HudText>(r#"{"secret":"bad name"}"#).is_err());
        assert_eq!(serde_json::from_str::<HudText>(r#"{"plain":"hi"}"#).unwrap(), HudText::plain("hi"));
    }

    proptest! {
        /// No secret value survives masking, wherever it sits in the text (values drawn from a set that cannot occur in
        /// the marker, so a match means a leak).
        #[test]
        fn no_raw_secret_in_masked_output(
            values in prop::collection::vec("[0-9]{4,10}", 1..4),
            prefix in "[a-z ]{0,20}", between in "[a-z ]{0,10}", suffix in "[a-z ]{0,20}",
        ) {
            let secrets: Vec<Secret> = values.iter().enumerate().map(|(i, v)| secret(&format!("S{}", (b'A' + i as u8) as char), v)).collect();
            let text = format!("{prefix}{}{between}{}{suffix}", values[0], values[values.len() - 1]);
            let out = mask(&text, &secrets).render();
            for v in &values {
                prop_assert!(!out.contains(v.as_str()), "{out} leaks {v}");
            }
            // Debug of every secret and every HudText built from one is value-free too.
            for sec in &secrets {
                let shown = format!("{sec:?}");
                let rendered = HudText::from(sec).render();
                prop_assert!(values.iter().all(|v| !shown.contains(v.as_str()) && !rendered.contains(v.as_str())));
            }
        }

        /// Overlapping and nested secrets built from substrings of the text: every character a secret covers disappears,
        /// every other character stays, whatever the overlaps (markers contain letters and spaces, the text only digits).
        #[test]
        fn overlapping_secrets_cover_exactly_their_ranges(
            text in "[0-3]{1,24}",
            picks in prop::collection::vec((0usize..24, 1usize..8), 1..5),
        ) {
            let secrets: Vec<Secret> = picks.iter().enumerate().map(|(i, (start, len))| {
                let start = start % text.len();
                let end = (start + len).min(text.len());
                secret(&format!("S{}", (b'A' + i as u8) as char), &text[start..end])
            }).collect();
            let out = mask(&text, &secrets).render();
            let mut covered = vec![false; text.len()];
            for s in &secrets {
                let v = s.value.as_str();
                for i in 0..=text.len() - v.len() {
                    if text[i..].starts_with(v) { covered[i..i + v.len()].iter_mut().for_each(|c| *c = true); }
                }
            }
            let kept: String = text.chars().zip(&covered).filter(|(_, c)| !**c).map(|(ch, _)| ch).collect();
            let digits: String = out.chars().filter(char::is_ascii_digit).collect();
            prop_assert_eq!(digits, kept);
            for s in &secrets {
                prop_assert!(!out.contains(s.value.as_str()));
            }
        }

        /// Text with no secret in it passes through unchanged (up to the length cut).
        #[test]
        fn clean_text_is_unchanged(text in "[a-z ]{0,100}") {
            let s = secret("K", "99999");
            prop_assert_eq!(mask(&text, &[s]).render(), text);
        }
    }
}
