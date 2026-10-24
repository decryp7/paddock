//! The menu model the UI sends (`menu_set`), as wire DTOs, and its validated form.
//!
//! The UI owns the menus: one model built from the contributions (`@decryp7/paddock-ui/shell/menu`), also behind the
//! in-app menu bar on Windows and Linux. [`parse`] turns the untrusted wire model into an [`AppMenu`] once, at the edge:
//! sizes, ids and labels are checked and key bindings become native accelerators.

use crate::accelerator::accelerator;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// At most this many top-level menus.
pub const MAX_MENUS: usize = 16;
/// At most this many entries over all menus and submenus.
pub const MAX_ENTRIES: usize = 1000;
/// Submenus nest at most this deep (a top-level menu's items are level 1).
pub const MAX_DEPTH: usize = 4;
/// The longest item or menu id.
pub const MAX_ID: usize = 128;
/// The longest label, in characters.
pub const MAX_LABEL: usize = 120;

/// The id of the shell's Quit command: on macOS last in the app menu (the system's own Quit would skip the app's
/// unsaved-work prompt), elsewhere where the app placed it.
pub const QUIT_COMMAND: &str = "pd.app.quit";

// ---- wire DTOs (generated into ui/projects/paddock-ui/shell/menu/generated/menu.ts) ----

/// The menus of the focused window, as the UI sends them with `menu_set`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeMenuModel {
    pub menus: Vec<NativeTopMenu>,
}

/// A top-level menu. The ids `app`, `edit`, `window` and `help` get the standard macOS items and roles.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTopMenu {
    pub id: String,
    pub label: String,
    pub items: Vec<NativeMenuEntry>,
}

/// One entry of a menu.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NativeMenuEntry {
    /// An item the UI runs; its id is the command id the click is routed back as.
    #[serde(rename_all = "camelCase")]
    Item {
        id: String,
        label: String,
        /// The key binding in the shell's syntax (`Mod+Shift+O`); the shell turns it into an accelerator.
        keybinding: Option<String>,
        /// `null` for a plain item; true or false for a toggle with a check mark.
        checked: Option<bool>,
        enabled: bool,
    },
    Separator,
    /// A disabled caption (a group label).
    Header {
        label: String,
    },
    Submenu {
        label: String,
        items: Vec<NativeMenuEntry>,
    },
}

// ---- validated model ----

/// A menu item the UI runs: plain, or a toggle with a check mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
    /// A native accelerator (`CmdOrCtrl+Shift+O`); `None` when there is no (usable) key binding.
    pub accelerator: Option<String>,
    pub checked: Option<bool>,
    pub enabled: bool,
}

/// An entry of a validated menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Item(Item),
    Separator,
    Header(String),
    Submenu(String, Vec<Entry>),
}

/// The role of a top-level menu: the ones macOS gives standard items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRole {
    /// The app menu (About, Settings...), merged with Services and Hide; [`QUIT_COMMAND`] goes last.
    App,
    /// Merged with Undo, Redo, Cut, Copy, Paste, Select All.
    Edit,
    /// Merged with Minimize and Zoom; the window list of macOS.
    Window,
    /// The help menu of macOS (with its search field).
    Help,
    Other,
}

/// A validated top-level menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopMenu {
    pub role: MenuRole,
    pub label: String,
    pub items: Vec<Entry>,
}

/// The validated menu model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppMenu {
    pub menus: Vec<TopMenu>,
}

/// Why a menu model was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuError {
    TooManyMenus(usize),
    TooManyEntries,
    TooDeep,
    BadId(String),
    DuplicateId(String),
    BadLabel(String),
}

impl fmt::Display for MenuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MenuError::TooManyMenus(n) => write!(
                f,
                "the menu bar has {n} menus; at most {MAX_MENUS} are allowed"
            ),
            MenuError::TooManyEntries => {
                write!(f, "the menus have more than {MAX_ENTRIES} entries")
            }
            MenuError::TooDeep => {
                write!(f, "submenus are nested more than {MAX_DEPTH} levels deep")
            }
            MenuError::BadId(id) => write!(f, "invalid menu item id {:?}", truncate(id, 40)),
            MenuError::DuplicateId(id) => write!(f, "the menu item id {id:?} is used twice"),
            MenuError::BadLabel(l) => write!(f, "invalid menu label {:?}", truncate(l, 40)),
        }
    }
}

impl std::error::Error for MenuError {}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// An id the menus accept: 1 to [`MAX_ID`] bytes of ASCII letters, digits and `._:/-`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
}

fn label(text: String) -> Result<String, MenuError> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > MAX_LABEL
        || trimmed.chars().any(char::is_control)
    {
        return Err(MenuError::BadLabel(text));
    }
    Ok(trimmed.to_string())
}

