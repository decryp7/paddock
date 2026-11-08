use crate::*;
use paddock_limits::{Bounded, Cut, CutRow, FieldPath, Next, Order, Selection, SortKey, Total};

const F5: CutRow<paddock_limits::First> = CutRow::of(&FIRST_5);
const NARROW: CutRow<paddock_limits::First> = CutRow::of(&FIRST_NARROW);
const SETTING: CutRow<paddock_limits::First> = CutRow::of(&SETTING_ROW);
const TOP: CutRow<paddock_limits::TopBy> = CutRow::of(&TOP_3);
const LAST: CutRow<paddock_limits::Last> = CutRow::of(&LAST_2);
const SAMPLE: CutRow<paddock_limits::Sample> = CutRow::of(&SAMPLE_2);
const MAYBE_ROW: CutRow<paddock_limits::Maybe> = CutRow::of(&MAYBE);
const ROWS: FieldPath = FieldPath::lit("rows");

fn key() -> SortKey {
    SortKey {
        field: FieldPath::lit("durationMs"),
        order: Order::Desc,
    }
}

/// Asserts every field of the one cut.
fn only_cut<T>(b: &Bounded<T>) -> &Cut {
    assert_eq!(b.cuts().len(), 1);
    &b.cuts()[0]
}

#[test]
fn first_at_the_limit_has_no_cut_and_one_over_has_one() {
    let at = Bounded::first(0..5, &F5, ROWS);
    assert_eq!(at.result(), &vec![0, 1, 2, 3, 4]);
    assert!(at.is_complete());

    let over = Bounded::first(0..1000, &F5, ROWS);
    assert_eq!(over.result().len(), 5);
    let c = only_cut(&over);
    assert_eq!(c.limit().as_str(), "test.first");
    assert_eq!(c.of().as_str(), "rows");
    assert_eq!(c.shown(), 5);
    assert_eq!(c.total(), Total::AtLeast { n: 6 });
    assert_eq!(c.selection(), &Selection::First { from: 0 });
    assert_eq!(c.next(), Next::ShowMore);
}

#[test]
fn first_reads_one_more_item_and_no_further() {
    let mut pulled = 0u32;
    let b = Bounded::first(
        std::iter::from_fn(|| {
            pulled += 1;
            Some(pulled)
        }),
        &F5,
        ROWS,
    );
    assert_eq!(b.result().len(), 5);
    assert_eq!(pulled, 6);
}

#[test]
fn first_with_uses_the_setting_value() {
    let b = Bounded::first_with(0..100, &SETTING, SETTING.limit_of(10).unwrap(), ROWS);
    assert_eq!(b.result().len(), 10);
    let c = only_cut(&b);
    assert_eq!(c.total(), Total::AtLeast { n: 11 });
    assert_eq!(c.next(), Next::RaiseSetting);
    assert!(
        Bounded::first_with(0..10, &SETTING, SETTING.limit_of(10).unwrap(), ROWS).is_complete()
    );
}

#[test]
fn next_comes_from_the_row() {
    assert_eq!(
        only_cut(&Bounded::first(0..9, &NARROW, ROWS)).next(),
        Next::Narrow
    );
    assert_eq!(
        only_cut(&Bounded::ranked(vec![1, 2, 3, 4], &TOP, ROWS, key())).next(),
        Next::None
    );
}

#[test]
fn page_says_where_it_starts_and_how_to_get_more() {
    let b = Bounded::page(vec![0; 3], 3, 10, &F5, ROWS);
    let c = only_cut(&b);
    assert_eq!(c.shown(), 3);
    assert_eq!(c.total(), Total::Exact { n: 10 });
    assert_eq!(c.selection(), &Selection::First { from: 3 });
    assert_eq!(c.next(), Next::Page);
    // The last page is still a cut: the earlier items are not in it.
    assert_eq!(
        only_cut(&Bounded::page(vec![0; 3], 7, 10, &F5, ROWS)).selection(),
        &Selection::First { from: 7 }
    );
    // A single page of everything is complete.
    assert!(Bounded::page(vec![0; 4], 0, 4, &F5, ROWS).is_complete());
}

