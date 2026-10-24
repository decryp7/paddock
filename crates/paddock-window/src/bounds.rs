//! Remembered window bounds, restored on the right monitor.
//!
//! A top-level window remembers its outer top-left corner (physical px), its inner size (logical px), the scale of the
//! monitor it was on, and whether it was maximized or full screen. On restore the shell picks the monitor the remembered
//! rectangle overlaps most (else the nearest one, never the primary display as such: `paddock_dialogs::placement`),
//! scales the size to that monitor, shrinks it to the work area and moves it inside, so a window saved on a display that
//! is gone, or on one whose resolution changed, opens fully visible. A new window cascades from the one it was opened
//! from, on that window's monitor.
//!
//! The file is versioned (`{ "version": 1, "windows": { "main": {...} } }`); missing, damaged or newer files fall back
//! to no memory. `main` is remembered as `main`, every other top-level window (`w-<n>`) as `window` (labels are per
//! run); pop-outs and dialogs keep their own (`paddock-dialogs`, `paddock-panels`).

use paddock_commands::Label;
use paddock_dialogs::placement::{Monitor, Placement, Rect, TITLE_BAR, monitor_of};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The version of the bounds file this crate writes.
pub const BOUNDS_VERSION: u32 = 1;
/// The key of the main window.
pub const MAIN_KEY: &str = "main";
/// The key of every other top-level window.
pub const WINDOW_KEY: &str = "window";
/// The most entries a bounds file keeps.
pub const MAX_ENTRIES: usize = 64;
/// The longest key.
pub const MAX_KEY: usize = 64;
/// How far (logical px) a new window opens right of and below the window it was opened from.
pub const CASCADE: f64 = 32.0;
/// Coordinates and sizes beyond this are nonsense and ignored.
const MAX_COORD: f64 = 1_000_000.0;

/// What a window remembers of its place.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedBounds {
    /// Outer top-left corner, physical px.
    pub x: f64,
    pub y: f64,
    /// Inner size, logical px of the monitor it was on.
    pub width: f64,
    pub height: f64,
    /// The scale factor of that monitor.
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

fn one() -> f64 {
    1.0
}

impl SavedBounds {
    /// `None` when a value is not a finite number in range or the size is not positive.
    pub fn sanitized(self) -> Option<SavedBounds> {
        let ok = |v: f64| v.is_finite() && v.abs() <= MAX_COORD;
        let fine = [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(ok)
            && self.width > 0.0
            && self.height > 0.0;
        fine.then(|| SavedBounds {
            scale: if self.scale.is_finite() && self.scale > 0.0 && self.scale <= 16.0 {
                self.scale
            } else {
                1.0
            },
            ..self
        })
    }

    /// The outer rectangle (physical px) this memory covers, from its inner size and scale.
    fn rect(&self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width * self.scale,
            height: (self.height + TITLE_BAR) * self.scale,
        }
    }
}

/// The key a top-level window's bounds are remembered under, or `None` for a window that keeps its own (a dialog, a
/// pop-out, a tray popover, a webview outside the scheme).
pub fn key_of(label: &str) -> Option<&'static str> {
    match Label::parse(label)? {
        Label::Main => Some(MAIN_KEY),
        Label::Window(_) => Some(WINDOW_KEY),
        _ => None,
    }
}

/// Whether `key` may name an entry (letters, digits, `-`, `_`, `.`, `:`; 1 to [`MAX_KEY`] characters).
pub fn valid_key(key: &str) -> bool {
    (1..=MAX_KEY).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

/// Why a bounds file was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadProblem {
    /// Not the file's format.
    Damaged(String),
    /// Written by a newer version of the shell.
    Newer(u32),
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(default)]
    windows: BTreeMap<String, serde_json::Value>,
}

