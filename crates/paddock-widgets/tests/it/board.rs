//! A grid surface (ADR 0044): rectangles on the wire, arrange edits, revisions, previews, the view's arrangements.

use paddock_widgets::{
    Breakpoint, Catalogue, Cells, Dir, Edit, GridGesture, Kind, KindSize, KindSpec, Layout,
    Placement, Preset, Rect, Refresh, Size, SizeRule, Steps, Stored, Surface, WidgetError, apply,
    assign_ids, check_catalogue, read, view,
};
use serde_json::json;

use crate::catalogue::{Gadget, GadgetKind, TestSurface, disk, id};

const SMALL: SizeRule = SizeRule {
    widths: Steps::Range { min: 2, max: 6 },
    heights: Steps::Range { min: 2, max: 6 },
    default: Size { w: 3, h: 3 },
};
const WIDE: SizeRule = SizeRule {
    widths: Steps::Only(&[6, 12]),
    heights: Steps::Range { min: 3, max: 8 },
    default: Size { w: 6, h: 4 },
};

static KINDS: [KindSpec<GadgetKind, SizeRule>; 3] = [
    KindSpec {
        kind: GadgetKind::Clock,
        title: "Clock",
        summary: "The time now",
        icon: "clock",
        size: SMALL,
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: GadgetKind::DiskSpace,
        title: "Disk space",
        summary: "Free space per volume",
        icon: "disk",
        size: WIDE,
        max_instances: 3,
        refresh: Refresh::Every { secs: 60 },
    },
    KindSpec {
        kind: GadgetKind::Alarms,
        title: "Active alarms",
        summary: "Open alarms",
        icon: "bell",
        size: SMALL,
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
];

pub struct Board;

impl Catalogue for Board {
    type SurfaceId = TestSurface;
    type Widget = Gadget;
    type Place = Cells;

    const SURFACE_ID: TestSurface = TestSurface::Popover;
    const SURFACE: Surface = Surface {
        noun: "board",
        schema_version: 2,
        columns: 12,
        max_placements: 6,
        min_placements: 0,
        max_user_presets: 2,
        max_preset_placements: 6,
        max_bytes: 8192,
    };
    const BREAKPOINTS: &'static [Breakpoint] = &[
        Breakpoint {
            min: 840,
            columns: 12,
        },
        Breakpoint {
            min: 560,
            columns: 6,
        },
        Breakpoint { min: 0, columns: 1 },
    ];

    fn kinds() -> &'static [Kind<Self>] {
        &KINDS
    }

    fn upgrade(_from: u32, value: serde_json::Value) -> Result<serde_json::Value, String> {
        paddock_widgets::spans_to_cells::<Self>(value)
    }

    fn defaults() -> Vec<Placement<Gadget, Cells>> {
        assign_ids([
            (cells(0, 0, 6, 4), disk("all")),
            (cells(6, 0, 3, 3), Gadget::Clock),
            (cells(6, 3, 3, 3), Gadget::Alarms { rows: 2 }),
        ])
    }

    fn presets() -> Vec<Preset<Gadget, Cells>> {
        vec![Preset {
            id: crate::catalogue::pid("one"),
            name: "One".into(),
            builtin: false,
            placements: assign_ids([(cells(0, 0, 3, 3), Gadget::Clock)]),
        }]
    }
}

fn cells(x: u8, y: u16, w: u8, h: u8) -> Cells {
    Cells {
        rect: Rect::new(x, y, w, h),
    }
}

fn start() -> Layout<Gadget, Cells> {
    Layout::defaults::<Board>()
}

fn rect(layout: &Layout<Gadget, Cells>, name: &str) -> Rect {
    layout
        .placements
        .iter()
        .find(|p| p.id == id(name))
        .unwrap()
        .place
        .rect
}

fn arrange(
    layout: &Layout<Gadget, Cells>,
    gesture: GridGesture,
) -> Result<Layout<Gadget, Cells>, WidgetError> {
    apply::<Board>(layout, Edit::Arrange { gesture })
}

