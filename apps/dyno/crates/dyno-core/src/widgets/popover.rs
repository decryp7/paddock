//! The tray popup's catalogue: which widgets it offers, their options, spans, limits and the defaults.

use super::DynoSurface;
use paddock_widgets::{assign_spans, Catalogue, KindSpec, Placement, Refresh, Surface, Widget};
use serde::{Deserialize, Serialize};

/// Which volume a disk space widget shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum VolumeChoice {
    /// Every volume that is mounted (the first few).
    All,
    /// The system volume.
    System,
    /// One volume, by its id (the mount point, `Volume::id`).
    Mount(String),
}

/// The longest volume id a widget keeps, in characters.
pub const MAX_VOLUME_ID: usize = 256;

/// A widget of the tray popup; its variants carry its options.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PopoverWidget {
    /// Active alarms, most severe first.
    ActiveAlarms,
    /// Free space per volume.
    DiskSpace {
        /// Which volume.
        volume: VolumeChoice,
    },
    /// CPU load and memory in use, with the last minute.
    CpuMemory,
    /// Download and upload now, with the last minute.
    Network,
    /// The backup check's last report.
    BackupHealth,
    /// The newest problem of the last 7 days.
    LastIssue,
    /// The pinned performance counters.
    PinnedCounters,
    /// The busiest apps.
    TopApps,
}

/// The kinds of [`PopoverWidget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum PopoverKind {
    /// [`PopoverWidget::ActiveAlarms`].
    ActiveAlarms,
    /// [`PopoverWidget::DiskSpace`].
    DiskSpace,
    /// [`PopoverWidget::CpuMemory`].
    CpuMemory,
    /// [`PopoverWidget::Network`].
    Network,
    /// [`PopoverWidget::BackupHealth`].
    BackupHealth,
    /// [`PopoverWidget::LastIssue`].
    LastIssue,
    /// [`PopoverWidget::PinnedCounters`].
    PinnedCounters,
    /// [`PopoverWidget::TopApps`].
    TopApps,
}

impl Widget for PopoverWidget {
    type Kind = PopoverKind;

    fn kind(&self) -> PopoverKind {
        match self {
            PopoverWidget::ActiveAlarms => PopoverKind::ActiveAlarms,
            PopoverWidget::DiskSpace { .. } => PopoverKind::DiskSpace,
            PopoverWidget::CpuMemory => PopoverKind::CpuMemory,
            PopoverWidget::Network => PopoverKind::Network,
            PopoverWidget::BackupHealth => PopoverKind::BackupHealth,
            PopoverWidget::LastIssue => PopoverKind::LastIssue,
            PopoverWidget::PinnedCounters => PopoverKind::PinnedCounters,
            PopoverWidget::TopApps => PopoverKind::TopApps,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            PopoverWidget::DiskSpace {
                volume: VolumeChoice::Mount(id),
            } if id.trim().is_empty() || id.chars().count() > MAX_VOLUME_ID || id.chars().any(char::is_control) => {
                Err("Choose a volume from the list.".into())
            }
            _ => Ok(()),
        }
    }
}

impl PopoverWidget {
    /// A disk space widget for `volume`.
    pub fn disk(volume: VolumeChoice) -> PopoverWidget {
        PopoverWidget::DiskSpace { volume }
    }
}

/// The popup's catalogue (ADR 0035 §5).
pub struct PopoverCatalogue;