#[test]
fn ranked_keeps_the_limit_and_counts_what_it_saw() {
    let b = Bounded::ranked((0..10).rev().collect(), &TOP, ROWS, key());
    assert_eq!(b.result(), &vec![9, 8, 7]);
    let c = only_cut(&b);
    assert_eq!(c.shown(), 3);
    assert_eq!(c.total(), Total::Exact { n: 10 });
    assert_eq!(
        c.selection(),
        &Selection::TopBy {
            key: key(),
            from: 0
        }
    );
    assert!(Bounded::ranked(vec![1, 2, 3], &TOP, ROWS, key()).is_complete());
}

#[test]
fn counted_last_and_sampled() {
    let c = Bounded::counted(vec![1, 2], 9, &F5, ROWS);
    assert_eq!(only_cut(&c).total(), Total::Exact { n: 9 });
    assert_eq!(only_cut(&c).selection(), &Selection::First { from: 0 });
    assert!(Bounded::counted(vec![1, 2], 2, &F5, ROWS).is_complete());

    let l = Bounded::last(vec![8, 9], 9, &LAST, ROWS);
    assert_eq!(only_cut(&l).selection(), &Selection::Last);
    assert_eq!(only_cut(&l).total(), Total::Exact { n: 9 });
    assert!(Bounded::last(vec![8, 9], 2, &LAST, ROWS).is_complete());

    let s = Bounded::sampled(vec![1, 5], 14, &SAMPLE, ROWS);
    assert_eq!(only_cut(&s).selection(), &Selection::Sample);
    assert!(Bounded::sampled(vec![1, 5], 2, &SAMPLE, ROWS).is_complete());
}

#[test]
fn maybe_is_the_only_way_to_an_unknown_total() {
    let c = Cut::maybe(
        &MAYBE_ROW,
        FieldPath::lit("fields"),
        512,
        Selection::First { from: 0 },
    );
    assert_eq!(c.total(), Total::Unknown);
    assert_eq!(c.shown(), 512);
    assert_eq!(c.limit().as_str(), "test.maybe");
    assert_eq!(c.next(), Next::None);
    let b = Bounded::with(vec![1], Some(c));
    assert!(!b.is_complete());
}

#[test]
fn complete_with_and_map() {
    let b = Bounded::complete(7);
    assert!(b.is_complete());
    assert_eq!(*b.result(), 7);
    let b = Bounded::with(1, Cut::at_least(&F5, ROWS, 5, 6)).map(|n| n + 1);
    assert_eq!(*b.result(), 2);
    assert_eq!(b.cuts().len(), 1);
    assert_eq!(b.into_result(), 2);
    assert!(Cut::at_least(&F5, ROWS, 6, 6).is_none());
    assert!(Cut::page(&F5, ROWS, 0, 6, 6).is_none());
    assert!(Cut::counted_top(&TOP, ROWS, 6, 5, key(), 0).is_none());
}

#[test]
fn a_limit_outside_the_rows_bounds_is_refused() {
    // SETTING_ROW: min 1, max 100.
    assert!(
        SETTING
            .limit_of(0)
            .unwrap_err()
            .to_string()
            .contains("outside 1 to 100")
    );
    assert!(SETTING.limit_of(101).is_err());
    assert_eq!(SETTING.limit_of(1).unwrap().get(), 1);
    assert_eq!(SETTING.limit_of(100).unwrap().get(), 100);
}

#[test]
fn two_cuts_on_different_lists_and_a_replacement_of_the_same_list() {
    let a = Bounded::with((), Cut::at_least(&F5, FieldPath::lit("a"), 5, 6));
    let b = a.and(Cut::at_least(&F5, FieldPath::lit("b"), 5, 7));
    assert_eq!(b.cuts().len(), 2);
    let c = b.and(Cut::at_least(&F5, FieldPath::lit("a"), 5, 9));
    assert_eq!(c.cuts().len(), 2);
    assert_eq!(c.cuts()[0].total(), Total::AtLeast { n: 9 });
    assert_eq!(c.and(None).cuts().len(), 2);
}