#[test]
fn the_catalogue_is_valid_and_the_wire_has_rectangles() {
    check_catalogue::<Board>().unwrap();
    let v = serde_json::to_value(start()).unwrap();
    assert_eq!(
        v["placements"][0],
        json!({ "id": "disk-space", "x": 0, "y": 0, "w": 6, "h": 4, "widget": { "kind": "diskSpace", "volume": "all" } })
    );
    let back = read::<Board>(&v);
    assert!(back.notes.is_empty());
    assert_eq!(back.layout, start());
}

#[test]
fn broken_grid_catalogues_are_named() {
    struct NoRoomy;
    static BAD: [KindSpec<GadgetKind, SizeRule>; 1] = [KindSpec {
        kind: GadgetKind::Clock,
        title: "Clock",
        summary: "s",
        icon: "i",
        size: SizeRule {
            widths: Steps::Range { min: 2, max: 20 },
            heights: Steps::Range { min: 2, max: 3 },
            default: Size { w: 2, h: 2 },
        },
        max_instances: 1,
        refresh: Refresh::Live,
    }];
    impl Catalogue for NoRoomy {
        type SurfaceId = TestSurface;
        type Widget = Gadget;
        type Place = Cells;
        const SURFACE_ID: TestSurface = TestSurface::Popover;
        const SURFACE: Surface = Board::SURFACE;
        const BREAKPOINTS: &'static [Breakpoint] = &[Breakpoint {
            min: 100,
            columns: 6,
        }];
        fn kinds() -> &'static [Kind<Self>] {
            &BAD
        }
        fn defaults() -> Vec<Placement<Gadget, Cells>> {
            Vec::new()
        }
    }
    let message = check_catalogue::<NoRoomy>().unwrap_err();
    assert!(message.contains("breakpoints"), "{message}");
}

#[test]
fn the_view_describes_sizes_and_arrangements() {
    let stored = Stored::<Board>::default();
    let v = view(&stored);
    assert!(matches!(
        &v.catalogue.kinds[1].size,
        KindSize::Cells {
            default_w: 6,
            default_h: 4,
            ..
        }
    ));
    assert_eq!(v.arrangements.len(), 3);
    assert_eq!(
        (v.arrangements[0].min_width, v.arrangements[0].columns),
        (840, 12)
    );
    assert_eq!(v.arrangements[1].columns, 6);
    assert!(v.arrangements[1].placements.iter().all(|p| p.rect.w <= 6));
    assert!(v.arrangements[2].placements.iter().all(|p| p.rect.w == 1));
    let order = |a: &paddock_widgets::Arrangement| {
        a.placements
            .iter()
            .map(|p| p.id.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&v.arrangements[0]), order(&v.arrangements[1]));
    assert_eq!(order(&v.arrangements[0]), order(&v.arrangements[2]));
    assert_eq!(v.moves.len(), 3);
    let clock = v.moves.iter().find(|m| m.id == id("clock")).unwrap();
    assert!(clock.moves.left && !clock.moves.up);
    let wire = serde_json::to_value(&v).unwrap();
    assert_eq!(wire["revision"], 1);
    assert_eq!(
        wire["catalogue"]["kinds"][1]["size"]["cells"]["defaultW"],
        6
    );
}

#[test]
fn a_surface_without_breakpoints_has_one_arrangement() {
    let v = view(&Stored::<crate::board::Flat>::default());
    assert_eq!(v.arrangements.len(), 1);
    assert_eq!(v.arrangements[0].columns, 12);
}

pub struct Flat;

impl Catalogue for Flat {
    type SurfaceId = TestSurface;
    type Widget = Gadget;
    type Place = Cells;
    const SURFACE_ID: TestSurface = TestSurface::Popover;
    const SURFACE: Surface = Board::SURFACE;
    fn kinds() -> &'static [Kind<Self>] {
        &KINDS
    }
    fn defaults() -> Vec<Placement<Gadget, Cells>> {
        Board::defaults()
    }
}

