//! The grid core: gravity, swap, push, the keyboard gestures, `moves` and the derived arrangements (ADR 0044).

use paddock_widgets::{
    Block, Dir, GridError, GridGesture, Item, MAX_ROWS, PlacementId, Rect, Size, SizeRule, Steps,
    arrange, compact, derive, moves, reading_order,
};
use proptest::prelude::*;

const RULE: SizeRule = SizeRule {
    widths: Steps::Range { min: 2, max: 12 },
    heights: Steps::Range { min: 2, max: 14 },
    default: Size { w: 3, h: 3 },
};
const ONLY: SizeRule = SizeRule {
    widths: Steps::Only(&[3, 6, 12]),
    heights: Steps::Only(&[4, 8]),
    default: Size { w: 3, h: 4 },
};

fn id(s: &str) -> PlacementId {
    PlacementId::try_from(s.to_string()).unwrap()
}

fn item(name: &str, x: u8, y: u16, w: u8, h: u8) -> Item {
    Item {
        id: id(name),
        rect: Rect::new(x, y, w, h),
        rule: RULE,
    }
}

fn rect_of(items: &[Item], name: &str) -> Rect {
    items.iter().find(|i| i.id == id(name)).unwrap().rect
}

fn ids(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.id.to_string()).collect()
}

fn place(name: &str, rect: Rect) -> GridGesture {
    GridGesture::Place { id: id(name), rect }
}

fn nudge(name: &str, dir: Dir) -> GridGesture {
    GridGesture::Nudge { id: id(name), dir }
}

fn grow(name: &str, dir: Dir) -> GridGesture {
    GridGesture::Grow { id: id(name), dir }
}

/// Four 6x4 tiles in two rows.
fn tiles() -> Vec<Item> {
    vec![
        item("a", 0, 0, 6, 4),
        item("b", 6, 0, 6, 4),
        item("c", 0, 4, 6, 4),
        item("d", 6, 4, 6, 4),
    ]
}

// ---- Steps and rules ----

#[test]
fn steps_navigate_ranges_and_lists() {
    let r = Steps::Range { min: 2, max: 5 };
    assert_eq!((r.min(), r.max()), (2, 5));
    assert_eq!(r.up(2), Some(3));
    assert_eq!(r.up(5), None);
    assert_eq!(r.up(255), None);
    assert_eq!(r.up(0), Some(2));
    assert_eq!(r.down(3), Some(2));
    assert_eq!(r.down(2), None);
    assert_eq!(r.down(0), None);
    assert_eq!(r.down(200), Some(5));
    assert_eq!(r.nearest(0), 2);
    assert_eq!(r.nearest(9), 5);
    assert!(r.allows(4) && !r.allows(6));
    let o = Steps::Only(&[3, 6, 12]);
    assert_eq!((o.min(), o.max()), (3, 12));
    assert_eq!(o.up(3), Some(6));
    assert_eq!(o.up(12), None);
    assert_eq!(o.down(6), Some(3));
    assert_eq!(o.down(3), None);
    assert_eq!(o.nearest(4), 3, "ties go to the smaller");
    assert_eq!(o.nearest(5), 6);
    assert_eq!(o.nearest(200), 12);
    assert!(o.allows(6) && !o.allows(7));
}

#[test]
fn steps_validity() {
    assert!(Steps::Range { min: 1, max: 1 }.is_valid());
    assert!(!Steps::Range { min: 0, max: 3 }.is_valid());
    assert!(!Steps::Range { min: 4, max: 3 }.is_valid());
    assert!(Steps::Only(&[1, 2]).is_valid());
    assert!(!Steps::Only(&[]).is_valid());
    assert!(!Steps::Only(&[0, 2]).is_valid());
    assert!(!Steps::Only(&[3, 3]).is_valid());
    let empty = Steps::Only(&[]);
    assert_eq!((empty.min(), empty.max(), empty.nearest(5)), (1, 1, 1));
    assert!(RULE.is_valid(12));
    assert!(!RULE.is_valid(8), "a width above the columns");
    let bad_default = SizeRule {
        default: Size { w: 1, h: 3 },
        ..RULE
    };
    assert!(!bad_default.is_valid(12));
    assert!(RULE.allows(Size { w: 2, h: 2 }) && !RULE.allows(Size { w: 1, h: 2 }));
}

