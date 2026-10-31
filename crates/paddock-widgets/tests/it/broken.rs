//! Catalogues that are wrong in one way each: `check_catalogue` names the problem; reading still never panics.

use paddock_widgets::{
    Catalogue, KindSpec, Placement, Preset, Refresh, Surface, assign_ids, builtin_presets,
    check_catalogue,
};

use crate::catalogue::{Gadget, GadgetKind, Gadgets, KINDS, disk, id};

const SURFACE: Surface = Surface {
    id: "popover",
    schema_version: 1,
    columns: 2,
    max_placements: 4,
    min_placements: 0,
    max_user_presets: 1,
    max_preset_placements: 3,
    max_bytes: 4096,
};

const fn spec(
    kind: GadgetKind,
    title: &'static str,
    spans: &'static [u8],
    max_instances: u8,
) -> KindSpec<GadgetKind> {
    KindSpec {
        kind,
        title,
        summary: "s",
        icon: "i",
        spans,
        max_instances,
        refresh: Refresh::Live,
    }
}

macro_rules! broken {
    ($name:ident, $kinds:expr, $defaults:expr, $presets:expr, $min:expr) => {
        struct $name;
        impl Catalogue for $name {
            type Widget = Gadget;
            const SURFACE: Surface = Surface {
                min_placements: $min,
                ..SURFACE
            };
            fn kinds() -> &'static [KindSpec<GadgetKind>] {
                $kinds
            }
            fn defaults() -> Vec<Placement<Gadget>> {
                $defaults
            }
            fn presets() -> Vec<Preset<Gadget>> {
                $presets
            }
        }
    };
}

static NO_KINDS: [KindSpec<GadgetKind>; 0] = [];
static TWICE: [KindSpec<GadgetKind>; 2] = [
    spec(GadgetKind::Clock, "Clock", &[1], 1),
    spec(GadgetKind::Clock, "Clock", &[1], 1),
];
static NO_TITLE: [KindSpec<GadgetKind>; 1] = [spec(GadgetKind::Clock, "", &[1], 1)];
static NO_SPANS: [KindSpec<GadgetKind>; 1] = [spec(GadgetKind::Clock, "Clock", &[], 1)];
static ZERO_SPAN: [KindSpec<GadgetKind>; 1] = [spec(GadgetKind::Clock, "Clock", &[0], 1)];
static WIDE_SPAN: [KindSpec<GadgetKind>; 1] = [spec(GadgetKind::Clock, "Clock", &[3], 1)];
static NEVER: [KindSpec<GadgetKind>; 1] = [spec(GadgetKind::Clock, "Clock", &[1], 0)];
static CLOCK: [KindSpec<GadgetKind>; 1] = [spec(GadgetKind::Clock, "Clock", &[1], 1)];

fn two_clocks() -> Vec<Placement<Gadget>> {
    assign_ids([(1, Gadget::Clock), (1, Gadget::Clock)])
}

broken!(NoKinds, &NO_KINDS, Vec::new(), Vec::new(), 0);
broken!(Twice, &TWICE, Vec::new(), Vec::new(), 0);
broken!(NoTitle, &NO_TITLE, Vec::new(), Vec::new(), 0);
broken!(NoSpans, &NO_SPANS, Vec::new(), Vec::new(), 0);
broken!(ZeroSpan, &ZERO_SPAN, Vec::new(), Vec::new(), 0);
broken!(WideSpan, &WIDE_SPAN, Vec::new(), Vec::new(), 0);
broken!(Never, &NEVER, Vec::new(), Vec::new(), 0);
broken!(BadDefaults, &CLOCK, two_clocks(), Vec::new(), 0);
broken!(TooFewDefaults, &CLOCK, Vec::new(), Vec::new(), 1);
broken!(
    BadPreset,
    &CLOCK,
    Vec::new(),
    vec![Preset {
        id: id("two"),
        name: "Two".into(),
        builtin: false,
        placements: two_clocks()
    }],
    0
);

fn message<C: Catalogue>() -> String {
    check_catalogue::<C>().expect_err("a broken catalogue")
}

#[test]
fn the_test_catalogue_is_valid() {
    assert_eq!(check_catalogue::<Gadgets>(), Ok(()));
    assert_eq!(KINDS.len(), 3);
    let _ = disk("x");
}

#[test]
fn each_problem_is_named() {
    assert_eq!(message::<NoKinds>(), "the catalogue has no kinds");
    assert!(message::<Twice>().contains("listed twice"));
    assert!(message::<NoTitle>().contains("needs a title"));
    assert!(message::<NoSpans>().contains("needs spans between 1 and 2"));
    assert!(message::<ZeroSpan>().contains("needs spans"));
    assert!(message::<WideSpan>().contains("needs spans"));
    assert!(message::<Never>().contains("max_instances is 0"));
    assert!(message::<BadDefaults>().contains("defaults"));
    assert!(message::<TooFewDefaults>().contains("defaults"));
    assert!(message::<BadPreset>().contains("preset two"));
}

#[test]
fn builtin_presets_drop_invalid_placements_and_are_marked() {
    let presets = builtin_presets::<BadPreset>();
    assert_eq!(presets.len(), 1);
    assert!(presets[0].builtin);
    assert_eq!(presets[0].placements.len(), 1);
}
