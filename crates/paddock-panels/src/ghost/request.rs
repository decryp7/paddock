//! What the UI sends to start a drag ghost, parsed once into typed, bounded values.

use super::geometry::GhostSize;
use crate::place::{MAX_SIDE as MAX_SIZE, MIN_SIZE};
use crate::wire::GhostRequest;
use paddock_dialogs::window::clean_title;
use serde::Serialize;

const MIN_WIDTH: f64 = MIN_SIZE.0;
const MIN_HEIGHT: f64 = MIN_SIZE.1;

/// The largest snapshot accepted (a PNG data URL); the UI scales its snapshot down to stay below.
pub const MAX_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
/// The largest icon (an SVG path on the 24x24 icon grid).
pub const MAX_ICON_BYTES: usize = 1024;
const PNG_PREFIX: &str = "data:image/png;base64,";

/// What the ghost page shows (sent to it as JSON).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GhostContent {
    pub title: String,
    pub icon: Option<String>,
    pub snapshot: Option<String>,
    pub dark: bool,
}

/// A parsed request: what to show and how large.
#[derive(Debug, Clone, PartialEq)]
pub struct Ghost {
    pub content: GhostContent,
    pub size: GhostSize,
}

fn finite(v: f64, what: &str) -> Result<f64, String> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("the ghost's {what} is not a number"))
    }
}

/// An SVG path's `d`: commands, numbers, separators.
fn valid_icon(d: &str) -> bool {
    d.len() <= MAX_ICON_BYTES
        && d.bytes()
            .all(|b| b.is_ascii_digit() || b"MmLlHhVvCcSsQqTtAaZz .,-+e".contains(&b))
}

/// A PNG data URL in base64, within the size limit.
fn valid_snapshot(s: &str) -> bool {
    s.len() <= MAX_SNAPSHOT_BYTES
        && s.strip_prefix(PNG_PREFIX).is_some_and(|b64| {
            !b64.is_empty()
                && b64
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        })
}

impl GhostRequest {
    /// The ghost to show: sizes clamped like a pop-out window's, the title cleaned; a malformed icon or snapshot is
    /// refused (the UI only sends what it made itself).
    pub fn parse(self) -> Result<Ghost, String> {
        let width = finite(self.width, "width")?.clamp(MIN_WIDTH, MAX_SIZE);
        let height = finite(self.height, "height")?.clamp(MIN_HEIGHT, MAX_SIZE);
        let grab_x = finite(self.grab_x, "grab point")?.clamp(0.0, width);
        let grab_y = finite(self.grab_y, "grab point")?.clamp(0.0, height);
        let icon = self.icon.filter(|d| !d.is_empty());
        if icon.as_deref().is_some_and(|d| !valid_icon(d)) {
            return Err("the ghost's icon is not an SVG path".into());
        }
        let snapshot = self.snapshot.filter(|s| !s.is_empty());
        if snapshot.as_deref().is_some_and(|s| !valid_snapshot(s)) {
            return Err(format!(
                "the ghost's snapshot is not a PNG data URL of at most {} MiB",
                MAX_SNAPSHOT_BYTES / (1024 * 1024)
            ));
        }
        Ok(Ghost {
            content: GhostContent {
                title: clean_title(&self.title),
                icon,
                snapshot,
                dark: self.dark.unwrap_or(false),
            },
            size: GhostSize {
                width,
                height,
                grab_x,
                grab_y,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> GhostRequest {
        serde_json::from_value(serde_json::json!({
            "title": "Inspector",
            "icon": "M6 9l6 6l6-6",
            "snapshot": "data:image/png;base64,iVBORw0KGgo=",
            "width": 560.0,
            "height": 420.0,
            "grabX": 120.0,
            "grabY": 12.0,
            "dark": true
        }))
        .unwrap()
    }

    #[test]
    fn a_request_parses_into_the_ghost() {
        let g = request().parse().unwrap();
        assert_eq!(g.content.title, "Inspector");
        assert_eq!(g.content.icon.as_deref(), Some("M6 9l6 6l6-6"));
        assert_eq!(
            g.size,
            GhostSize {
                width: 560.0,
                height: 420.0,
                grab_x: 120.0,
                grab_y: 12.0
            }
        );
        let json = serde_json::to_value(&g.content).unwrap();
        assert_eq!(json["snapshot"], "data:image/png;base64,iVBORw0KGgo=");
        assert_eq!(json["dark"], true);
    }

    #[test]
    fn sizes_are_clamped_like_a_pop_out_s_and_the_grab_point_stays_on_the_ghost() {
        let g = GhostRequest {
            width: 10.0,
            height: 1e9,
            grab_x: 900.0,
            grab_y: -5.0,
            ..request()
        }
        .parse()
        .unwrap();
        assert_eq!((g.size.width, g.size.height), (MIN_WIDTH, MAX_SIZE));
        assert_eq!((g.size.grab_x, g.size.grab_y), (MIN_WIDTH, 0.0));
    }

    #[test]
    fn nonsense_is_refused() {
        for bad in [
            GhostRequest {
                width: f64::NAN,
                ..request()
            },
            GhostRequest {
                grab_y: f64::INFINITY,
                ..request()
            },
            GhostRequest {
                icon: Some("<script>".into()),
                ..request()
            },
            GhostRequest {
                icon: Some("M".repeat(MAX_ICON_BYTES + 1)),
                ..request()
            },
            GhostRequest {
                snapshot: Some("data:image/svg+xml;base64,PHN2Zz4=".into()),
                ..request()
            },
            GhostRequest {
                snapshot: Some("data:image/png;base64,\"onload".into()),
                ..request()
            },
            GhostRequest {
                snapshot: Some(format!("{PNG_PREFIX}{}", "A".repeat(MAX_SNAPSHOT_BYTES))),
                ..request()
            },
        ] {
            assert!(bad.clone().parse().is_err(), "{bad:?}");
        }
        assert!(
            serde_json::from_value::<GhostRequest>(serde_json::json!({
                "title": "x", "width": 1, "height": 1, "grabX": 0, "grabY": 0, "html": "<b>"
            }))
            .is_err()
        );
    }

    #[test]
    fn empty_extras_are_none_and_the_title_is_cleaned() {
        let g = GhostRequest {
            title: "  Time\u{7}line  ".into(),
            icon: Some(String::new()),
            snapshot: Some(String::new()),
            ..request()
        }
        .parse()
        .unwrap();
        assert_eq!(g.content.title, "Timeline");
        assert_eq!((g.content.icon, g.content.snapshot), (None, None));
    }
}