#[test]
fn rect_geometry() {
    let r = Rect::new(2, 3, 4, 5);
    assert_eq!(
        (r.right(), r.bottom(), r.size()),
        (6, 8, Size { w: 4, h: 5 })
    );
    assert!(r.overlaps(&Rect::new(5, 7, 3, 3)));
    assert!(
        !r.overlaps(&Rect::new(6, 3, 3, 3)),
        "touching edges do not overlap"
    );
    assert!(!r.overlaps(&Rect::new(2, 8, 3, 3)));
    assert!(
        !r.overlaps(&Rect::new(2, 3, 0, 3)),
        "an empty rectangle overlaps nothing"
    );
    assert!(r.fits(12) && !r.fits(5));
    assert!(!Rect::new(0, 0, 0, 1).fits(12));
    assert!(!Rect::new(0, MAX_ROWS, 1, 1).fits(12));
}

// ---- Gravity ----

#[test]
fn compact_raises_everything_and_is_idempotent() {
    let items = vec![
        item("a", 0, 5, 6, 3),
        item("b", 6, 9, 6, 3),
        item("c", 0, 20, 6, 3),
    ];
    let out = compact(items).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(0, 0, 6, 3));
    assert_eq!(rect_of(&out, "b"), Rect::new(6, 0, 6, 3));
    assert_eq!(rect_of(&out, "c"), Rect::new(0, 3, 6, 3));
    assert_eq!(compact(out.clone()).unwrap(), out);
}

#[test]
fn compact_leaves_holes_beside_tall_widgets() {
    // A tall widget on the left, a short one on the right, a wide one below both: no hole is filled sideways.
    let items = vec![
        item("tall", 0, 0, 6, 8),
        item("short", 6, 0, 6, 2),
        item("wide", 0, 8, 12, 2),
    ];
    let out = compact(items).unwrap();
    assert_eq!(rect_of(&out, "wide"), Rect::new(0, 8, 12, 2));
}

#[test]
fn compact_resolves_overlap_by_pushing_down() {
    let out = compact(vec![item("a", 0, 0, 6, 3), item("b", 0, 1, 6, 3)]).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(0, 0, 6, 3));
    assert_eq!(rect_of(&out, "b"), Rect::new(0, 3, 6, 3));
}

#[test]
fn compact_reports_no_room_past_the_last_row() {
    let items = vec![item("a", 0, 0, 12, 14), item("b", 0, 0, 12, 14)];
    let mut many = items;
    for n in 0..40 {
        many.push(item(&format!("x{n}"), 0, 0, 12, 14));
    }
    assert_eq!(compact(many), Err(GridError::NoRoom));
}

#[test]
fn reading_order_is_row_column_id() {
    let out = reading_order(vec![
        item("z", 0, 0, 2, 2),
        item("b", 6, 0, 2, 2),
        item("a", 6, 0, 2, 2),
        item("c", 0, 2, 2, 2),
    ]);
    assert_eq!(ids(&out), ["z", "a", "b", "c"]);
}

// ---- Place: swap, push, clamp ----

#[test]
fn dropping_on_a_same_size_tile_swaps() {
    let out = arrange(&tiles(), 12, &place("a", Rect::new(6, 0, 6, 4))).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(6, 0, 6, 4));
    assert_eq!(rect_of(&out, "b"), Rect::new(0, 0, 6, 4));
    assert_eq!(rect_of(&out, "c"), Rect::new(0, 4, 6, 4));
}

#[test]
fn dropping_across_two_tiles_pushes_them_down() {
    let out = arrange(&tiles(), 12, &place("a", Rect::new(3, 4, 6, 4))).unwrap();
    // c and d are both overlapped: pushed below a, then gravity.
    assert!(!rect_of(&out, "a").overlaps(&rect_of(&out, "c")));
    assert!(!rect_of(&out, "a").overlaps(&rect_of(&out, "d")));
    assert_eq!(rect_of(&out, "b").y, 0);
}