/// The remembered bounds in a file's text. Bad entries are dropped one by one; a damaged or newer file is a problem
/// (the caller falls back to no memory and logs it).
pub fn parse(text: &str) -> Result<BTreeMap<String, SavedBounds>, LoadProblem> {
    let file: File = serde_json::from_str(text).map_err(|e| LoadProblem::Damaged(e.to_string()))?;
    if file.version > BOUNDS_VERSION {
        return Err(LoadProblem::Newer(file.version));
    }
    Ok(file
        .windows
        .into_iter()
        .filter(|(k, _)| valid_key(k))
        .filter_map(|(k, v)| {
            let b = serde_json::from_value::<SavedBounds>(v).ok()?.sanitized()?;
            Some((k, b))
        })
        .take(MAX_ENTRIES)
        .collect())
}

/// The file's text for `windows` (at most [`MAX_ENTRIES`], valid keys only).
pub fn to_text(windows: &BTreeMap<String, SavedBounds>) -> String {
    let file = File {
        version: BOUNDS_VERSION,
        windows: windows
            .iter()
            .filter(|(k, _)| valid_key(k))
            .take(MAX_ENTRIES)
            .filter_map(|(k, b)| Some((k.clone(), serde_json::to_value(b).ok()?)))
            .collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Where a window opens: its outer top-left corner (physical px), inner size (logical px of the monitor it opens on)
/// and that monitor's scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restored {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl Restored {
    /// As a dialog-layer placement, for `paddock_dialogs::placement::screen_position` (points on macOS, physical px
    /// elsewhere).
    pub fn placement(&self) -> Placement {
        Placement {
            x: self.x,
            y: self.y,
            width: self.width * self.scale,
            height: self.height * self.scale,
            scale: self.scale,
            remembered: true,
        }
    }
}

/// `r` moved (and shrunk when it is larger) so that it lies inside `work`.
pub fn inside(r: Rect, work: &Rect) -> Rect {
    let width = r.width.min(work.width);
    let height = r.height.min(work.height);
    Rect {
        x: r.x.clamp(work.x, work.x + work.width - width),
        y: r.y.clamp(work.y, work.y + work.height - height),
        width,
        height,
    }
}

/// Where a remembered window opens among `monitors` (work areas), at least `min` (logical px) when the monitor has
/// room. Without monitor information it opens as remembered.
pub fn restore(saved: &SavedBounds, monitors: &[Monitor], min: (f64, f64)) -> Restored {
    let as_saved = Restored {
        x: saved.x,
        y: saved.y,
        width: saved.width.max(min.0),
        height: saved.height.max(min.1),
        scale: saved.scale,
        maximized: saved.maximized,
        fullscreen: saved.fullscreen,
    };
    let Some(m) = monitor_of(&saved.rect(), monitors).and_then(|i| monitors.get(i)) else {
        return as_saved;
    };
    let s = m.scale();
    let outer = Rect {
        x: saved.x,
        y: saved.y,
        width: saved.width.max(min.0) * s,
        height: (saved.height.max(min.1) + TITLE_BAR) * s,
    };
    let fitted = inside(outer, &m.work);
    Restored {
        x: fitted.x,
        y: fitted.y,
        width: fitted.width / s,
        height: fitted.height / s - TITLE_BAR,
        scale: s,
        ..as_saved
    }
}

/// Where a new window of inner size `size` (logical px) opens: [`CASCADE`] right of and below `parent` (its outer
/// rectangle, physical px), on the parent's monitor and inside its work area. `None` without a parent or monitors.
pub fn cascade(parent: Option<Rect>, size: (f64, f64), monitors: &[Monitor]) -> Option<Restored> {
    let parent = parent.filter(Rect::is_usable)?;
    let m = monitors.get(monitor_of(&parent, monitors)?)?;
    let s = m.scale();
    let fitted = inside(
        Rect {
            x: parent.x + CASCADE * s,
            y: parent.y + CASCADE * s,
            width: size.0 * s,
            height: (size.1 + TITLE_BAR) * s,
        },
        &m.work,
    );
    Some(Restored {
        x: fitted.x,
        y: fitted.y,
        width: fitted.width / s,
        height: fitted.height / s - TITLE_BAR,
        scale: s,
        maximized: false,
        fullscreen: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn monitor(x: f64, y: f64, width: f64, height: f64, scale: f64) -> Monitor {
        Monitor {
            work: rect(x, y, width, height),
            scale,
        }
    }

    fn saved(x: f64, y: f64, width: f64, height: f64, scale: f64) -> SavedBounds {
        SavedBounds {
            x,
            y,
            width,
            height,
            scale,
            maximized: false,
            fullscreen: false,
        }
    }

    /// A Retina laptop (2x, 1512x945 points) and a 1x display to its right.
    fn desk() -> Vec<Monitor> {
        vec![
            monitor(0.0, 50.0, 3024.0, 1840.0, 2.0),
            monitor(3024.0, 0.0, 2560.0, 1400.0, 1.0),
        ]
    }

    #[test]
    fn a_window_reopens_where_it_was_on_its_own_monitor() {
        let r = restore(
            &saved(3200.0, 100.0, 1200.0, 800.0, 1.0),
            &desk(),
            (400.0, 300.0),
        );
        assert_eq!(
            (r.x, r.y, r.width, r.height, r.scale),
            (3200.0, 100.0, 1200.0, 800.0, 1.0)
        );
        let r = restore(
            &saved(100.0, 200.0, 1000.0, 700.0, 2.0),
            &desk(),
            (400.0, 300.0),
        );
        assert_eq!(
            (r.x, r.y, r.width, r.height, r.scale),
            (100.0, 200.0, 1000.0, 700.0, 2.0)
        );
    }

    #[test]
    fn a_window_from_a_display_that_is_gone_opens_inside_the_nearest_one() {
        // Saved on a third display far right; the nearest is the 1x one.
        let r = restore(
            &saved(9000.0, 300.0, 1200.0, 800.0, 1.0),
            &desk(),
            (400.0, 300.0),
        );
        assert_eq!(r.x, 3024.0 + 2560.0 - 1200.0);
        assert_eq!((r.y, r.width, r.height), (300.0, 1200.0, 800.0));
        // Above the top of every display (negative y): moved down into the work area.
        let r = restore(
            &saved(200.0, -900.0, 800.0, 600.0, 2.0),
            &desk(),
            (400.0, 300.0),
        );
        assert_eq!(r.y, 50.0);
        assert_eq!(r.scale, 2.0);
    }

    #[test]
    fn a_window_larger_than_its_monitor_shrinks_to_fit_and_the_minimum_holds_where_it_can() {
        let small = [monitor(0.0, 0.0, 1280.0, 800.0, 1.0)];
        let r = restore(
            &saved(0.0, 0.0, 3000.0, 2000.0, 1.0),
            &small,
            (400.0, 300.0),
        );
        assert_eq!((r.width, r.height), (1280.0, 800.0 - TITLE_BAR));
        let r = restore(
            &saved(10.0, 10.0, 100.0, 100.0, 1.0),
            &small,
            (400.0, 300.0),
        );
        assert_eq!((r.width, r.height), (400.0, 300.0));
    }

    #[test]
    fn a_window_saved_at_2x_moved_to_a_1x_monitor_keeps_its_logical_size() {
        // Saved on the Retina laptop, the window's top-left now overlaps the 1x display (the laptop moved).
        let one = [monitor(0.0, 0.0, 2560.0, 1400.0, 1.0)];
        let r = restore(
            &saved(100.0, 100.0, 1000.0, 600.0, 2.0),
            &one,
            (400.0, 300.0),
        );
        assert_eq!((r.width, r.height, r.scale), (1000.0, 600.0, 1.0));
    }

    #[test]
    fn without_monitors_the_window_opens_as_remembered() {
        let mut s = saved(5.0, 6.0, 300.0, 200.0, 1.5);
        s.maximized = true;
        let r = restore(&s, &[], (400.0, 300.0));
        assert_eq!(
            (r.x, r.y, r.width, r.height, r.scale),
            (5.0, 6.0, 400.0, 300.0, 1.5)
        );
        assert!(r.maximized && !r.fullscreen);
        let p = r.placement();
        assert_eq!((p.width, p.height, p.scale), (600.0, 450.0, 1.5));
    }

    #[test]
    fn a_new_window_cascades_from_its_parent_on_the_parents_monitor() {
        let parent = rect(3100.0, 100.0, 1400.0, 900.0);
        let r = cascade(Some(parent), (1440.0, 900.0), &desk()).unwrap();
        assert_eq!((r.x, r.y, r.scale), (3132.0, 132.0, 1.0));
        let edge = cascade(
            Some(rect(4400.0, 100.0, 1100.0, 900.0)),
            (1440.0, 900.0),
            &desk(),
        )
        .unwrap();
        assert_eq!(
            edge.x,
            3024.0 + 2560.0 - 1440.0,
            "kept inside the work area"
        );
        let retina = rect(100.0, 100.0, 2000.0, 1400.0);
        let r = cascade(Some(retina), (800.0, 600.0), &desk()).unwrap();
        assert_eq!((r.x, r.y, r.scale), (164.0, 164.0, 2.0));
        assert_eq!(cascade(None, (800.0, 600.0), &desk()), None);
        assert_eq!(cascade(Some(parent), (800.0, 600.0), &[]), None);
        assert_eq!(
            cascade(Some(rect(f64::NAN, 0.0, 1.0, 1.0)), (800.0, 600.0), &desk()),
            None
        );
    }

    #[test]
    fn keys_follow_the_label_scheme() {
        assert_eq!(key_of("main"), Some(MAIN_KEY));
        assert_eq!(key_of("w-4"), Some(WINDOW_KEY));
        for none in ["d-main-1", "p-main-1", "tray", "browser"] {
            assert_eq!(key_of(none), None, "{none}");
        }
        assert!(valid_key("panel:logs.1") && !valid_key("") && !valid_key("a b"));
        assert!(!valid_key(&"k".repeat(MAX_KEY + 1)));
    }

    #[test]
    fn the_file_round_trips_and_bad_entries_or_files_fall_back() {
        let mut map = BTreeMap::new();
        map.insert("main".to_string(), saved(1.0, 2.0, 300.0, 200.0, 2.0));
        map.insert("bad key".to_string(), saved(1.0, 2.0, 300.0, 200.0, 2.0));
        let text = to_text(&map);
        let back = parse(&text).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back["main"], map["main"]);
        let mixed = r#"{ "version": 1, "windows": {
            "main": { "x": 1, "y": 2, "width": 300, "height": 200 },
            "window": { "x": 1, "y": 2, "width": -3, "height": 200 },
            "huge": { "x": 1e300, "y": 2, "width": 3, "height": 200 },
            "odd": { "x": 1, "y": 2, "width": 3, "height": 2, "scale": -1 },
            "junk": 7,
            "a b": { "x": 1, "y": 2, "width": 3, "height": 200 }
        } }"#;
        let m = parse(mixed).unwrap();
        assert_eq!(m.keys().collect::<Vec<_>>(), ["main", "odd"]);
        assert_eq!((m["main"].scale, m["odd"].scale), (1.0, 1.0));
        assert!(matches!(parse("{"), Err(LoadProblem::Damaged(_))));
        assert_eq!(
            parse(r#"{ "version": 2, "windows": {} }"#),
            Err(LoadProblem::Newer(2))
        );
        assert_eq!(parse(r#"{ "version": 1 }"#).unwrap().len(), 0);
        let many: BTreeMap<String, SavedBounds> = (0..100)
            .map(|i| (format!("k{i:03}"), saved(0.0, 0.0, 1.0, 1.0, 1.0)))
            .collect();
        assert_eq!(parse(&to_text(&many)).unwrap().len(), MAX_ENTRIES);
    }

    #[test]
    fn inside_moves_and_shrinks() {
        let work = rect(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            inside(rect(90.0, -5.0, 20.0, 20.0), &work),
            rect(80.0, 0.0, 20.0, 20.0)
        );
        assert_eq!(
            inside(rect(0.0, 0.0, 200.0, 50.0), &work),
            rect(0.0, 0.0, 100.0, 50.0)
        );
    }
}
