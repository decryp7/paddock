//! Spelling distance, capped.

/// Most characters of each side compared.
const MAX_CHARS: usize = 64;

/// How many letters a word of `len` letters may be off and still count as a typo: one per four, at least one.
pub fn allowed_typos(len: usize) -> usize {
    (len / 4).max(1)
}

/// Edit distance (insert, delete, substitute, swap of neighbours) when it is at most `cap`, else `None`. Inputs are cut at 64
/// characters.
pub fn edit_distance_capped(a: &str, b: &str, cap: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().take(MAX_CHARS).collect();
    let b: Vec<char> = b.chars().take(MAX_CHARS).collect();
    if a.len().abs_diff(b.len()) > cap {
        return None;
    }
    let mut prev2: Vec<usize> = vec![0; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut prev_min = 0;
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            let mut best = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
            if i > 0 && j > 0 && *ca == b[j - 1] && a[i - 1] == *cb {
                best = best.min(prev2[j - 1] + 1);
            }
            cur.push(best);
        }
        // A distance never shrinks along a row, and a swap looks back two rows: when both are over the cap, so is the result.
        let cur_min = cur.iter().copied().min().unwrap_or(0);
        if cur_min > cap && prev_min > cap {
            return None;
        }
        prev_min = cur_min;
        prev2 = prev;
        prev = cur;
    }
    let d = prev[b.len()];
    (d <= cap).then_some(d)
}