#[test]
fn a_drop_into_empty_space_rises() {
    let out = arrange(&tiles(), 12, &place("a", Rect::new(0, 30, 6, 4))).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(0, 4, 6, 4));
    assert_eq!(rect_of(&out, "c"), Rect::new(0, 0, 6, 4), "the gap closes");
}

#[test]
fn a_resize_pushes_neighbours_down() {
    let out = arrange(&tiles(), 12, &place("a", Rect::new(0, 0, 6, 8))).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(0, 0, 6, 8));
    assert_eq!(rect_of(&out, "c"), Rect::new(0, 8, 6, 4));
    assert_eq!(rect_of(&out, "b"), Rect::new(6, 0, 6, 4));
}

#[test]
fn a_resized_widget_does_not_swap() {
    // a is resized to cover b exactly but with a new size: no swap, b is pushed.
    let items = vec![item("a", 0, 0, 3, 4), item("b", 3, 0, 6, 4)];
    let out = arrange(&items, 12, &place("a", Rect::new(3, 0, 6, 4))).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(3, 0, 6, 4));
    assert_eq!(rect_of(&out, "b"), Rect::new(3, 4, 6, 4));
}

#[test]
fn place_clamps_to_the_rule_and_the_grid() {
    let items = vec![Item {
        id: id("a"),
        rect: Rect::new(0, 0, 3, 4),
        rule: ONLY,
    }];
    let out = arrange(&items, 12, &place("a", Rect::new(11, 9999, 5, 7))).unwrap();
    // width 5 -> 6 (nearer than 3), height 7 -> 8, x pulled in, y rises.
    assert_eq!(rect_of(&out, "a"), Rect::new(6, 0, 6, 8));
    let out = arrange(&items, 12, &place("a", Rect::new(0, 0, 0, 0))).unwrap();
    assert_eq!(rect_of(&out, "a").size(), Size { w: 3, h: 4 });
}

#[test]
fn place_on_its_own_place_changes_nothing() {
    let out = arrange(&tiles(), 12, &place("b", Rect::new(6, 0, 6, 4))).unwrap();
    assert_eq!(rect_of(&out, "b"), Rect::new(6, 0, 6, 4));
    assert_eq!(out.len(), 4);
}

#[test]
fn cascade_pushes_what_the_pushed_widget_overlaps() {
    let items = vec![
        item("m", 0, 0, 4, 2),
        item("a", 6, 0, 6, 3),
        item("b", 6, 3, 6, 3),
        item("c", 6, 6, 6, 3),
    ];
    // m is dropped onto a (overlap at columns 6-7 row 0): a goes below m, b below a, c below b.
    let out = arrange(&items, 12, &place("m", Rect::new(4, 0, 4, 2))).unwrap();
    assert_eq!(rect_of(&out, "m"), Rect::new(4, 0, 4, 2));
    for pair in [("a", "b"), ("b", "c"), ("m", "a")] {
        assert!(
            !rect_of(&out, pair.0).overlaps(&rect_of(&out, pair.1)),
            "{pair:?}"
        );
    }
    assert!(rect_of(&out, "a").y >= 2);
    assert!(rect_of(&out, "b").y >= rect_of(&out, "a").bottom() as u16);
}

#[test]
fn unknown_ids_and_invalid_arrangements_are_refused() {
    assert_eq!(
        arrange(&tiles(), 12, &place("nope", Rect::new(0, 0, 3, 3))),
        Err(GridError::Unknown)
    );
    let overlapping = vec![item("a", 0, 0, 6, 4), item("b", 3, 0, 6, 4)];
    assert_eq!(
        arrange(&overlapping, 12, &nudge("a", Dir::Down)),
        Err(GridError::Invalid)
    );
    let outside = vec![item("a", 8, 0, 6, 4)];
    assert_eq!(
        arrange(&outside, 12, &nudge("a", Dir::Down)),
        Err(GridError::Invalid)
    );
    let twins = vec![item("a", 0, 0, 3, 3), item("a", 6, 0, 3, 3)];
    assert_eq!(
        arrange(&twins, 12, &nudge("a", Dir::Down)),
        Err(GridError::Invalid)
    );
}

