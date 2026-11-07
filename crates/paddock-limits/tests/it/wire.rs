use crate::*;
use paddock_limits::{
    Bounded, BoundedError, Buckets, Cut, CutRow, FieldPath, Limit, LimitExceeded, Page, Scope,
};
use proptest::prelude::*;

const F5: CutRow<paddock_limits::First> = CutRow::of(&FIRST_5);
const TOP: CutRow<paddock_limits::TopBy> = CutRow::of(&TOP_3);

#[test]
fn field_paths() {
    for ok in [
        "",
        "traces",
        "callees.selfMs",
        "groups[3].ids",
        "a[0]",
        "a.b.c",
        "x9",
        &"a".repeat(64),
        "resultant",
        "results.x",
    ] {
        assert!(FieldPath::parse(ok).is_ok(), "{ok:?}");
    }
    for bad in [
        "result",
        "result.x",
        "result[0]",
        ".a",
        "a.",
        "a..b",
        "A",
        "1a",
        "a[",
        "a[]",
        "a[x]",
        "a[1",
        "a[1]b",
        "a[4294967296]",
        "a b",
        "a-b",
        "a[1][2]",
        &"a".repeat(65),
    ] {
        assert_eq!(FieldPath::parse(bad), Err(BoundedError::Path), "{bad:?}");
    }
    assert!(FieldPath::parse("a[4294967295]").is_ok());
    assert_eq!(FieldPath::root().as_str(), "");
    assert!(BoundedError::Path.to_string().contains("field path"));
}

#[test]
#[should_panic(expected = "not a field path")]
fn a_bad_literal_path_is_a_programmer_error() {
    let _ = FieldPath::lit("Bad");
}

#[test]
fn bounded_json_is_camel_case_and_round_trips() {
    let b = Bounded::page(vec![1, 2], 500, 1204, &F5, FieldPath::lit("rows"));
    let json = serde_json::to_value(&b).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"result": [1, 2], "cuts": [{
            "limit": "test.first", "of": "rows", "shown": 2,
            "total": {"kind": "exact", "n": 1204}, "selection": {"kind": "first", "from": 500}, "next": "page"
        }]})
    );
    let back: Bounded<Vec<i32>> = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
    let top = Bounded::ranked(
        vec![3, 2, 1, 0],
        &TOP,
        FieldPath::lit("t"),
        paddock_limits::SortKey {
            field: FieldPath::lit("ms"),
            order: paddock_limits::Order::Asc,
        },
    );
    let again: Bounded<Vec<i32>> =
        serde_json::from_str(&serde_json::to_string(&top).unwrap()).unwrap();
    assert_eq!(again, top);
    let un = serde_json::to_value(Cut::maybe(
        &CutRow::<paddock_limits::Maybe>::of(&MAYBE),
        FieldPath::root(),
        1,
        paddock_limits::Selection::Sample,
    ))
    .unwrap();
    assert_eq!(un["total"], serde_json::json!({"kind": "unknown"}));
}

fn cut_json(
    shown: u64,
    total: serde_json::Value,
    selection: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({"limit": "test.first", "of": "rows", "shown": shown, "total": total, "selection": selection, "next": "page"})
}

