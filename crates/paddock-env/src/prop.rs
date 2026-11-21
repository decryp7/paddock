//! The case count of a property test: its own default, or the `PROPTEST_CASES` the job sets (rule 89: the nightly runs the property
//! tests with 10000).
//!
//! proptest reads `PROPTEST_CASES` only into `ProptestConfig::default()`, so a test that fixes its count with
//! `ProptestConfig::with_cases(n)` ignores it. Such a test asks here instead:
//! `ProptestConfig::with_cases(paddock_testkit::proptest_cases(400))` (paddock-testkit re-exports this; a crate that paddock-testkit depends on, such as paddock-jobs, reads
//! it from here, because a dev-dependency on the kit would make a publish cycle, ADR 0100).

/// The count `PROPTEST_CASES` names, or `default` when it is unset, empty, not a number or zero.
pub fn proptest_cases(default: u32) -> u32 {
    cases_from(std::env::var("PROPTEST_CASES").ok().as_deref(), default)
}

/// [`proptest_cases`] over a given value (the pure core).
pub fn cases_from(value: Option<&str>, default: u32) -> u32 {
    value
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_replaces_the_default_and_anything_else_keeps_it() {
        assert_eq!(cases_from(Some("10000"), 400), 10_000);
        assert_eq!(cases_from(Some(" 64 "), 400), 64);
        for bad in [None, Some(""), Some("many"), Some("-3"), Some("0")] {
            assert_eq!(cases_from(bad, 400), 400, "{bad:?}");
        }
    }
}
