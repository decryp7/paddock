//! The test cost class "build": a test that starts a compiler or `cargo build` says so in its name, and nextest gives the class its time
//! budget from one place per workspace (the `# waiver:` override of the default profile in `.config/nextest.toml`, which every profile
//! inherits; ER-12).
//!
//! A test declares the class by the prefix [`PREFIX`] on its function's name (`build_the_fixture_is_found`). Why a class and not the default
//! limit: a build inside a test costs what a cold compile costs, not what the test's own work costs. After a change of the build
//! configuration (CI run 9371, a rustflags change) the in-test `cargo build` of the plugin fixture was a cold rebuild of its dependency
//! graph, and ten tests that call it reached the 60 s hang limit together (docs/benchmarks.md, "test build cost class").
//!
//! Every place a test starts a build calls [`charge`] first, before it looks whether the program is already built (a warm run must judge
//! the name as a cold one does). Under nextest, [`charge`] reads the running test's name (`NEXTEST_TEST_NAME`) and refuses a test whose last
//! name segment lacks the prefix, with the next step. Outside nextest (`cargo test`) it does nothing: a plain test run has no per-test
//! limit to protect. `tools/paddock-dev/tests/it/build_cost.rs` checks that every place that starts a build calls [`charge`] and that each
//! workspace that has such a place has the class's override.

/// The prefix of the test function's name that declares the class.
pub const PREFIX: &str = "build_";

/// The nextest filterset that selects the class: the last segment of the test's name starts with [`PREFIX`]. Every `.config/nextest.toml`
/// with the class uses exactly this string (the paddock-dev test compares them).
pub const FILTER: &str = "test(/(^|::)build_[^:]*$/)";

/// The variable nextest sets to the running test's name (`module::function`, without the binary id).
pub const TEST_NAME_ENV: &str = "NEXTEST_TEST_NAME";

/// Whether the test called `test` (the name nextest gives it, `None` outside nextest) is in the class.
///
/// # Errors
///
/// The finding, with the next step, when a test under nextest does not carry the prefix.
pub fn judge(test: Option<&str>, call: &str) -> Result<(), String> {
    let Some(test) = test.filter(|t| !t.is_empty()) else {
        return Ok(());
    };
    let last = test.rsplit("::").next().unwrap_or(test);
    if last.starts_with(PREFIX) {
        return Ok(());
    }
    Err(format!(
        "build-cost: {test} starts a build ({call}) but its name does not start with `{PREFIX}`. Next step: rename the test function \
         `{PREFIX}{last}`, so the nextest override of the build class (`{FILTER}` in .config/nextest.toml) gives it the class's time \
         budget; a cold build is not the work of the default limit (ER-12)."
    ))
}

/// Charges the running test to the build class for the work `call` names: call it before the build is started, and before the check
/// whether it is needed.
///
/// # Panics
///
/// When the test runs under nextest and is not in the class ([`judge`] says what to do).
pub fn charge(call: &str) {
    let name = crate::ambient::ambient()
        .var(TEST_NAME_ENV)
        .map(|v| v.to_string_lossy().into_owned());
    if let Err(finding) = judge(name.as_deref(), call) {
        panic!("{finding}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_outside_nextest_is_never_judged() {
        assert_eq!(judge(None, "x"), Ok(()));
        assert_eq!(judge(Some(""), "x"), Ok(()));
    }

    #[test]
    fn the_last_segment_of_the_name_decides() {
        assert_eq!(judge(Some("faults::build_a_crash"), "x"), Ok(()));
        assert_eq!(judge(Some("build_a_crash"), "x"), Ok(()));
        assert!(judge(Some("faults::a_crash"), "x").is_err());
        // A module named like the class is not the test's own declaration.
        assert!(judge(Some("build_cost::a_crash"), "x").is_err());
    }

    #[test]
    fn a_misplaced_test_is_told_the_new_name_and_the_class() {
        let said = judge(
            Some("faults::a_crash"),
            "paddock_plugin_test::fixture::locate",
        )
        .unwrap_err();
        assert!(
            said.contains("rename the test function `build_a_crash`"),
            "{said}"
        );
        assert!(
            said.contains("paddock_plugin_test::fixture::locate") && said.contains(FILTER),
            "{said}"
        );
    }

    #[test]
    fn the_filter_is_the_prefix_as_a_nextest_regex() {
        assert_eq!(FILTER, format!("test(/(^|::){PREFIX}[^:]*$/)"));
    }
}