#[test]
fn a_flow_surface_has_no_arrangements() {
    let v = view(&Stored::<crate::catalogue::Gadgets>::default());
    assert!(v.arrangements.is_empty() && v.moves.is_empty());
}

#[test]
fn arrange_moves_swaps_and_resizes() {
    let swapped = arrange(
        &start(),
        GridGesture::Nudge {
            id: id("clock"),
            dir: Dir::Down,
        },
    )
    .unwrap();
    assert_eq!(rect(&swapped, "clock"), Rect::new(6, 3, 3, 3));
    assert_eq!(rect(&swapped, "alarms"), Rect::new(6, 0, 3, 3));
    let wider = arrange(
        &start(),
        GridGesture::Grow {
            id: id("disk-space"),
            dir: Dir::Right,
        },
    )
    .unwrap();
    assert_eq!(rect(&wider, "disk-space").w, 12, "the next allowed width");
    assert_eq!(rect(&wider, "clock").y, 4, "pushed down");
    let placed = arrange(
        &start(),
        GridGesture::Place {
            id: id("clock"),
            rect: Rect::new(0, 99, 9, 9),
        },
    )
    .unwrap();
    assert_eq!(
        rect(&placed, "clock").size(),
        Size { w: 6, h: 6 },
        "clamped to the kind's rule"
    );
}

#[test]
fn arrange_errors_are_typed() {
    let g = |gesture| arrange(&start(), gesture).unwrap_err();
    assert!(matches!(
        g(GridGesture::Nudge {
            id: id("nope"),
            dir: Dir::Up
        }),
        WidgetError::UnknownPlacement(_)
    ));
    assert_eq!(
        g(GridGesture::Nudge {
            id: id("disk-space"),
            dir: Dir::Left
        }),
        WidgetError::Blocked(paddock_widgets::Block::Edge(Dir::Left))
    );
    let message = g(GridGesture::Nudge {
        id: id("disk-space"),
        dir: Dir::Up,
    })
    .to_string();
    assert_eq!(message, "Already at the top edge.");
    for (b, text) in [
        (paddock_widgets::Block::Largest, "Can't get any larger."),
        (paddock_widgets::Block::Smallest, "Can't get any smaller."),
        (paddock_widgets::Block::Unchanged, "Nothing to change."),
        (
            paddock_widgets::Block::Edge(Dir::Right),
            "Already at the right edge.",
        ),
        (
            paddock_widgets::Block::Edge(Dir::Down),
            "Already at the bottom edge.",
        ),
        (
            paddock_widgets::Block::Edge(Dir::Left),
            "Already at the left edge.",
        ),
    ] {
        assert_eq!(WidgetError::Blocked(b).to_string(), text);
    }
    let on_flow = apply::<crate::catalogue::Gadgets>(
        &crate::catalogue::defaults(),
        Edit::Arrange {
            gesture: GridGesture::ToFirst { id: id("clock") },
        },
    );
    assert!(matches!(on_flow, Err(WidgetError::Invalid(m)) if m.contains("no grid")));
}

#[test]
fn grid_surfaces_refuse_spans_and_flow_edits() {
    let add_span = Edit::Add {
        widget: disk("x"),
        span: Some(2),
        at: None,
    };
    assert!(matches!(
        apply::<Board>(&start(), add_span),
        Err(WidgetError::Invalid(_))
    ));
    let resize = Edit::Resize {
        id: id("clock"),
        span: 2,
    };
    assert!(matches!(
        apply::<Board>(&start(), resize),
        Err(WidgetError::Invalid(_))
    ));
    let mv = Edit::Move {
        id: id("clock"),
        to: 0,
    };
    assert!(matches!(
        apply::<Board>(&start(), mv),
        Err(WidgetError::Invalid(_))
    ));
}

