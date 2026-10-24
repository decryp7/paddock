//! The native menu bar (macOS) of a validated model, as a pure spec: the standard macOS items added, and what applying a
//! new spec over the shown one takes ([`diff`]).

use crate::model::{AppMenu, Entry, Item, MenuRole, QUIT_COMMAND, TopMenu};

/// Standard macOS items, implemented by the system (the Edit ones act on the webview's text fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Predefined {
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    Zoom,
}

/// One node of the native menu bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeNode {
    Item(Item),
    /// A disabled caption.
    Header(String),
    Separator,
    Predefined(Predefined),
    Submenu {
        label: String,
        role: MenuRole,
        items: Vec<NativeNode>,
    },
}

fn natives(entries: &[Entry]) -> Vec<NativeNode> {
    entries
        .iter()
        .map(|e| match e {
            Entry::Item(i) => NativeNode::Item(i.clone()),
            Entry::Separator => NativeNode::Separator,
            Entry::Header(h) => NativeNode::Header(h.clone()),
            Entry::Submenu(label, items) => NativeNode::Submenu {
                label: label.clone(),
                role: MenuRole::Other,
                items: natives(items),
            },
        })
        .collect()
}

/// `standard`, then the model's items after a separator (when there are any).
fn merged(standard: Vec<NativeNode>, model: Option<&TopMenu>) -> Vec<NativeNode> {
    let mut items = standard;
    if let Some(m) = model.filter(|m| !m.items.is_empty()) {
        items.push(NativeNode::Separator);
        items.extend(natives(&m.items));
    }
    items
}

/// The native menu bar for a model: the app menu (titled `app_name`) first, Edit and Window always present with their
/// standard items, the model's other menus in its order. Without its own Edit menu, the standard one follows the
/// model's first menu (File); without a Window menu, the standard one goes before Help. Pure.
pub fn native_spec(model: &AppMenu, app_name: &str) -> Vec<NativeNode> {
    use Predefined as P;
    let find = |role| model.menus.iter().find(|m| m.role == role);
    let mut app_items: Vec<NativeNode> = find(MenuRole::App)
        .map(|m| natives(&m.items))
        .unwrap_or_default();
    if !app_items.is_empty() {
        app_items.push(NativeNode::Separator);
    }
    app_items.extend([
        NativeNode::Predefined(P::Services),
        NativeNode::Separator,
        NativeNode::Predefined(P::Hide),
        NativeNode::Predefined(P::HideOthers),
        NativeNode::Predefined(P::ShowAll),
    ]);
    // Quit is the model's own command (it asks about unsaved work first), placed last.
    if let Some(quit) = app_items
        .iter()
        .position(|n| matches!(n, NativeNode::Item(i) if i.id == QUIT_COMMAND))
    {
        let item = app_items.remove(quit);
        app_items.extend([NativeNode::Separator, item]);
    }
    let edit = || NativeNode::Submenu {
        label: find(MenuRole::Edit).map_or_else(|| "Edit".to_string(), |m| m.label.clone()),
        role: MenuRole::Edit,
        items: merged(
            vec![
                NativeNode::Predefined(P::Undo),
                NativeNode::Predefined(P::Redo),
                NativeNode::Separator,
                NativeNode::Predefined(P::Cut),
                NativeNode::Predefined(P::Copy),
                NativeNode::Predefined(P::Paste),
                NativeNode::Predefined(P::SelectAll),
            ],
            find(MenuRole::Edit),
        ),
    };
    let window = || NativeNode::Submenu {
        label: find(MenuRole::Window).map_or_else(|| "Window".to_string(), |m| m.label.clone()),
        role: MenuRole::Window,
        items: merged(
            vec![
                NativeNode::Predefined(P::Minimize),
                NativeNode::Predefined(P::Zoom),
            ],
            find(MenuRole::Window),
        ),
    };
    let mut bar = vec![NativeNode::Submenu {
        label: app_name.to_string(),
        role: MenuRole::App,
        items: app_items,
    }];
    let (mut has_edit, mut has_window) = (false, false);
    let model_has_edit = find(MenuRole::Edit).is_some();
    for m in &model.menus {
        match m.role {
            MenuRole::App => {}
            MenuRole::Edit => {
                has_edit = true;
                bar.push(edit());
            }
            MenuRole::Window => {
                has_window = true;
                bar.push(window());
            }
            role => {
                bar.push(NativeNode::Submenu {
                    label: m.label.clone(),
                    role,
                    items: natives(&m.items),
                });
                if !model_has_edit && !has_edit && bar.len() == 2 {
                    has_edit = true;
                    bar.push(edit());
                }
            }
        }
    }
    if !has_edit {
        bar.push(edit());
    }
    if !has_window {
        let help = bar.iter().position(|n| {
            matches!(
                n,
                NativeNode::Submenu {
                    role: MenuRole::Help,
                    ..
                }
            )
        });
        bar.insert(help.unwrap_or(bar.len()), window());
    }
    bar
}

/// Ids of the items the UI runs, depth first.
pub fn item_ids(spec: &[NativeNode]) -> Vec<&str> {
    items_of(spec).into_iter().map(|i| i.id.as_str()).collect()
}

