//! The real "shown" test of a macOS 26 menu-bar item: ControlCenter draws every app's status items, and an item is on screen
//! when an on-screen window owned by the Control Center process carries the item's autosave name and has a size. A status item
//! that exists in the app but that ControlCenter parks off screen has no such window. The owner is matched by process id (the
//! owner's name is localized). Reading a window's name needs the system's window-name access; when the names are not readable
//! ([`Verdict::NamesUnreadable`]) the test cannot say and the caller falls back to the status item's own window (visible, not
//! occluded, sized). Pure: the windows are read elsewhere (`tray_platform`), so the judgement is testable on any OS.

/// One on-screen window as the window server lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// The owning process's id.
    pub owner_pid: i32,
    /// The window's name (the autosave name for a status item); `None` when the window server does not tell it.
    pub name: Option<String>,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

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
pub fn judge(windows: &[Window], control_center_pid: i32, autosave_name: &str) -> Verdict {
    let cc: Vec<&Window> = windows.iter().filter(|w| w.owner_pid == control_center_pid).collect();
    if cc
        .iter()
        .any(|w| w.name.as_deref() == Some(autosave_name) && w.width > 0.0 && w.height > 0.0)
    {
        return Verdict::Shown;
    }
    if cc.iter().all(|w| w.name.is_none()) && !cc.is_empty() {
        return Verdict::NamesUnreadable;
    }
    Verdict::NotShown {
        control_center_windows: cc.len(),
    }
}

/// How many windows are listed and how many different owners they have, for the log (no process names).
pub fn owners_summary(windows: &[Window]) -> String {
    let owners: std::collections::BTreeSet<i32> = windows.iter().map(|w| w.owner_pid).collect();
    format!("{} window(s) of {} process(es)", windows.len(), owners.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CC: i32 = 449;
    const NAME: &str = "net.decryptology.dyno.menubar.cpu-0";

    fn win(pid: i32, name: Option<&str>, w: f64) -> Window {
        Window {
            owner_pid: pid,
            name: name.map(str::to_string),
            width: w,
            height: 24.0,
        }
    }

    #[test]
    fn a_control_center_window_named_after_the_item_is_shown_whatever_language_the_owner_name_is() {
        // Only the pid identifies the owner: there is no owner name in the model at all.
        let list = [win(CC, Some(NAME), 40.0), win(7, None, 9.0)];
        assert_eq!(judge(&list, CC, NAME), Verdict::Shown);
    }

    #[test]
    fn an_item_without_such_a_window_is_not_shown_and_the_count_is_told() {
        let list = [win(CC, Some("com.apple.menuextra.clock"), 60.0)];
        assert_eq!(judge(&list, CC, NAME), Verdict::NotShown { control_center_windows: 1 });
    }

    #[test]
    fn a_window_of_another_process_or_without_size_does_not_count() {
        assert_ne!(judge(&[win(7, Some(NAME), 40.0)], CC, NAME), Verdict::Shown);
        assert_ne!(judge(&[win(CC, Some(NAME), 0.0)], CC, NAME), Verdict::Shown);
    }

    #[test]
    fn the_listing_is_counted_without_names() {
        let list = [win(1, None, 1.0), win(CC, None, 1.0), win(1, None, 1.0)];
        assert_eq!(owners_summary(&list), "3 window(s) of 2 process(es)");
        assert_eq!(owners_summary(&[]), "0 window(s) of 0 process(es)");
    }

    #[test]
    fn unnamed_control_center_windows_cannot_say() {
        assert_eq!(judge(&[win(CC, None, 40.0)], CC, "x"), Verdict::NamesUnreadable);
        assert_eq!(judge(&[], CC, "x"), Verdict::NotShown { control_center_windows: 0 });
    }
}
