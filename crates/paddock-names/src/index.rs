//! The index that keeps a search over 250,000 entries fast: it finds the few entries a query can possibly match, and the
//! catalogue then runs the full ranking on only those. It never changes a result: candidates are a superset of every entry a
//! rung can match (a test compares an indexed search with a scan of all entries, on fixed and on random catalogues).
//!
//! - hash maps from a squashed spelling, a joined name, an alias or a user alias to its entries (the exact, joined and alias
//!   rungs);
//! - a sorted list of the distinct words of all names and fields with the entries that hold each (parts, prefix and words: every
//!   query word must be, start or (from three letters) be inside some word of the entry);
//! - for typos, a letter-set mask per word and per joined name: two strings within `n` edits differ in at most `2n` letters of
//!   their sets, which discards almost every candidate before the edit distance is computed.

use crate::catalogue::{Prepared, Query};
use crate::distance::{allowed_typos, edit_distance_capped};
use crate::normalise::squash;
use std::collections::HashMap;

/// Shortest query word that may be found inside another word (the same rule as the ranking).
pub(crate) const MIN_INSIDE: usize = 3;

struct Word {
    text: String,
    mask: u64,
    /// Entries holding the word, ascending.
    entries: Vec<u32>,
}

struct Joined {
    text: String,
    len: usize,
    mask: u64,
    entries: Vec<u32>,
}

#[derive(Default)]
pub(crate) struct Index {
    /// Distinct words, sorted by text.
    words: Vec<Word>,
    joined: Vec<Joined>,
    joined_at: HashMap<String, usize>,
    spelling: HashMap<String, Vec<u32>>,
    alias: HashMap<String, Vec<u32>>,
    user: HashMap<String, Vec<u32>>,
}

/// The letters a string uses, as bits (a-z, 0-9, everything else folded into 28 more bits).
fn mask(s: &str) -> u64 {
    s.chars().fold(0u64, |m, c| {
        let bit = match c {
            'a'..='z' => c as u32 - 'a' as u32,
            '0'..='9' => 26 + (c as u32 - '0' as u32),
            _ => 36 + (c as u32 % 28),
        };
        m | (1 << bit)
    })
}

/// Edits that two words may be apart and still be considered by either typo rule.
fn scan_cap(a: &str, b: &str) -> usize {
    let (la, lb) = (a.chars().count(), b.chars().count());
    allowed_typos(la.max(lb))
        .max((lb / 3).max(1))
        .max((la / 3).max(1))
}

/// How many of each letter a string has (a-z, 0-9, everything else folded into four more buckets).
fn counts(s: &str) -> [u8; 40] {
    let mut c = [0u8; 40];
    for ch in s.chars() {
        let i = match ch {
            'a'..='z' => ch as usize - 'a' as usize,
            '0'..='9' => 26 + (ch as usize - '0' as usize),
            _ => 36 + (ch as usize % 4),
        };
        c[i] = c[i].saturating_add(1);
    }
    c
}

/// The bag distance: strings within `cap` edits have at most `cap` letters that the other lacks, each way.
fn bag_within(a: &[u8; 40], b: &[u8; 40], cap: usize) -> bool {
    let (mut over, mut under) = (0usize, 0usize);
    for i in 0..40 {
        if a[i] > b[i] {
            over += usize::from(a[i] - b[i]);
        } else {
            under += usize::from(b[i] - a[i]);
        }
    }
    over <= cap && under <= cap
}

fn near(a: &str, ma: u64, b: &str, mb: u64) -> bool {
    let cap = scan_cap(a, b);
    (ma ^ mb).count_ones() as usize <= 2 * cap && edit_distance_capped(a, b, cap).is_some()
}

