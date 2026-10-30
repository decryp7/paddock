//! Path globs for waivers: `*` within a segment, `**` as a whole segment across segments, `?` one character.
//!
//! Matching is linear in the product of the pattern and the path (the classic two-pointer wildcard algorithm per segment, and a
//! table over segments for `**`), never exponential, and a pattern is bounded when it is read ([`validate_glob`]).

/// The longest glob accepted in a waiver file.
pub const MAX_GLOB_BYTES: usize = 256;
/// The most `*` characters in one glob.
pub const MAX_GLOB_STARS: usize = 16;

/// Why a glob was refused.
pub fn validate_glob(glob: &str) -> Result<(), String> {
    if glob.is_empty() {
        return Err("the path glob is empty".to_owned());
    }
    if glob.len() > MAX_GLOB_BYTES {
        return Err(format!(
            "the path glob is {} bytes; at most {MAX_GLOB_BYTES} are accepted",
            glob.len()
        ));
    }
    let stars = glob.bytes().filter(|b| *b == b'*').count();
    if stars > MAX_GLOB_STARS {
        return Err(format!("the path glob has {stars} `*`; at most {MAX_GLOB_STARS} are accepted"));
    }
    Ok(())
}

/// One segment: `*` and `?` only. Returns the verdict; `steps` counts character comparisons.
fn segment(p: &[u8], t: &[u8], steps: &mut usize) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        *steps += 1;
        if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// [`path_matches`] and the number of steps it took (a test of the bound counts steps, not seconds).
pub fn path_matches_counted(pattern: &str, path: &str) -> (bool, usize) {
    let mut steps = 0usize;
    let ps: Vec<&[u8]> = pattern.split('/').map(str::as_bytes).collect();
    let ts: Vec<&[u8]> = path.split('/').map(str::as_bytes).collect();
    // ok[i][j]: pattern segments i.. match path segments j..
    let mut ok = vec![vec![false; ts.len() + 1]; ps.len() + 1];
    ok[ps.len()][ts.len()] = true;
    for i in (0..ps.len()).rev() {
        for j in (0..=ts.len()).rev() {
            steps += 1;
            ok[i][j] = if ps[i] == b"**" {
                ok[i + 1][j] || (j < ts.len() && ok[i][j + 1])
            } else {
                j < ts.len() && ok[i + 1][j + 1] && segment(ps[i], ts[j], &mut steps)
            };
        }
    }
    (ok[0][0], steps)
}

/// True when `path` (`/`-separated) matches `pattern`. A pattern without `**` never crosses a `/`: `*.rs` does not match `src/a.rs`;
/// write `**/*.rs`.
pub fn path_matches(pattern: &str, path: &str) -> bool {
    path_matches_counted(pattern, path).0
}

#[cfg(test)]
mod tests {
    use super::path_matches as m;
    use super::*;

    #[test]
    fn star_stays_in_a_segment_and_double_star_crosses() {
        assert!(m("src/*.rs", "src/a.rs") && !m("src/*.rs", "src/x/a.rs"));
        assert!(m("src/**", "src/x/y/a.rs") && m("**/*.rs", "a.rs") && m("**/*.rs", "src/x/a.rs"));
        assert!(!m("*.rs", "src/a.rs") && m("a?c", "abc") && !m("a?c", "a/c"));
        assert!(m("apps/*", "apps/pitcrew") && !m("apps/*", "apps/pitcrew/x"));
        assert!(m("exact.toml", "exact.toml") && !m("exact.toml", "exact.tom"));
        assert!(m("a*b*c", "aXXbYYc") && !m("a*b*c", "aXXbYY") && m("*", "") && m("**", "") && m("src/**/x.rs", "src/x.rs"));
    }

    /// The reviewer's case: `*a` fourteen times against an 80-character path took over ten minutes. The bound is counted in steps.
    #[test]
    fn the_hostile_glob_is_bounded_in_steps_not_seconds() {
        let pattern = "*a".repeat(14);
        let path = format!("{}b", "a".repeat(79));
        let (matched, steps) = path_matches_counted(&pattern, &path);
        assert!(!matched);
        assert!(steps <= pattern.len() * path.len() * 2, "{steps} steps");
        let (_, deep) = path_matches_counted(&"**/".repeat(10).replace("**/**/", "**/a/"), &"a/".repeat(60));
        assert!(deep < 100_000, "{deep} steps");
    }

    #[test]
    fn globs_are_bounded_when_read() {
        assert!(validate_glob("src/**/*.rs").is_ok());
        assert!(validate_glob("").is_err());
        assert!(validate_glob(&"a".repeat(257)).unwrap_err().contains("257 bytes"));
        assert!(validate_glob(&"*a".repeat(17)).unwrap_err().contains("17 `*`"));
        assert!(validate_glob(&"*a".repeat(16)).is_ok());
    }
}