/// What applying a new spec over the shown one takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    Unchanged,
    /// Same structure: only these items' check marks or enabled states changed (updated in place).
    States(Vec<Item>),
    /// Built anew.
    Rebuild,
}

/// The spec with item states blanked: two specs with the same shape differ only in states.
fn shape(spec: &[NativeNode]) -> Vec<NativeNode> {
    spec.iter()
        .map(|n| match n {
            NativeNode::Item(i) => NativeNode::Item(Item {
                enabled: true,
                checked: i.checked.map(|_| false),
                ..i.clone()
            }),
            NativeNode::Submenu { label, role, items } => NativeNode::Submenu {
                label: label.clone(),
                role: *role,
                items: shape(items),
            },
            other => other.clone(),
        })
        .collect()
}

fn items_of(spec: &[NativeNode]) -> Vec<&Item> {
    let mut out = Vec::new();
    for n in spec {
        match n {
            NativeNode::Item(i) => out.push(i),
            NativeNode::Submenu { items, .. } => out.extend(items_of(items)),
            _ => {}
        }
    }
    out
}

/// Compares the shown spec with a new one: unchanged, states only (a toggle or an enabled state), or a rebuild (labels,
/// accelerators, items or menus changed). Pure.
pub fn diff(shown: Option<&[NativeNode]>, new: &[NativeNode]) -> Update {
    let Some(old) = shown else {
        return Update::Rebuild;
    };
    if old == new {
        return Update::Unchanged;
    }
    if shape(old) != shape(new) {
        return Update::Rebuild;
    }
    let changed = items_of(old)
        .into_iter()
        .zip(items_of(new))
        .filter(|(a, b)| a != b)
        .map(|(_, b)| b.clone())
        .collect();
    Update::States(changed)
}

