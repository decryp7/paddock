//! Grouping the spellings of one name into one entity with variants.
//!
//! `CompanyA.Graphic.Builder.exe`, `graphicbuilder.exe` and `GraphicBuilder.exe` become one [`Group`] whose canonical spelling
//! is the most frequent one and whose variants carry their own counts. Names group by their normalised words
//! ([`crate::normalise`]). A group whose first word is unknown (a vendor) also joins the group that is the same name without
//! it. The caller keeps its own metadata (scopes, time ranges, files) and groups per scope.

use crate::normalise::{file_name, normalise, strip_extension, vendor_prefixes};
use crate::segment::{Vocabulary, split_raw};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most spellings kept per group.
pub const MAX_SPELLINGS: usize = 24;

/// A name with how often it was seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The name as written.
    pub name: String,
    /// How often.
    pub count: u64,
}

/// One way the data wrote the name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spelling {
    /// As written.
    pub name: String,
    /// How often.
    pub count: u64,
}

/// One entity: all the spellings that mean the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// The normalised words joined by a space: the group's identity (`graphic builder`).
    pub key: String,
    /// Title-cased words: `Graphic Builder`.
    pub display: String,
    /// The most frequent spelling first.
    pub spellings: Vec<Spelling>,
    /// Total count of all spellings.
    pub count: u64,
}

impl Group {
    /// The most frequent spelling.
    pub fn canonical(&self) -> &str {
        self.spellings
            .first()
            .map_or(&self.display, |s| s.name.as_str())
    }

    /// Joins `other` (the same entity) into this group: counts add, a shared spelling adds its counts.
    pub fn absorb(&mut self, other: Group) {
        self.count += other.count;
        for s in other.spellings {
            match self.spellings.iter_mut().find(|x| x.name == s.name) {
                Some(x) => x.count += s.count,
                None => self.spellings.push(s),
            }
        }
        self.finish();
    }

    fn finish(&mut self) {
        self.spellings
            .sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        self.spellings.truncate(MAX_SPELLINGS);
    }
}

/// The data's own words: pieces of names that are separated (`Graphic` and `Builder` in `CompanyA.Graphic.Builder.exe`). A
/// name written as one piece teaches nothing, so run-together spellings never vouch for themselves.
pub fn vocabulary_from(candidates: &[Candidate]) -> Vocabulary {
    let mut v = Vocabulary::builtin();
    for c in candidates {
        let stem = strip_extension(file_name(&c.name));
        let pieces = split_raw(stem);
        if pieces.len() < 2 {
            continue;
        }
        for p in pieces
            .iter()
            .filter(|p| p.chars().count() >= 3 && p.chars().all(char::is_alphabetic))
        {
            v.add(p, c.count.clamp(1, 1000));
        }
    }
    v
}

/// Groups `candidates` as [`Style::Path`](crate::Style::Path) names. Deterministic: most events first, then by key.
pub fn group(candidates: &[Candidate], vocab: &Vocabulary) -> Vec<Group> {
    let vendors = vendor_prefixes(candidates.iter().map(|c| c.name.as_str()));
    let mut by_key: BTreeMap<String, Group> = BTreeMap::new();
    for c in candidates {
        let n = normalise(&c.name, vocab, &vendors);
        let key = n.key();
        if key.is_empty() {
            continue;
        }
        let g = Group {
            key: key.clone(),
            display: n.display,
            spellings: vec![Spelling {
                name: c.name.clone(),
                count: c.count,
            }],
            count: c.count,
        };
        match by_key.get_mut(&key) {
            Some(existing) => existing.absorb(g),
            None => {
                by_key.insert(key, g);
            }
        }
    }
    // A vendor that appears once (so was not detected) is an unknown first word: join the group without it.
    let keys: Vec<String> = by_key.keys().cloned().collect();
    for key in keys {
        let words: Vec<&str> = key.split(' ').collect();
        // Cut one or two leading words that look like a vendor: unknown to the vocabulary, or a single letter.
        let target = (1..=2.min(words.len().saturating_sub(1)))
            .filter(|cut| {
                words[..*cut]
                    .iter()
                    .any(|w| w.chars().count() == 1 || !vocab.is_common(w))
            })
            .filter(|cut| {
                !words[..*cut]
                    .iter()
                    .any(|w| w.chars().any(|c| c.is_ascii_digit()))
            })
            .map(|cut| words[cut..].join(" "))
            .find(|rest| by_key.contains_key(rest));
        let Some(rest) = target else { continue };
        if let Some(g) = by_key.remove(&key)
            && let Some(t) = by_key.get_mut(&rest)
        {
            t.absorb(g);
        }
    }
    let mut v: Vec<Group> = by_key.into_values().collect();
    for g in &mut v {
        g.finish();
    }
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
    v
}