#[test]
fn a_cut_with_nothing_left_out_is_refused() {
    let first = serde_json::json!({"kind": "first", "from": 0});
    let exact = |n: u64| serde_json::json!({"kind": "exact", "n": n});
    assert!(serde_json::from_value::<Cut>(cut_json(5, exact(5), first.clone())).is_err());
    assert!(serde_json::from_value::<Cut>(cut_json(6, exact(5), first.clone())).is_err());
    assert!(
        serde_json::from_value::<Cut>(cut_json(
            5,
            serde_json::json!({"kind": "atLeast", "n": 5}),
            first.clone()
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<Cut>(cut_json(
            4,
            serde_json::json!({"kind": "atLeast", "n": 5}),
            first.clone()
        ))
        .is_ok()
    );
    assert!(
        serde_json::from_value::<Cut>(cut_json(
            4,
            serde_json::json!({"kind": "atLeast", "n": 6}),
            first.clone()
        ))
        .is_ok()
    );
    // from + shown past the total.
    assert!(
        serde_json::from_value::<Cut>(cut_json(
            3,
            exact(10),
            serde_json::json!({"kind": "first", "from": 8})
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<Cut>(cut_json(
            3,
            exact(10),
            serde_json::json!({"kind": "first", "from": 7})
        ))
        .is_ok()
    );
    assert!(
        serde_json::from_value::<Cut>(cut_json(
            1,
            exact(10),
            serde_json::json!({"kind": "first", "from": u64::MAX})
        ))
        .is_err()
    );
    // An unknown total needs no arithmetic.
    assert!(
        serde_json::from_value::<Cut>(cut_json(3, serde_json::json!({"kind": "unknown"}), first))
            .is_ok()
    );
    // A bad path or key.
    let mut bad = cut_json(1, exact(9), serde_json::json!({"kind": "last"}));
    bad["of"] = "result.rows".into();
    assert!(serde_json::from_value::<Cut>(bad).is_err());
    let mut bad = cut_json(1, exact(9), serde_json::json!({"kind": "last"}));
    bad["limit"] = "Not A Key".into();
    assert!(serde_json::from_value::<Cut>(bad).is_err());
}

fn bounded_json(cuts: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({"result": 0, "cuts": cuts})
}

#[test]
fn bounded_refuses_too_many_misplaced_and_duplicate_cuts() {
    let one = |i: usize| {
        let mut c = cut_json(
            1,
            serde_json::json!({"kind": "exact", "n": 9}),
            serde_json::json!({"kind": "last"}),
        );
        c["of"] = format!("l{i}").into();
        c
    };
    assert!(
        serde_json::from_value::<Bounded<i32>>(bounded_json((0..32).map(one).collect())).is_ok()
    );
    let err = serde_json::from_value::<Bounded<i32>>(bounded_json((0..33).map(one).collect()))
        .unwrap_err();
    assert!(err.to_string().contains("more than 32"), "{err}");

    let dup = bounded_json(vec![one(1), one(1)]);
    assert!(
        serde_json::from_value::<Bounded<i32>>(dup)
            .unwrap_err()
            .to_string()
            .contains("same list")
    );

    let mut agg = cut_json(
        31,
        serde_json::json!({"kind": "exact", "n": 40}),
        serde_json::json!({"kind": "first", "from": 0}),
    );
    agg["limit"] = "bounded.cuts".into();
    agg["of"] = "".into();
    assert!(
        serde_json::from_value::<Bounded<i32>>(bounded_json(vec![one(1), agg.clone()])).is_ok()
    );
    assert!(
        serde_json::from_value::<Bounded<i32>>(bounded_json(vec![agg, one(1)]))
            .unwrap_err()
            .to_string()
            .contains("aggregate")
    );
    for e in [
        BoundedError::Arithmetic,
        BoundedError::TooManyCuts,
        BoundedError::Aggregate,
        BoundedError::Duplicate,
    ] {
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = &e;
    }
}

#[test]
fn limit_exceeded_round_trips() {
    let e = LimitExceeded::new(
        LimitId::new("plugin.memory"),
        Quantity::Bytes(1 << 30),
        Scope::Plugin,
    )
    .reaching(Quantity::Bytes(3 << 30));
    let json = serde_json::to_value(e).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"id": "plugin.memory", "limit": {"kind": "bytes", "n": 1073741824u64}, "reached": {"kind": "bytes", "n": 3221225472u64}, "scope": "plugin"})
    );
    assert_eq!(serde_json::from_value::<LimitExceeded>(json).unwrap(), e);
}

#[test]
fn request_types_are_never_zero() {
    assert!(Limit::new(0).is_err());
    assert_eq!(Limit::new(500).unwrap().get(), 500);
    assert!(Buckets::new(0).is_err());
    assert_eq!(Buckets::new(64).unwrap().get(), 64);
    assert_eq!(
        Limit::new(0).unwrap_err().to_string(),
        "limit must be at least 1"
    );
    let page: Page = serde_json::from_str(r#"{"offset":10,"limit":50}"#).unwrap();
    assert_eq!((page.offset, page.limit.get()), (10, 50));
    assert!(serde_json::from_str::<Page>(r#"{"offset":10,"limit":0}"#).is_err());
    assert!(serde_json::from_str::<Buckets>("0").is_err());
    let _: &dyn std::error::Error = &Limit::new(0).unwrap_err();
}

proptest! {
    /// No constructor yields a cut with nothing left out, and what a constructor makes survives the wire.
    #[test]
    fn constructors_never_make_an_empty_cut(len in 0usize..40, total in 0u64..60, offset in 0u64..30) {
        let first = Bounded::first(0..len, &F5, FieldPath::lit("rows"));
        prop_assert_eq!(first.is_complete(), len <= 5);
        prop_assert_eq!(first.result().len(), len.min(5));

        let counted = Bounded::counted(vec![0u8; len], total, &F5, FieldPath::lit("rows"));
        prop_assert_eq!(counted.is_complete(), (len as u64) >= total);

        if offset + len as u64 <= total.max(offset + len as u64) && offset + (len as u64) <= total {
            let page = Bounded::page(vec![0u8; len], offset, total, &F5, FieldPath::lit("rows"));
            prop_assert_eq!(page.is_complete(), (len as u64) >= total);
            let back: Bounded<Vec<u8>> = serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
            prop_assert_eq!(back, page);
        }
        let back: Bounded<Vec<usize>> = serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap();
        prop_assert_eq!(back, first);
    }

    /// The wire form refuses every cut that a constructor could not have made.
    #[test]
    fn the_wire_refuses_what_constructors_cannot_make(shown in 0u64..50, n in 0u64..50, from in 0u64..50, kind in 0u8..3) {
        let total = match kind { 0 => serde_json::json!({"kind":"exact","n":n}), 1 => serde_json::json!({"kind":"atLeast","n":n}), _ => serde_json::json!({"kind":"unknown"}) };
        let v = cut_json(shown, total, serde_json::json!({"kind":"first","from":from}));
        let accepted = serde_json::from_value::<Cut>(v).is_ok();
        let valid = match kind { 0 => shown < n && from + shown <= n, 1 => shown < n && from + shown < n, _ => true };
        prop_assert_eq!(accepted, valid);
    }

    #[test]
    fn field_path_parse_never_panics(s in ".{0,100}") {
        let _ = FieldPath::parse(&s);
    }
}

#[test]
fn an_at_least_total_with_a_ranking_checks_the_same_arithmetic() {
    let top = |shown: u64, n: u64| {
        cut_json(
            shown,
            serde_json::json!({"kind": "atLeast", "n": n}),
            serde_json::json!({"kind": "topBy", "key": {"field": "ms", "order": "desc"}, "from": 0}),
        )
    };
    assert!(serde_json::from_value::<Cut>(top(3, 3)).is_err());
    assert!(serde_json::from_value::<Cut>(top(3, 4)).is_ok());
}

#[test]
fn an_aggregate_that_arrived_without_a_count_is_replaced_not_trusted() {
    let mut agg = cut_json(
        31,
        serde_json::json!({"kind": "unknown"}),
        serde_json::json!({"kind": "first", "from": 0}),
    );
    agg["limit"] = "bounded.cuts".into();
    agg["of"] = "".into();
    let b: Bounded<i32> = serde_json::from_value(bounded_json(vec![agg])).unwrap();
    let b = b.and(Cut::at_least(&F5, FieldPath::lit("x"), 5, 6));
    // The aggregate is dropped and rebuilt from the cuts that are really there.
    assert!(
        b.cuts()
            .iter()
            .all(|c| c.limit().as_str() != "bounded.cuts")
    );
    assert_eq!(b.cuts().len(), 1);
}

#[test]
fn rows_serialize_with_their_setting_path() {
    let json = serde_json::to_value(crate::SETTING_ROW).unwrap();
    assert_eq!(
        json["value"],
        serde_json::json!({"kind": "setting", "path": "performance.testCap"})
    );
    assert_eq!(json["remedy"]["path"], "performance.testCap");
    assert_eq!(json["id"], "test.setting");
}
