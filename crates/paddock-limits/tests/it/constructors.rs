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
    let b = Bounded::first_with(0..100, &SETTING, 10, ROWS);
    assert_eq!(b.result().len(), 10);
    let c = only_cut(&b);
    assert_eq!(c.total(), Total::AtLeast { n: 11 });
    assert_eq!(c.next(), Next::RaiseSetting);
    assert!(Bounded::first_with(0..10, &SETTING, 10, ROWS).is_complete());
    assert!(Bounded::first_with(0..0, &SETTING, 0, ROWS).is_complete());
    assert_eq!(
        Bounded::first_with(0..3, &SETTING, 0, ROWS).result().len(),
        0
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
    assert!(
        Cut::counted(
            &TOP,
            ROWS,
            6,
            5,
            Selection::TopBy {
                key: key(),
                from: 0
            }
        )
        .is_none()
    );
}

#[test]
#[should_panic(expected = "does not match")]
fn a_selection_of_another_kind_is_a_programmer_error() {
    let _ = Cut::counted(&TOP, ROWS, 1, 9, Selection::Sample);
}

#[test]
#[should_panic(expected = "test.first")]
fn from_plus_shown_past_the_total_is_a_programmer_error() {
    let _ = Cut::page(&F5, ROWS, 9, 3, 10);
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
fn describe_has_ids_paths_and_counts_only() {
    let top = Bounded::ranked((0..10).collect(), &TOP, FieldPath::lit("traces"), key());
    assert_eq!(
        only_cut(&top).describe(),
        "test.top: ranked by durationMs desc 3 shown of 10 in traces"
    );
    let first = Bounded::first(0..99, &F5, ROWS);
    assert_eq!(
        only_cut(&first).describe(),
        "test.first: the first 5 shown of more than 5 in rows"
    );
    let page = Bounded::page(vec![0; 3], 500, 1204, &F5, ROWS);
    assert_eq!(
        only_cut(&page).describe(),
        "test.first: from 501 3 shown of 1,204 in rows"
    );
    let last = Bounded::last(vec![1], 5, &LAST, FieldPath::root());
    assert_eq!(
        only_cut(&last).describe(),
        "test.last: the last 1 shown of 5 in the value"
    );
    let s = Bounded::sampled(vec![1], 5, &SAMPLE, ROWS);
    assert_eq!(
        only_cut(&s).describe(),
        "test.sample: a sample 1 shown of 5 in rows"
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
        "test.maybe: ranked by n asc from rank 5 512 shown of an unknown number in rows"
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
