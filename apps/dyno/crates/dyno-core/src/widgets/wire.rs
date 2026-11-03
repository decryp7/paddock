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

/// One surface's layout view. A dashboard placement is flattened (`x`, `y`, `w`, `h` beside `id`), which postcard cannot
/// write, so like [`JsonOnLink`] this travels as one JSON string on the binary link and as itself everywhere else.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS), ts(rename_all = "camelCase"))]
pub enum SurfaceView {
    /// The tray popup.
    Popover(PopoverView),
    /// The dashboard.
    Dashboard(DashboardView),
    /// The menu bar.
    MenuBar(MenuBarView),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum ViewRef<'a> {
    Popover(&'a PopoverView),
    Dashboard(&'a DashboardView),
    MenuBar(&'a MenuBarView),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ViewOwned {
    Popover(PopoverView),
    Dashboard(DashboardView),
    MenuBar(MenuBarView),
}

impl Serialize for SurfaceView {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let view = match self {
            SurfaceView::Popover(v) => ViewRef::Popover(v),
            SurfaceView::Dashboard(v) => ViewRef::Dashboard(v),
            SurfaceView::MenuBar(v) => ViewRef::MenuBar(v),
        };
        if serializer.is_human_readable() {
            view.serialize(serializer)
        } else {
            serializer.serialize_str(&serde_json::to_string(&view).map_err(S::Error::custom)?)
        }
    }
}

impl<'de> Deserialize<'de> for SurfaceView {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let owned = if deserializer.is_human_readable() {
            ViewOwned::deserialize(deserializer)?
        } else {
            serde_json::from_str(&String::deserialize(deserializer)?).map_err(D::Error::custom)?
        };
        Ok(match owned {
            ViewOwned::Popover(v) => SurfaceView::Popover(v),
            ViewOwned::Dashboard(v) => SurfaceView::Dashboard(v),
            ViewOwned::MenuBar(v) => SurfaceView::MenuBar(v),
        })
    }
}

/// An edit of one surface's layout (the variant names the surface). A grid gesture is an internally tagged enum, which
/// postcard cannot read, so like [`SurfaceView`] it travels as one JSON string on the binary link.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS), ts(rename_all = "camelCase"))]
pub enum SurfaceEdit {
    /// Edit the tray popup.
    Popover(Edit<PopoverWidget>),
    /// Edit the dashboard.
    Dashboard(Edit<DashboardWidget>),
    /// Edit the menu bar.
    MenuBar(Edit<MenuBarItem>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum EditRef<'a> {
    Popover(&'a Edit<PopoverWidget>),
    Dashboard(&'a Edit<DashboardWidget>),
    MenuBar(&'a Edit<MenuBarItem>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum EditOwned {
    Popover(Edit<PopoverWidget>),
    Dashboard(Edit<DashboardWidget>),
    MenuBar(Edit<MenuBarItem>),
}

impl Serialize for SurfaceEdit {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let edit = match self {
            SurfaceEdit::Popover(e) => EditRef::Popover(e),
            SurfaceEdit::Dashboard(e) => EditRef::Dashboard(e),
            SurfaceEdit::MenuBar(e) => EditRef::MenuBar(e),
        };
        if serializer.is_human_readable() {
            edit.serialize(serializer)
        } else {
            serializer.serialize_str(&serde_json::to_string(&edit).map_err(S::Error::custom)?)
        }
    }
}

impl<'de> Deserialize<'de> for SurfaceEdit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let owned = if deserializer.is_human_readable() {
            EditOwned::deserialize(deserializer)?
        } else {
            serde_json::from_str(&String::deserialize(deserializer)?).map_err(D::Error::custom)?
        };
        Ok(match owned {
            EditOwned::Popover(e) => SurfaceEdit::Popover(e),
            EditOwned::Dashboard(e) => SurfaceEdit::Dashboard(e),
            EditOwned::MenuBar(e) => SurfaceEdit::MenuBar(e),
        })
    }
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
                id: PlacementId::new("vertical-bars-2").unwrap(),
            }),
            None,
        )
        .unwrap();
        assert_eq!(s.menu_bar.layout().placements.len(), 1);
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
