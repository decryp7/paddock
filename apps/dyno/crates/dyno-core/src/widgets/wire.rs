//! The widget layouts on the app <-> agent link: which surface a request is about, and what comes back. One enum per
//! direction instead of a generic request, because each surface has its own widget type (and postcard has no
//! self-describing value to hide it in).

use super::{DashboardKind, DashboardWidget, DynoSurface, MenuBarItem, MenuBarKind, PopoverKind, PopoverWidget};
use crate::settings::Settings;
use paddock_widgets::{view, Cells, Edit, LayoutView, Span, WidgetError};
use serde::{Deserialize, Serialize};

/// A value that postcard cannot carry by itself (it holds an internally tagged enum, which needs a self-describing format):
/// on the binary link it travels as one JSON string, everywhere else (the webview, tests) as the value itself.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonOnLink<T>(pub T);

impl<T: Serialize> Serialize for JsonOnLink<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        if serializer.is_human_readable() {
            self.0.serialize(serializer)
        } else {
            serializer.serialize_str(&serde_json::to_string(&self.0).map_err(S::Error::custom)?)
        }
    }
}

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for JsonOnLink<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        if deserializer.is_human_readable() {
            T::deserialize(deserializer).map(JsonOnLink)
        } else {
            let text = String::deserialize(deserializer)?;
            serde_json::from_str(&text).map(JsonOnLink).map_err(D::Error::custom)
        }
    }
}

/// The popup's layout as the chooser draws it.
pub type PopoverView = LayoutView<DynoSurface, PopoverWidget, Span, PopoverKind>;
/// The dashboard's layout as the chooser draws it.
pub type DashboardView = LayoutView<DynoSurface, DashboardWidget, Cells, DashboardKind>;
/// The menu bar's layout as the chooser draws it.
pub type MenuBarView = LayoutView<DynoSurface, MenuBarItem, Span, MenuBarKind>;

/// One surface's layout view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum SurfaceView {
    /// The tray popup.
    Popover(PopoverView),
    /// The dashboard.
    Dashboard(DashboardView),
    /// The menu bar.
    MenuBar(MenuBarView),
}

/// An edit of one surface's layout (the variant names the surface).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum SurfaceEdit {
    /// Edit the tray popup.
    Popover(Edit<PopoverWidget>),
    /// Edit the dashboard.
    Dashboard(Edit<DashboardWidget>),
    /// Edit the menu bar.
    MenuBar(Edit<MenuBarItem>),
}

impl SurfaceEdit {
    /// The surface it edits.
    pub fn surface(&self) -> DynoSurface {
        match self {
            SurfaceEdit::Popover(_) => DynoSurface::Popover,
            SurfaceEdit::Dashboard(_) => DynoSurface::Dashboard,
            SurfaceEdit::MenuBar(_) => DynoSurface::MenuBar,
        }
    }
}

/// The layout view of `surface` in `settings`.
pub fn layout_view(settings: &Settings, surface: DynoSurface) -> SurfaceView {
    match surface {
        DynoSurface::Popover => SurfaceView::Popover(view(&settings.popover)),
        DynoSurface::Dashboard => SurfaceView::Dashboard(view(&settings.dashboard)),
        DynoSurface::MenuBar => SurfaceView::MenuBar(view(&settings.menu_bar)),
    }
}

/// Applies `edit` to the layout in `settings` and returns the new view; the settings are unchanged when it is refused.
/// With `base_revision` the edit is refused as [`WidgetError::Stale`] unless the layout is still at that revision (the
/// edit was computed on an older view, e.g. in another window). The first edit of the dashboard drops the 1.0 value kept
/// under the old key (ADR 0044 §4).
pub fn edit_layout(settings: &mut Settings, edit: SurfaceEdit, base_revision: Option<u64>) -> Result<SurfaceView, WidgetError> {
    let surface = edit.surface();
    match edit {
        SurfaceEdit::Popover(e) => apply_at(&mut settings.popover, e, base_revision)?,
        SurfaceEdit::Dashboard(e) => {
            apply_at(&mut settings.dashboard, e, base_revision)?;
            settings.dashboard_v1 = None;
        }
        SurfaceEdit::MenuBar(e) => apply_at(&mut settings.menu_bar, e, base_revision)?,
    }
    Ok(layout_view(settings, surface))
}

