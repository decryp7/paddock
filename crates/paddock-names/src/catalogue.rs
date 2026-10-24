//! Finding things by what the user typed, however it is written (rule 20): parts, joins, separators, case, plurals, filler
//! words, word order and typos. Every match says how it was made and why, and a true tie becomes a short question instead of a
//! guess.
//!
//! The ways an [`Entry`] matches ([`How`]), best first: the user's own alias, the same letters as a spelling (exact), the
//! same words written differently (`graphic builder` is `graphicbuilder.exe`), the words as consecutive parts of the name,
//! words that start the name's words, an alias, every word found somewhere in the name or its other fields in any order, and
//! last a typo. The score is the way's base plus a little for the entry's weight; two entries matching the same way with
//! scores closer than [`TIE_GAP`] are a true tie ([`Search::clarify`]). Typos are used only when nothing matches better.
//!
//! Pure, deterministic (score, then weight, then id) and bounded: a [`Catalogue`] holds at most [`MAX_ENTRIES`] entries and
//! reads at most [`MAX_TEXT_CHARS`] characters of a query.

use crate::distance::{allowed_typos, edit_distance_capped};
use crate::group::{Candidate, vocabulary_from};
use crate::normalise::{NormName, Style, normalise_as, squash, vendor_prefixes};
use crate::segment::Vocabulary;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// Most entries a catalogue holds.
pub const MAX_ENTRIES: usize = 250_000;
/// Most characters of a query that are read.
pub const MAX_TEXT_CHARS: usize = 256;
/// Most characters of any name, field or alias that are read.
pub const MAX_NAME_CHARS: usize = 256;
/// Most names (spellings) of one entry that are read.
pub const MAX_NAMES: usize = 24;
/// Most other fields of one entry that are read.
pub const MAX_FIELDS: usize = 8;
/// Most aliases of each kind of one entry that are read.
pub const MAX_ALIASES: usize = 16;
/// Most matches one search returns.
pub const MAX_MATCHES: usize = 50;
/// Most options in a clarifying question.
pub const MAX_OPTIONS: usize = 3;
/// Scores closer than this, for the same way, are a true tie.
pub const TIE_GAP: f64 = 0.04;
const FILLER: &[&str] = &["the", "a", "an", "my", "our", "that", "this", "please"];
const WEIGHT_SHARE: f64 = 0.02;
/// Shortest word that may be found inside another word (shorter ones must equal or start a word).
const MIN_INSIDE: usize = 3;

/// One thing that can be found.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The caller's id; results carry it. Unique within a catalogue (ties are broken by it).
    pub id: String,
    /// The ways the thing is written, the first is shown: `["CompanyA.Graphic.Builder.exe", "graphicbuilder.exe"]`.
    pub names: Vec<String>,
    /// Other text that also finds it, ranked below the names: a counter's object and instance, a symbol's namespace.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Alternative names (a model's suggestion, a synonym).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Names the user gave it; they outrank everything.
    #[serde(default)]
    pub user_aliases: Vec<String>,
    /// What it is in a word a person says (`process`), for the clarifying question.
    #[serde(default)]
    pub kind: Option<String>,
    /// How common or important it is (an event count); more of it ranks slightly higher.
    #[serde(default)]
    pub weight: u64,
}

impl Entry {
    /// An entry with one name.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Entry {
        Entry {
            id: id.into(),
            names: vec![name.into()],
            ..Entry::default()
        }
    }

    /// Adds other names (spellings).
    pub fn spelled(mut self, names: &[&str]) -> Entry {
        self.names.extend(names.iter().map(|n| n.to_string()));
        self
    }

    /// Sets the other text that finds it.
    pub fn fields(mut self, fields: &[&str]) -> Entry {
        self.fields = fields.iter().map(|n| n.to_string()).collect();
        self
    }

    /// Sets the weight.
    pub fn weight(mut self, weight: u64) -> Entry {
        self.weight = weight;
        self
    }

    /// Sets the kind word.
    pub fn kind(mut self, kind: &str) -> Entry {
        self.kind = Some(kind.to_string());
        self
    }
}

/// Why a catalogue could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamesError {
    /// More than [`MAX_ENTRIES`] entries.
    TooManyEntries(usize),
}