fn cuts_on(n: usize) -> Bounded<()> {
    let mut b = Bounded::complete(());
    for i in 0..n {
        let of = FieldPath::parse(&format!("lists[{i}].ids")).unwrap();
        b = b.and(Cut::at_least(&F5, of, 5, 6));
    }
    b
}

#[test]
fn thirty_two_cuts_fit_and_the_thirty_third_makes_the_aggregate() {
    let b = cuts_on(32);
    assert_eq!(b.cuts().len(), 32);
    assert!(b.cuts().iter().all(|c| c.limit().as_str() == "test.first"));

    let b = cuts_on(33);
    assert_eq!(b.cuts().len(), 32);
    let agg = &b.cuts()[31];
    assert_eq!(agg.limit(), BOUNDED_CUTS.id);
    assert_eq!(agg.of().as_str(), "");
    assert_eq!(agg.shown(), 31);
    assert_eq!(agg.total(), Total::Exact { n: 33 });
    assert_eq!(agg.selection(), &Selection::First { from: 0 });
    assert_eq!(agg.next(), Next::None);
    assert_eq!(b.cuts()[30].of().as_str(), "lists[30].ids");

    // Re-aggregates, and the aggregate stays last.
    let b = cuts_on(45);
    assert_eq!(b.cuts().len(), 32);
    assert_eq!(b.cuts()[31].total(), Total::Exact { n: 45 });
    // A list already cut is replaced in place, not counted again.
    let b = b.and(Cut::at_least(&F5, FieldPath::lit("lists[0].ids"), 5, 99));
    assert_eq!(b.cuts()[31].total(), Total::Exact { n: 45 });
    assert_eq!(b.cuts()[0].total(), Total::AtLeast { n: 99 });
}

#[test]
fn describe_reads_as_a_sentence_with_ids_paths_and_counts_only() {
    let top = Bounded::ranked((0..10).collect(), &TOP, FieldPath::lit("traces"), key());
    assert_eq!(
        only_cut(&top).describe(),
        "traces: showing the top 3 by durationMs descending of 10 (test.top)"
    );
    let first = Bounded::first(0..99, &F5, ROWS);
    assert_eq!(
        only_cut(&first).describe(),
        "rows: showing the first 5 of more than 5 (test.first)"
    );
    let page = Bounded::page(vec![0; 3], 500, 1204, &F5, ROWS);
    assert_eq!(
        only_cut(&page).describe(),
        "rows: showing 501 to 503 of 1,204 (test.first)"
    );
    let last = Bounded::last(vec![1], 5, &LAST, FieldPath::root());
    assert_eq!(
        only_cut(&last).describe(),
        "the value: showing the last 1 of 5 (test.last)"
    );
    let s = Bounded::sampled(vec![1], 5, &SAMPLE, ROWS);
    assert_eq!(
        only_cut(&s).describe(),
        "rows: showing a sample of 1 of 5 (test.sample)"
    );
    let c = Cut::maybe(
        &MAYBE_ROW,
        ROWS,
        512,
        Selection::TopBy {
            key: SortKey {
                field: FieldPath::lit("n"),
                order: Order::Asc,
            },
            from: 4,
        },
    );
    assert_eq!(
        c.describe(),
        "rows: showing the top 512 by n ascending, from rank 5 of an unknown number (test.maybe)"
    );
    let stale = Bounded::page(Vec::<u8>::new(), 5000, 1204, &F5, ROWS);
    assert_eq!(
        only_cut(&stale).describe(),
        "rows: showing none from 5,001 of an unknown number (test.first)"
    );
}

