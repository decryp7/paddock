//! The abbreviation rule of layer 1: `gfxbldr` is `graphic builder`, `rptbldr` is `report builder`.
//!
//! The query, without separators, splits in order into one piece per word of the name. Each piece starts with its
//! word's first letter and is a subsequence of the word (`bldr` of `builder`), or is a conventional software
//! abbreviation of the word ([`CONVENTIONAL`]). Names of one word are never abbreviations (too many chance matches),
//! the piece count is fixed by the name's words, and the query must be shorter than the name, so the rule cannot
//! turn a typo or the name itself into a match. Pure and bounded ([`MAX_QUERY_CHARS`], [`MAX_WORDS`]).
//!
//! Measured on ADR 0053's invented set: 23 of 24 abbreviations found, 0 of its 18 negatives matched, still 23 of 24
//! with 2,000 distractor names added (the miss, `lickchk`, has its `k` inside `lic|kchk`).

/// Longest query (letters and digits) the rule looks at.
pub const MAX_QUERY_CHARS: usize = 32;
/// Most words of a name the rule looks at (the segmenter's own bound).
pub const MAX_WORDS: usize = 8;
/// Shortest query the rule answers; shorter ones match too many names by chance.
pub const MIN_QUERY_CHARS: usize = 4;

/// Abbreviations that are conventions of software naming and are not letters of the word: (abbreviation, word).
/// A closed list of conventions, not of names; names come from the logs.
pub const CONVENTIONAL: &[(&str, &str)] = &[("gfx", "graphic"), ("lic", "license"), ("xfer", "transfer"), ("pkg", "package")];

/// Whether `piece` can stand for `word`.
fn piece_of(piece: &[char], word: &[char]) -> bool {
    if piece.is_empty() || piece.len() > word.len() {
        return false;
    }
    let text: String = piece.iter().collect();
    if CONVENTIONAL.iter().any(|(a, w)| {
        *a == text
            && (word.iter().copied().eq(w.chars()) || (word.last() == Some(&'s') && word[..word.len() - 1].iter().copied().eq(w.chars())))
    }) {
        return true;
    }
    if piece[0] != word[0] {
        return false;
    }
    let mut rest = word.iter();
    piece.iter().all(|c| rest.any(|w| w == c))
}

/// Whether the squashed (lower-case letters and digits) `query` is an abbreviation of the name made of `words`.
pub fn is_abbreviation<S: AsRef<str>>(query: &str, words: &[S]) -> bool {
    let q: Vec<char> = query.chars().collect();
    let w: Vec<Vec<char>> = words.iter().map(|w| w.as_ref().chars().collect()).collect();
    let name_len: usize = w.iter().map(Vec::len).sum();
    if w.len() < 2 || w.len() > MAX_WORDS || q.len() < MIN_QUERY_CHARS.max(w.len()) || q.len() > MAX_QUERY_CHARS || q.len() >= name_len {
        return false;
    }
    // The word pieces cannot all start at once: the first letter of the first word is a cheap filter.
    let first_ok = w[0].first() == q.first()
        || CONVENTIONAL
            .iter()
            .any(|(a, word)| q.iter().copied().take(a.len()).eq(a.chars()) && w[0].iter().copied().eq(word.chars()));
    if !first_ok {
        return false;
    }
    // `reach[i]`: the first k words can use exactly the first i letters.
    let mut reach = vec![false; q.len() + 1];
    reach[0] = true;
    for word in &w {
        let mut next = vec![false; q.len() + 1];
        for i in (0..q.len()).filter(|i| reach[*i]) {
            for j in i + 1..=q.len() {
                if piece_of(&q[i..j], word) {
                    next[j] = true;
                }
            }
        }
        reach = next;
    }
    reach[q.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<&str> {
        s.split(' ').collect()
    }

    #[test]
    fn letters_of_each_word_in_order() {
        assert!(is_abbreviation("rptbldr", &words("report builder")));
        assert!(is_abbreviation("netmon", &words("network monitor")));
        assert!(is_abbreviation("mplayr", &words("media player")));
        assert!(is_abbreviation("srchsvc", &words("search service")));
    }

    #[test]
    fn conventional_abbreviations_stand_for_their_word() {
        assert!(is_abbreviation("gfxbldr", &words("graphic builder")));
        assert!(is_abbreviation("licchk", &words("license checker")));
        assert!(
            is_abbreviation("pkgmgr", &words("packages manager")),
            "a plural word takes the singular's abbreviation"
        );
        assert!(!is_abbreviation("gfxbldr", &words("graph builder")));
    }

    #[test]
    fn the_rule_cannot_make_a_match_out_of_other_things() {
        // The name itself, a typo of it, one word, or a different word are not abbreviations.
        assert!(!is_abbreviation("graphicbuilder", &words("graphic builder")));
        assert!(!is_abbreviation("grapgicbuilder", &words("graphic builder")));
        assert!(!is_abbreviation("builder", &words("graphic builder")));
        assert!(!is_abbreviation("graphic", &words("graphic builder")));
        assert!(!is_abbreviation("tryagnt", &words("tree agent")));
        assert!(!is_abbreviation("grapebldr", &words("graphic builder")));
        assert!(
            !is_abbreviation("tbldr", &words("graphic builder")),
            "pieces start with the word's first letter"
        );
        assert!(
            !is_abbreviation("rptbldr", &["reportbuilder"]),
            "a one-word name is never abbreviated"
        );
        assert!(!is_abbreviation("rbl", &words("report builder")), "too short");
        assert!(!is_abbreviation("", &words("report builder")));
        assert!(!is_abbreviation("rptbldr", &[] as &[&str]));
    }

    #[test]
    fn inputs_are_bounded() {
        let long = "r".repeat(MAX_QUERY_CHARS + 1);
        assert!(!is_abbreviation(&long, &words("report builder tester")));
        let many: Vec<String> = (0..MAX_WORDS + 1).map(|_| "report".to_string()).collect();
        assert!(!is_abbreviation("rptrptrptrptrptrptrptrptrpt", &many));
        // Odd input never panics.
        for q in ["日本語名前", "a\u{0}bcd", "ééééé", "1234", "r r r r"] {
            let _ = is_abbreviation(q, &words("report builder"));
            let _ = is_abbreviation(q, &["日本", "語"]);
        }
    }

    proptest::proptest! {
        /// Any query against any words: no panic, and an answer only for names of 2..=MAX_WORDS words.
        #[test]
        fn never_panics_and_stays_bounded(q in "\\PC{0,80}", w in proptest::collection::vec("\\PC{0,12}", 0..12)) {
            let hit = is_abbreviation(&q, &w);
            if hit {
                proptest::prop_assert!(w.len() >= 2 && w.len() <= MAX_WORDS);
                proptest::prop_assert!(q.chars().count() <= MAX_QUERY_CHARS);
            }
        }
    }
}
