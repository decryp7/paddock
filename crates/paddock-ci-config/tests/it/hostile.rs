//! The parsers never panic on hostile text (C36): truncations, bit flips, NULs and bad UTF-8 of a workflow-shaped seed.
use paddock_ci_config::jobs::{Declared, model};
use paddock_ci_config::{expr, yaml};

const SEED: &str = "jobs:\n  j:\n    if: ${{ needs.plan.outputs.ui == 'true' && contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"j\"]'), 'j') }}\n    strategy:\n      matrix:\n        os: [a, b]\n    steps:\n      - run: ./dev x\n";

#[test]
fn the_yaml_reader_never_panics() {
    let hostile = paddock_testkit::corpus(SEED.as_bytes());
    assert!(
        paddock_testkit::never_panics(&hostile, |b| yaml::parse(&String::from_utf8_lossy(b)))
            .is_empty()
    );
}

#[test]
fn the_job_model_never_panics() {
    let hostile = paddock_testkit::corpus(SEED.as_bytes());
    assert!(
        paddock_testkit::never_panics(&hostile, |b| model(
            &String::from_utf8_lossy(b),
            &Declared::default()
        ))
        .is_empty()
    );
}

#[test]
fn the_expression_evaluator_never_panics() {
    let hostile = paddock_testkit::corpus(b"a == 'x' && contains(fromJSON(b || '[]'), 'c') || !d");
    assert!(
        paddock_testkit::never_panics(&hostile, |b| expr::condition(
            &String::from_utf8_lossy(b),
            &|_: &str| expr::Val::Unknown
        ))
        .is_empty()
    );
}
