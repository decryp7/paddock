//! Grouping the spellings of one name into one entity with variants.
//!
//! `CompanyA.Graphic.Builder.exe`, `graphicbuilder.exe` and `GraphicBuilder.exe` become one [`NameGroup`] whose
//! canonical spelling is the most frequent one and whose variants carry their own counts. Names group by their
//! normalised words (see [`super::normalise`]) within a scope (an entity kind or a field). A group whose first word is
//! unknown (a vendor) also joins the group that is the same name without it. Names that co-occur in the same events
//! and look alike can be joined too ([`merge_cooccurring`]).

use super::context::Cooccurrence;
use super::normalise::{normalise, vendor_prefixes};
use super::segment::Vocabulary;
use crate::catalogue::{Candidate, Scope};
use crate::entities::EntityKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Most spellings kept per group.
pub const MAX_SPELLINGS: usize = 24;

/// One way the logs wrote the name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spelling {
    pub name: String,
    pub count: u64,
}

/// One entity: all the spellings that mean the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameGroup {
    pub scope: Scope,
    /// The normalised words joined by a space: the group's identity (`graphic builder`).
    pub key: String,
    /// Title-cased words: `Graphic Builder`.
    pub display: String,
    /// The most frequent spelling first.
    pub spellings: Vec<Spelling>,
    /// Events with any spelling.
    pub count: u64,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    pub seen_in: Vec<String>,
}

impl NameGroup {
    /// The most frequent spelling.
    pub fn canonical(&self) -> &str {
        self.spellings.first().map(|s| s.name.as_str()).unwrap_or(&self.display)
    }

    /// Stable id of the group within its scope.
    pub fn id(&self) -> String {
        format!("{}|{}", self.scope.encode(), self.key)
    }

    /// What the group is, for words: the entity kind's id or the field name.
    pub fn kind_label(&self) -> String {
        match &self.scope {
            Scope::Entity(k) => k.id().to_string(),
            Scope::Field(f) => f.clone(),
        }
    }

    pub(super) fn absorb(&mut self, other: NameGroup) {
        self.count += other.count;
        self.first_ts = min_opt(self.first_ts, other.first_ts);
        self.last_ts = max_opt(self.last_ts, other.last_ts);
        for s in other.seen_in {
            if !self.seen_in.contains(&s) {
                self.seen_in.push(s);
            }
        }
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
        self.seen_in.sort();
        self.seen_in.truncate(crate::catalogue::MAX_SEEN_IN);
    }
}

fn min_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) | (None, x) => x,
    }
}

fn max_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) | (None, x) => x,
    }
}

/// Whether names of this scope are grouped (names for things; not addresses, URLs, ids or codes).
pub fn groupable(scope: &Scope) -> bool {
    match scope {
        Scope::Entity(k) => matches!(
            k,
            EntityKind::Executable | EntityKind::Library | EntityKind::File | EntityKind::Exception
        ),
        Scope::Field(_) => true,
    }
}

/// Groups the groupable candidates. Deterministic: most events first, then by key.
pub fn group(candidates: &[Candidate], vocab: &Vocabulary) -> Vec<NameGroup> {
    let usable: Vec<&Candidate> = candidates.iter().filter(|c| groupable(&c.scope)).collect();
    let vendors = vendor_prefixes(usable.iter().map(|c| c.name.as_str()));
    let mut by_key: BTreeMap<(Scope, String), NameGroup> = BTreeMap::new();
    for c in usable {
        let n = normalise(&c.name, vocab, &vendors);
        let key = n.key();
        if key.is_empty() {
            continue;
        }
        let g = NameGroup {
            scope: c.scope.clone(),
            key: key.clone(),
            display: n.display,
            spellings: vec![Spelling {
                name: c.name.clone(),
                count: c.count,
            }],
            count: c.count,
            first_ts: c.first_ts,
            last_ts: c.last_ts,
            seen_in: c.seen_in.clone(),
        };
        match by_key.get_mut(&(c.scope.clone(), key.clone())) {
            Some(existing) => existing.absorb(g),
            None => {
                by_key.insert((c.scope.clone(), key), g);
            }
        }
    }
    // A vendor that appears once (so was not detected) is an unknown first word: join the group without it.
    let keys: Vec<(Scope, String)> = by_key.keys().cloned().collect();
    for (scope, key) in keys {
        let words: Vec<&str> = key.split(' ').collect();
        // Cut one or two leading words that look like a vendor: unknown to the vocabulary, or a single letter.
        let target = (1..=2.min(words.len().saturating_sub(1)))
            .filter(|cut| words[..*cut].iter().any(|w| w.chars().count() == 1 || !vocab.is_common(w)))
            .filter(|cut| !words[..*cut].iter().any(|w| w.chars().any(|c| c.is_ascii_digit())))
            .map(|cut| words[cut..].join(" "))
            .find(|rest| by_key.contains_key(&(scope.clone(), rest.clone())));
        let Some(rest) = target else { continue };
        if let Some(g) = by_key.remove(&(scope.clone(), key)) {
            if let Some(t) = by_key.get_mut(&(scope, rest)) {
                t.absorb(g);
            }
        }
    }
    let mut v: Vec<NameGroup> = by_key.into_values().collect();
    for g in &mut v {
        g.finish();
        g.display = display_of(&g.key);
    }
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
    v
}

