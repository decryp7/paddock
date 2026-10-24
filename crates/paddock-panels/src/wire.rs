//! What the UI sends and gets back (camelCase JSON), and the pop-out layer's events.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// To the host window only: the panel layer's events, one stream. Payload: [`PanelsEvent`].
pub const PANELS_EVENT: &str = "paddock://panels";
/// UI state forwarded between the windows of a host (`ui_sync_publish`). Payload: the message as published.
pub const UI_SYNC_EVENT: &str = "paddock://ui-sync";

/// What happened to the pop-outs of a host; sent to the host window only.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PanelsEvent {
    /// A pop-out is gone (closed, or asked to dock back): dock its panel again.
    #[serde(rename_all = "camelCase")]
    Returned { panel: String, label: String },
    /// A pop-out opened, moved, resized or closed (persist the layout).
    Changed,
    /// A pop-out window is being moved; the cursor in logical screen px. The host shows its dock guides under the
    /// cursor and docks the panel when the window is dropped on one.
    #[serde(rename_all = "camelCase")]
    Moved {
        panel: String,
        label: String,
        x: f64,
        y: f64,
    },
}

/// A window's place the UI asks for or reports (logical px). With a `scale`, `x`/`y` are logical px of the display with
/// that scale factor (what the drag ghost reports), so a pop-out dropped on a second display lands there exactly.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PopOutBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The scale factor of the display `x` and `y` are logical px of.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub scale: Option<f64>,
}

/// What the UI asks for when popping a panel out.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PopOutRequest {
    /// The panel id (`pn.code` or `pn.code#key`).
    pub panel: String,
    /// The panel title (the window title is "<title> - <host title>").
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// Where to open; without it the place remembered for the view, else beside the host window.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub bounds: Option<PopOutBounds>,
    /// Stay above the host window (an owned window), not above everything.
    #[serde(default)]
    pub keep_on_top: bool,
}

/// A pop-out as the UI sees it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopOutInfo {
    pub label: String,
    pub host: String,
    pub panel: String,
    pub title: String,
    /// Where it is now (logical px), when the platform says.
    pub bounds: Option<PopOutBounds>,
    /// The name of the display it is on, when the platform says.
    pub monitor: Option<String>,
    pub keep_on_top: bool,
}

/// Which windows of a host get a forwarded message.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncScope {
    /// The host and its pop-outs.
    Full,
    /// The host, its pop-outs and its dialogs (theme and formats).
    Display,
}

/// A message to forward to the other windows of the sender's host.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncPublish {
    pub scope: SyncScope,
    /// The message (a JSON object; the receivers parse it again).
    #[cfg_attr(test, ts(type = "unknown"))]
    pub message: Value,
}

/// The ghost of a tab dragged out of its window, as the UI asks for it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GhostRequest {
    /// The panel's name, shown in the ghost's title bar.
    pub title: String,
    /// The panel's icon: the `d` of an SVG path on a 24x24 grid.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub icon: Option<String>,
    /// A snapshot of the panel taken when the drag started: `data:image/png;base64,...`.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub snapshot: Option<String>,
    /// The window's colour scheme is dark (the ghost matches it).
    #[serde(default)]
    pub dark: bool,
    /// The size the popped-out window will get (logical px).
    pub width: f64,
    pub height: f64,
    /// Where the pointer holds the ghost, from its top-left corner (logical px).
    pub grab_x: f64,
    pub grab_y: f64,
}

/// The kind of a [`PanelError`].
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelErrorCode {
    /// The app did not set the panel layer up (a wiring bug).
    NotReady,
    /// The window is gone.
    Closed,
    /// The most pop-outs of a host are open already.
    TooMany,
    /// A bad panel id, title or bounds.
    Invalid,
    /// A forwarded message is larger than allowed.
    TooLarge,
    /// A window forwards messages faster than allowed; this one was dropped.
    RateLimited,
    /// Not for this window (a pop-out cannot host pop-outs, a pop-out of another host).
    NotAllowed,
    /// The platform refused (the message says how).
    Failed,
}

/// Why a panel command failed (the command's error).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelError {
    pub code: PanelErrorCode,
    /// For the user, as is.
    pub message: String,
}

impl PanelError {
    pub fn new(code: PanelErrorCode, message: impl Into<String>) -> Self {
        PanelError {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for PanelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PanelError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_are_tagged_and_camel_case() {
        let e = PanelsEvent::Returned {
            panel: "pn.code".into(),
            label: "p-main-1".into(),
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({ "kind": "returned", "panel": "pn.code", "label": "p-main-1" })
        );
        assert_eq!(
            serde_json::to_value(PanelsEvent::Changed).unwrap(),
            json!({ "kind": "changed" })
        );
        let m = PanelsEvent::Moved {
            panel: "a.b".into(),
            label: "p-main-1".into(),
            x: 1.5,
            y: 2.0,
        };
        assert_eq!(serde_json::to_value(m).unwrap()["kind"], "moved");
    }

    #[test]
    fn requests_refuse_unknown_fields_and_default_the_rest() {
        let r: PopOutRequest = serde_json::from_value(json!({ "panel": "pn.code" })).unwrap();
        assert!(r.bounds.is_none() && r.title.is_none() && !r.keep_on_top);
        assert!(
            serde_json::from_value::<PopOutRequest>(json!({ "panel": "pn.code", "evil": 1 }))
                .is_err()
        );
        let b: PopOutBounds =
            serde_json::from_value(json!({ "x": 1, "y": 2, "width": 3, "height": 4 })).unwrap();
        assert_eq!(b.scale, None);
        assert!(
            serde_json::from_value::<SyncPublish>(json!({ "scope": "everyone", "message": {} }))
                .is_err()
        );
        let p: SyncPublish =
            serde_json::from_value(json!({ "scope": "display", "message": { "t": "bye" } }))
                .unwrap();
        assert_eq!(p.scope, SyncScope::Display);
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let e = PanelError::new(PanelErrorCode::TooMany, "close one first");
        assert_eq!(e.to_string(), "close one first");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({ "code": "tooMany", "message": "close one first" })
        );
    }
}
