//! PDH returns lists as one buffer of NUL-separated strings ended by an empty string (a "multi-string").

/// The strings of a multi-string buffer (UTF-16 code units). Reading stops at the empty string that ends the list or
/// at the end of the buffer; invalid UTF-16 is replaced; at most `max` strings are returned.
pub fn multi_sz(buf: &[u16], max: usize) -> Vec<String> {
    buf.split(|&c| c == 0)
        .take_while(|s| !s.is_empty())
        .take(max)
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(parts: &[&str]) -> Vec<u16> {
        let mut v = Vec::new();
        for p in parts {
            v.extend(p.encode_utf16());
            v.push(0);
        }
        v.push(0);
        v
    }

    #[test]
    fn it_splits_at_nuls_and_stops_at_the_empty_string() {
        assert_eq!(
            multi_sz(&enc(&["Memory", "Processor", "Physical Disk"]), 10),
            ["Memory", "Processor", "Physical Disk"]
        );
        assert!(multi_sz(&enc(&[]), 10).is_empty());
        assert!(multi_sz(&[], 10).is_empty());
    }

    #[test]
    fn it_is_bounded_and_tolerates_a_missing_terminator_and_bad_utf16() {
        assert_eq!(multi_sz(&enc(&["a", "b", "c"]), 2), ["a", "b"]);
        let mut cut = enc(&["abc"]);
        cut.truncate(3);
        assert_eq!(multi_sz(&cut, 5), ["abc"]);
        assert_eq!(multi_sz(&[0xD800, 0], 5), ["\u{FFFD}"]);
        assert_eq!(multi_sz(&enc(&["Prozessor (Gesamt)", "é"]), 5)[1], "é");
    }
}