// ---- Nudge ----

#[test]
fn nudge_moves_one_cell_into_free_space() {
    let items = vec![item("a", 3, 0, 3, 3), item("b", 9, 0, 3, 3)];
    let out = arrange(&items, 12, &nudge("a", Dir::Right)).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(4, 0, 3, 3));
    let out = arrange(&items, 12, &nudge("a", Dir::Left)).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(2, 0, 3, 3));
}

#[test]
fn nudge_swaps_with_a_same_size_neighbour() {
    let out = arrange(&tiles(), 12, &nudge("a", Dir::Right)).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(6, 0, 6, 4));
    assert_eq!(rect_of(&out, "b"), Rect::new(0, 0, 6, 4));
    let out = arrange(&tiles(), 12, &nudge("a", Dir::Down)).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(0, 4, 6, 4));
    assert_eq!(rect_of(&out, "c"), Rect::new(0, 0, 6, 4));
}

#[test]
fn a_swap_is_undone_by_the_reverse_nudge() {
    let there = arrange(&tiles(), 12, &nudge("a", Dir::Right)).unwrap();
    let back = arrange(&there, 12, &nudge("a", Dir::Left)).unwrap();
    assert_eq!(back, reading_order(tiles()));
}

#[test]
fn nudge_passes_a_neighbour_of_another_size() {
    let items = vec![item("a", 0, 0, 3, 3), item("b", 3, 0, 6, 3)];
    let out = arrange(&items, 12, &nudge("a", Dir::Right)).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(9, 0, 3, 3), "just past b");
    assert_eq!(rect_of(&out, "b"), Rect::new(3, 0, 6, 3));
    let back = arrange(&out, 12, &nudge("a", Dir::Left)).unwrap();
    assert_eq!(
        rect_of(&back, "a"),
        Rect::new(0, 0, 3, 3),
        "just past b, to the left"
    );
}

#[test]
fn nudge_right_clamps_at_the_edge_and_pushes() {
    let items = vec![item("a", 2, 0, 6, 3), item("b", 8, 0, 4, 3)];
    let out = arrange(&items, 12, &nudge("a", Dir::Right)).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(6, 0, 6, 3));
    let pushed = rect_of(&out, "b");
    assert_eq!((pushed.x, pushed.w, pushed.h), (8, 4, 3));
    assert!(pushed.y >= 3, "b is pushed below a");
}

#[test]
fn nudge_up_passes_the_widget_above_and_down_passes_the_one_below() {
    let items = vec![
        item("top", 0, 0, 12, 2),
        item("mid", 0, 2, 6, 3),
        item("low", 0, 5, 12, 2),
    ];
    let out = arrange(&items, 12, &nudge("mid", Dir::Up)).unwrap();
    assert_eq!(rect_of(&out, "mid").y, 0);
    assert_eq!(rect_of(&out, "top").y, 3);
    let out = arrange(&items, 12, &nudge("mid", Dir::Down)).unwrap();
    assert_eq!(rect_of(&out, "low").y, 2);
    assert_eq!(rect_of(&out, "mid").y, 4);
}

#[test]
fn nudge_at_an_edge_is_blocked_with_the_direction() {
    let b = |g| arrange(&tiles(), 12, &g);
    assert_eq!(
        b(nudge("a", Dir::Left)),
        Err(GridError::Blocked(Block::Edge(Dir::Left)))
    );
    assert_eq!(
        b(nudge("a", Dir::Up)),
        Err(GridError::Blocked(Block::Edge(Dir::Up)))
    );
    assert_eq!(
        b(nudge("b", Dir::Right)),
        Err(GridError::Blocked(Block::Edge(Dir::Right)))
    );
    assert_eq!(
        b(nudge("c", Dir::Down)),
        Err(GridError::Blocked(Block::Edge(Dir::Down)))
    );
    assert_eq!(
        b(nudge("d", Dir::Down)),
        Err(GridError::Blocked(Block::Edge(Dir::Down)))
    );
}