/// Validates the UI's menu model (sizes, ids, labels) and converts key bindings to accelerators. Pure.
pub fn parse(dto: NativeMenuModel) -> Result<AppMenu, MenuError> {
    if dto.menus.len() > MAX_MENUS {
        return Err(MenuError::TooManyMenus(dto.menus.len()));
    }
    let mut seen = HashSet::new();
    let mut count = 0usize;
    let mut menus = Vec::with_capacity(dto.menus.len());
    for m in dto.menus {
        let role = match m.id.as_str() {
            "app" => MenuRole::App,
            "edit" => MenuRole::Edit,
            "window" => MenuRole::Window,
            "help" => MenuRole::Help,
            id if valid_id(id) => MenuRole::Other,
            id => return Err(MenuError::BadId(id.to_string())),
        };
        menus.push(TopMenu {
            role,
            label: label(m.label)?,
            items: entries(m.items, 1, &mut seen, &mut count)?,
        });
    }
    Ok(AppMenu { menus })
}

fn entries(
    dtos: Vec<NativeMenuEntry>,
    depth: usize,
    seen: &mut HashSet<String>,
    count: &mut usize,
) -> Result<Vec<Entry>, MenuError> {
    if depth > MAX_DEPTH {
        return Err(MenuError::TooDeep);
    }
    let mut out = Vec::with_capacity(dtos.len());
    for dto in dtos {
        *count += 1;
        if *count > MAX_ENTRIES {
            return Err(MenuError::TooManyEntries);
        }
        out.push(match dto {
            NativeMenuEntry::Item {
                id,
                label: text,
                keybinding,
                checked,
                enabled,
            } => {
                if !valid_id(&id) {
                    return Err(MenuError::BadId(id));
                }
                if !seen.insert(id.clone()) {
                    return Err(MenuError::DuplicateId(id));
                }
                Entry::Item(Item {
                    id,
                    label: label(text)?,
                    // A binding a menu cannot show still works in the page; the item just has no shortcut.
                    accelerator: keybinding.as_deref().and_then(accelerator),
                    checked,
                    enabled,
                })
            }
            NativeMenuEntry::Separator => Entry::Separator,
            NativeMenuEntry::Header { label: text } => Entry::Header(label(text)?),
            NativeMenuEntry::Submenu { label: text, items } => {
                Entry::Submenu(label(text)?, entries(items, depth + 1, seen, count)?)
            }
        });
    }
    Ok(out)
}

// ---- the command's answer and error, and its event ----

/// The command that sends the menus (`menu_set`).
pub const MENU_SET_COMMAND: &str = "menu_set";
/// The event carrying the id of the chosen native menu item to the target window.
pub const MENU_EVENT: &str = "paddock://menu";

/// What `menu_set` did with a model.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MenuApplied {
    /// The menu bar already showed it.
    Unchanged,
    /// Check marks or enabled states updated in place.
    States,
    /// The menu bar was built anew.
    Rebuilt,
    /// Not shown: the caller is not the focused window (its model is sent again when it gets the focus).
    NotFocused,
    /// Not shown: this platform has no native menu bar for the app (the UI draws its own).
    InApp,
}

/// Why `menu_set` refused a model.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuSetError {
    pub code: MenuSetErrorCode,
    pub message: String,
}

/// The kind of a [`MenuSetError`]: `invalid`, the model broke a limit or a rule (the UI's bug); `native`, the menu bar
/// could not be built or the menus are not set up.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MenuSetErrorCode {
    Invalid,
    Native,
}

