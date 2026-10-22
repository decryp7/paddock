//! Token counting and truncation helpers (`/x/v1/tokens`, `/x/v1/truncate`). The exact count comes from the loaded
//! model's tokenizer (llama-server `/tokenize`); without one Pitwall estimates (about four characters per token for
//! Latin text, one per character for CJK) and says so.

/// An estimated token count.
pub fn estimate(text: &str) -> u32 {
    let mut latin = 0usize;
    let mut wide = 0usize;
    for c in text.chars() {
        if (c as u32) >= 0x2E80 {
            wide += 1;
        } else {
            latin += 1;
        }
    }
    (latin.div_ceil(4) + wide).min(u32::MAX as usize) as u32
}

/// Where to cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Keep the start.
    Head,
    /// Keep the end.
    Tail,
    /// Keep both ends, cutting the middle.
    Middle,
}

/// The marker put where text was cut.
pub const ELLIPSIS: &str = "\n[…]\n";

/// Cuts `text` to about `max_tokens` (estimated), on character boundaries. Returns the text and whether it was cut.
pub fn truncate(text: &str, max_tokens: u32, keep: Keep) -> (String, bool) {
    if estimate(text) <= max_tokens {
        return (text.to_string(), false);
    }
    let chars: Vec<char> = text.chars().collect();
    // Find the most characters whose estimate fits, by bisection over a prefix length.
    let fits = |n: usize| estimate(&chars[..n].iter().collect::<String>()) <= max_tokens;
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let n = lo;
    let out = match keep {
        Keep::Head => chars[..n].iter().collect::<String>() + ELLIPSIS.trim_end(),
        Keep::Tail => ELLIPSIS.trim_start().to_string() + &chars[chars.len() - n..].iter().collect::<String>(),
        Keep::Middle => {
            let half = n / 2;
            chars[..half].iter().collect::<String>() + ELLIPSIS + &chars[chars.len() - (n - half)..].iter().collect::<String>()
        }
    };
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates() {
        assert_eq!(estimate(""), 0);
        assert_eq!(estimate("abcd"), 1);
        assert_eq!(estimate("abcde"), 2);
        assert_eq!(estimate("日本語"), 3);
    }

    #[test]
    fn truncation() {
        let t = "a".repeat(400);
        assert_eq!(truncate(&t, 1000, Keep::Head), (t.clone(), false));
        let (h, cut) = truncate(&t, 10, Keep::Head);
        assert!(cut && h.starts_with("aaaa") && h.ends_with("[…]"));
        assert_eq!(h.matches('a').count(), 40);
        let (tl, _) = truncate(&t, 10, Keep::Tail);
        assert!(tl.starts_with("[…]") && tl.ends_with('a'));
        let (m, _) = truncate(&t, 10, Keep::Middle);
        assert!(m.contains(ELLIPSIS) && m.starts_with('a') && m.ends_with('a'));
        let (z, cut) = truncate("日本語の文章", 2, Keep::Head);
        assert!(cut && z.starts_with("日本"));
        let (zero, _) = truncate("abcdef", 0, Keep::Middle);
        assert_eq!(zero, ELLIPSIS);
    }
}
