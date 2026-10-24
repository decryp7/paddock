//! Budgets and results files are read from disk: no input makes the parsers panic (conformance check C36).
use paddock_bench::{Budgets, Results};
use paddock_testkit::assert_never_panics;
use std::path::Path;

#[test]
fn budgets_and_results_survive_hostile_text() {
    let budgets = b"[bench.parse]\nmax = 10.0\ntolerance_pct = 5\n";
    let results = br#"{"version":1,"benches":[{"name":"parse","unit":"ms","samples":[1.0,2.0]}]}"#;
    assert_never_panics(&[budgets, results], |b| {
        let text = String::from_utf8_lossy(b);
        let _ = Budgets::parse(&text, Path::new("budgets.toml"));
        let _ = Results::parse(&text, Path::new("results.json"));
    });
}
