//! `limit_cases!` and `limit_case!`: one test per limit (family ADR 0019 check 1, Debrief ADR 0065 check 4).
//!
//! `limit_cases!` is the form a crate uses: it lists every case of a registry, generates one `#[test]` per case (so each
//! runs in its own process under nextest too) and one test that compares the list of ids with the registry's rows in both
//! directions, so a limit without a case, and a case for a limit that is not in the registry, both fail. The stop form
//! drives the code to the limit and passes the `LimitExceeded` it returned; the cut form passes a reply with the data at the
//! limit (no cut) and one over it (a cut with exact numbers). Triggers use fake clocks and sizes set small, never sleeps.
//!
//! ```ignore
//! limit_cases! { REGISTRY;
//!     plugin_memory: "plugin.memory" => stop(got = run_fake_plugin_over_cap(), limit = Quantity::Bytes(64 << 20),
//!                                            setting = Some("performance.pluginMemoryMb")),
//!     traces_list: "traces.list" => cut(at = list(200), over = list(201), of = "traces", shown = 200,
//!                                       total = Total::AtLeast { n: 201 }, selection = Selection::First { from: 0 }),
//! }
//! ```

use paddock_limits::{
    Bounded, LimitExceeded, LimitId, Quantity, Registry, RowKind, RowValue, Selection, Total,
};

/// Fails (panics) unless `ids` (the ids of a crate's `limit_cases!`) and the rows of `registry` are the same set.
///
/// # Panics
/// Listing the rows without a case and the cases without a row.
pub fn assert_cases_match(registry: &Registry, ids: &[&str]) {
    let rows: Vec<&str> = registry.rows().iter().map(|r| r.id.as_str()).collect();
    let without_case: Vec<&&str> = rows.iter().filter(|r| !ids.contains(r)).collect();
    let without_row: Vec<&&str> = ids.iter().filter(|i| !rows.contains(i)).collect();
    assert!(
        without_case.is_empty() && without_row.is_empty(),
        "limit rows without a case: {without_case:?}; cases without a row: {without_row:?}"
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

/// Every case of a registry, as `#[test]`s: `name: "limit.id" => stop(..)` or `=> cut(..)` with the arguments of
/// [`limit_case!`], plus `LIMIT_CASE_IDS` and the test that compares the ids with the registry. **Once per module, with
/// all of one registry's cases in it**: the comparison is against the whole registry, so a registry whose cases live in
/// several modules is compared with `assert_cases_match` in one test over the union of their ids.
#[macro_export]
macro_rules! limit_cases {
    ($registry:expr; $( $name:ident : $id:literal => $kind:ident ( $($body:tt)* ) ),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                $crate::limit_case!($kind, $registry, $id, $($body)*)
            }
        )*
        /// The ids this module has a case for.
        const LIMIT_CASE_IDS: &[&str] = &[$($id),*];
        #[test]
        fn every_limit_row_has_a_case_and_every_case_a_row() {
            $crate::limits::assert_cases_match(&$registry, LIMIT_CASE_IDS)
        }
    };
}
