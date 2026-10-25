//! Regular expressions written by users, compiled with limits: the pattern's length and the compiled program's size
//! are capped, so a pattern can neither exhaust memory nor take long to build. (The `regex` crate matches in linear
//! time, so matching cannot blow up.)

use crate::limits::MAX_PATTERN_LEN;
use regex::{Regex, RegexBuilder};

/// Largest compiled program (1 MiB).
const SIZE_LIMIT: usize = 1 << 20;

/// Compiles a user pattern. `case_insensitive` for file and tag names. The error says what to fix.
pub fn compile(pattern: &str, case_insensitive: bool) -> Result<Regex, String> {
    if pattern.len() > MAX_PATTERN_LEN {
        return Err(format!("The pattern is too long (at most {MAX_PATTERN_LEN} characters)."));
    }
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .size_limit(SIZE_LIMIT)
        .dfa_size_limit(SIZE_LIMIT)
        .build()
        .map_err(|e| match e {
            regex::Error::CompiledTooBig(_) => "The pattern is too complex; simplify it.".to_string(),
            other => format!("The pattern is not a valid regular expression: {}", first_line(&other.to_string())),
        })
}

fn first_line(s: &str) -> String {
    s.lines().rev().find(|l| l.starts_with("error:")).unwrap_or(s).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_compile_within_limits() {
        assert!(compile(r"^app-\d+\.zip$", false).unwrap().is_match("app-12.zip"));
        assert!(compile("abc", true).unwrap().is_match("ABC"));
        assert!(compile(&"a".repeat(MAX_PATTERN_LEN + 1), false).unwrap_err().contains("too long"));
        let bad = compile("(", false).unwrap_err();
        assert!(bad.contains("not a valid regular expression"), "{bad}");
        assert!(compile(r"\w{1000}\w{1000}", false).unwrap_err().contains("too complex"));
    }
}
