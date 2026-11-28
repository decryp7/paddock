//! Test helpers shared by the escape monitor's tests and the tests of its CI edges ([`crate::escape_ci`]).

use super::{Failure, Record, Sel};
use std::collections::BTreeMap;

/// A commit id for `n`.
pub(crate) fn c(n: usize) -> String {
    format!("{n:040x}")
}

/// A record for commit `n`: the change reaches `ui` (with a reason); `selected` says whether the plan ran it.
pub(crate) fn rec(n: usize, selected: bool) -> Record {
    let mut r = Record {
        commit: c(n),
        base: Some(c(n.saturating_sub(1))),
        at: 1_000_000,
        reach: BTreeMap::from([("ui".to_string(), vec!["npm ui: a dependency changed".to_string()])]),
        selected: BTreeMap::new(),
    };
    if selected {
        r.selected.insert(
            "ui".into(),
            Sel {
                os: vec!["ubuntu-latest".into()],
                runs: "all".into(),
                targets: None,
            },
        );
    }
    r
}

pub(crate) fn fail_ui() -> Failure {
    Failure {
        project: "ui".into(),
        os: "ubuntu-latest".into(),
        target: "vitest:ui/a.spec.ts".into(),
    }
}