impl fmt::Display for NamesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NamesError::TooManyEntries(n) => {
                write!(
                    f,
                    "{n} names are too many to search by meaning (at most {MAX_ENTRIES}); narrow the scope first"
                )
            }
        }
    }
}

impl std::error::Error for NamesError {}

/// How an entry matched the words, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum How {
    /// The user said so (one of the entry's user aliases).
    UserAlias,
    /// Same letters as a spelling.
    Exact,
    /// The same words written differently (`graphic builder`, `graphicbuilder.exe`).
    Joined,
    /// The words are consecutive parts of the name.
    Parts,
    /// An alias.
    Alias,
    /// Every word starts a word of the name.
    Prefix,
    /// Every word is found in the name or its other fields, in any order.
    Words,
    /// Close in spelling.
    Typo,
}

impl How {
    fn base(self) -> f64 {
        match self {
            How::UserAlias => 1.3,
            How::Exact => 1.0,
            How::Joined => 0.95,
            How::Parts => 0.85,
            How::Alias => 0.8,
            How::Prefix => 0.7,
            How::Words => 0.65,
            How::Typo => 0.55,
        }
    }

    /// The id shown to the assistant and the UI.
    pub fn id(self) -> &'static str {
        match self {
            How::UserAlias => "userAlias",
            How::Exact => "exact",
            How::Joined => "joined",
            How::Parts => "parts",
            How::Alias => "alias",
            How::Prefix => "prefix",
            How::Words => "words",
            How::Typo => "typo",
        }
    }
}

/// An entry that matched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    /// The entry's id.
    pub id: String,
    /// The entry's shown name.
    pub display: String,
    /// How it matched.
    pub how: How,
    /// Higher is better; only comparable within one search.
    pub score: f64,
    /// A phrase saying why it matched.
    pub why: String,
}

/// A short question for a true tie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clarification {
    /// "By 'x', do you mean A or B?"
    pub question: String,
    /// The ids of the options, in the order of the question.
    pub options: Vec<String>,
}

/// The result of a search.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    /// The matches, best first.
    pub matches: Vec<Match>,
    /// Set when the top matches are a true tie.
    pub clarify: Option<Clarification>,
}

struct Prepared {
    id: String,
    display: String,
    kind: Option<String>,
    weight: u64,
    /// Each spelling squashed (letters and digits), and its file stem squashed.
    spellings: Vec<String>,
    /// The distinct token lists of the names.
    names: Vec<Vec<String>>,
    /// Each name's tokens joined.
    joined: Vec<String>,
    /// Tokens of the other fields.
    field_tokens: Vec<String>,
    aliases: Vec<String>,
    user_aliases: Vec<String>,
}

/// A searchable set of entries.
pub struct Catalogue {
    vocab: Vocabulary,
    style: Style,
    entries: Vec<Prepared>,
    max_weight: u64,
}

impl Catalogue {
    /// Builds a catalogue of [`Style::Path`] names; the vocabulary is learned from the names themselves
    /// ([`vocabulary_from`]) so splits follow how the data writes things.
    pub fn new(entries: Vec<Entry>) -> Result<Catalogue, NamesError> {
        Self::build(entries, Style::Path, None)
    }

    /// Builds a catalogue of plain words and codes ([`Style::Text`]): counters, symbols, model names.
    pub fn text(entries: Vec<Entry>) -> Result<Catalogue, NamesError> {
        Self::build(entries, Style::Text, None)
    }

    /// Builds a catalogue with a vocabulary of the caller's.
    pub fn with_vocabulary(
        entries: Vec<Entry>,
        style: Style,
        vocab: Vocabulary,
    ) -> Result<Catalogue, NamesError> {
        Self::build(entries, style, Some(vocab))
    }

