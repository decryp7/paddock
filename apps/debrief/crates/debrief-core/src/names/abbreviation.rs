//! The abbreviation rule of layer 1: `rptbldr` is `report builder`, `netmon` is `network monitor`.
//!
//! The query, without separators, splits in order into one piece per word of the name. Each piece starts with its
//! word's first letter and is a subsequence of the word (`bldr` of `builder`). There is no list of conventional
//! abbreviations (`gfx` for `graphic`): the rule is generic, and a convention that matters comes later as data (the
//! names pack), only for gaps that were measured. Names of one word are never abbreviations (too many chance
//! matches), the piece count is fixed by the name's words, and the query must be shorter than the name, so the rule
//! cannot turn a typo or the name itself into a match. Pure, bounded ([`MAX_QUERY_CHARS`], [`MAX_WORDS`]) and free
//! of allocation: it runs once per name of the book.
//!
//! Measured on ADR 0053's invented set (`names_eval`): 22 of 24 abbreviations found, 0 of its 18 negatives matched,
//! the same with 2,000 distractor names added. The two misses are `gfxbldr` (`gfx` is not letters of `graphic`) and
//! `lickchk` (its `k` is inside `lic|kchk`).

/// Longest query (letters and digits) the rule looks at.
pub const MAX_QUERY_CHARS: usize = 32;
/// Most words of a name the rule looks at (the segmenter's own bound).
pub const MAX_WORDS: usize = 8;
/// Shortest query the rule answers; shorter ones match too many names by chance.
pub const MIN_QUERY_CHARS: usize = 4;

/// Whether `piece` can stand for `word`: it starts with the word's first letter and is a subsequence of the word.
fn piece_of(piece: &[char], word: &str) -> bool {
    let mut letters = word.chars();
    match (piece.first(), letters.next()) {
        (Some(p), Some(w)) if *p == w => {}
        _ => return false,
    }
    piece[1..].iter().all(|c| letters.any(|w| w == *c))
}

/// Whether the squashed (lower-case letters and digits) `query` is an abbreviation of the name made of `words`.
pub fn is_abbreviation<S: AsRef<str>>(query: &str, words: &[S]) -> bool {
    // Cheap checks first, nothing allocated: the word count, then the first letters.
    if words.len() < 2 || words.len() > MAX_WORDS {
        return false;
    }
    let (Some(q0), Some(w0)) = (query.chars().next(), words[0].as_ref().chars().next()) else {
        return false;
    };
    if q0 != w0 {
        return false;
    }
    let mut buf = ['\0'; MAX_QUERY_CHARS];
    let mut n = 0;
    for c in query.chars() {
        if n == MAX_QUERY_CHARS {
            return false;
        }
        buf[n] = c;
        n += 1;
    }
    let q = &buf[..n];
    let name_len: usize = words.iter().map(|w| w.as_ref().chars().count()).sum();
    if q.len() < MIN_QUERY_CHARS.max(words.len()) || q.len() >= name_len {
        return false;
    }
    // `reach` bit i: the words so far can use exactly the first i letters (at most 33 bits).
    let mut reach: u64 = 1;
    for word in words {
        let word = word.as_ref();
        let mut next: u64 = 0;
        for i in (0..q.len()).filter(|i| reach >> i & 1 == 1) {
            for j in i + 1..=q.len() {
                if piece_of(&q[i..j], word) {
                    next |= 1 << j;
                }
            }
        }
        reach = next;
    }
    reach >> q.len() & 1 == 1
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
    fn conventions_that_are_not_letters_of_the_word_are_not_known() {
        // No curated list in code: `gfx` is not a subsequence of `graphic`, so it is not matched.
        assert!(!is_abbreviation("gfxbldr", &words("graphic builder")));
        assert!(!is_abbreviation("xfermgr", &words("transfers manager")));
        // `pkg` happens to be letters of `packages`, so the generic rule finds it (plural or not).
        assert!(is_abbreviation("pkgmgr", &words("packages manager")));
        assert!(is_abbreviation("pkgmgr", &words("package manager")));
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
