use crate::*;
use paddock_limits::{
    Bounded, BoundedError, Buckets, Cut, CutRow, FieldPath, Limit, LimitExceeded, Page, Scope,
    Total,
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
    .reaching(Quantity::Bytes(3 << 30))
    .unwrap();
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
    /// No constructor yields a cut with nothing left out or panics, whatever the numbers (a stale offset, a total that
    /// grew or shrank); what a constructor makes survives the wire; a page is complete only when it is everything.
    #[test]
    fn constructors_never_panic_and_never_make_an_empty_or_a_hidden_cut(len in 0usize..40, total in 0u64..60, offset in 0u64..60) {
        let first = Bounded::first(0..len, &F5, FieldPath::lit("rows"));
        prop_assert_eq!(first.is_complete(), len <= 5);
        prop_assert_eq!(first.result().len(), len.min(5));

        let counted = Bounded::counted(vec![0u8; len], total, &F5, FieldPath::lit("rows"));
        prop_assert_eq!(counted.is_complete(), (len as u64) >= total);

        let page = Bounded::page(vec![0u8; len], offset, total, &F5, FieldPath::lit("rows"));
        prop_assert_eq!(page.is_complete(), offset == 0 && (len as u64) >= total);
        for cut in page.cuts() {
            let end = offset + len as u64;
            match cut.total() {
                Total::Exact { n } => prop_assert!(end <= n && cut.shown() < n),
                Total::Unknown => prop_assert!(end > total),
                Total::AtLeast { .. } => prop_assert!(false, "a page never has a lower bound"),
            }
        }
        let back: Bounded<Vec<u8>> = serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
        prop_assert_eq!(back, page);
        let back: Bounded<Vec<usize>> = serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap();
        prop_assert_eq!(back, first);
    }

    /// The wire form refuses every cut that a constructor could not have made.
    #[test]
    fn the_wire_refuses_what_constructors_cannot_make(shown in 0u64..50, n in 0u64..50, from in 0u64..50, kind in 0u8..3) {
        let total = match kind { 0 => serde_json::json!({"kind":"exact","n":n}), 1 => serde_json::json!({"kind":"atLeast","n":n}), _ => serde_json::json!({"kind":"unknown"}) };
        let v = cut_json(shown, total, serde_json::json!({"kind":"first","from":from}));
        let accepted = serde_json::from_value::<Cut>(v).is_ok();
        let valid = match kind { 0 => shown < n && from + shown <= n, 1 => shown < n && from == 0, _ => true };
        prop_assert_eq!(accepted, valid);
    }

    #[test]
    fn field_path_parse_never_panics(s in ".{0,100}") {
        let _ = FieldPath::parse(&s);
    }
}

#[test]
fn an_at_least_total_exists_only_for_the_first_items_from_zero() {
    let at_least = |shown: u64, n: u64, selection: serde_json::Value| {
        cut_json(
            shown,
            serde_json::json!({"kind": "atLeast", "n": n}),
            selection,
        )
    };
    let first = |from: u64| serde_json::json!({"kind": "first", "from": from});
    let top = |from: u64| serde_json::json!({"kind": "topBy", "key": {"field": "ms", "order": "desc"}, "from": from});
    assert!(serde_json::from_value::<Cut>(at_least(3, 4, first(0))).is_ok());
    assert!(serde_json::from_value::<Cut>(at_least(3, 3, first(0))).is_err());
    // A page from the middle, a ranking and a tail have an exact or an unknown total, never a lower bound.
    assert!(serde_json::from_value::<Cut>(at_least(3, 9, first(2))).is_err());
    assert!(serde_json::from_value::<Cut>(at_least(3, 4, top(0))).is_err());
    assert!(serde_json::from_value::<Cut>(at_least(3, 9, top(2))).is_err());
    assert!(
        serde_json::from_value::<Cut>(at_least(3, 9, serde_json::json!({"kind": "last"}))).is_err()
    );
}

fn aggregate_json(
    of: &str,
    shown: u64,
    total: serde_json::Value,
    selection: serde_json::Value,
    next: &str,
) -> serde_json::Value {
    let mut agg = cut_json(shown, total, selection);
    agg["limit"] = "bounded.cuts".into();
    agg["of"] = of.into();
    agg["next"] = next.into();
    agg
}