#[test]
fn add_takes_the_default_size_at_the_first_free_place() {
    let out = apply::<Board>(
        &start(),
        Edit::Add {
            widget: disk("d"),
            span: None,
            at: None,
        },
    )
    .unwrap();
    let added = out
        .placements
        .iter()
        .find(|p| p.id == id("disk-space-2"))
        .unwrap();
    assert_eq!(added.place.rect.size(), Size { w: 6, h: 4 });
    assert_eq!(
        (added.place.rect.x, added.place.rect.y),
        (0, 4),
        "no free place beside, so below"
    );
    let ids: Vec<_> = out.placements.iter().map(|p| p.id.to_string()).collect();
    assert_eq!(
        ids,
        ["disk-space", "clock", "alarms", "disk-space-2"],
        "reading order"
    );
}

#[test]
fn remove_closes_the_gap_and_every_edit_is_undoable() {
    let removed = apply::<Board>(&start(), Edit::Remove { id: id("clock") }).unwrap();
    assert_eq!(rect(&removed, "alarms"), Rect::new(6, 0, 3, 3), "gravity");
    let moved = arrange(
        &start(),
        GridGesture::Nudge {
            id: id("clock"),
            dir: Dir::Down,
        },
    )
    .unwrap();
    let undone = apply::<Board>(&moved, Edit::Undo).unwrap();
    assert_eq!(undone, start());
    assert_eq!(apply::<Board>(&undone, Edit::Redo).unwrap(), moved);
}

#[test]
fn presets_and_reset_settle_on_the_grid() {
    let applied = apply::<Board>(
        &start(),
        Edit::ApplyPreset {
            id: crate::catalogue::pid("one"),
        },
    )
    .unwrap();
    assert_eq!(applied.placements.len(), 1);
    let reset = apply::<Board>(&applied, Edit::ResetToDefaults).unwrap();
    assert_eq!(reset, start());
}

#[test]
fn overlaps_and_sizes_outside_the_rule_are_refused() {
    let mut bad = start();
    bad.placements[1].place = cells(2, 0, 3, 3);
    assert!(
        matches!(bad.validate::<Board>(), Err(WidgetError::Invalid(m)) if m.contains("overlap"))
    );
    let mut outside = start();
    outside.placements[1].place = cells(11, 0, 3, 3);
    assert!(outside.validate::<Board>().is_err());
    let mut size = start();
    size.placements[0].place = cells(0, 0, 7, 4);
    let e = size.validate::<Board>().unwrap_err();
    assert!(matches!(e, WidgetError::SizeNotAllowed { .. }));
    assert_eq!(
        e.to_string(),
        "This widget can be 6 or 12 columns wide and 3 to 8 rows tall."
    );
}

#[test]
fn stored_revisions_refuse_stale_edits_and_previews_change_nothing() {
    let mut s = Stored::<Board>::default();
    let r1 = s.revision();
    let gesture = GridGesture::Nudge {
        id: id("clock"),
        dir: Dir::Down,
    };
    let preview = s.preview(r1, gesture.clone()).unwrap();
    assert_eq!(s.revision(), r1);
    assert_eq!(s.layout(), &start(), "a preview does not apply");
    assert_eq!(
        preview.revision,
        r1 + 1,
        "the previewed view is the result's"
    );
    let applied = s
        .edit_at(
            r1,
            Edit::Arrange {
                gesture: gesture.clone(),
            },
        )
        .unwrap();
    assert_eq!(applied.revision, r1 + 1);
    assert_eq!(
        s.edit_at(
            r1,
            Edit::Arrange {
                gesture: gesture.clone()
            }
        ),
        Err(WidgetError::Stale)
    );
    assert_eq!(s.preview(r1, gesture), Err(WidgetError::Stale));
}