#[test]
fn nudge_down_passes_a_widget_that_gravity_lifted() {
    let items = vec![item("a", 0, 0, 6, 3), item("z", 0, MAX_ROWS - 3, 6, 3)];
    let items = compact(items).unwrap();
    // gravity lifts z to row 3: a can pass it
    assert!(arrange(&items, 12, &nudge("a", Dir::Down)).is_ok());
}

// ---- Grow ----

#[test]
fn grow_changes_width_and_height_by_one_step() {
    let items = vec![item("a", 0, 0, 4, 3)];
    let g = |d| arrange(&items, 12, &grow("a", d)).unwrap();
    assert_eq!(rect_of(&g(Dir::Right), "a"), Rect::new(0, 0, 5, 3));
    assert_eq!(rect_of(&g(Dir::Left), "a"), Rect::new(0, 0, 3, 3));
    assert_eq!(rect_of(&g(Dir::Down), "a"), Rect::new(0, 0, 4, 4));
    assert_eq!(rect_of(&g(Dir::Up), "a"), Rect::new(0, 0, 4, 2));
}

#[test]
fn grow_follows_a_list_rule_and_reports_the_limits() {
    let items = vec![Item {
        id: id("a"),
        rect: Rect::new(0, 0, 6, 8),
        rule: ONLY,
    }];
    let g = |d| arrange(&items, 12, &grow("a", d));
    assert_eq!(rect_of(&g(Dir::Right).unwrap(), "a").w, 12);
    assert_eq!(rect_of(&g(Dir::Left).unwrap(), "a").w, 3);
    assert_eq!(g(Dir::Down), Err(GridError::Blocked(Block::Largest)));
    assert_eq!(rect_of(&g(Dir::Up).unwrap(), "a").h, 4);
    let smallest = vec![Item {
        id: id("a"),
        rect: Rect::new(0, 0, 3, 4),
        rule: ONLY,
    }];
    assert_eq!(
        arrange(&smallest, 12, &grow("a", Dir::Left)),
        Err(GridError::Blocked(Block::Smallest))
    );
    assert_eq!(
        arrange(&smallest, 12, &grow("a", Dir::Up)),
        Err(GridError::Blocked(Block::Smallest))
    );
}

#[test]
fn grow_stops_at_the_grid_edge() {
    let items = vec![item("a", 8, 0, 4, 3)];
    assert_eq!(
        arrange(&items, 12, &grow("a", Dir::Right)),
        Err(GridError::Blocked(Block::Edge(Dir::Right)))
    );
    let bottom = vec![item("a", 0, MAX_ROWS - 3, 4, 3)];
    assert_eq!(
        arrange(&bottom, 12, &grow("a", Dir::Down)),
        Err(GridError::Blocked(Block::Edge(Dir::Down)))
    );
}

#[test]
fn grow_pushes_neighbours_down() {
    let out = arrange(&tiles(), 12, &grow("a", Dir::Down)).unwrap();
    assert_eq!(rect_of(&out, "a").h, 5);
    assert_eq!(rect_of(&out, "c").y, 5);
}

// ---- Home and End ----

#[test]
fn to_first_and_to_last() {
    let first = arrange(&tiles(), 12, &GridGesture::ToFirst { id: id("d") }).unwrap();
    assert_eq!(rect_of(&first, "d"), Rect::new(0, 0, 6, 4));
    assert_eq!(first[0].id, id("d"));
    let last = arrange(&tiles(), 12, &GridGesture::ToLast { id: id("a") }).unwrap();
    // After d, the last one in reading order; it does not rise into the hole under c, which is earlier.
    assert_eq!(rect_of(&last, "a"), Rect::new(0, 5, 6, 4));
    assert_eq!(last.last().unwrap().id, id("a"));
    let stacked = vec![item("a", 0, 0, 12, 3), item("b", 0, 3, 12, 3)];
    let last = arrange(&stacked, 12, &GridGesture::ToLast { id: id("a") }).unwrap();
    assert_eq!(last.last().unwrap().id, id("a"));
}

