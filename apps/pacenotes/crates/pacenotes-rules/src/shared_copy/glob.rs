//! Path globs for waivers: `*` within a segment, `**` across segments, `?` one character.

/// True when `path` (`/`-separated) matches `pattern`. A pattern without `/` and without `**` matches the whole path only if it has no
/// `/` itself, so `*.rs` does not match `src/a.rs`; write `**/*.rs`.
pub fn path_matches(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') if p.get(1) == Some(&b'*') => {
                let rest = &p[2..];
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=t.len()).any(|i| go(rest, &t[i..]))
            }
            Some(b'*') => (0..=t.len())
                .take_while(|&i| i == 0 || t[i - 1] != b'/')
                .any(|i| go(&p[1..], &t[i..])),
            Some(b'?') => t.first().is_some_and(|c| *c != b'/') && go(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::path_matches as m;

    #[test]
    fn star_stays_in_a_segment_and_double_star_crosses() {
        assert!(m("src/*.rs", "src/a.rs") && !m("src/*.rs", "src/x/a.rs"));
        assert!(m("src/**", "src/x/y/a.rs") && m("**/*.rs", "a.rs") && m("**/*.rs", "src/x/a.rs"));
        assert!(!m("*.rs", "src/a.rs") && m("a?c", "abc") && !m("a?c", "a/c"));
        assert!(m("apps/*", "apps/pitcrew") && !m("apps/*", "apps/pitcrew/x"));
        assert!(m("exact.toml", "exact.toml") && !m("exact.toml", "exact.tom"));
    }
}