fn v1() -> serde_json::Value {
    json!({
        "version": 1,
        "placements": [
            { "id": "disk-space", "span": 6, "widget": { "kind": "diskSpace", "volume": "all" } },
            { "id": "clock", "span": 3, "widget": { "kind": "clock" } },
            { "id": "alarms", "span": 3, "widget": { "kind": "alarms", "rows": 2 } },
            { "id": "disk-space-2", "span": 12, "widget": { "kind": "diskSpace", "volume": "d" } },
            { "id": "gone", "span": 3, "widget": { "kind": "hidden" } },
            { "id": "bad", "widget": { "kind": "clock" } }
        ],
        "presets": [
            { "id": "mine", "name": "Mine", "placements": [{ "id": "clock", "span": 2, "widget": { "kind": "clock" } }] }
        ]
    })
}

#[test]
fn a_span_layout_is_upgraded_to_cells_as_it_looked() {
    let r = read::<Board>(&v1());
    assert_eq!(r.upgraded, Some(1));
    assert_eq!(r.legacy, Some(v1()));
    let rects: Vec<_> = r
        .layout
        .placements
        .iter()
        .map(|p| (p.id.to_string(), p.place.rect))
        .collect();
    // Row 1: 6 + 3 + 3 columns, stretched to the tallest card (4 rows, inside each kind's heights); row 2: full width.
    assert_eq!(rects[0], ("disk-space".into(), Rect::new(0, 0, 6, 4)));
    assert_eq!(rects[1], ("clock".into(), Rect::new(6, 0, 3, 4)));
    assert_eq!(rects[2], ("alarms".into(), Rect::new(9, 0, 3, 4)));
    assert_eq!(rects[3], ("disk-space-2".into(), Rect::new(0, 4, 12, 4)));
    assert_eq!(r.layout.version, 2);
    assert_eq!(
        r.layout.presets[0].placements[0].place.rect,
        Rect::new(0, 0, 2, 3)
    );
    assert!(
        matches!(
            r.notes[0].code,
            paddock_widgets::NoteCode::Dropped { count: 2 }
        ),
        "{:?}",
        r.notes
    );
}

#[test]
fn the_legacy_value_stays_until_the_first_edit() {
    let mut s: Stored<Board> = serde_json::from_value(v1()).unwrap();
    assert_eq!(s.legacy(), Some(&v1()));
    assert_eq!(s.take_upgraded(), Some(1));
    assert_eq!(s.take_upgraded(), None);
    let written = serde_json::to_value(&s).unwrap();
    assert_eq!(written["version"], 2);
    assert_eq!(s.legacy(), Some(&v1()), "reading and saving are not edits");
    s.edit(Edit::Remove { id: id("clock") }).unwrap();
    assert_eq!(s.legacy(), None);
}

#[test]
fn a_version_2_layout_is_not_upgraded_and_an_unusable_one_takes_the_damaged_path() {
    let current = serde_json::to_value(start()).unwrap();
    let r = read::<Board>(&current);
    assert_eq!((r.upgraded, r.legacy), (None, None));
    let not_a_layout = json!({ "version": 1, "placements": 3 });
    let r = read::<Board>(&not_a_layout);
    assert!(r.unread.is_some() && r.layout == start());
    // An upgrade that fails as a whole keeps the old value.
    struct Failing;
    impl Catalogue for Failing {
        type SurfaceId = TestSurface;
        type Widget = Gadget;
        type Place = Cells;
        const SURFACE_ID: TestSurface = TestSurface::Popover;
        const SURFACE: Surface = Board::SURFACE;
        fn kinds() -> &'static [Kind<Self>] {
            &KINDS
        }
        fn upgrade(_: u32, _: serde_json::Value) -> Result<serde_json::Value, String> {
            Err("no".into())
        }
        fn defaults() -> Vec<Placement<Gadget, Cells>> {
            Board::defaults()
        }
    }
    let r = read::<Failing>(&v1());
    assert_eq!(r.unread, Some(v1()));
}

