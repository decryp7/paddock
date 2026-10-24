//! The tray icon's model: what its menu lists, its tooltip and icon, and what a click does. The app builds a
//! [`TrayView`] from its own state (windows, recent items, background work: Debrief's and Pitwall's menus are
//! contributions, not shell code); the adapter draws it and redraws only when it changed (rebuilding a menu closes it
//! when it is open). Without a tray (a Linux desktop without an app indicator, a session without a taskbar) the app
//! degrades: closing the last window quits.
//!
//! Ported from Debrief's `tray.rs` and Pitwall's `tray.rs`/`tray_model.rs`.

use std::collections::HashSet;

/// The longest menu text, in characters (longer ones end with an ellipsis).
pub const MAX_TEXT: usize = 80;
/// The longest tooltip, in characters.
pub const MAX_TOOLTIP: usize = 120;
/// The most entries a menu (with its submenus) has.
pub const MAX_ENTRIES: usize = 64;
/// The longest item id.
pub const MAX_ID: usize = 64;

/// The shell's own menu ids. `pd.show` shows (or rebuilds) the main window, `pd.new-window` opens a new one, `pd.quit`
/// quits; every other id is the app's.
pub const SHOW_ID: &str = "pd.show";
pub const NEW_WINDOW_ID: &str = "pd.new-window";
pub const QUIT_ID: &str = "pd.quit";
/// `pd.window:<label>` shows that window (a window list in the menu).
pub const WINDOW_ID_PREFIX: &str = "pd.window:";

/// One entry of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEntry {
    Item {
        id: String,
        text: String,
        enabled: bool,
    },
    Separator,
    /// One level deep: entries of a submenu are items and separators.
    Submenu {
        text: String,
        items: Vec<TrayEntry>,
    },
}

/// One line of menu text: no control characters, at most [`MAX_TEXT`] characters.
pub fn short(text: &str) -> String {
    clip(text, MAX_TEXT)
}

fn clip(text: &str, max: usize) -> String {
    let t: String = text.chars().filter(|c| !c.is_control()).collect();
    if t.chars().count() > max {
        format!("{}\u{2026}", t.chars().take(max - 1).collect::<String>())
    } else {
        t
    }
}

/// An enabled item.
pub fn item(id: impl Into<String>, text: &str) -> TrayEntry {
    TrayEntry::Item {
        id: id.into(),
        text: short(text),
        enabled: true,
    }
}

/// A line of text that cannot be chosen (a status, "Nothing running").
pub fn disabled(text: &str) -> TrayEntry {
    TrayEntry::Item {
        id: String::new(),
        text: short(text),
        enabled: false,
    }
}

/// A submenu.
pub fn submenu(text: &str, items: Vec<TrayEntry>) -> TrayEntry {
    TrayEntry::Submenu {
        text: short(text),
        items,
    }
}

/// What the tray shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayView {
    pub tooltip: String,
    pub menu: Vec<TrayEntry>,
    /// The app's name for the icon to show (its state: `idle`, `busy`, ...); the app maps it to image bytes.
    pub icon: String,
}

impl TrayView {
    pub fn new(tooltip: &str, menu: Vec<TrayEntry>, icon: &str) -> TrayView {
        TrayView {
            tooltip: clip(tooltip, MAX_TOOLTIP),
            menu,
            icon: icon.to_string(),
        }
    }
}

/// Why a menu was refused (a bug in the app's model, reported once instead of drawing half a menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuProblem {
    TooMany,
    /// An enabled item without an id, or an id that is not printable ASCII of 1 to [`MAX_ID`] characters.
    BadId(String),
    DuplicateId(String),
    /// A submenu inside a submenu.
    TooDeep,
}

impl std::fmt::Display for MenuProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MenuProblem::TooMany => write!(f, "the tray menu has more than {MAX_ENTRIES} entries"),
            MenuProblem::BadId(id) => write!(f, "the tray menu item id {id:?} is not valid"),
            MenuProblem::DuplicateId(id) => write!(f, "the tray menu lists {id:?} twice"),
            MenuProblem::TooDeep => f.write_str("the tray menu nests a submenu in a submenu"),
        }
    }
}

fn valid_id(id: &str) -> bool {
    (1..=MAX_ID).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_graphic())
}