fn apply_at<C: paddock_widgets::Catalogue>(
    stored: &mut paddock_widgets::Stored<C>,
    edit: Edit<C::Widget>,
    base_revision: Option<u64>,
) -> Result<(), WidgetError> {
    match base_revision {
        Some(base) => stored.edit_at(base, edit).map(|_| ()),
        None => stored.edit(edit).map(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::PlacementId;

    #[test]
    fn an_edit_changes_only_its_surface_and_comes_back_as_a_view() {
        let mut s = Settings::default();
        let before_dash = s.dashboard.clone();
        let id = PlacementId::new("cpu-memory").unwrap();
        let SurfaceView::Popover(v) = edit_layout(&mut s, SurfaceEdit::Popover(Edit::Remove { id }), None).unwrap() else {
            panic!("a popover view")
        };
        assert_eq!(v.layout.placements.len(), 2);
        assert!(v.can_undo);
        assert_eq!(s.dashboard, before_dash);
    }

    #[test]
    fn a_stale_revision_is_refused_and_the_first_dashboard_edit_drops_the_legacy_value() {
        let mut s = Settings::default();
        s.dashboard_v1 = Some(JsonOnLink(serde_json::json!({ "version": 1 })));
        let id = PlacementId::new("cpu").unwrap();
        let stale = edit_layout(&mut s, SurfaceEdit::Dashboard(Edit::Remove { id: id.clone() }), Some(99));
        assert_eq!(stale, Err(WidgetError::Stale));
        assert!(s.dashboard_v1.is_some(), "a refused edit is not an edit");
        let rev = s.dashboard.revision();
        edit_layout(&mut s, SurfaceEdit::Dashboard(Edit::Remove { id }), Some(rev)).unwrap();
        assert!(s.dashboard_v1.is_none());
    }

    #[test]
    fn a_refused_edit_changes_nothing() {
        let mut s = Settings::default();
        // From an edited layout: a refusal that put the defaults back would change it.
        edit_layout(
            &mut s,
            SurfaceEdit::MenuBar(Edit::Remove {
                id: PlacementId::new("number-2").unwrap(),
            }),
            None,
        )
        .unwrap();
        assert_eq!(s.menu_bar.layout().placements.len(), 3);
        let before = s.clone();
        let id = PlacementId::new("nope").unwrap();
        let e = edit_layout(&mut s, SurfaceEdit::MenuBar(Edit::Remove { id }), None).unwrap_err();
        assert!(matches!(e, WidgetError::UnknownPlacement(_)));
        assert_eq!(s, before);
        assert!(s.menu_bar.layout().can_undo(), "the undo steps of the earlier edit are kept too");
    }

    #[test]
    fn a_state_envelope_crosses_the_binary_link_as_json_and_the_webview_as_itself() {
        use crate::widgets::popover_values::{PopoverState, PopoverValue};
        let states: Vec<PopoverState> = vec![paddock_widgets::PlacementState {
            id: PlacementId::new("top-apps").unwrap(),
            state: paddock_widgets::WidgetState::Ready {
                value: PopoverValue::TopApps { rows: vec![] },
                as_of: 7,
                stale_since: None,
            },
        }];
        let wrapped = JsonOnLink(states.clone());
        assert_eq!(
            postcard::from_bytes::<JsonOnLink<Vec<PopoverState>>>(&postcard::to_stdvec(&wrapped).unwrap()).unwrap(),
            wrapped
        );
        assert_eq!(serde_json::to_value(&wrapped).unwrap(), serde_json::to_value(&states).unwrap());
        assert!(
            postcard::to_stdvec(&states).is_ok()
                && postcard::from_bytes::<Vec<PopoverState>>(&postcard::to_stdvec(&states).unwrap()).is_err()
        );
    }

    /// Every edit variant on every surface, and every surface's view, survive postcard and JSON unchanged: the wire has
    /// no second encoding hidden in a string (externally tagged enums, placements written in field order).
    #[test]
    fn every_edit_and_every_view_round_trip_through_postcard_and_json() {
        use paddock_widgets::{Dir, GridGesture, PresetId, Rect};
        let id = PlacementId::new("cpu").unwrap();
        let preset = PresetId::new("mine").unwrap();
        let gestures = [
            GridGesture::Place {
                id: id.clone(),
                rect: Rect::new(1, 2, 3, 4),
            },
            GridGesture::Nudge {
                id: id.clone(),
                dir: Dir::Left,
            },
            GridGesture::Grow {
                id: id.clone(),
                dir: Dir::Down,
            },
            GridGesture::ToFirst { id: id.clone() },
            GridGesture::ToLast { id: id.clone() },
        ];
        let mut dashboard: Vec<Edit<DashboardWidget>> = vec![
            Edit::Add {
                widget: DashboardWidget::new(DashboardKind::Graph, crate::widgets::Metric::Memory),
                span: None,
                at: Some(2),
            },
            Edit::Remove { id: id.clone() },
            Edit::Move { id: id.clone(), to: 1 },
            Edit::Resize { id: id.clone(), span: 3 },
            Edit::Replace {
                id: id.clone(),
                widget: DashboardWidget::new(DashboardKind::Ring, crate::widgets::Metric::Battery),
            },
            Edit::ApplyPreset { id: preset.clone() },
            Edit::SavePreset { name: "Mine".into() },
            Edit::DeletePreset { id: preset },
            Edit::ResetToDefaults,
            Edit::Undo,
            Edit::Redo,
        ];
        dashboard.extend(gestures.into_iter().map(|gesture| Edit::Arrange { gesture }));
        let mut edits: Vec<SurfaceEdit> = dashboard.into_iter().map(SurfaceEdit::Dashboard).collect();
        edits.push(SurfaceEdit::Popover(Edit::Remove { id: id.clone() }));
        edits.push(SurfaceEdit::MenuBar(Edit::Undo));
        for e in edits {
            assert_eq!(
                postcard::from_bytes::<SurfaceEdit>(&postcard::to_stdvec(&e).unwrap()).unwrap(),
                e,
                "postcard {e:?}"
            );
            assert_eq!(
                serde_json::from_value::<SurfaceEdit>(serde_json::to_value(&e).unwrap()).unwrap(),
                e,
                "json {e:?}"
            );
        }
        let mut s = Settings::default();
        edit_layout(
            &mut s,
            SurfaceEdit::Dashboard(Edit::Arrange {
                gesture: GridGesture::ToLast { id },
            }),
            None,
        )
        .ok();
        for surface in [DynoSurface::Popover, DynoSurface::Dashboard, DynoSurface::MenuBar] {
            let v = layout_view(&s, surface);
            assert_eq!(
                postcard::from_bytes::<SurfaceView>(&postcard::to_stdvec(&v).unwrap()).unwrap(),
                v,
                "postcard {surface:?}"
            );
            assert_eq!(
                serde_json::from_value::<SurfaceView>(serde_json::to_value(&v).unwrap()).unwrap(),
                v,
                "json {surface:?}"
            );
        }
    }

    #[test]
    fn the_edit_and_the_view_round_trip_through_postcard() {
        let e = SurfaceEdit::Popover(Edit::Add {
            widget: PopoverWidget::Network,
            span: Some(2),
            at: None,
        });
        assert_eq!(postcard::from_bytes::<SurfaceEdit>(&postcard::to_stdvec(&e).unwrap()).unwrap(), e);
        let v = layout_view(&Settings::default(), DynoSurface::Dashboard);
        assert_eq!(postcard::from_bytes::<SurfaceView>(&postcard::to_stdvec(&v).unwrap()).unwrap(), v);
    }
}