/// Review P1-1, the three probes: no number panics, nothing is reported complete that is not, and a window that overshoots
/// the total has an unknown total.
#[test]
fn windows_over_stale_totals_are_cuts_with_an_unknown_total() {
    for (len, offset, total) in [(0usize, 5000u64, 1204u64), (300, 1000, 1204), (5, 10, 5)] {
        let b = Bounded::page(vec![0u8; len], offset, total, &F5, ROWS);
        let c = only_cut(&b);
        assert_eq!(c.total(), Total::Unknown, "{len} {offset} {total}");
        assert_eq!(c.selection(), &Selection::First { from: offset });
        assert_eq!(c.next(), Next::Page);
    }
    // The last page ends exactly at the total: still a cut (the earlier rows are not in it) with an exact total.
    let last = Bounded::page(vec![0u8; 4], 6, 10, &F5, ROWS);
    assert_eq!(only_cut(&last).total(), Total::Exact { n: 10 });
    // Numbers at the edge of u64 do not overflow.
    let huge = Bounded::page(vec![0u8; 2], u64::MAX, u64::MAX, &F5, ROWS);
    assert_eq!(only_cut(&huge).total(), Total::Unknown);
    let top = Cut::counted_top(&TOP, ROWS, 2, 3, key(), u64::MAX).unwrap();
    assert_eq!(top.total(), Total::Unknown);
    // The remaining windows.
    assert!(Cut::counted_first(&F5, ROWS, 5, 5, 0).is_none());
    assert!(Cut::counted_last(&LAST, ROWS, 2, 2).is_none());
    assert!(Cut::counted_sample(&SAMPLE, ROWS, 3, 2).is_none());
}

