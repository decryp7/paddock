//! The test catalogue: three kinds (a plain one, one with options and `max_instances` 2, one clamped), on a two-column
//! surface with small limits.

use paddock_widgets::{
    Catalogue, KindSpec, Layout, Placement, PlacementId, Preset, PresetId, Refresh, Surface,
    Widget, assign_spans,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Gadget {
    Clock,
    DiskSpace {
        volume: String,
    },
    Alarms {
        rows: u8,
    },
    /// Valid, but not in the catalogue.
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GadgetKind {
    Clock,
    DiskSpace,
    Alarms,
    Hidden,
}

impl Widget for Gadget {
    type Kind = GadgetKind;

    fn kind(&self) -> GadgetKind {
        match self {
            Gadget::Clock => GadgetKind::Clock,
            Gadget::DiskSpace { .. } => GadgetKind::DiskSpace,
            Gadget::Alarms { .. } => GadgetKind::Alarms,
            Gadget::Hidden => GadgetKind::Hidden,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Gadget::DiskSpace { volume } if volume.is_empty() || volume.chars().count() > 16 => {
                Err("Choose a volume of 1 to 16 characters.".into())
            }
            _ => Ok(()),
        }
    }

    fn clamped(self) -> Gadget {
        match self {
            Gadget::Alarms { rows } => Gadget::Alarms {
                rows: rows.clamp(1, 3),
            },
            Gadget::DiskSpace { volume } => Gadget::DiskSpace {
                volume: volume.trim().to_string(),
            },
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestSurface {
    Popover,
}

pub static KINDS: [KindSpec<GadgetKind>; 3] = [
    KindSpec {
        kind: GadgetKind::Clock,
        title: "Clock",
        summary: "The time now",
        icon: "clock",
        size: &[1, 2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: GadgetKind::DiskSpace,
        title: "Disk space",
        summary: "Free space per volume",
        icon: "disk",
        size: &[2],
        max_instances: 2,
        refresh: Refresh::Every { secs: 60 },
    },
    KindSpec {
        kind: GadgetKind::Alarms,
        title: "Active alarms",
        summary: "Open alarms",
        icon: "bell",
        size: &[1],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
];

/// The test catalogue, with the fewest placements and the most saved presets as parameters.
pub struct Cat<const MIN: u16, const PRESETS: u8>;

pub type Gadgets = Cat<0, 2>;

impl<const MIN: u16, const PRESETS: u8> Catalogue for Cat<MIN, PRESETS> {
    type SurfaceId = TestSurface;
    type Widget = Gadget;
    type Place = paddock_widgets::Span;

    const SURFACE_ID: TestSurface = TestSurface::Popover;
    const SURFACE: Surface = Surface {
        noun: "popup",
        schema_version: 2,
        columns: 2,
        max_placements: 4,
        min_placements: MIN,
        max_user_presets: PRESETS,
        max_preset_placements: 3,
        max_bytes: 4096,
    };

    fn kinds() -> &'static [KindSpec<GadgetKind>] {
        &KINDS
    }

    fn defaults() -> Vec<Placement<Gadget>> {
        assign_spans([
            (1, Gadget::Clock),
            (2, disk("all")),
            (1, Gadget::Alarms { rows: 2 }),
        ])
    }

    fn presets() -> Vec<Preset<Gadget>> {
        vec![Preset {
            id: pid("compact"),
            name: "Compact".into(),
            builtin: false,
            placements: assign_spans([(2, Gadget::Clock)]),
        }]
    }
}

pub fn disk(volume: &str) -> Gadget {
    Gadget::DiskSpace {
        volume: volume.into(),
    }
}

pub fn id(text: &str) -> PlacementId {
    PlacementId::new(text).expect("a valid id")
}

pub fn pid(text: &str) -> PresetId {
    PresetId::new(text).expect("a valid id")
}

pub fn defaults() -> Layout<Gadget> {
    Layout::defaults::<Gadgets>()
}

pub fn ids(layout: &Layout<Gadget>) -> Vec<String> {
    layout.placements.iter().map(|p| p.id.to_string()).collect()
}

/// A layout document of the surface's current version.
pub fn doc(placements: Value) -> Value {
    json!({ "version": 2, "placements": placements })
}

/// A placement as stored.
pub fn stored(id: &str, span: u8, widget: Value) -> Value {
    json!({ "id": id, "span": span, "widget": widget })
}
