//! Shrink-only baselines by text (lesson 2026-11-04, C71's baseline): a baseline file lists what is known to be wrong today,
//! one entry per line, keyed by the text of the site (a hash or a snippet), never by a line number that moves. The file only
//! ever shrinks: an entry the code no longer produces is stale and is removed; an entry never appears by itself. `--write`
//! style tools use [`Baseline::shrunk`] (it cannot add) and [`ensure_shrink_only`] (it refuses a file that has grown).
//!
//! Blank lines and `#` comment lines are kept as they are and are not entries.

use std::collections::BTreeSet;

/// Whether `key` can be an entry: one line, no surrounding white space, not empty, not starting with `#` (that is a comment). A key
/// that cannot is never in a baseline, so a site with such a key always counts as new (it cannot be baselined by mistake).
pub fn is_storable(key: &str) -> bool {
    !key.is_empty() && key == key.trim() && !key.starts_with('#') && !key.contains('\n') && !key.contains('\r')
}

/// A parsed baseline file.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    lines: Vec<String>,
    entries: BTreeSet<String>,
}

/// The comparison of a baseline with what a run produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    /// Produced now and not in the baseline: a regression (an error).
    pub new: Vec<String>,
    /// In the baseline and not produced now: stale, to be removed.
    pub stale: Vec<String>,
}

fn entry_of(line: &str) -> Option<&str> {
    let t = line.trim();
    (!t.is_empty() && !t.starts_with('#')).then_some(t)
}

impl Baseline {
    /// Reads the file's text.
    pub fn parse(text: &str) -> Baseline {
        let lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
        let entries = lines.iter().filter_map(|l| entry_of(l)).map(str::to_string).collect();
        Baseline { lines, entries }
    }

    /// The entries.
    pub fn entries(&self) -> &BTreeSet<String> {
        &self.entries
    }

    /// Compares the baseline with the keys a run produced (sorted, without duplicates in the result).
    pub fn judge(&self, current: &[String]) -> Judgement {
        let now: BTreeSet<&str> = current.iter().map(String::as_str).collect();
        Judgement {
            new: now.iter().filter(|k| !self.entries.contains(**k)).map(|k| k.to_string()).collect(),
            stale: self.entries.iter().filter(|e| !now.contains(e.as_str())).cloned().collect(),
        }
    }

    /// The file's text without its stale entries, everything else byte for byte (line endings included). It never adds a line.
    pub fn shrunk(&self, current: &[String]) -> String {
        let now: BTreeSet<&str> = current.iter().map(String::as_str).collect();
        let mut out = String::new();
        for l in &self.lines {
            if entry_of(l).is_some_and(|e| !now.contains(e)) {
                continue;
            }
            out.push_str(l);
        }
        out
    }
}

/// Refuses a baseline text that has an entry the old one did not (a baseline grows only by a human decision recorded
/// elsewhere). Returns the added entries.
pub fn ensure_shrink_only(old: &str, new: &str) -> Result<(), Vec<String>> {
    let old = Baseline::parse(old);
    let added: Vec<String> = Baseline::parse(new).entries.difference(&old.entries).cloned().collect();
    if added.is_empty() { Ok(()) } else { Err(added) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# header\nabc\n\n  def  \n# note\nghi\n";

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn judges_new_and_stale_by_text() {
        let b = Baseline::parse(FILE);
        let j = b.judge(&keys(&["abc", "zzz", "ghi", "zzz"]));
        assert_eq!(j.new, ["zzz"]);
        assert_eq!(j.stale, ["def"]);
    }

    #[test]
    fn shrunk_drops_only_stale_entries_and_keeps_comments() {
        let b = Baseline::parse(FILE);
        assert_eq!(b.shrunk(&keys(&["abc", "ghi", "new"])), "# header\nabc\n\n# note\nghi\n");
    }

    #[test]
    fn shrinking_never_adds_whatever_the_run_produced() {
        let b = Baseline::parse(FILE);
        // A small deterministic sweep over subsets of known and unknown keys.
        let pool = ["abc", "def", "ghi", "new1", "new2"];
        for mask in 0u32..32 {
            let current: Vec<String> = pool
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, k)| k.to_string())
                .collect();
            let shrunk = b.shrunk(&current);
            assert!(ensure_shrink_only(FILE, &shrunk).is_ok(), "mask {mask}");
            let again = Baseline::parse(&shrunk);
            assert!(again.judge(&current).stale.is_empty(), "mask {mask}: nothing stale is left");
            assert!(again.entries().is_subset(b.entries()));
        }
    }

    #[test]
    fn line_endings_and_a_missing_final_newline_are_kept() {
        let b = Baseline::parse("# h\r\nabc\r\ndef\r\nghi");
        assert_eq!(b.shrunk(&keys(&["abc", "ghi"])), "# h\r\nabc\r\nghi");
        assert_eq!(b.shrunk(&keys(&["abc", "def", "ghi"])), "# h\r\nabc\r\ndef\r\nghi");
        assert!(
            b.judge(&keys(&["abc", "def", "ghi"])).stale.is_empty(),
            "a CR is not part of the entry"
        );
    }

    #[test]
    fn keys_that_cannot_be_stored_are_always_new() {
        assert!(
            is_storable("a b")
                && !is_storable("#x")
                && !is_storable(" x")
                && !is_storable("x ")
                && !is_storable("")
                && !is_storable("a\nb")
        );
        let b = Baseline::parse("abc\n");
        let j = b.judge(&keys(&["#abc", " abc", "abc "]));
        assert_eq!(j.new.len(), 3);
        assert_eq!(j.stale, ["abc"], "none of those keys is the entry `abc`");
        assert_eq!(b.shrunk(&keys(&["#abc"])), "");
    }

    #[test]
    fn a_grown_file_is_refused_with_the_added_entries() {
        assert_eq!(ensure_shrink_only(FILE, "abc\nplanted\n"), Err(vec!["planted".to_string()]));
        assert!(ensure_shrink_only(FILE, "abc\n").is_ok());
    }
}