#[test]
fn to_first_and_to_last_report_when_nothing_changes() {
    assert_eq!(
        arrange(&tiles(), 12, &GridGesture::ToFirst { id: id("a") }),
        Err(GridError::Blocked(Block::Unchanged))
    );
    let wide = vec![item("a", 0, 0, 12, 3), item("b", 0, 3, 12, 3)];
    assert_eq!(
        arrange(&wide, 12, &GridGesture::ToLast { id: id("b") }),
        Err(GridError::Blocked(Block::Unchanged))
    );
}

// ---- Moves ----

#[test]
fn moves_agree_with_applying_each_gesture() {
    let m = moves(&tiles(), 12, &id("a"));
    assert!(!m.left && !m.up && !m.first, "a is first, top-left");
    assert!(m.right && m.down && m.last && m.wider && m.taller && m.narrower && m.shorter);
    let none = moves(&tiles(), 12, &id("nope"));
    assert_eq!(none, Default::default());
    let overlapping = vec![item("a", 0, 0, 6, 4), item("b", 3, 0, 6, 4)];
    assert_eq!(moves(&overlapping, 12, &id("a")), Default::default());
    assert_eq!(
        paddock_widgets::moves_all(&overlapping, 12),
        vec![Default::default(); 2]
    );
}

// ---- Derive ----

#[test]
fn derive_halves_and_fulls_in_reading_order() {
    let items = vec![
        item("a", 0, 0, 3, 4),
        item("b", 3, 0, 3, 4),
        item("c", 6, 0, 6, 4),
        item("d", 0, 4, 12, 3),
    ];
    let out = derive(&items, 12, 6).unwrap();
    assert_eq!(rect_of(&out, "a"), Rect::new(0, 0, 3, 4));
    assert_eq!(rect_of(&out, "b"), Rect::new(3, 0, 3, 4));
    assert_eq!(
        rect_of(&out, "c"),
        Rect::new(0, 4, 3, 4),
        "6 of 12 is half of 6, but a and b fill the row: next row"
    );
    assert_eq!(rect_of(&out, "d"), Rect::new(0, 8, 6, 3));
    let one = derive(&items, 12, 1).unwrap();
    assert!(one.iter().all(|i| i.rect.w == 1 && i.rect.x == 0));
    assert_eq!(ids(&one), ["a", "b", "c", "d"]);
    assert_eq!(rect_of(&one, "d").y, 12);
}

#[test]
fn derive_respects_the_kinds_scaled_minimum() {
    // Share 3/12 -> 1.5 of 6; the kind's minimum of 4 scaled is 2, still half (3). A minimum of 8 scaled is 4: full.
    let wide_min = SizeRule {
        widths: Steps::Range { min: 8, max: 12 },
        ..RULE
    };
    let items = vec![Item {
        id: id("a"),
        rect: Rect::new(0, 0, 8, 3),
        rule: wide_min,
    }];
    assert_eq!(rect_of(&derive(&items, 12, 6).unwrap(), "a").w, 6);
    let narrow_ok = vec![item("a", 0, 0, 2, 3)];
    assert_eq!(rect_of(&derive(&narrow_ok, 12, 6).unwrap(), "a").w, 3);
}

#[test]
fn derive_never_overlaps_and_reports_no_room() {
    let tall = vec![item("a", 0, 0, 12, 14), item("b", 0, 14, 12, 14)];
    assert!(derive(&tall, 12, 1).is_ok());
    let many: Vec<Item> = (0..40u16)
        .map(|n| item(&format!("x{n}"), 0, n * 12, 12, 14))
        .collect();
    assert_eq!(
        derive(&many, 12, 1),
        Err(GridError::NoRoom),
        "40 x 14 rows do not fit 512"
    );
    assert_eq!(derive(&[], 12, 6), Ok(vec![]));
    assert!(
        derive(&tiles(), 0, 0).is_ok(),
        "zero columns are treated as one"
    );
}