/// Review L6: the cuts kept past 32 are the 31 smallest in the structural order of their lists, whatever order they were
/// added in.
#[test]
fn the_cuts_kept_past_thirty_two_do_not_depend_on_the_order_they_were_added_in() {
    let paths: Vec<String> = (0..40)
        .map(|i| format!("lists[{}].ids", (i * 7) % 40))
        .collect();
    let build = |order: &[usize]| {
        let mut b = Bounded::complete(());
        for &i in order {
            b = b.and(Cut::at_least(
                &F5,
                FieldPath::parse(&paths[i]).unwrap(),
                5,
                6,
            ));
        }
        b
    };
    let forward: Vec<usize> = (0..40).collect();
    let backward: Vec<usize> = (0..40).rev().collect();
    let a = build(&forward);
    let b = build(&backward);
    assert_eq!(a, b);
    let kept: Vec<&str> = a.cuts()[..31].iter().map(|c| c.of().as_str()).collect();
    let expected: Vec<String> = (0..31).map(|i| format!("lists[{i}].ids")).collect();
    assert_eq!(
        kept,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(a.cuts()[31].total(), Total::Exact { n: 40 });
}

#[test]
fn field_paths_order_structurally() {
    let order = |a: &str, b: &str| {
        FieldPath::parse(a)
            .unwrap()
            .cmp(&FieldPath::parse(b).unwrap())
    };
    use std::cmp::Ordering::{Greater, Less};
    assert_eq!(order("groups", "groups[0]"), Less);
    assert_eq!(order("groups[0]", "groups[0].ids"), Less);
    assert_eq!(
        order("groups[2]", "groups[10]"),
        Less,
        "numeric, not string, order of indexes"
    );
    assert_eq!(order("callees", "callers"), Less);
    assert_eq!(order("b", "a.z"), Greater);
    assert_eq!(order("", "a"), Less);
    assert_eq!(
        FieldPath::lit("a").partial_cmp(&FieldPath::lit("a")),
        Some(std::cmp::Ordering::Equal)
    );
}

#[test]
fn a_row_handle_is_copyable_and_names_its_row() {
    let copy = F5;
    #[allow(clippy::clone_on_copy)]
    let cloned = copy.clone();
    assert_eq!(cloned.row().id.as_str(), "test.first");
    assert_eq!(cloned.limit(), 5);
}

#[test]
fn selections_know_their_kind_and_wire_forms_convert_both_ways() {
    use paddock_limits::{BoundedWire, CutWire, SelectionKind};
    let kinds = [
        (Selection::First { from: 0 }, SelectionKind::First),
        (Selection::Last, SelectionKind::Last),
        (
            Selection::TopBy {
                key: key(),
                from: 0,
            },
            SelectionKind::TopBy,
        ),
        (Selection::Sample, SelectionKind::Sample),
    ];
    for (selection, kind) in kinds {
        assert_eq!(selection.kind(), kind);
    }
    let b = Bounded::with(vec![1], Cut::at_least(&F5, ROWS, 5, 6));
    let cut = b.cuts()[0].clone();
    let wire = CutWire::from(cut.clone());
    assert_eq!(Cut::try_from(wire).unwrap(), cut);
    let wire = BoundedWire::from(b.clone());
    assert_eq!(Bounded::try_from(wire).unwrap(), b);
}

/// Review 2, P1: `describe()` adds to `from`; a `from` at the edge of u64 must not overflow, whether the cut came from a
/// constructor or from the wire.
#[test]
fn a_window_at_the_edge_of_u64_describes_without_overflow() {
    let page = Bounded::page(vec![0u8; 1], u64::MAX, 5, &F5, ROWS);
    let text = only_cut(&page).describe();
    assert!(text.starts_with("rows: showing "), "{text}");
    let top = Cut::counted_top(&TOP, ROWS, 2, 3, key(), u64::MAX).unwrap();
    assert!(top.describe().contains("from rank"), "{}", top.describe());
    let none = Bounded::page(Vec::<u8>::new(), u64::MAX, 5, &F5, ROWS);
    assert!(only_cut(&none).describe().contains("none from"));
    // The wire refuses what the constructors cannot make: from + shown past u64, even with an unknown total.
    let json = serde_json::json!({"limit": "test.first", "of": "rows", "shown": 1, "total": {"kind": "unknown"},
        "selection": {"kind": "first", "from": u64::MAX}, "next": "page"});
    assert!(serde_json::from_value::<Cut>(json).is_err());
    let ok = serde_json::json!({"limit": "test.first", "of": "rows", "shown": 1, "total": {"kind": "unknown"},
        "selection": {"kind": "first", "from": u64::MAX - 1}, "next": "page"});
    let cut = serde_json::from_value::<Cut>(ok).unwrap();
    assert!(
        cut.describe().contains("18,446,744,073,709,551,615"),
        "{}",
        cut.describe()
    );
}

/// Review 2, P2: an index with a leading zero is not a path (`groups[03]` ordered equal to `groups[3]` but was not `==`).
#[test]
fn an_index_with_a_leading_zero_is_not_a_field_path() {
    assert!(FieldPath::parse("groups[03]").is_err());
    assert!(FieldPath::parse("groups[00]").is_err());
    assert!(FieldPath::parse("groups[0]").is_ok());
    assert!(FieldPath::parse("groups[30]").is_ok());
}

#[test]
fn every_cut_has_a_key_for_the_words_of_the_ui() {
    let first = Bounded::first(0..99, &F5, ROWS);
    assert_eq!(only_cut(&first).words_key(), "first.atLeast");
    assert_eq!(
        only_cut(&Bounded::page(vec![0u8; 3], 3, 10, &F5, ROWS)).words_key(),
        "first.exact"
    );
    assert_eq!(
        only_cut(&Bounded::page(Vec::<u8>::new(), 50, 10, &F5, ROWS)).words_key(),
        "first.unknown"
    );
    assert_eq!(
        only_cut(&Bounded::ranked((0..9).collect(), &TOP, ROWS, key())).words_key(),
        "topBy.exact"
    );
    assert_eq!(
        only_cut(&Bounded::last(vec![1], 5, &LAST, ROWS)).words_key(),
        "last.exact"
    );
    assert_eq!(
        only_cut(&Bounded::sampled(vec![1], 5, &SAMPLE, ROWS)).words_key(),
        "sample.exact"
    );
    let maybe = |selection| Cut::maybe(&MAYBE_ROW, ROWS, 1, selection).words_key();
    assert_eq!(maybe(Selection::Last), "last.unknown");
    assert_eq!(maybe(Selection::Sample), "sample.unknown");
    assert_eq!(
        maybe(Selection::TopBy {
            key: key(),
            from: 0
        }),
        "topBy.unknown"
    );
    assert_eq!(maybe(Selection::First { from: 0 }), "first.unknown");
}