/// The spec as indented text, one node per line: the snapshot form of the native menu bar (tests and logs).
pub fn outline(spec: &[NativeNode]) -> String {
    fn walk(out: &mut String, nodes: &[NativeNode], depth: usize) {
        for n in nodes {
            let pad = "  ".repeat(depth);
            match n {
                NativeNode::Item(i) => {
                    let check = match i.checked {
                        Some(true) => "[x] ",
                        Some(false) => "[ ] ",
                        None => "",
                    };
                    let key = i
                        .accelerator
                        .as_deref()
                        .map(|a| format!("  {a}"))
                        .unwrap_or_default();
                    let off = if i.enabled { "" } else { "  (disabled)" };
                    out.push_str(&format!("{pad}{check}{} <{}>{key}{off}\n", i.label, i.id));
                }
                NativeNode::Header(h) => out.push_str(&format!("{pad}# {h}\n")),
                NativeNode::Separator => out.push_str(&format!("{pad}---\n")),
                NativeNode::Predefined(p) => out.push_str(&format!("{pad}({p:?})\n")),
                NativeNode::Submenu { label, role, items } => {
                    let role = if *role == MenuRole::Other {
                        String::new()
                    } else {
                        format!(" ({role:?})")
                    };
                    out.push_str(&format!("{pad}{label}{role} >\n"));
                    walk(out, items, depth + 1);
                }
            }
        }
    }
    let mut out = String::new();
    walk(&mut out, spec, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{item, model, sample};
    use serde_json::json;

    fn labels(spec: &[NativeNode]) -> Vec<&str> {
        spec.iter()
            .filter_map(|n| match n {
                NativeNode::Submenu { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect()
    }

    fn submenu<'a>(spec: &'a [NativeNode], name: &str) -> &'a [NativeNode] {
        spec.iter()
            .find_map(|n| match n {
                NativeNode::Submenu { label, items, .. } if label == name => Some(items.as_slice()),
                _ => None,
            })
            .expect("the submenu")
    }

    /// The macOS menu bar of a typical app, as a snapshot: a change to the standard items or their order shows here.
    #[test]
    fn the_macos_native_spec_snapshot() {
        let spec = native_spec(&sample(), "Pitwall");
        let want = "\
Pitwall (App) >
  About Pitwall <pd.about>
  ---
  Settings… <pd.settings.open>
  ---
  (Services)
  ---
  (Hide)
  (HideOthers)
  (ShowAll)
File >
  Import model… <pw.models.import>  CmdOrCtrl+O
Edit (Edit) >
  (Undo)
  (Redo)
  ---
  (Cut)
  (Copy)
  (Paste)
  (SelectAll)
  ---
  Find <pw.edit.find>
View >
  [x] Sidebar <pw.view.sidebar>
  Appearance >
    [ ] Compact <pw.view.compact>
  # Engines
Window (Window) >
  (Minimize)
  (Zoom)
  ---
  Reset window size <pd.window.reset>
Help (Help) >
  Keyboard shortcuts <pd.help.shortcuts>
";
        assert_eq!(outline(&spec), want);
    }

    #[test]
    fn the_spec_adds_the_standard_macos_items() {
        let spec = native_spec(&sample(), "Pitwall");
        assert_eq!(
            labels(&spec),
            ["Pitwall", "File", "Edit", "View", "Window", "Help"]
        );
        let app = submenu(&spec, "Pitwall");
        assert!(matches!(&app[0], NativeNode::Item(i) if i.id == "pd.about"));
        assert_eq!(
            app.last(),
            Some(&NativeNode::Predefined(Predefined::ShowAll)),
            "no Quit item in the model: none added"
        );
        // The model's Quit (it asks about unsaved work) goes last, after a separator.
        let with_quit = model(json!({ "menus": [
            { "id": "app", "label": "Pitwall", "items": [item(QUIT_COMMAND, "Quit Pitwall"), item("pd.about", "About Pitwall")] }
        ] }))
        .unwrap();
        let quit_spec = native_spec(&with_quit, "Pitwall");
        let quit_app = submenu(&quit_spec, "Pitwall");
        assert!(matches!(quit_app.last(), Some(NativeNode::Item(i)) if i.id == QUIT_COMMAND));
        assert_eq!(quit_app[quit_app.len() - 2], NativeNode::Separator);
        assert!(matches!(&quit_app[0], NativeNode::Item(i) if i.id == "pd.about"));
        assert_eq!(
            item_ids(&spec),
            [
                "pd.about",
                "pd.settings.open",
                "pw.models.import",
                "pw.edit.find",
                "pw.view.sidebar",
                "pw.view.compact",
                "pd.window.reset",
                "pd.help.shortcuts"
            ]
        );
    }

    #[test]
    fn an_empty_model_still_has_the_app_edit_and_window_menus() {
        let spec = native_spec(&AppMenu::default(), "Demo");
        assert_eq!(labels(&spec), ["Demo", "Edit", "Window"]);
        assert!(item_ids(&spec).is_empty());
        // Without its own Edit menu, Edit follows File; Window goes before Help.
        let m = model(json!({ "menus": [
            { "id": "file", "label": "File", "items": [] }, { "id": "view", "label": "View", "items": [] }, { "id": "help", "label": "Help", "items": [] }
        ] }))
        .unwrap();
        assert_eq!(
            labels(&native_spec(&m, "Demo")),
            ["Demo", "File", "Edit", "View", "Window", "Help"]
        );
        // An empty Edit or Window menu of the model gets the standard items only.
        let empty = model(json!({ "menus": [{ "id": "edit", "label": "Bearbeiten", "items": [] }, { "id": "window", "label": "Fenster", "items": [] }] })).unwrap();
        let spec = native_spec(&empty, "Demo");
        assert_eq!(labels(&spec), ["Demo", "Bearbeiten", "Fenster"]);
        assert_eq!(
            submenu(&spec, "Fenster"),
            [
                NativeNode::Predefined(Predefined::Minimize),
                NativeNode::Predefined(Predefined::Zoom)
            ]
        );
    }

    #[test]
    fn diff_updates_states_in_place_and_rebuilds_on_structure_changes() {
        let spec = native_spec(&sample(), "Pitwall");
        assert_eq!(diff(None, &spec), Update::Rebuild);
        assert_eq!(diff(Some(&spec), &spec), Update::Unchanged);

        let mut toggled = sample();
        let Entry::Item(sidebar) = &mut toggled.menus[3].items[0] else {
            panic!("an item")
        };
        sidebar.checked = Some(false);
        let Entry::Item(find) = &mut toggled.menus[2].items[0] else {
            panic!("an item")
        };
        find.enabled = false;
        let Update::States(changed) = diff(Some(&spec), &native_spec(&toggled, "Pitwall")) else {
            panic!("expected a state update")
        };
        assert_eq!(
            changed.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["pw.edit.find", "pw.view.sidebar"]
        );
        assert_eq!(changed[1].checked, Some(false));
        assert!(!changed[0].enabled);

        let mut relabelled = sample();
        let Entry::Item(find) = &mut relabelled.menus[2].items[0] else {
            panic!("an item")
        };
        find.label = "Find in models".into();
        assert_eq!(
            diff(Some(&spec), &native_spec(&relabelled, "Pitwall")),
            Update::Rebuild
        );

        let mut fewer = sample();
        fewer.menus[3].items.pop();
        assert_eq!(
            diff(Some(&spec), &native_spec(&fewer, "Pitwall")),
            Update::Rebuild
        );

        // A plain item becoming a toggle changes the structure (a check item is another native kind).
        let mut toggle = sample();
        let Entry::Item(find) = &mut toggle.menus[2].items[0] else {
            panic!("an item")
        };
        find.checked = Some(false);
        assert_eq!(
            diff(Some(&spec), &native_spec(&toggle, "Pitwall")),
            Update::Rebuild
        );
    }

    #[test]
    fn the_outline_marks_disabled_items() {
        let m = model(json!({ "menus": [{ "id": "file", "label": "File", "items": [
            { "kind": "item", "id": "a", "label": "A", "keybinding": null, "checked": null, "enabled": false }
        ] }] }))
        .unwrap();
        assert!(outline(&native_spec(&m, "Demo")).contains("A <a>  (disabled)"));
    }
}
