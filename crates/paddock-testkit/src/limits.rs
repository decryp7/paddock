//! `limit_case!`: one test per limit (family ADR 0019 check 1, Debrief ADR 0065 check 4).
//!
//! The stop form drives the code to the limit and passes the `LimitExceeded` it returned; the cut form passes a reply with
//! the data at the limit (no cut) and one over it (a cut with exact numbers). Both look the row up in the registry, assert
//! the id, the value, the Setting path and the rendered message, and record the id, so [`assert_all_exercised`] fails
//! when a registry row has no case. Triggers use fake clocks and sizes set small, never sleeps.
//!
//! ```ignore
//! limit_case!(stop, REGISTRY, "plugin.memory", got = run_fake_plugin_over_cap(), limit = Quantity::Bytes(64 << 20),
//!             setting = Some("performance.pluginMemoryMb"));
//! limit_case!(cut, REGISTRY, "traces.list", at = list(200), over = list(201), of = "traces", shown = 200,
//!             total = Total::AtLeast { n: 201 }, selection = Selection::First { from: 0 });
//! ```

use paddock_limits::{
    Bounded, LimitExceeded, LimitId, Quantity, Registry, RowKind, RowValue, Selection, Total,
};
use std::collections::BTreeSet;
use std::sync::Mutex;

static EXERCISED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

fn record(id: LimitId) {
    EXERCISED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.as_str());
}

/// The ids the cases in this process have exercised so far.
#[must_use]
pub fn exercised() -> Vec<&'static str> {
    EXERCISED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .copied()
        .collect()
}

/// Fails (panics) listing every row of `registry` that no case has exercised in this process. Call it from one test that
/// runs after the others, or from a test binary's last test with the cases in the same binary.
///
/// # Panics
/// When a row has no case.
pub fn assert_all_exercised(registry: &Registry) {
    let seen = exercised();
    let missing: Vec<_> = registry
        .rows()
        .iter()
        .filter(|r| !seen.contains(&r.id.as_str()))
        .map(|r| r.id.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "limit rows without a limit_case!: {missing:?}"
    );
}

/// What a stop case expects.
pub struct StopExpect {
    /// The limit's value.
    pub limit: Quantity,
    /// The Setting path the message must name, or `None` for a hard limit.
    pub setting: Option<&'static str>,
}

/// The stop form: see the module.
///
/// # Panics
/// On any mismatch, with the limit id in the message.
pub fn assert_stop_case(
    registry: &Registry,
    id: &'static str,
    got: LimitExceeded,
    expect: &StopExpect,
) {
    let id = LimitId::new(id);
    let row = registry
        .get(id)
        .unwrap_or_else(|| panic!("limit {id}: not in the registry"));
    assert!(
        matches!(row.kind, RowKind::Stop | RowKind::Both),
        "limit {id}: the row is a cut row, use the cut form"
    );
    assert_eq!(got.id, id, "limit {id}: the error carries another id");
    assert_eq!(
        got.limit, expect.limit,
        "limit {id}: the error carries another value"
    );
    let row_path = match row.value {
        RowValue::Setting { path } => Some(path.as_str()),
        _ => None,
    };
    assert_eq!(
        row_path, expect.setting,
        "limit {id}: the registry row's Setting path differs from the case"
    );
    let text = got.message(registry);
    assert!(
        text.contains(id.as_str()),
        "limit {id}: the message does not name the limit: {text}"
    );
    assert!(
        text.contains(&expect.limit.to_string()),
        "limit {id}: the message does not give the value: {text}"
    );
    match expect.setting {
        Some(path) => assert!(
            text.contains("Settings \u{2192}") && text.contains(path),
            "limit {id}: no Setting path in: {text}"
        ),
        None => assert!(
            !text.contains("Settings \u{2192}"),
            "limit {id}: a hard limit must not send the person to Settings: {text}"
        ),
    }
    record(id);
}

/// What a cut case expects of the reply over the limit.
pub struct CutExpect {
    /// The list.
    pub of: &'static str,
    /// Items shown.
    pub shown: u64,
    /// The total.
    pub total: Total,
    /// The selection.
    pub selection: Selection,
}

/// The cut form: `at` has the data at the limit (no cut for this id), `over` one over it (exactly one cut with these
/// numbers).
///
/// # Panics
/// On any mismatch, with the limit id in the message.
pub fn assert_cut_case<T, U>(
    registry: &Registry,
    id: &'static str,
    at: &Bounded<T>,
    over: &Bounded<U>,
    expect: &CutExpect,
) {
    let id = LimitId::new(id);
    let row = registry
        .get(id)
        .unwrap_or_else(|| panic!("limit {id}: not in the registry"));
    assert!(
        matches!(row.kind, RowKind::Cut | RowKind::Both),
        "limit {id}: the row is a stop row, use the stop form"
    );
    assert!(
        at.cuts().iter().all(|c| c.limit() != id),
        "limit {id}: data at the limit must not be cut"
    );
    let cuts: Vec<_> = over.cuts().iter().filter(|c| c.limit() == id).collect();
    assert_eq!(
        cuts.len(),
        1,
        "limit {id}: data over the limit must carry exactly one cut"
    );
    let cut = cuts[0];
    assert_eq!(cut.of().as_str(), expect.of, "limit {id}: of");
    assert_eq!(cut.shown(), expect.shown, "limit {id}: shown");
    assert_eq!(cut.total(), expect.total, "limit {id}: total");
    assert_eq!(cut.selection(), &expect.selection, "limit {id}: selection");
    assert!(
        row.selections.contains(&cut.selection().kind()) || matches!(cut.total(), Total::Unknown),
        "limit {id}: the row does not declare this selection"
    );
    assert!(
        cut.describe().contains(id.as_str()),
        "limit {id}: describe() does not name the limit"
    );
    record(id);
}

/// One test per limit, in the stop or the cut form (see the module).
#[macro_export]
macro_rules! limit_case {
    (stop, $registry:expr, $id:literal, got = $got:expr, limit = $limit:expr, setting = $setting:expr $(,)?) => {
        $crate::limits::assert_stop_case(
            &$registry,
            $id,
            $got,
            &$crate::limits::StopExpect {
                limit: $limit,
                setting: $setting,
            },
        )
    };
    (cut, $registry:expr, $id:literal, at = $at:expr, over = $over:expr, of = $of:literal, shown = $shown:expr,
     total = $total:expr, selection = $selection:expr $(,)?) => {
        $crate::limits::assert_cut_case(
            &$registry,
            $id,
            &$at,
            &$over,
            &$crate::limits::CutExpect {
                of: $of,
                shown: $shown,
                total: $total,
                selection: $selection,
            },
        )
    };
}
