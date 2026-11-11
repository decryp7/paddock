//! A CI job log line without the runner's timestamp and colours: the one reader for the planner's and the run readers' logs.

use regex::Regex;
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// Any ANSI escape sequence (colours, cursor moves).
static ANSI: LazyLock<Regex> = LazyLock::new(|| re(r"\x1b\[[0-9;]*[A-Za-z]"));
/// The runner's `2026-11-09T10:24:37.4427582Z ` stamp.
static STAMP: LazyLock<Regex> = LazyLock::new(|| re(r"^\d{4}-\d\d-\d\dT[0-9:.]+Z ?"));

/// A log line without the runner's timestamp and colours.
pub fn plain(line: &str) -> String {
    let no_colour = ANSI.replace_all(line.trim_end_matches('\r'), "");
    STAMP.replace(&no_colour, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_colours_and_carriage_returns_are_removed() {
        assert_eq!(plain("2026-11-09T10:24:37.4427582Z \x1b[32m  PASS\x1b[0m x\r"), "  PASS x");
        assert_eq!(plain("no stamp"), "no stamp");
    }
}