#[test]
fn derive_skips_cells_taken_by_tall_neighbours() {
    // Sparse placement: a tall half-width widget and two short ones: the second short one goes under the first.
    let items = vec![
        item("tall", 0, 0, 6, 10),
        item("s1", 6, 0, 6, 3),
        item("s2", 6, 3, 6, 3),
    ];
    let out = derive(&items, 12, 6).unwrap();
    assert_eq!(rect_of(&out, "tall"), Rect::new(0, 0, 3, 10));
    assert_eq!(rect_of(&out, "s1"), Rect::new(3, 0, 3, 3));
    assert_eq!(
        rect_of(&out, "s2"),
        Rect::new(3, 3, 3, 3),
        "the first free place after s1, beside the tall one"
    );
}

// ---- Properties ----

fn arbitrary_items() -> impl Strategy<Value = Vec<Item>> {
    prop::collection::vec((0u8..12, 0u16..40, 2u8..=12, 2u8..=14), 1..12).prop_map(|raw| {
        let items: Vec<Item> = raw
            .into_iter()
            .enumerate()
            .map(|(n, (x, y, w, h))| {
                let w = w.min(12);
                item(&format!("p{n}"), x.min(12 - w), y, w, h)
            })
            .collect();
        compact(items).unwrap_or_default()
    })
}

fn gesture_for(items: &[Item], pick: usize, kind: u8, a: u8, b: u16, c: u8, d: u8) -> GridGesture {
    let id = items[pick % items.len()].id.clone();
    let dir = [Dir::Left, Dir::Right, Dir::Up, Dir::Down][usize::from(a) % 4];
    match kind % 5 {
        0 => GridGesture::Place {
            id,
            rect: Rect::new(a % 14, b % 60, c % 14, d % 16),
        },
        1 => GridGesture::Nudge { id, dir },
        2 => GridGesture::Grow { id, dir },
        3 => GridGesture::ToFirst { id },
        _ => GridGesture::ToLast { id },
    }
}

fn is_end(g: &GridGesture) -> bool {
    matches!(g, GridGesture::ToLast { .. })
}

fn assert_valid(items: &[Item]) {
    for (n, a) in items.iter().enumerate() {
        assert!(a.rect.fits(12), "{a:?}");
        assert!(RULE.allows(a.rect.size()), "{a:?}");
        for b in &items[n + 1..] {
            assert!(!a.rect.overlaps(&b.rect), "{a:?} {b:?}");
        }
    }
}