#[test]
fn spans_to_cells_refuses_what_is_not_a_layout_or_a_grid() {
    assert!(paddock_widgets::spans_to_cells::<Board>(json!(3)).is_err());
    assert!(paddock_widgets::spans_to_cells::<Board>(json!({ "version": 1 })).is_err());
    // A flow catalogue has no size rules, so nothing converts, but the shape is kept.
    let out = paddock_widgets::spans_to_cells::<crate::catalogue::Gadgets>(v1()).unwrap();
    assert_eq!(out["placements"], json!([]));
    // Rows that do not fit the grid's height are left out.
    let many: Vec<_> = (0..200)
        .map(|n| json!({ "id": format!("w{n}"), "span": 12, "widget": { "kind": "diskSpace", "volume": "v" } }))
        .collect();
    let out = paddock_widgets::spans_to_cells::<Board>(json!({ "version": 1, "placements": many }))
        .unwrap();
    assert_eq!(
        out["placements"].as_array().unwrap().len(),
        128,
        "128 rows of 4"
    );
}

#[test]
fn previews_answer_from_the_recorded_view() {
    use paddock_widgets::{PreviewRequest, Previews};
    let previews = Previews::new().grid::<Board>();
    let stored = Stored::<Board>::default();
    let v = view(&stored);
    let request = |surface: &str, revision, seq| PreviewRequest {
        surface: surface.into(),
        revision,
        gesture: GridGesture::Nudge {
            id: id("clock"),
            dir: Dir::Down,
        },
        seq,
    };
    assert_eq!(
        previews.preview(&request("popover", 1, 1)),
        Err(WidgetError::Stale),
        "nothing recorded yet"
    );
    assert!(previews.record::<Board>(&v));
    let reply = previews
        .preview(&request("popover", v.revision, 7))
        .unwrap();
    assert_eq!(reply.seq, 7);
    assert_eq!(
        reply.view["layout"]["placements"][2]["y"], 3,
        "the clock swapped with the alarms"
    );
    assert_eq!(
        previews.preview(&request("popover", 99, 8)),
        Err(WidgetError::Stale)
    );
    assert!(matches!(
        previews.preview(&request("nope", 1, 1)),
        Err(WidgetError::Invalid(_))
    ));
    assert!(
        !Previews::new().record::<Board>(&v),
        "an unregistered surface records nothing"
    );
    let mut broken = v.clone();
    broken.layout.placements[1].place = cells(2, 0, 3, 3);
    assert!(
        !previews.record::<Board>(&broken),
        "an invalid view is not recorded"
    );
}

#[test]
fn place_defaults_and_gesture_ids() {
    use paddock_widgets::{Place, Span};
    assert_eq!(Span::from_rect(Rect::new(0, 0, 1, 1)), None);
    assert_eq!(Span { span: 1 }.rect(), None);
    assert_eq!(Cells::size_rule(&SMALL), Some(SMALL));
    assert_eq!(Span::size_rule(&&[1u8][..]), None);
    assert_eq!(
        Cells {
            rect: Rect::new(0, 0, 1, 1)
        }
        .span(),
        None
    );
    assert_eq!(
        Cells {
            rect: Rect::new(0, 0, 1, 1)
        }
        .with_span(2),
        None
    );
    let name = id("x");
    for g in [
        GridGesture::Place {
            id: name.clone(),
            rect: Rect::new(0, 0, 1, 1),
        },
        GridGesture::Nudge {
            id: name.clone(),
            dir: Dir::Up,
        },
        GridGesture::Grow {
            id: name.clone(),
            dir: Dir::Up,
        },
        GridGesture::ToFirst { id: name.clone() },
        GridGesture::ToLast { id: name.clone() },
    ] {
        assert_eq!(g.id(), &name);
    }
}