/// Checks a menu: at most [`MAX_ENTRIES`] entries, unique valid ids on enabled items, submenus one level deep.
pub fn validate(menu: &[TrayEntry]) -> Result<(), MenuProblem> {
    fn walk<'a>(
        entries: &'a [TrayEntry],
        nested: bool,
        seen: &mut HashSet<&'a str>,
        count: &mut usize,
    ) -> Result<(), MenuProblem> {
        for e in entries {
            *count += 1;
            if *count > MAX_ENTRIES {
                return Err(MenuProblem::TooMany);
            }
            match e {
                TrayEntry::Item { id, enabled, .. } if *enabled || !id.is_empty() => {
                    if !valid_id(id) {
                        return Err(MenuProblem::BadId(id.clone()));
                    }
                    if !seen.insert(id) {
                        return Err(MenuProblem::DuplicateId(id.clone()));
                    }
                }
                TrayEntry::Submenu { .. } if nested => return Err(MenuProblem::TooDeep),
                TrayEntry::Submenu { items, .. } => walk(items, true, seen, count)?,
                _ => {}
            }
        }
        Ok(())
    }
    walk(menu, false, &mut HashSet::new(), &mut 0)
}

/// What a menu choice asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    /// Show the main window (rebuilt when it was destroyed).
    Show,
    NewWindow,
    /// Show this window.
    ShowWindow(String),
    Quit,
    /// The app's own item.
    App(String),
}

/// The action of a menu item id.
pub fn action_of(id: &str) -> TrayAction {
    match id {
        SHOW_ID => TrayAction::Show,
        NEW_WINDOW_ID => TrayAction::NewWindow,
        QUIT_ID => TrayAction::Quit,
        _ => match id.strip_prefix(WINDOW_ID_PREFIX) {
            Some(label) if !label.is_empty() => TrayAction::ShowWindow(label.to_string()),
            _ => TrayAction::App(id.to_string()),
        },
    }
}

/// What a left click on the tray icon does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Toggle the tray popover.
    Popover,
    /// Open the menu (the platform does it).
    Menu,
    /// Show the main window.
    Show,
}

/// A left click: the popover when the app has one; otherwise the menu on macOS (a menu-bar item) and the window on
/// Windows and Linux (a taskbar icon). The right click always opens the menu.
pub fn left_click(has_popover: bool, os: &str) -> Click {
    match (has_popover, os) {
        (true, _) => Click::Popover,
        (false, "macos") => Click::Menu,
        (false, _) => Click::Show,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_one_bounded_line() {
        assert_eq!(short("a\nb"), "ab");
        let long = short(&"x".repeat(200));
        assert_eq!(long.chars().count(), MAX_TEXT);
        assert!(long.ends_with('\u{2026}'));
        let tip = TrayView::new(&"t".repeat(500), vec![], "idle");
        assert_eq!(tip.tooltip.chars().count(), MAX_TOOLTIP);
        assert_eq!(tip.icon, "idle");
    }

    #[test]
    fn menus_are_validated() {
        let ok = vec![
            item(SHOW_ID, "Show"),
            TrayEntry::Separator,
            submenu("Recent", vec![item("recent:0", "a"), disabled("none")]),
            disabled("Idle"),
            item(QUIT_ID, "Quit"),
        ];
        assert_eq!(validate(&ok), Ok(()));
        assert_eq!(
            validate(&[item("a", "1"), item("a", "2")]),
            Err(MenuProblem::DuplicateId("a".into()))
        );
        assert_eq!(
            validate(&[item("", "x")]),
            Err(MenuProblem::BadId(String::new()))
        );
        assert_eq!(
            validate(&[item("a b", "x")]),
            Err(MenuProblem::BadId("a b".into()))
        );
        assert_eq!(
            validate(&[submenu("a", vec![submenu("b", vec![])])]),
            Err(MenuProblem::TooDeep)
        );
        let many: Vec<TrayEntry> = (0..=MAX_ENTRIES).map(|_| TrayEntry::Separator).collect();
        assert_eq!(validate(&many), Err(MenuProblem::TooMany));
        for p in [
            MenuProblem::TooMany,
            MenuProblem::BadId("x".into()),
            MenuProblem::DuplicateId("x".into()),
            MenuProblem::TooDeep,
        ] {
            assert!(p.to_string().starts_with("the tray menu"), "{p}");
        }
    }

    #[test]
    fn ids_map_to_actions() {
        assert_eq!(action_of(SHOW_ID), TrayAction::Show);
        assert_eq!(action_of(NEW_WINDOW_ID), TrayAction::NewWindow);
        assert_eq!(action_of(QUIT_ID), TrayAction::Quit);
        assert_eq!(
            action_of("pd.window:w-2"),
            TrayAction::ShowWindow("w-2".into())
        );
        assert_eq!(
            action_of("pd.window:"),
            TrayAction::App("pd.window:".into())
        );
        assert_eq!(action_of("stop:x"), TrayAction::App("stop:x".into()));
    }

    #[test]
    fn a_left_click_opens_the_popover_the_menu_or_the_window() {
        assert_eq!(left_click(true, "windows"), Click::Popover);
        assert_eq!(left_click(false, "macos"), Click::Menu);
        assert_eq!(left_click(false, "linux"), Click::Show);
    }
}