fn display_of(key: &str) -> String {
    key.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What one [`merge_cooccurring_stats`] pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeStats {
    /// Groups joined into another.
    pub merged: usize,
    /// Groups with a spelling in the co-occurrence data: the only ones that can join.
    pub candidates: usize,
    /// Pairs whose names and events were compared (at most `candidates` squared, usually far fewer).
    pub comparisons: usize,
}

/// A group name's letters and its trigrams, computed once per group (not once per comparison).
struct Gram {
    squashed: String,
    trigrams: HashSet<[char; 3]>,
}

impl Gram {
    fn of(key: &str) -> Gram {
        let squashed = super::resolve::squash(key);
        let trigrams = trigram_set(&squashed);
        Gram { squashed, trigrams }
    }

    /// Whether two names share at least half of the trigrams of the shorter one (very short names: when equal).
    fn similar(&self, other: &Gram) -> bool {
        if self.trigrams.is_empty() || other.trigrams.is_empty() {
            return self.squashed == other.squashed;
        }
        let common = self.trigrams.iter().filter(|t| other.trigrams.contains(*t)).count();
        common * 2 >= self.trigrams.len().min(other.trigrams.len())
    }
}

/// Joins groups of one scope whose names look alike and that appear in the same events: if at least
/// `min_strength` (above 0) of the rarer one's events also hold the other, and their letters share at least half of
/// their trigrams. Returns the number of merges. Deterministic.
pub fn merge_cooccurring(groups: &mut Vec<NameGroup>, cooc: &Cooccurrence, min_strength: f64) -> usize {
    merge_cooccurring_stats(groups, cooc, min_strength).merged
}

/// [`merge_cooccurring`] with its counts. Only groups that have a spelling in `cooc` can join (the others have no
/// events to vouch for them), and of those only pairs that share a trigram are compared: the trigrams of every
/// candidate are computed once and looked up in an index, so the work follows the number of candidates (a few
/// hundred), not the number of groups (up to 50,000).
pub fn merge_cooccurring_stats(groups: &mut Vec<NameGroup>, cooc: &Cooccurrence, min_strength: f64) -> MergeStats {
    let min_strength = min_strength.max(f64::MIN_POSITIVE);
    let candidates: Vec<usize> = (0..groups.len())
        .filter(|i| groups[*i].spellings.iter().any(|s| cooc.knows(&s.name)))
        .collect();
    let mut stats = MergeStats {
        candidates: candidates.len(),
        ..MergeStats::default()
    };
    if candidates.len() >= 2 {
        let grams: Vec<Gram> = candidates.iter().map(|i| Gram::of(&groups[*i].key)).collect();
        let mut by_trigram: HashMap<[char; 3], Vec<usize>> = HashMap::new();
        let mut by_short: HashMap<&str, Vec<usize>> = HashMap::new();
        for (p, g) in grams.iter().enumerate() {
            if g.trigrams.is_empty() {
                by_short.entry(g.squashed.as_str()).or_default().push(p);
            }
            for t in &g.trigrams {
                by_trigram.entry(*t).or_default().push(p);
            }
        }
        let mut slots: Vec<Option<NameGroup>> = std::mem::take(groups).into_iter().map(Some).collect();
        for (p, &i) in candidates.iter().enumerate() {
            if slots[i].is_none() {
                continue;
            }
            let mut neighbours: Vec<usize> = grams[p]
                .trigrams
                .iter()
                .filter_map(|t| by_trigram.get(t))
                .chain(by_short.get(grams[p].squashed.as_str()).filter(|_| grams[p].trigrams.is_empty()))
                .flatten()
                .copied()
                .filter(|q| *q > p)
                .collect();
            neighbours.sort_unstable();
            neighbours.dedup();
            for q in neighbours {
                let j = candidates[q];
                let (Some(a), Some(b)) = (&slots[i], &slots[j]) else { continue };
                if a.scope != b.scope {
                    continue;
                }
                stats.comparisons += 1;
                if grams[p].similar(&grams[q]) && together(a, b, cooc, min_strength) {
                    if let Some(g) = slots[j].take() {
                        if let Some(target) = slots[i].as_mut() {
                            target.absorb(g);
                        }
                        stats.merged += 1;
                    }
                }
            }
        }
        *groups = slots.into_iter().flatten().collect();
    }
    groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
    stats
}

/// Positions of the groups among the first `limit` (most events first) that have a look-alike of their scope in the
/// same range: the ones worth reading events for before [`merge_cooccurring`].
pub fn look_alikes(groups: &[NameGroup], limit: usize) -> Vec<usize> {
    let top = &groups[..groups.len().min(limit)];
    let grams: Vec<Gram> = top.iter().map(|g| Gram::of(&g.key)).collect();
    let mut out = Vec::new();
    for (i, a) in top.iter().enumerate() {
        if top
            .iter()
            .enumerate()
            .any(|(j, b)| i != j && a.scope == b.scope && grams[i].similar(&grams[j]))
        {
            out.push(i);
        }
    }
    out
}

/// Whether two groups could appear in one event: the same scope and look-alike names, time ranges that overlap and, when
/// both lists of sources are complete (shorter than [`MAX_SEEN_IN`](crate::catalogue::MAX_SEEN_IN)), a source in common.
/// Unknown ranges and lists that were cut are taken as possible.
fn could_share_events(a: &NameGroup, b: &NameGroup) -> bool {
    let ranges_meet = match (a.first_ts, a.last_ts, b.first_ts, b.last_ts) {
        (Some(af), Some(al), Some(bf), Some(bl)) => af <= bl && bf <= al,
        _ => true,
    };
    let complete = |g: &NameGroup| !g.seen_in.is_empty() && g.seen_in.len() < crate::catalogue::MAX_SEEN_IN;
    let sources_meet = !(complete(a) && complete(b)) || a.seen_in.iter().any(|s| b.seen_in.contains(s));
    ranges_meet && sources_meet
}

/// [`look_alikes`] that also could appear together in an event ([`could_share_events`]): a name whose look-alikes are in
/// other sources or at other times cannot join them, so its events need not be read.
pub fn joinable_look_alikes(groups: &[NameGroup], limit: usize) -> Vec<usize> {
    let top = &groups[..groups.len().min(limit)];
    let grams: Vec<Gram> = top.iter().map(|g| Gram::of(&g.key)).collect();
    let mut out = Vec::new();
    for (i, a) in top.iter().enumerate() {
        if top
            .iter()
            .enumerate()
            .any(|(j, b)| i != j && a.scope == b.scope && grams[i].similar(&grams[j]) && could_share_events(a, b))
        {
            out.push(i);
        }
    }
    out
}

fn together(a: &NameGroup, b: &NameGroup, cooc: &Cooccurrence, min: f64) -> bool {
    a.spellings
        .iter()
        .any(|x| b.spellings.iter().any(|y| cooc.strength(&x.name, &y.name) >= min))
}

#[cfg(test)]
fn similar(a: &str, b: &str) -> bool {
    Gram::of(a).similar(&Gram::of(b))
}

fn trigram_set(s: &str) -> HashSet<[char; 3]> {
    let c: Vec<char> = s.chars().collect();
    c.windows(3).map(|w| [w[0], w[1], w[2]]).collect()
}

/// Index from lower-cased spelling to the position of its group in `groups`.
pub fn spelling_index(groups: &[NameGroup]) -> HashMap<(Scope, String), usize> {
    let mut m = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        for s in &g.spellings {
            m.insert((g.scope.clone(), s.name.to_lowercase()), i);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(name: &str, n: u64) -> Candidate {
        Candidate {
            scope: Scope::Entity(EntityKind::Executable),
            name: name.into(),
            count: n,
            first_ts: Some(n as i64),
            last_ts: Some(100 + n as i64),
            seen_in: vec![format!("s{n}.log")],
        }
    }

    #[test]
    fn absorbing_a_group_adds_the_counts_of_a_shared_spelling() {
        let mut a = group(&[cand("graphicbuilder.exe", 4)], &Vocabulary::builtin()).remove(0);
        let b = group(
            &[cand("graphicbuilder.exe", 6), cand("Graphic.Builder.exe", 1)],
            &Vocabulary::builtin(),
        )
        .remove(0);
        a.absorb(b);
        assert_eq!(a.spellings[0].count, 10);
        assert_eq!(a.spellings.len(), 2);
        assert_eq!(a.count, 11);
    }

    #[test]
    fn a_field_group_is_named_by_its_field() {
        let mut c = cand("alpha", 1);
        c.scope = Scope::Field("host".into());
        let g = group(&[c], &Vocabulary::builtin());
        assert_eq!(g[0].kind_label(), "host");
    }

    #[test]
    fn very_short_names_are_alike_only_when_equal() {
        assert!(similar("ab", "ab"));
        assert!(!similar("ab", "ba"));
    }

    #[test]
    fn spellings_group_into_one_entity() {
        let c = [
            cand("CompanyA.Graphic.Builder.exe", 5),
            cand("graphicbuilder.exe", 12),
            cand("GraphicBuilder.exe", 3),
            cand("graphic-builder", 1),
            cand("CompanyA.Tray.Agent.exe", 7),
            cand("other.exe", 2),
        ];
        let g = group(&c, &Vocabulary::builtin());
        let gb = g.iter().find(|g| g.key == "graphic builder").unwrap();
        assert_eq!(gb.count, 21);
        assert_eq!(gb.canonical(), "graphicbuilder.exe");
        assert_eq!(gb.spellings.len(), 4);
        assert_eq!(gb.display, "Graphic Builder");
        assert_eq!(gb.first_ts, Some(1));
        assert_eq!(gb.last_ts, Some(112));
        assert_eq!(gb.seen_in.len(), 4);
        assert_eq!(gb.id(), "executable|graphic builder");
        assert_eq!(gb.kind_label(), "executable");
        assert!(g.iter().any(|g| g.key == "tray agent"));
        assert_eq!(g.len(), 3);
    }

    #[test]
    fn single_vendor_prefix_joins_the_group_without_it() {
        let c = [cand("CompanyA.Graphic.Builder.exe", 5), cand("graphicbuilder.exe", 12)];
        let g = group(&c, &Vocabulary::builtin());
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].spellings.len(), 2);
        // A known first word is not a vendor.
        let c = [cand("Graphic.Builder.Core.exe", 5), cand("builder.core.exe", 12)];
        assert_eq!(group(&c, &Vocabulary::builtin()).len(), 2);
    }

    #[test]
    fn kinds_stay_apart_and_ids_are_not_grouped() {
        let mut dll = cand("graphicbuilder.dll", 4);
        dll.scope = Scope::Entity(EntityKind::Library);
        let mut ip = cand("10.0.0.1", 9);
        ip.scope = Scope::Entity(EntityKind::Ip);
        let g = group(&[cand("graphicbuilder.exe", 1), dll, ip, cand("....", 3)], &Vocabulary::builtin());
        assert_eq!(g.len(), 2);
        assert!(!groupable(&Scope::Entity(EntityKind::Guid)));
        assert!(groupable(&Scope::Field("ProcessName".into())));
    }

    #[test]
    fn cooccurring_lookalikes_merge() {
        let c = [cand("gfxbuilder.exe", 5), cand("gfx-bilder.exe", 4), cand("tray.exe", 3)];
        let mut g = group(&c, &Vocabulary::builtin());
        assert_eq!(g.len(), 3);
        let mut cooc = Cooccurrence::default();
        cooc.add_event(&["gfxbuilder.exe".to_string(), "gfx-bilder.exe".to_string(), "tray.exe".to_string()]);
        cooc.add_event(&["gfxbuilder.exe".to_string(), "gfx-bilder.exe".to_string()]);
        assert_eq!(merge_cooccurring(&mut g, &cooc, 0.8), 1);
        assert_eq!(g.len(), 2);
        // Unrelated names are never joined, however often they co-occur.
        assert_eq!(merge_cooccurring(&mut g, &cooc, 0.8), 0);
        let idx = spelling_index(&g);
        assert_eq!(idx.len(), 3);
    }

    /// The merge as it was before blocking: every pair of the same scope, trigram sets built per comparison.
    fn merge_every_pair(groups: &mut Vec<NameGroup>, cooc: &Cooccurrence, min: f64) -> usize {
        let mut merged = 0;
        let mut i = 0;
        while i < groups.len() {
            let mut j = i + 1;
            while j < groups.len() {
                if groups[i].scope == groups[j].scope
                    && similar(&groups[i].key, &groups[j].key)
                    && together(&groups[i], &groups[j], cooc, min)
                {
                    let g = groups.remove(j);
                    groups[i].absorb(g);
                    merged += 1;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
        merged
    }

    /// A small deterministic generator, so the fixtures need no dependency and never change.
    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    /// `n` executable names built from a few syllables, so that many are look-alikes of each other, and some files.
    fn crowd(n: usize, seed: &mut u64) -> Vec<NameGroup> {
        let syl = ["ba", "ko", "du", "re", "mi", "ta", "lo", "ne"];
        let mut cands = Vec::new();
        for i in 0..n {
            let name: String = (0..3 + (next(seed) % 2) as usize).map(|_| syl[(next(seed) % 8) as usize]).collect();
            let mut c = cand(&format!("{name}.exe"), 1 + next(seed) % 90);
            if i % 5 == 0 {
                c.scope = Scope::Entity(EntityKind::File);
            }
            cands.push(c);
        }
        group(&cands, &Vocabulary::builtin())
    }

    fn shape(groups: &[NameGroup]) -> Vec<(String, Vec<String>, u64)> {
        groups
            .iter()
            .map(|g| (g.id(), g.spellings.iter().map(|s| s.name.clone()).collect(), g.count))
            .collect()
    }

    #[test]
    fn blocking_gives_the_result_of_comparing_every_pair() {
        for round in 1..=12u64 {
            let mut seed = 0x9e37_79b9_7f4a_7c15 ^ round;
            let groups = crowd(70, &mut seed);
            assert!(groups.len() > 20, "{}", groups.len());
            let mut cooc = Cooccurrence::default();
            for _ in 0..60 {
                let names: Vec<String> = (0..2 + next(&mut seed) % 3)
                    .map(|_| groups[(next(&mut seed) % groups.len() as u64) as usize].canonical().to_string())
                    .collect();
                cooc.add_event(&names);
            }
            let (mut fast, mut slow) = (groups.clone(), groups);
            let stats = merge_cooccurring_stats(&mut fast, &cooc, 0.5);
            let merged = merge_every_pair(&mut slow, &cooc, 0.5);
            assert_eq!(stats.merged, merged, "round {round}");
            assert_eq!(shape(&fast), shape(&slow), "round {round}");
            assert!(stats.comparisons <= stats.candidates * stats.candidates, "{stats:?}");
        }
    }

    #[test]
    fn only_groups_with_events_are_compared() {
        // 3,000 look-alike groups; 12 of them appear in the events: the comparisons follow those, not 3,000 x 3,000.
        let mut seed = 7;
        let mut groups = crowd(3_000, &mut seed);
        assert!(groups.len() > 1_000);
        let mut cooc = Cooccurrence::default();
        let known: Vec<String> = groups.iter().take(12).map(|g| g.canonical().to_string()).collect();
        for pair in known.chunks(2) {
            cooc.add_event(pair);
        }
        let stats = merge_cooccurring_stats(&mut groups, &cooc, 0.6);
        // The events name spellings, not scopes: a file with the same spelling as a listed program counts too.
        assert!((12..=24).contains(&stats.candidates), "{stats:?}");
        assert!(stats.comparisons <= stats.candidates * stats.candidates, "{stats:?}");
    }

    /// The gate by counting, not timing (the 2026-11-06 idle measurement: comparing every pair of 23,224 groups was 114 s of
    /// a 124 s rebuild): 23,000 groups in families of three look-alike spellings, ten of them in the events. Only those are
    /// candidates, and the comparisons follow the candidates, not the groups.
    #[test]
    fn twenty_three_thousand_groups_cost_the_comparisons_of_their_ten_candidates() {
        let mut cands = Vec::new();
        for family in 0..7_667u32 {
            for variant in ["a", "b", "c"] {
                cands.push(cand(&format!("fam{family:05}zq{variant}.exe"), 5));
            }
        }
        let mut groups = group(&cands, &Vocabulary::builtin());
        assert!(groups.len() >= 23_000, "{} groups", groups.len());
        let mut cooc = Cooccurrence::default();
        let known: Vec<String> = groups.iter().take(10).map(|g| g.canonical().to_string()).collect();
        for pair in known.chunks(2) {
            cooc.add_event(pair);
        }
        let stats = merge_cooccurring_stats(&mut groups, &cooc, 0.6);
        assert!(stats.candidates >= 10 && stats.candidates <= 2 * 10, "{stats:?}");
        assert!(stats.comparisons <= stats.candidates * stats.candidates, "{stats:?}");
    }

    #[test]
    fn a_name_without_events_never_joins_and_a_zero_strength_does_not_join_everything() {
        let c = [cand("gfxbuilder.exe", 5), cand("gfx-bilder.exe", 4)];
        let mut g = group(&c, &Vocabulary::builtin());
        let mut cooc = Cooccurrence::default();
        cooc.add_event(&["gfxbuilder.exe".to_string()]);
        let stats = merge_cooccurring_stats(&mut g, &cooc, 0.0);
        assert_eq!((stats.merged, stats.candidates, stats.comparisons), (0, 1, 0));
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn names_that_cannot_share_an_event_are_not_worth_reading() {
        let mk = |name: &str, src: &[&str], span: (i64, i64)| {
            let mut c = cand(name, 5);
            c.seen_in = src.iter().map(|s| s.to_string()).collect();
            c.first_ts = Some(span.0);
            c.last_ts = Some(span.1);
            c
        };
        let groups = group(
            &[
                mk("gfxbuilder.exe", &["a.log"], (0, 10)),
                mk("gfx-bilder.exe", &["a.log"], (5, 20)),
                mk("gfxbuilders.exe", &["b.log"], (5, 20)),
                mk("gfxbuilder2.exe", &["a.log"], (50, 60)),
                mk("tray.exe", &["a.log"], (0, 10)),
            ],
            &Vocabulary::builtin(),
        );
        let names = |ix: Vec<usize>| ix.into_iter().map(|i| groups[i].key.clone()).collect::<Vec<_>>();
        let every = names(look_alikes(&groups, 50));
        let joinable = names(joinable_look_alikes(&groups, 50));
        assert!(joinable.len() < every.len(), "{every:?} against {joinable:?}");
        assert!(!joinable.iter().any(|k| k == "tray"));
        // Complete source lists with nothing in common: never joinable; a cut list or an unknown range always is.
        let mut a = groups[0].clone();
        let mut b = groups[0].clone();
        a.seen_in = vec!["a.log".into()];
        b.seen_in = vec!["b.log".into()];
        assert!(!could_share_events(&a, &b));
        b.seen_in = (0..crate::catalogue::MAX_SEEN_IN).map(|i| format!("s{i}")).collect();
        assert!(could_share_events(&a, &b), "a cut list may hold the source");
        b.seen_in = vec!["b.log".into()];
        b.first_ts = None;
        assert!(!could_share_events(&a, &b), "sources still differ");
        a.seen_in = vec![];
        assert!(could_share_events(&a, &b), "unknown sources");
    }

    #[test]
    fn very_short_names_join_only_when_equal_and_look_alikes_use_the_same_blocking() {
        let c = [cand("ab.exe", 5), cand("zz.exe", 4)];
        let g = group(&c, &Vocabulary::builtin());
        assert!(look_alikes(&g, 10).is_empty());
        let mut cooc = Cooccurrence::default();
        cooc.add_event(&["ab.exe".to_string(), "zz.exe".to_string()]);
        let mut g = g;
        let stats = merge_cooccurring_stats(&mut g, &cooc, 0.5);
        assert_eq!((stats.merged, stats.comparisons), (0, 0), "no shared trigram, no comparison");
        let mut seed = 3;
        let crowd = crowd(60, &mut seed);
        let slow: Vec<usize> = (0..crowd.len().min(40))
            .filter(|i| {
                (0..crowd.len().min(40)).any(|j| *i != j && crowd[*i].scope == crowd[j].scope && similar(&crowd[*i].key, &crowd[j].key))
            })
            .collect();
        assert_eq!(look_alikes(&crowd, 40), slow);
    }
}
