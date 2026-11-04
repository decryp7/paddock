//! The real "shown" test of a macOS 26 menu-bar item: ControlCenter draws every app's status items, and an item is on screen
//! when an on-screen window owned by Control Center carries the item's autosave name and has a size. A status item that
//! exists in the app but that ControlCenter parks off screen has no such window. Pure: the windows are read elsewhere
//! (`tray_platform`), so the judgement is testable on any OS.

/// One on-screen window as the window server lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// The owning process's name.
    pub owner: String,
    /// The window's name (the autosave name for a status item); `None` when the window server does not tell it.
    pub name: Option<String>,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

/// The owner name of the window server's status-item windows.
pub const CONTROL_CENTER: &str = "Control Center";

/// What the list says about one autosave name.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// An on-screen Control Center window carries the name.
    Shown,
    /// Control Center's windows are named and none carries it.
    NotShown {
        /// How many on-screen Control Center windows there are.
        control_center_windows: usize,
    },
    /// No Control Center window has a readable name, so the list cannot say (window names need a permission this app may lack).
    NamesUnreadable,
}

/// Judges the item `autosave_name` against the on-screen `windows`.
pub fn judge(windows: &[Window], autosave_name: &str) -> Verdict {
    let cc: Vec<&Window> = windows.iter().filter(|w| w.owner == CONTROL_CENTER).collect();
    if cc.iter().any(|w| w.name.as_deref() == Some(autosave_name) && w.width > 0.0 && w.height > 0.0) {
        return Verdict::Shown;
    }
    if cc.iter().all(|w| w.name.is_none()) && !cc.is_empty() {
        return Verdict::NamesUnreadable;
    }
    Verdict::NotShown {
        control_center_windows: cc.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(owner: &str, name: Option<&str>, w: f64) -> Window {
        Window {
            owner: owner.into(),
            name: name.map(str::to_string),
            width: w,
            height: 24.0,
        }
    }

    #[test]
    fn a_control_center_window_named_after_the_item_is_shown() {
        let list = [win("Control Center", Some("net.decryptology.dyno.menubar.cpu-0"), 40.0), win("Dock", None, 9.0)];
        assert_eq!(judge(&list, "net.decryptology.dyno.menubar.cpu-0"), Verdict::Shown);
    }

    #[test]
    fn an_item_without_such_a_window_is_not_shown_and_the_count_is_told() {
        let list = [win("Control Center", Some("com.apple.menuextra.clock"), 60.0)];
        assert_eq!(
            judge(&list, "net.decryptology.dyno.menubar.cpu-0"),
            Verdict::NotShown {
                control_center_windows: 1
            }
        );
    }

    #[test]
    fn a_window_of_another_owner_or_without_size_does_not_count() {
        let name = "net.decryptology.dyno.menubar.cpu-0";
        assert_ne!(judge(&[win("Dyno", Some(name), 40.0)], name), Verdict::Shown);
        assert_ne!(judge(&[win("Control Center", Some(name), 0.0)], name), Verdict::Shown);
    }

    #[test]
    fn unnamed_control_center_windows_cannot_say() {
        assert_eq!(judge(&[win("Control Center", None, 40.0)], "x"), Verdict::NamesUnreadable);
        assert_eq!(judge(&[], "x"), Verdict::NotShown { control_center_windows: 0 });
    }
}