    fn build(
        entries: Vec<Entry>,
        style: Style,
        vocab: Option<Vocabulary>,
    ) -> Result<Catalogue, NamesError> {
        if entries.len() > MAX_ENTRIES {
            return Err(NamesError::TooManyEntries(entries.len()));
        }
        let cut = |s: &String| s.chars().take(MAX_NAME_CHARS).collect::<String>();
        let entries: Vec<Entry> = entries
            .into_iter()
            .map(|e| Entry {
                id: e.id,
                names: e.names.iter().take(MAX_NAMES).map(cut).collect(),
                fields: e.fields.iter().take(MAX_FIELDS).map(cut).collect(),
                aliases: e.aliases.iter().take(MAX_ALIASES).map(cut).collect(),
                user_aliases: e.user_aliases.iter().take(MAX_ALIASES).map(cut).collect(),
                kind: e.kind,
                weight: e.weight,
            })
            .collect();
        let vocab = vocab.unwrap_or_else(|| {
            let learn: Vec<Candidate> = entries
                .iter()
                .flat_map(|e| {
                    e.names.iter().map(|n| Candidate {
                        name: n.clone(),
                        count: e.weight,
                    })
                })
                .collect();
            vocabulary_from(&learn)
        });
        let vendors: HashSet<String> = if style == Style::Path {
            vendor_prefixes(
                entries
                    .iter()
                    .flat_map(|e| e.names.iter().map(String::as_str)),
            )
        } else {
            HashSet::new()
        };
        let max_weight = entries.iter().map(|e| e.weight).max().unwrap_or(0);
        let prepared = entries
            .into_iter()
            .map(|e| prepare(e, style, &vocab, &vendors))
            .collect();
        Ok(Catalogue {
            vocab,
            style,
            entries: prepared,
            max_weight,
        })
    }

    /// How many entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The vocabulary the splits use.
    pub fn vocabulary(&self) -> &Vocabulary {
        &self.vocab
    }

    /// The entries the words refer to, best first, at most `limit` (1 to [`MAX_MATCHES`]). An empty query matches nothing.
    pub fn search(&self, text: &str, limit: usize) -> Search {
        let q = Query::parse(text, self.style, &self.vocab);
        if q.squashed.is_empty() {
            return Search::default();
        }
        let top = (self.max_weight as f64 + 1.0).ln().max(f64::MIN_POSITIVE);
        let mut found: Vec<(Match, u64)> = Vec::new();
        for p in &self.entries {
            let Some((how, why, tier)) = self.how(p, &q) else {
                continue;
            };
            let share = WEIGHT_SHARE * ((p.weight as f64 + 1.0).ln() / top);
            let score = how.base() + share - 0.01 * tier;
            found.push((
                Match {
                    id: p.id.clone(),
                    display: p.display.clone(),
                    how,
                    score,
                    why,
                },
                p.weight,
            ));
        }
        found.sort_by(|a, b| {
            b.0.score
                .total_cmp(&a.0.score)
                .then(b.1.cmp(&a.1))
                .then_with(|| a.0.id.cmp(&b.0.id))
        });
        // Typos only when nothing matches better.
        if found.iter().any(|m| m.0.how != How::Typo) {
            found.retain(|m| m.0.how != How::Typo);
        }
        found.truncate(limit.clamp(1, MAX_MATCHES));
        let matches: Vec<Match> = found.into_iter().map(|m| m.0).collect();
        let clarify = self.clarification(&matches, text);
        Search { matches, clarify }
    }

    /// How `p` matches, the phrase saying so and a tier (0 for the name, up to 4 for the fields) that orders matches of
    /// one way.
    fn how(&self, p: &Prepared, q: &Query) -> Option<(How, String, f64)> {
        if p.user_aliases.iter().any(|a| squash(a) == q.squashed) {
            return Some((How::UserAlias, "you named it this".into(), 0.0));
        }
        if p.spellings.contains(&q.squashed) {
            return Some((How::Exact, "the name as written".into(), 0.0));
        }
        if !q.joined.is_empty() && p.joined.contains(&q.joined) {
            let phrase = if p.names.iter().any(|n| n.len() == q.tokens.len()) {
                "same name spelled differently (case, separators, version)"
            } else {
                "same name written as one word"
            };
            return Some((How::Joined, phrase.into(), 0.0));
        }
        if !q.tokens.is_empty()
            && p.names.iter().any(|n| {
                q.tokens.len() < n.len()
                    && n.windows(q.tokens.len())
                        .any(|w| w.iter().zip(&q.tokens).all(|(a, b)| a == b))
            })
        {
            return Some((How::Parts, "your words are parts of this name".into(), 0.0));
        }
        if p.aliases.iter().any(|a| squash(a) == q.squashed) {
            return Some((How::Alias, "a known alias of this name".into(), 0.0));
        }
        if !q.tokens.is_empty()
            && p.names.iter().any(|n| {
                q.tokens
                    .iter()
                    .all(|t| t.chars().count() >= 2 && n.iter().any(|k| k.starts_with(t.as_str())))
            })
        {
            return Some((
                How::Prefix,
                "your words start the words of this name".into(),
                0.0,
            ));
        }
        if let Some(tier) = words_tier(p, q) {
            return Some((How::Words, "every word is in this name".into(), tier));
        }
        self.typo(p, q)
    }