impl From<MenuError> for MenuSetError {
    fn from(e: MenuError) -> Self {
        MenuSetError {
            code: MenuSetErrorCode::Invalid,
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn model(v: serde_json::Value) -> Result<AppMenu, MenuError> {
        parse(serde_json::from_value(v).expect("a wire model"))
    }

    pub(crate) fn item(id: &str, label: &str) -> serde_json::Value {
        json!({ "kind": "item", "id": id, "label": label, "keybinding": null, "checked": null, "enabled": true })
    }

    /// A model like an app's: the app menu, File, Edit, View with a submenu and a caption, Window and Help.
    pub(crate) fn sample() -> AppMenu {
        model(json!({ "menus": [
            { "id": "app", "label": "Pitwall", "items": [item("pd.about", "About Pitwall"), { "kind": "separator" }, item("pd.settings.open", "Settings…")] },
            { "id": "file", "label": "File", "items": [
                { "kind": "item", "id": "pw.models.import", "label": "Import model…", "keybinding": "Mod+O", "checked": null, "enabled": true }
            ] },
            { "id": "edit", "label": "Edit", "items": [item("pw.edit.find", "Find")] },
            { "id": "view", "label": "View", "items": [
                { "kind": "item", "id": "pw.view.sidebar", "label": "Sidebar", "keybinding": null, "checked": true, "enabled": true },
                { "kind": "submenu", "label": "Appearance", "items": [
                    { "kind": "item", "id": "pw.view.compact", "label": "Compact", "keybinding": null, "checked": false, "enabled": true }
                ] },
                { "kind": "header", "label": "Engines" }
            ] },
            { "id": "window", "label": "Window", "items": [item("pd.window.reset", "Reset window size")] },
            { "id": "help", "label": "Help", "items": [item("pd.help.shortcuts", "Keyboard shortcuts")] }
        ] }))
        .expect("the sample is valid")
    }

    #[test]
    fn parse_validates_ids_labels_and_sizes() {
        let m = sample();
        assert_eq!(m.menus.len(), 6);
        assert_eq!(m.menus[0].role, MenuRole::App);
        assert_eq!(m.menus[3].role, MenuRole::Other);
        let Entry::Item(open) = &m.menus[1].items[0] else {
            panic!("an item")
        };
        assert_eq!(open.accelerator.as_deref(), Some("CmdOrCtrl+O"));

        let bad_id = model(
            json!({ "menus": [{ "id": "file", "label": "File", "items": [item("rm -rf", "x")] }] }),
        );
        assert!(matches!(bad_id, Err(MenuError::BadId(_))));
        let dup = model(
            json!({ "menus": [{ "id": "file", "label": "File", "items": [item("a", "x"), item("a", "y")] }] }),
        );
        assert_eq!(dup, Err(MenuError::DuplicateId("a".into())));
        let empty_label = model(json!({ "menus": [{ "id": "file", "label": "  ", "items": [] }] }));
        assert!(matches!(empty_label, Err(MenuError::BadLabel(_))));
        let control = model(
            json!({ "menus": [{ "id": "file", "label": "File", "items": [item("a", "x\u{7}")] }] }),
        );
        assert!(matches!(control, Err(MenuError::BadLabel(_))));
        let long = model(
            json!({ "menus": [{ "id": "file", "label": "x".repeat(MAX_LABEL + 1), "items": [] }] }),
        );
        assert!(matches!(long, Err(MenuError::BadLabel(_))));
        let bad_top = model(json!({ "menus": [{ "id": "", "label": "File", "items": [] }] }));
        assert!(matches!(bad_top, Err(MenuError::BadId(_))));
        let long_id = model(
            json!({ "menus": [{ "id": "f", "label": "F", "items": [item(&"a".repeat(MAX_ID + 1), "x")] }] }),
        );
        assert!(matches!(long_id, Err(MenuError::BadId(_))));
        let bad_header = model(
            json!({ "menus": [{ "id": "f", "label": "F", "items": [{ "kind": "header", "label": "" }] }] }),
        );
        assert!(matches!(bad_header, Err(MenuError::BadLabel(_))));
        let trimmed =
            model(json!({ "menus": [{ "id": "f", "label": "  File ", "items": [] }] })).unwrap();
        assert_eq!(trimmed.menus[0].label, "File");

        let many = (0..=MAX_MENUS)
            .map(|i| json!({ "id": format!("m{i}"), "label": "M", "items": [] }))
            .collect::<Vec<_>>();
        assert_eq!(
            model(json!({ "menus": many })),
            Err(MenuError::TooManyMenus(MAX_MENUS + 1))
        );
        let entries = (0..=MAX_ENTRIES)
            .map(|i| item(&format!("i{i}"), "I"))
            .collect::<Vec<_>>();
        assert_eq!(
            model(json!({ "menus": [{ "id": "f", "label": "F", "items": entries }] })),
            Err(MenuError::TooManyEntries)
        );
        let mut deep = json!([item("leaf", "Leaf")]);
        for _ in 0..MAX_DEPTH {
            deep = json!([{ "kind": "submenu", "label": "S", "items": deep }]);
        }
        assert_eq!(
            model(json!({ "menus": [{ "id": "f", "label": "F", "items": deep }] })),
            Err(MenuError::TooDeep)
        );
        // Unknown fields are rejected: the contract is exact.
        let extra: Result<NativeMenuModel, _> =
            serde_json::from_value(json!({ "menus": [], "script": "x" }));
        assert!(extra.is_err());
        let extra_item: Result<NativeMenuModel, _> = serde_json::from_value(
            json!({ "menus": [{ "id": "f", "label": "F", "items": [
            { "kind": "item", "id": "a", "label": "A", "keybinding": null, "checked": null, "enabled": true, "onclick": "x" }
        ] }] }),
        );
        assert!(extra_item.is_err());
        // A binding a menu cannot show is dropped, not fatal.
        let odd = model(json!({ "menus": [{ "id": "f", "label": "F", "items": [
            { "kind": "item", "id": "a", "label": "A", "keybinding": "Hyper+Q", "checked": null, "enabled": true }
        ] }] }))
        .unwrap();
        let Entry::Item(a) = &odd.menus[0].items[0] else {
            panic!("an item")
        };
        assert_eq!(a.accelerator, None);
        for e in [
            MenuError::TooManyMenus(20),
            MenuError::TooManyEntries,
            MenuError::TooDeep,
            MenuError::BadId("x".into()),
            MenuError::DuplicateId("x".into()),
            MenuError::BadLabel("x".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn ids_allow_command_ids_and_nothing_else() {
        for ok in [
            "pd.app.quit",
            "sd.say-hello",
            "view:timeline",
            "recent/1",
            "a",
        ] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "a b", "a\nb", "é", "<script>"] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }
}