/// No item can move up by one row without overlapping another.
fn assert_gravity(items: &[Item]) {
    for a in items {
        if a.rect.y == 0 {
            continue;
        }
        let up = Rect {
            y: a.rect.y - 1,
            ..a.rect
        };
        assert!(
            items.iter().any(|b| b.id != a.id && b.rect.overlaps(&up)),
            "{a:?} could rise in {items:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    #[test]
    fn arrangements_stay_valid_compact_and_deterministic(
        items in arbitrary_items(),
        pick in any::<usize>(), kind in any::<u8>(), a in any::<u8>(), b in any::<u16>(), c in any::<u8>(), d in any::<u8>(),
    ) {
        assert_valid(&items);
        assert_gravity(&items);
        let g = gesture_for(&items, pick, kind, a, b, c, d);
        let first = arrange(&items, 12, &g);
        let second = arrange(&items, 12, &g);
        prop_assert_eq!(&first, &second);
        if let Ok(out) = first {
            assert_valid(&out);
            prop_assert_eq!(out.len(), items.len());
            prop_assert_eq!(reading_order(out.clone()), out.clone());
            if is_end(&g) {
                // End: the moved placement is last in reading order; the others are compact.
                let GridGesture::ToLast { id } = &g else { unreachable!() };
                prop_assert_eq!(&out.last().unwrap().id, id);
            } else {
                assert_gravity(&out);
                prop_assert_eq!(compact(out.clone()).unwrap(), out);
            }
        }
    }

    #[test]
    fn moves_match_applying_every_gesture(items in arbitrary_items(), pick in any::<usize>()) {
        let id = items[pick % items.len()].id.clone();
        let m = moves(&items, 12, &id);
        let ok = |g| arrange(&items, 12, &g).is_ok();
        let n = |dir| GridGesture::Nudge { id: id.clone(), dir };
        let g = |dir| GridGesture::Grow { id: id.clone(), dir };
        prop_assert_eq!(m.left, ok(n(Dir::Left)));
        prop_assert_eq!(m.right, ok(n(Dir::Right)));
        prop_assert_eq!(m.up, ok(n(Dir::Up)));
        prop_assert_eq!(m.down, ok(n(Dir::Down)));
        prop_assert_eq!(m.wider, ok(g(Dir::Right)));
        prop_assert_eq!(m.narrower, ok(g(Dir::Left)));
        prop_assert_eq!(m.taller, ok(g(Dir::Down)));
        prop_assert_eq!(m.shorter, ok(g(Dir::Up)));
        prop_assert_eq!(m.first, ok(GridGesture::ToFirst { id: id.clone() }));
        prop_assert_eq!(m.last, ok(GridGesture::ToLast { id: id.clone() }));
        // The batch answer is the single one.
        let all = paddock_widgets::moves_all(&items, 12);
        let position = items.iter().position(|i| i.id == id).unwrap();
        prop_assert_eq!(all[position], m);
    }

    #[test]
    fn derived_arrangements_keep_reading_order_and_do_not_overlap(items in arbitrary_items(), columns in prop::sample::select(vec![1u8, 6])) {
        if let Ok(out) = derive(&items, 12, columns) {
            prop_assert_eq!(ids(&out), ids(&reading_order(items.clone())));
            for (n, a) in out.iter().enumerate() {
                prop_assert!(a.rect.fits(columns));
                for b in &out[n + 1..] {
                    prop_assert!(!a.rect.overlaps(&b.rect));
                }
            }
        }
    }
}

// ---- A board with no room below ----

/// 36 full-width items of 14 rows: the stack ends at row 504 of 512.
fn full_board() -> Vec<Item> {
    (0..36u16)
        .map(|n| item(&format!("f{n}"), 0, n * 14, 12, 14))
        .collect()
}

#[test]
fn a_push_past_the_last_row_is_no_room() {
    let r = arrange(&full_board(), 12, &place("f0", Rect::new(0, 10, 12, 14))).map(|v| v.len());
    assert_eq!(r, Err(GridError::NoRoom));
}

#[test]
fn moves_on_a_board_with_no_room_are_still_what_applying_each_gesture_says() {
    let items = full_board();
    let all = paddock_widgets::moves_all(&items, 12);
    for (n, i) in items.iter().enumerate() {
        let id = i.id.clone();
        let ok = |g| arrange(&items, 12, &g).is_ok();
        let m = all[n];
        assert_eq!(m.up, ok(nudge(id.as_str(), Dir::Up)), "{id}");
        assert_eq!(m.down, ok(nudge(id.as_str(), Dir::Down)), "{id}");
        assert_eq!(m.left, ok(nudge(id.as_str(), Dir::Left)), "{id}");
        assert_eq!(m.taller, ok(grow(id.as_str(), Dir::Down)), "{id}");
        assert_eq!(m.shorter, ok(grow(id.as_str(), Dir::Up)), "{id}");
        assert_eq!(m.first, ok(GridGesture::ToFirst { id: id.clone() }), "{id}");
        assert_eq!(m.last, ok(GridGesture::ToLast { id: id.clone() }), "{id}");
    }
    // The first of the stack cannot go up or be first; passing the second needs 14 more rows than there are.
    assert!(!all[0].up && !all[0].first);
    assert!(!all[0].down || arrange(&items, 12, &nudge("f0", Dir::Down)).is_ok());
}