    fn typo(&self, p: &Prepared, q: &Query) -> Option<(How, String, f64)> {
        // A typo has a word nobody knows; "grape builder" is two real words, so it is another thing.
        if q.squashed.chars().count() < 4 || q.tokens.iter().all(|t| self.vocab.knows(t)) {
            return None;
        }
        let word_by_word = p
            .names
            .iter()
            .filter(|n| n.len() == q.tokens.len() && n.len() > 1)
            .find_map(|n| {
                // Each word may be a letter or two off, but a different word is another thing.
                q.tokens
                    .iter()
                    .zip(n)
                    .map(|(a, b)| edit_distance_capped(a, b, (b.chars().count() / 3).max(1)))
                    .sum::<Option<usize>>()
            });
        let joined = || {
            p.joined.iter().find_map(|j| {
                edit_distance_capped(&q.squashed, j, allowed_typos(j.chars().count()))
            })
        };
        let anywhere = || fuzzy_words(p, q);
        let d = [word_by_word, joined(), anywhere()]
            .into_iter()
            .flatten()
            .min()?;
        Some((
            How::Typo,
            format!(
                "close spelling ({d} letter{} off)",
                if d == 1 { "" } else { "s" }
            ),
            d as f64,
        ))
    }

    fn clarification(&self, found: &[Match], text: &str) -> Option<Clarification> {
        let top = found.first()?;
        if top.how == How::UserAlias {
            return None;
        }
        let tied: Vec<&Match> = found
            .iter()
            .take_while(|m| m.how == top.how && top.score - m.score < TIE_GAP)
            .take(MAX_OPTIONS)
            .collect();
        if tied.len() < 2 {
            return None;
        }
        let kinds: std::collections::HashMap<&str, &str> = self
            .entries
            .iter()
            .filter_map(|e| e.kind.as_deref().map(|k| (e.id.as_str(), k)))
            .collect();
        let names: Vec<String> = tied
            .iter()
            .map(|m| match kinds.get(m.id.as_str()) {
                Some(k) => format!("{} ({k})", m.display),
                None => m.display.clone(),
            })
            .collect();
        let (last, init) = names.split_last()?;
        let list = format!("{} or {last}", init.join(", "));
        Some(Clarification {
            question: format!("By '{}', do you mean {list}?", text.trim()),
            options: tied.iter().map(|m| m.id.clone()).collect(),
        })
    }
}

/// The parsed query: the squashed letters, the tidy words and the words joined.
struct Query {
    squashed: String,
    tokens: Vec<String>,
    joined: String,
}

impl Query {
    fn parse(text: &str, style: Style, vocab: &Vocabulary) -> Query {
        let text: String = text.trim().chars().take(MAX_TEXT_CHARS).collect();
        let squashed = squash(&text);
        if squashed.is_empty() {
            return Query {
                squashed,
                tokens: Vec::new(),
                joined: String::new(),
            };
        }
        let tokens = tidy(
            normalise_as(&text, style, vocab, &HashSet::new()).tokens,
            vocab,
        );
        let joined = tokens.concat();
        Query {
            squashed,
            tokens,
            joined,
        }
    }
}

/// The query's words without filler (`the`, `my`) and with plurals made singular when the singular is a known word.
fn tidy(tokens: Vec<String>, vocab: &Vocabulary) -> Vec<String> {
    let kept: Vec<String> = tokens
        .iter()
        .filter(|t| !FILLER.contains(&t.as_str()))
        .cloned()
        .collect();
    let tokens = if kept.is_empty() { tokens } else { kept };
    tokens
        .into_iter()
        .map(|t| match t.strip_suffix('s') {
            Some(one)
                if one.chars().count() >= 3
                    && !one.ends_with('s')
                    && vocab.knows(one)
                    && !vocab.knows(&t) =>
            {
                one.to_string()
            }
            _ => t,
        })
        .collect()
}

