//! The widget layouts on the app <-> agent link: which surface a request is about, and what comes back. One enum per
//! direction instead of a generic request, because each surface has its own widget type (and postcard has no
//! self-describing value to hide it in).

use super::{DashboardKind, DashboardWidget, DynoSurface, MenuBarItem, MenuBarKind, PopoverKind, PopoverWidget};
use crate::settings::Settings;
use paddock_widgets::{view, Edit, LayoutView, WidgetError};
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
pub type PopoverView = LayoutView<DynoSurface, PopoverWidget, PopoverKind>;
/// The dashboard's layout as the chooser draws it.
pub type DashboardView = LayoutView<DynoSurface, DashboardWidget, DashboardKind>;
/// The menu bar's layout as the chooser draws it.
pub type MenuBarView = LayoutView<DynoSurface, MenuBarItem, MenuBarKind>;

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
pub fn edit_layout(settings: &mut Settings, edit: SurfaceEdit) -> Result<SurfaceView, WidgetError> {
    let surface = edit.surface();
    match edit {
        SurfaceEdit::Popover(e) => settings.popover.edit(e).map(|_| ())?,
        SurfaceEdit::Dashboard(e) => settings.dashboard.edit(e).map(|_| ())?,
        SurfaceEdit::MenuBar(e) => settings.menu_bar.edit(e).map(|_| ())?,
    }
    Ok(layout_view(settings, surface))
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
        let SurfaceView::Popover(v) = edit_layout(&mut s, SurfaceEdit::Popover(Edit::Remove { id })).unwrap() else {
            panic!("a popover view")
        };
        assert_eq!(v.layout.placements.len(), 2);
        assert!(v.can_undo);
        assert_eq!(s.dashboard, before_dash);
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
        )
        .unwrap();
        assert_eq!(s.menu_bar.layout().placements.len(), 3);
        let before = s.clone();
        let id = PlacementId::new("nope").unwrap();
        let e = edit_layout(&mut s, SurfaceEdit::MenuBar(Edit::Remove { id })).unwrap_err();
        assert!(matches!(e, WidgetError::UnknownPlacement(_)));
        assert_eq!(s, before);
        assert!(s.menu_bar.layout().can_undo(), "the undo level of the earlier edit is kept too");
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
