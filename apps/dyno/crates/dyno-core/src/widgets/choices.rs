//! What the dashboard and menu-bar choosers may offer: for each kind, a ready default widget and the metrics the engine will
//! accept for it. The UI builds no widget value and offers no metric the engine would refuse (rule: the engine decides).

use super::dashboard::{DashboardKind, DashboardWidget};
use super::menu_bar::{MenuBarItem, MenuBarKind};
use super::metric::Metric;
use super::popover::{PopoverKind, PopoverWidget, VolumeChoice};
use paddock_widgets::Catalogue;
use serde::{Deserialize, Serialize};

/// One metric a picker offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MetricChoice {
    /// The metric.
    pub metric: Metric,
    /// Its name, sentence case.
    pub title: String,
}

/// What the chooser needs to add a dashboard widget of one kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DashboardKindChoices {
    /// The kind.
    pub kind: DashboardKind,
    /// A valid widget of this kind with default options (its metric is the first accepted).
    pub default: DashboardWidget,
    /// The metrics this kind accepts, in display order.
    pub metrics: Vec<MetricChoice>,
}

/// What the chooser needs to add a popup widget of one kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PopoverKindChoices {
    /// The kind.
    pub kind: PopoverKind,
    /// A valid widget of this kind with default options (disk space shows all volumes).
    pub default: PopoverWidget,
}

/// What the chooser needs to add a menu-bar item of one kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MenuBarKindChoices {
    /// The kind.
    pub kind: MenuBarKind,
    /// A valid item of this kind (CPU).
    pub default: MenuBarItem,
}

/// The choices of both editable surfaces' kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WidgetChoices {
    /// Popup kinds, in the catalogue's order.
    pub popover: Vec<PopoverKindChoices>,
    /// Dashboard kinds, in the catalogue's order.
    pub dashboard: Vec<DashboardKindChoices>,
    /// Menu-bar kinds, in the catalogue's order.
    pub menu_bar: Vec<MenuBarKindChoices>,
    /// Every metric a menu-bar item may show.
    pub menu_bar_metrics: Vec<MetricChoice>,
}

fn choice(metric: Metric) -> MetricChoice {
    MetricChoice {
        title: metric.title(),
        metric,
    }
}

fn menu_bar_default(kind: MenuBarKind) -> MenuBarItem {
    let metric = Metric::CpuTotal;
    match kind {
        MenuBarKind::LineGraph => MenuBarItem::LineGraph { metric },
        MenuBarKind::VerticalBars => MenuBarItem::VerticalBars { metric },
        MenuBarKind::HorizontalMeter => MenuBarItem::HorizontalMeter { metric },
        MenuBarKind::Ring => MenuBarItem::Ring { metric },
        MenuBarKind::Number => MenuBarItem::Number {
            metric,
            unit: "%".into(),
            label: String::new(),
        },
        MenuBarKind::GraphAndNumber => MenuBarItem::GraphAndNumber { metric },
    }
}

fn popover_default(kind: PopoverKind) -> PopoverWidget {
    match kind {
        PopoverKind::ActiveAlarms => PopoverWidget::ActiveAlarms,
        PopoverKind::DiskSpace => PopoverWidget::disk(VolumeChoice::All),
        PopoverKind::CpuMemory => PopoverWidget::CpuMemory,
        PopoverKind::Network => PopoverWidget::Network,
        PopoverKind::BackupHealth => PopoverWidget::BackupHealth,
        PopoverKind::LastIssue => PopoverWidget::LastIssue,
        PopoverKind::PinnedCounters => PopoverWidget::PinnedCounters,
        PopoverKind::TopApps => PopoverWidget::TopApps,
    }
}

/// The choices, built from the catalogues.
pub fn widget_choices() -> WidgetChoices {
    let dashboard = super::DashboardCatalogue::kinds()
        .iter()
        .filter_map(|spec| {
            let metrics: Vec<MetricChoice> = Metric::CHOICES
                .iter()
                .filter(|m| spec.kind.accepts(m))
                .cloned()
                .map(choice)
                .collect();
            let first = metrics.first()?.metric.clone();
            Some(DashboardKindChoices {
                kind: spec.kind,
                default: DashboardWidget::new(spec.kind, first),
                metrics,
            })
        })
        .collect();
    let menu_bar = super::MenuBarCatalogue::kinds()
        .iter()
        .map(|spec| MenuBarKindChoices {
            kind: spec.kind,
            default: menu_bar_default(spec.kind),
        })
        .collect();
    let popover = super::PopoverCatalogue::kinds()
        .iter()
        .map(|spec| PopoverKindChoices {
            kind: spec.kind,
            default: popover_default(spec.kind),
        })
        .collect();
    WidgetChoices {
        popover,
        dashboard,
        menu_bar,
        menu_bar_metrics: Metric::CHOICES.iter().cloned().map(choice).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::Widget;

    #[test]
    fn every_default_is_a_widget_the_engine_accepts() {
        let c = widget_choices();
        assert_eq!(c.dashboard.len(), super::super::DashboardCatalogue::kinds().len());
        for k in &c.dashboard {
            assert_eq!(k.default.validate(), Ok(()), "{:?}", k.kind);
            assert_eq!(k.default.kind, k.kind);
            assert!(!k.metrics.is_empty());
        }
        for k in &c.menu_bar {
            assert_eq!(k.default.validate(), Ok(()), "{:?}", k.kind);
            assert_eq!(k.default.kind(), k.kind);
        }
    }

    #[test]
    fn every_popup_default_is_valid_and_of_its_kind() {
        let c = widget_choices();
        assert_eq!(c.popover.len(), super::super::PopoverCatalogue::kinds().len());
        for k in &c.popover {
            assert_eq!(k.default.validate(), Ok(()), "{:?}", k.kind);
            assert_eq!(k.default.kind(), k.kind);
        }
        let disk = c.popover.iter().find(|k| k.kind == PopoverKind::DiskSpace).unwrap();
        assert_eq!(disk.default, PopoverWidget::disk(VolumeChoice::All));
    }

    #[test]
    fn a_kind_offers_exactly_the_metrics_it_accepts() {
        let c = widget_choices();
        let of = |kind| c.dashboard.iter().find(|k| k.kind == kind).unwrap();
        let heat: Vec<&Metric> = of(DashboardKind::Heatmap).metrics.iter().map(|m| &m.metric).collect();
        assert_eq!(heat, [&Metric::CpuPerCore]);
        assert_eq!(of(DashboardKind::Heatmap).default.metric, Metric::CpuPerCore);
        let ring = of(DashboardKind::Ring);
        assert!(ring.metrics.iter().all(|m| m.metric.is_percent()));
        assert!(!ring.metrics.iter().any(|m| m.metric == Metric::NetworkDown));
        let top: Vec<&Metric> = of(DashboardKind::TopProcesses).metrics.iter().map(|m| &m.metric).collect();
        assert!(top.contains(&&Metric::NetworkUp) && !top.contains(&&Metric::Fan));
        assert_eq!(of(DashboardKind::Graph).metrics.len(), Metric::CHOICES.len());
        assert_eq!(of(DashboardKind::Graph).default.metric, Metric::CpuTotal);
    }

    #[test]
    fn the_menu_bar_offers_every_metric_with_a_title() {
        let c = widget_choices();
        assert_eq!(c.menu_bar_metrics.len(), 13);
        assert_eq!(c.menu_bar_metrics[1].title, "CPU per core");
        assert_eq!(Metric::Process("chrome".into()).title(), "Process chrome");
    }
}