impl Index {
    pub(crate) fn build(entries: &[Prepared]) -> Index {
        let mut ix = Index::default();
        let mut words: HashMap<&str, Vec<u32>> = HashMap::new();
        for (i, p) in entries.iter().enumerate() {
            let i = i as u32;
            for w in p.names.iter().flatten().chain(&p.field_tokens) {
                let list = words.entry(w.as_str()).or_default();
                if list.last() != Some(&i) {
                    list.push(i);
                }
            }
            for s in &p.spellings {
                push(ix.spelling.entry(s.clone()).or_default(), i);
            }
            for a in &p.aliases {
                push(ix.alias.entry(squash(a)).or_default(), i);
            }
            for a in &p.user_aliases {
                push(ix.user.entry(squash(a)).or_default(), i);
            }
            for j in &p.joined {
                let at = *ix.joined_at.entry(j.clone()).or_insert_with(|| {
                    ix.joined.push(Joined {
                        text: j.clone(),
                        len: j.chars().count(),
                        mask: mask(j),
                        entries: Vec::new(),
                    });
                    ix.joined.len() - 1
                });
                push(&mut ix.joined[at].entries, i);
            }
        }
        ix.words = words
            .into_iter()
            .map(|(t, entries)| Word {
                text: t.to_string(),
                mask: mask(t),
                entries,
            })
            .collect();
        ix.words.sort_by(|a, b| a.text.cmp(&b.text));
        ix
    }

    /// The entries a strict (not typo) rung can match: the hash hits plus the entries that hold, for every query word, a word that
    /// equals, starts with or (three letters or more) contains it. Ascending, without duplicates.
    pub(crate) fn candidates(&self, q: &Query) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for map in [&self.user, &self.spelling, &self.alias] {
            if let Some(v) = map.get(&q.squashed) {
                out.extend(v);
            }
        }
        if let Some(at) = self
            .joined_at
            .get(&q.joined)
            .filter(|_| !q.joined.is_empty())
        {
            out.extend(&self.joined[*at].entries);
        }
        if !q.tokens.is_empty() {
            let mut per_word: Vec<Vec<u32>> = q.tokens.iter().map(|t| self.holding(t)).collect();
            per_word.sort_by_key(Vec::len);
            let mut all = per_word.remove(0);
            for next in &per_word {
                all.retain(|e| next.binary_search(e).is_ok());
            }
            out.extend(all);
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The entries that hold a word starting with `t` or near it (a typo of it).
    fn near_or_starting(&self, t: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let from = self.words.partition_point(|w| w.text.as_str() < t);
        for w in self.words[from..]
            .iter()
            .take_while(|w| w.text.starts_with(t))
        {
            out.extend(&w.entries);
        }
        let mt = mask(t);
        for w in &self.words {
            if near(t, mt, &w.text, w.mask) {
                out.extend(&w.entries);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The entries that hold a word equal to, starting with or (three letters or more) containing `t`.
    fn holding(&self, t: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let from = self.words.partition_point(|w| w.text.as_str() < t);
        for w in self.words[from..]
            .iter()
            .take_while(|w| w.text.starts_with(t))
        {
            out.extend(&w.entries);
        }
        if t.chars().count() >= MIN_INSIDE {
            for w in &self.words {
                if !w.text.starts_with(t) && w.text.contains(t) {
                    out.extend(&w.entries);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The entries a typo rule can match: those that, for every query word, hold a word that starts with it or is near it, or whose
    /// joined name is near the joined query. Ascending, without duplicates.
    pub(crate) fn fuzzy_candidates(&self, q: &Query) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        if !q.tokens.is_empty() {
            let mut per_word: Vec<Vec<u32>> =
                q.tokens.iter().map(|t| self.near_or_starting(t)).collect();
            per_word.sort_by_key(Vec::len);
            let mut all = per_word.remove(0);
            for next in &per_word {
                all.retain(|e| next.binary_search(e).is_ok());
            }
            out = all;
        }
        let (mq, cq, lq) = (
            mask(&q.squashed),
            counts(&q.squashed),
            q.squashed.chars().count(),
        );
        for j in &self.joined {
            // The joined rule of the ranking: a name may be a quarter of its letters off.
            let cap = allowed_typos(j.len);
            if j.len.abs_diff(lq) <= cap
                && (mq ^ j.mask).count_ones() as usize <= 2 * cap
                && bag_within(&cq, &counts(&j.text), cap)
                && edit_distance_capped(&q.squashed, &j.text, cap).is_some()
            {
                out.extend(&j.entries);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn push(list: &mut Vec<u32>, i: u32) {
    if list.last() != Some(&i) {
        list.push(i);
    }
}