const KINDS: &[KindSpec<PopoverKind>] = &[
    KindSpec {
        kind: PopoverKind::ActiveAlarms,
        title: "Active alarms",
        summary: "What needs attention now, the most severe first",
        icon: "alarm",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverKind::DiskSpace,
        title: "Disk space",
        summary: "Free space on your volumes",
        icon: "disk",
        size: &[2],
        max_instances: 4,
        refresh: Refresh::Every { secs: 60 },
    },
    KindSpec {
        kind: PopoverKind::CpuMemory,
        title: "CPU and memory",
        summary: "Load and use now, with the last minute",
        icon: "cpu",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverKind::Network,
        title: "Network",
        summary: "Download and upload now, with the last minute",
        icon: "network",
        size: &[1, 2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverKind::BackupHealth,
        title: "Backup",
        summary: "When your last backup ran and whether it is healthy",
        icon: "backup",
        size: &[1, 2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverKind::LastIssue,
        title: "Last issue",
        summary: "The newest problem the system recorded",
        icon: "issue",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverKind::PinnedCounters,
        title: "Pinned counters",
        summary: "Your own performance counters",
        icon: "counters",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverKind::TopApps,
        title: "Top apps",
        summary: "The apps using the most CPU",
        icon: "apps",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
];

impl Catalogue for PopoverCatalogue {
    type SurfaceId = DynoSurface;
    type Widget = PopoverWidget;
    type Place = paddock_widgets::Span;
    const SURFACE_ID: DynoSurface = DynoSurface::Popover;
    const SURFACE: Surface = Surface {
        noun: "popup",
        schema_version: 1,
        columns: 2,
        max_placements: 8,
        min_placements: 0,
        max_user_presets: 0,
        max_preset_placements: 8,
        max_bytes: super::MAX_BYTES,
    };

    fn kinds() -> &'static [KindSpec<PopoverKind>] {
        KINDS
    }

    fn defaults() -> Vec<Placement<PopoverWidget>> {
        assign_spans([
            (2, PopoverWidget::ActiveAlarms),
            (2, PopoverWidget::disk(VolumeChoice::All)),
            (2, PopoverWidget::CpuMemory),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::{check_catalogue, read, Edit, Stored};

    #[test]
    fn the_catalogue_is_consistent() {
        check_catalogue::<PopoverCatalogue>().unwrap();
    }

    #[test]
    fn the_defaults_are_alarms_disk_space_and_cpu_memory() {
        let kinds: Vec<PopoverKind> = PopoverCatalogue::defaults().iter().map(|p| p.widget.kind()).collect();
        assert_eq!(kinds, [PopoverKind::ActiveAlarms, PopoverKind::DiskSpace, PopoverKind::CpuMemory]);
        assert!(PopoverCatalogue::defaults().iter().all(|p| p.place.span == 2));
    }

    #[test]
    fn disk_space_may_be_placed_four_times_and_the_rest_once() {
        let max: Vec<(PopoverKind, u8)> = PopoverCatalogue::kinds().iter().map(|k| (k.kind, k.max_instances)).collect();
        assert_eq!(max.iter().filter(|(_, m)| *m == 4).count(), 1);
        assert!(max.contains(&(PopoverKind::DiskSpace, 4)));
        assert!(max.iter().filter(|(k, _)| *k != PopoverKind::DiskSpace).all(|(_, m)| *m == 1));
    }

    #[test]
    fn an_unknown_kind_in_the_file_is_dropped_with_a_note() {
        let value = serde_json::json!({"version": 1, "placements": [
            {"id": "cpu-memory", "span": 2, "widget": "cpuMemory"},
            {"id": "gpu", "span": 2, "widget": "gpuHeat"},
        ], "presets": []});
        let r = read::<PopoverCatalogue>(&value);
        assert_eq!(r.layout.placements.len(), 1);
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn a_volume_needs_an_id_and_the_popup_holds_eight() {
        let mut s = Stored::<PopoverCatalogue>::default();
        let bad = s.edit(Edit::Add {
            widget: PopoverWidget::disk(VolumeChoice::Mount(" ".into())),
            span: None,
            at: None,
        });
        assert!(bad.is_err());
        for _ in 0..3 {
            s.edit(Edit::Add {
                widget: PopoverWidget::disk(VolumeChoice::System),
                span: None,
                at: None,
            })
            .unwrap();
        }
        assert_eq!(s.layout().placements.len(), 6);
        assert!(s.layout().placements.iter().any(|p| p.id.as_str() == "disk-space-4"));
    }

    #[test]
    fn the_wire_shape_is_camel_case_and_externally_tagged() {
        let p = PopoverWidget::disk(VolumeChoice::Mount("/Volumes/Backup".into()));
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json, serde_json::json!({"diskSpace": {"volume": {"mount": "/Volumes/Backup"}}}));
        assert_eq!(serde_json::to_value(PopoverWidget::CpuMemory).unwrap(), "cpuMemory");
        let back: PopoverWidget = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }
}