#[test]
fn an_upgrade_that_returns_no_layout_is_damaged() {
    struct Odd;
    impl Catalogue for Odd {
        type SurfaceId = TestSurface;
        type Widget = Gadget;
        type Place = Cells;
        const SURFACE_ID: TestSurface = TestSurface::Popover;
        const SURFACE: Surface = Board::SURFACE;
        fn kinds() -> &'static [Kind<Self>] {
            &KINDS
        }
        fn upgrade(_: u32, _: serde_json::Value) -> Result<serde_json::Value, String> {
            Ok(json!(3))
        }
        fn defaults() -> Vec<Placement<Gadget, Cells>> {
            Board::defaults()
        }
    }
    let r = read::<Odd>(&v1());
    assert_eq!(r.unread, Some(v1()));
    let out = paddock_widgets::spans_to_cells::<Board>(
        json!({ "version": 1, "placements": [], "presets": [5] }),
    )
    .unwrap();
    assert_eq!(out["presets"], json!([5]));
}

#[test]
fn the_outcome_names_neighbours_and_resize_limits_and_clears_after_other_edits() {
    let mut s = Stored::<Board>::default();
    let v = s
        .edit(Edit::Arrange {
            gesture: GridGesture::Grow {
                id: id("disk-space"),
                dir: Dir::Right,
            },
        })
        .unwrap();
    let o = v.outcome.unwrap();
    assert_eq!(
        (o.position, o.w, o.limit),
        (1, 12, Some(paddock_widgets::Block::Largest))
    );
    assert_eq!((o.after, o.before), (None, Some(id("clock"))));
    let v = s
        .edit(Edit::Arrange {
            gesture: GridGesture::Grow {
                id: id("disk-space"),
                dir: Dir::Down,
            },
        })
        .unwrap();
    assert_eq!(v.outcome.unwrap().limit, None, "taller still has steps");
    let v = s
        .edit(Edit::Arrange {
            gesture: GridGesture::Grow {
                id: id("disk-space"),
                dir: Dir::Left,
            },
        })
        .unwrap();
    assert_eq!(
        v.outcome.unwrap().limit,
        Some(paddock_widgets::Block::Smallest),
        "6 is this kind's narrowest width"
    );
    let v = s
        .edit(Edit::Arrange {
            gesture: GridGesture::Grow {
                id: id("disk-space"),
                dir: Dir::Up,
            },
        })
        .unwrap();
    assert_eq!(v.outcome.unwrap().limit, None);
    let v = s.edit(Edit::Remove { id: id("clock") }).unwrap();
    assert_eq!(v.outcome, None);
    // Smallest in each direction.
    let mut small = Stored::<Board>::default();
    small
        .edit(Edit::Arrange {
            gesture: GridGesture::Place {
                id: id("clock"),
                rect: Rect::new(6, 0, 2, 2),
            },
        })
        .unwrap();
    let v = small.edit(Edit::Arrange {
        gesture: GridGesture::Grow {
            id: id("clock"),
            dir: Dir::Left,
        },
    });
    assert!(matches!(
        v,
        Err(WidgetError::Blocked(paddock_widgets::Block::Smallest))
    ));
}

#[test]
fn change_kind_on_a_grid_keeps_a_rectangle_the_new_kind_allows_and_refuses_one_it_does_not() {
    // The clock (3 x 3) becomes alarms of the same size rule: the rectangle and the id stay.
    let one = apply::<Board>(&start(), Edit::Remove { id: id("alarms") }).unwrap();
    let changed = apply::<Board>(
        &one,
        Edit::ChangeKind {
            id: id("clock"),
            widget: Gadget::Alarms { rows: 9 },
        },
    )
    .unwrap();
    assert_eq!(rect(&changed, "clock"), rect(&one, "clock"));
    assert_eq!(
        changed
            .placements
            .iter()
            .find(|p| p.id == id("clock"))
            .unwrap()
            .widget,
        Gadget::Alarms { rows: 3 }
    );
    // Disk space is 6 or 12 wide: a 3-wide rectangle does not suit it.
    assert_eq!(
        apply::<Board>(
            &one,
            Edit::ChangeKind {
                id: id("clock"),
                widget: disk("x"),
            },
        ),
        Err(WidgetError::Invalid(
            "This widget's size does not suit that kind; resize it first.".into()
        ))
    );
}