#[test]
fn the_aggregate_must_have_the_shape_the_constructors_give_it() {
    let one = |i: usize| {
        let mut c = cut_json(
            1,
            serde_json::json!({"kind": "exact", "n": 9}),
            serde_json::json!({"kind": "last"}),
        );
        c["of"] = format!("l{i}").into();
        c
    };
    let exact = |n: u64| serde_json::json!({"kind": "exact", "n": n});
    let first0 = serde_json::json!({"kind": "first", "from": 0});
    let with = |agg: serde_json::Value| {
        let mut cuts: Vec<_> = (0..31).map(one).collect();
        cuts.push(agg);
        serde_json::from_value::<Bounded<i32>>(bounded_json(cuts))
    };
    assert!(with(aggregate_json("", 31, exact(40), first0.clone(), "none")).is_ok());
    // Each part of the shape on its own.
    assert!(
        with(aggregate_json(
            "rows",
            31,
            exact(40),
            first0.clone(),
            "none"
        ))
        .is_err(),
        "of must be empty"
    );
    assert!(
        with(aggregate_json("", 30, exact(40), first0.clone(), "none")).is_err(),
        "shown is the 31 before it"
    );
    assert!(
        with(aggregate_json("", 31, exact(31), first0.clone(), "none")).is_err(),
        "the count is more than 31"
    );
    assert!(
        with(aggregate_json(
            "",
            31,
            serde_json::json!({"kind": "unknown"}),
            first0.clone(),
            "none"
        ))
        .is_err()
    );
    assert!(
        with(aggregate_json(
            "",
            31,
            exact(40),
            serde_json::json!({"kind": "first", "from": 1}),
            "none"
        ))
        .is_err()
    );
    assert!(
        with(aggregate_json(
            "",
            31,
            exact(40),
            serde_json::json!({"kind": "last"}),
            "none"
        ))
        .is_err()
    );
    assert!(
        with(aggregate_json("", 31, exact(40), first0.clone(), "page")).is_err(),
        "it offers nothing"
    );
    // Only as the 32nd cut.
    let short: Vec<_> = (0..5)
        .map(one)
        .chain([aggregate_json("", 31, exact(40), first0, "none")])
        .collect();
    assert!(serde_json::from_value::<Bounded<i32>>(bounded_json(short)).is_err());
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

#[test]
fn quantities_and_limits_exceeded_are_checked_on_the_wire() {
    assert!(
        serde_json::from_value::<Quantity>(serde_json::json!({"kind": "percent", "n": 100}))
            .is_ok()
    );
    assert!(
        serde_json::from_value::<Quantity>(serde_json::json!({"kind": "percent", "n": 101}))
            .is_err()
    );
    let e = |limit: serde_json::Value, reached: serde_json::Value| serde_json::json!({"id": "plugin.memory", "limit": limit, "reached": reached, "scope": "plugin"});
    let bytes = |n: u64| serde_json::json!({"kind": "bytes", "n": n});
    assert!(serde_json::from_value::<LimitExceeded>(e(bytes(1), bytes(2))).is_ok());
    assert!(
        serde_json::from_value::<LimitExceeded>(e(
            bytes(1),
            serde_json::json!({"kind": "count", "n": 2})
        ))
        .is_err()
    );
    let mismatch = LimitExceeded::new(LimitId::new("a.b"), Quantity::Bytes(1), Scope::File)
        .reaching(Quantity::Count(1));
    assert!(mismatch.unwrap_err().to_string().contains("observed value"));
}

/// Serializing borrows the reply: a value that cannot be cloned is still written.
#[test]
fn serializing_a_bounded_does_not_need_a_clone() {
    #[derive(serde::Serialize)]
    struct NotClone(Vec<u8>);
    let b = Bounded::with(
        NotClone(vec![1, 2]),
        Cut::at_least(&F5, FieldPath::lit("rows"), 5, 6),
    );
    let json = serde_json::to_value(&b).unwrap();
    assert_eq!(json["result"], serde_json::json!([1, 2]));
    assert_eq!(json["cuts"].as_array().unwrap().len(), 1);
}

/// The wire is for self-describing formats: postcard cannot read the internally tagged variants back.
#[test]
fn postcard_cannot_carry_a_bounded() {
    let b = Bounded::with(vec![1u8], Cut::at_least(&F5, FieldPath::lit("rows"), 5, 6));
    let bytes = postcard::to_stdvec(&b);
    let back = bytes.map(|bytes| postcard::from_bytes::<Bounded<Vec<u8>>>(&bytes));
    assert!(matches!(back, Err(_) | Ok(Err(_))), "{back:?}");
}

#[test]
fn every_quantity_reads_back_from_the_wire() {
    for q in [
        Quantity::Bytes(7),
        Quantity::Count(7),
        Quantity::Ratio(7),
        Quantity::Millis(7),
        Quantity::Percent(7),
    ] {
        let back: Quantity = serde_json::from_value(serde_json::to_value(q).unwrap()).unwrap();
        assert_eq!(back, q);
    }
    let without: LimitExceeded =
        serde_json::from_value(serde_json::json!({"id": "a.b", "limit": {"kind": "count", "n": 3}, "reached": null, "scope": "query"})).unwrap();
    assert_eq!(without.reached, None);
}

/// The job wire carries a `LimitExceeded` as postcard between processes: the quantity must read back from it.
#[test]
fn quantities_and_limits_exceeded_survive_postcard() {
    for q in [
        Quantity::Bytes(7),
        Quantity::Count(1 << 40),
        Quantity::Ratio(200),
        Quantity::Millis(108_000),
        Quantity::Percent(80),
    ] {
        let bytes = postcard::to_stdvec(&q).unwrap();
        let back: Quantity =
            postcard::from_bytes(&bytes).unwrap_or_else(|e| panic!("{q:?} {bytes:?} {e:?}"));
        assert_eq!(back, q);
    }
    let e = LimitExceeded::new(
        LimitId::new("plugin.memory"),
        Quantity::Bytes(1 << 30),
        Scope::Plugin,
    )
    .reaching(Quantity::Bytes(3 << 30))
    .unwrap();
    let back: LimitExceeded = postcard::from_bytes(&postcard::to_stdvec(&e).unwrap()).unwrap();
    assert_eq!(back, e);
    let some: Option<LimitExceeded> =
        postcard::from_bytes(&postcard::to_stdvec(&Some(e)).unwrap()).unwrap();
    assert_eq!(some, Some(e));
    // A hostile compact value is refused, not wrapped: percent 200, ratio over 32 bits, an unknown kind.
    assert!(
        postcard::from_bytes::<Quantity>(&postcard::to_stdvec(&(4u32, 200u64)).unwrap()).is_err()
    );
    assert!(
        postcard::from_bytes::<Quantity>(&postcard::to_stdvec(&(2u32, 1u64 << 40)).unwrap())
            .is_err()
    );
    assert!(
        postcard::from_bytes::<Quantity>(&postcard::to_stdvec(&(9u32, 1u64)).unwrap()).is_err()
    );
    assert!(postcard::from_bytes::<Quantity>(&[]).is_err());
    assert!(postcard::from_bytes::<Quantity>(&postcard::to_stdvec(&1u32).unwrap()).is_err());
    // JSON: an unknown extra field is ignored, a missing one is refused.
    assert!(
        serde_json::from_value::<Quantity>(serde_json::json!({"kind": "count", "n": 1, "x": 1}))
            .is_ok()
    );
    assert!(serde_json::from_value::<Quantity>(serde_json::json!({"kind": "count"})).is_err());
    assert!(serde_json::from_value::<Quantity>(serde_json::json!({"n": 1})).is_err());
}

#[test]
fn a_quantity_of_the_wrong_shape_is_refused_with_a_reason() {
    let refused = |v: serde_json::Value| {
        serde_json::from_value::<Quantity>(v)
            .unwrap_err()
            .to_string()
    };
    assert!(refused(serde_json::json!(5)).contains("a quantity"));
    assert!(refused(serde_json::json!([])).contains("invalid length 0"));
    assert!(refused(serde_json::json!(["bytes"])).contains("invalid length 1"));
    assert!(refused(serde_json::json!({"kind": "ratio", "n": 1u64 << 40})).contains("32 bits"));
    assert!(refused(serde_json::json!({"kind": "furlongs", "n": 1})).contains("furlongs"));
    // As a sequence, the way a self-describing format may also write a struct.
    assert_eq!(
        serde_json::from_value::<Quantity>(serde_json::json!(["count", 3])).unwrap(),
        Quantity::Count(3)
    );
}

/// Review 3: ADR 0063's MessagePack option and any bincode boundary carry a `Quantity` too: every kind, at its own width.
#[test]
fn every_quantity_survives_messagepack_and_bincode() {
    let all = [
        Quantity::Bytes(u64::MAX),
        Quantity::Count(7),
        Quantity::Ratio(u32::MAX),
        Quantity::Millis(108_000),
        Quantity::Percent(100),
    ];
    for q in all {
        let packed = rmp_serde::to_vec(&q).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Quantity>(&packed).unwrap(),
            q,
            "messagepack {q:?}"
        );
        let named = rmp_serde::to_vec_named(&q).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Quantity>(&named).unwrap(),
            q,
            "messagepack, named {q:?}"
        );
        let fixed = bincode::serialize(&q).unwrap();
        assert_eq!(
            bincode::deserialize::<Quantity>(&fixed).unwrap(),
            q,
            "bincode {q:?}"
        );
        let compact = postcard::to_stdvec(&q).unwrap();
        assert_eq!(
            postcard::from_bytes::<Quantity>(&compact).unwrap(),
            q,
            "postcard {q:?}"
        );
    }
}

/// Review 3: a key that repeats is refused, not resolved by taking the last one.
#[test]
fn a_repeated_key_is_refused() {
    let refused = |text: &str| {
        serde_json::from_str::<Quantity>(text)
            .unwrap_err()
            .to_string()
    };
    assert!(refused(r#"{"kind":"bytes","kind":"count","n":1}"#).contains("duplicate field `kind`"));
    assert!(refused(r#"{"kind":"count","n":1,"n":2}"#).contains("duplicate field `n`"));
    assert_eq!(
        serde_json::from_str::<Quantity>(r#"{"n":3,"kind":"count"}"#).unwrap(),
        Quantity::Count(3)
    );
}

/// Review 3: the aggregate counts cuts that may have been counted before, so its words say "up to".
#[test]
fn the_aggregate_says_up_to() {
    let mut b = Bounded::complete(());
    for i in 0..40 {
        b = b.and(Cut::at_least(
            &F5,
            FieldPath::parse(&format!("l{i:02}")).unwrap(),
            5,
            6,
        ));
    }
    let text = b.cuts()[31].describe();
    assert!(text.contains("up to 40"), "{text}");
    assert!(
        text.contains("31") && text.contains("bounded.cuts"),
        "{text}"
    );
}