fn prepare(e: Entry, style: Style, vocab: &Vocabulary, vendors: &HashSet<String>) -> Prepared {
    let norm: Vec<NormName> = e
        .names
        .iter()
        .map(|n| normalise_as(n, style, vocab, vendors))
        .collect();
    let mut spellings: Vec<String> = Vec::new();
    for n in &e.names {
        for s in [
            squash(n),
            squash(crate::normalise::strip_extension(
                crate::normalise::file_name(n),
            )),
        ] {
            if !s.is_empty() && !spellings.contains(&s) {
                spellings.push(s);
            }
        }
    }
    let mut names: Vec<Vec<String>> = Vec::new();
    for n in &norm {
        if !n.tokens.is_empty() && !names.contains(&n.tokens) {
            names.push(n.tokens.clone());
        }
    }
    let joined = names.iter().map(|n| n.concat()).collect();
    let field_tokens = e
        .fields
        .iter()
        .flat_map(|f| normalise_as(f, Style::Text, vocab, &HashSet::new()).tokens)
        .collect();
    let display = e
        .names
        .first()
        .map(|n| match norm.first() {
            Some(n0) if !n0.display.is_empty() && style == Style::Path => n0.display.clone(),
            _ => n.clone(),
        })
        .unwrap_or_else(|| e.id.clone());
    Prepared {
        id: e.id,
        display,
        kind: e.kind,
        weight: e.weight,
        spellings,
        names,
        joined,
        field_tokens,
        aliases: e.aliases,
        user_aliases: e.user_aliases,
    }
}

/// Every query word found in the name or its other fields; the tier is the average of the words' tiers (a word equal to a name
/// word is 0, starting one 1, equal to a field word 2, starting one 3, inside a name word 4, inside a field word 5).
fn words_tier(p: &Prepared, q: &Query) -> Option<f64> {
    if q.tokens.is_empty() {
        return None;
    }
    let mut total = 0usize;
    for t in &q.tokens {
        let tier = p
            .names
            .iter()
            .filter_map(|n| word_tier(t, n.iter().map(String::as_str), [0, 1, 4]))
            .min()
            .into_iter()
            .chain(word_tier(
                t,
                p.field_tokens.iter().map(String::as_str),
                [3, 4, 4],
            ))
            .min()?;
        total += tier;
    }
    Some(total as f64 / q.tokens.len() as f64)
}

/// The best tier of `t` among `words`: equal, starts, contains.
fn word_tier<'a>(
    t: &str,
    words: impl Iterator<Item = &'a str>,
    tiers: [usize; 3],
) -> Option<usize> {
    words
        .filter_map(|w| {
            if w == t {
                Some(tiers[0])
            } else if w.starts_with(t) {
                Some(tiers[1])
            } else if t.chars().count() >= MIN_INSIDE && w.contains(t) {
                Some(tiers[2])
            } else {
                None
            }
        })
        .min()
}

/// Every query word is a word of the entry or a typo of one (a word under four letters must be exact), at least one a typo.
/// Returns the letters off in all.
fn fuzzy_words(p: &Prepared, q: &Query) -> Option<usize> {
    if q.tokens.is_empty() {
        return None;
    }
    let all: Vec<&str> = p
        .names
        .iter()
        .flatten()
        .chain(&p.field_tokens)
        .map(String::as_str)
        .collect();
    let mut off = 0;
    for t in &q.tokens {
        if all.iter().any(|w| w.starts_with(t.as_str())) {
            continue;
        }
        if t.chars().count() < 4 {
            return None;
        }
        let best = all
            .iter()
            .filter_map(|w| {
                edit_distance_capped(
                    t,
                    w,
                    allowed_typos(w.chars().count().max(t.chars().count())),
                )
            })
            .min()?;
        off += best;
    }
    (off > 0).then_some(off)
}

impl From<&crate::group::Group> for Entry {
    /// The group as an entry: its key is the id, its spellings the names, its count the weight.
    fn from(g: &crate::group::Group) -> Entry {
        Entry {
            id: g.key.clone(),
            names: g.spellings.iter().map(|s| s.name.clone()).collect(),
            weight: g.count,
            ..Entry::default()
        }
    }
}
