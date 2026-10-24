//! The menu part of the app's generated `paddock-manifest.json` (ADR 0012 §2, written by the UI's manifest spec), checked
//! at build/test time: every menu item routes to a command, every key converts to a native accelerator, and a command
//! is placed in one menu only (the native menu bar shows an id once). Run [`check`] in a test of the app.
//!
//! ```
//! let manifest = r#"{
//!   "version": 1,
//!   "commands": [{ "id": "pw.models.refresh", "title": "pw.models.refresh" }],
//!   "keybindings": [{ "command": "pw.models.refresh", "key": "Mod+R", "accelerator": "CmdOrCtrl+R", "scope": "window" }],
//!   "menus": [{ "menu": "view", "command": "pw.models.refresh", "group": "models", "order": 100, "accelerator": "CmdOrCtrl+R" }]
//! }"#;
//! assert_eq!(paddock_menu::manifest::check(manifest), Vec::<String>::new());
//! ```

use crate::accelerator::accelerator;
use crate::model::{MAX_ENTRIES, valid_id};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// The menu-bar menus (`SHELL_MENUS` in `@decryp7/paddock-ui/shell`): a command sits in one of them at most. Other
/// menus are context menus (or submenus, which the manifest does not place), and may repeat a menu-bar command.
pub const MENU_BAR: [&str; 6] = ["app", "file", "edit", "view", "window", "help"];

/// The manifest version this crate reads (`MANIFEST_VERSION` in `@decryp7/paddock-ui/shell`).
pub const MANIFEST_VERSION: u32 = 1;

// The other sections belong to other crates' checks, so unknown fields are allowed at the top level only.
#[derive(Debug, Deserialize)]
struct Manifest {
    version: u32,
    commands: Vec<Command>,
    keybindings: Vec<Keybinding>,
    menus: Vec<MenuItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    id: String,
    #[allow(dead_code)]
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Keybinding {
    command: String,
    key: String,
    accelerator: String,
    scope: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MenuItem {
    menu: String,
    command: String,
    #[allow(dead_code)]
    group: String,
    #[allow(dead_code)]
    order: f64,
    accelerator: Option<String>,
}

/// Every problem of the manifest's menus and keys, one sentence each (empty: valid).
pub fn check(manifest_json: &str) -> Vec<String> {
    let manifest: Manifest = match serde_json::from_str(manifest_json) {
        Ok(m) => m,
        Err(e) => return vec![format!("the manifest is not readable: {e}")],
    };
    let mut out = Vec::new();
    if manifest.version != MANIFEST_VERSION {
        out.push(format!(
            "the manifest is version {}; this crate reads version {MANIFEST_VERSION}",
            manifest.version
        ));
    }
    let commands: BTreeSet<&str> = manifest.commands.iter().map(|c| c.id.as_str()).collect();
    if manifest.menus.len() > MAX_ENTRIES {
        out.push(format!(
            "the menus have {} items; at most {MAX_ENTRIES} are allowed",
            manifest.menus.len()
        ));
    }
    let mut window_keys: BTreeMap<&str, String> = BTreeMap::new();
    for k in &manifest.keybindings {
        if !commands.contains(k.command.as_str()) {
            out.push(format!(
                "keybinding {} names the missing command {}",
                k.key, k.command
            ));
        }
        match accelerator(&k.key) {
            None => out.push(format!(
                "keybinding {} of {} has no native accelerator",
                k.key, k.command
            )),
            Some(a) => {
                if k.accelerator.trim().is_empty() {
                    out.push(format!(
                        "keybinding {} of {} has an empty accelerator",
                        k.key, k.command
                    ));
                }
                if k.scope == "window" {
                    window_keys.entry(k.command.as_str()).or_insert(a);
                }
            }
        }
    }
    let mut placed: BTreeMap<&str, &str> = BTreeMap::new();
    for m in &manifest.menus {
        let at = format!("menu item {}/{}", m.menu, m.command);
        if !commands.contains(m.command.as_str()) {
            out.push(format!(
                "{at} does not route: no command {} in the manifest",
                m.command
            ));
        }
        if !valid_id(&m.command) {
            out.push(format!("{at}: the command id is not a native menu item id"));
        }
        let in_bar = MENU_BAR.contains(&m.menu.as_str());
        if let Some(first) = in_bar
            .then(|| placed.insert(m.command.as_str(), m.menu.as_str()))
            .flatten()
        {
            out.push(format!(
                "command {} is in two menus ({first} and {}); the native menu bar shows a command once",
                m.command, m.menu
            ));
        }
        if m.accelerator.is_some() && !window_keys.contains_key(m.command.as_str()) {
            out.push(format!(
                "{at} shows an accelerator but the command has no window-scope keybinding"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(menus: serde_json::Value, keybindings: serde_json::Value) -> String {
        json!({
            "version": 1,
            "commands": [{ "id": "sd.greet", "title": "sd.greet" }, { "id": "sd.reset", "title": "sd.reset" }],
            "keybindings": keybindings,
            "menus": menus,
            "pages": ["sd.home"],
            "messages": {}
        })
        .to_string()
    }

    #[test]
    fn a_consistent_manifest_has_no_problems() {
        let m = manifest(
            json!([
                { "menu": "help", "command": "sd.greet", "group": "demo", "order": 100, "accelerator": "CmdOrCtrl+Shift+H" },
                { "menu": "help", "command": "sd.reset", "group": "demo", "order": 200, "accelerator": null }
            ]),
            json!([{ "command": "sd.greet", "key": "Mod+Shift+H", "accelerator": "CmdOrCtrl+Shift+H", "scope": "window" }]),
        );
        assert_eq!(check(&m), Vec::<String>::new());
    }

    #[test]
    fn a_context_menu_may_repeat_a_menu_bar_command() {
        let m = manifest(
            json!([
                { "menu": "help", "command": "sd.greet", "group": "demo", "order": 100, "accelerator": null },
                { "menu": "sd.greetings", "command": "sd.greet", "group": "demo", "order": 100, "accelerator": null }
            ]),
            json!([]),
        );
        assert_eq!(check(&m), Vec::<String>::new());
    }

    #[test]
    fn every_defect_is_reported() {
        let m = manifest(
            json!([
                { "menu": "help", "command": "sd.gone", "group": "demo", "order": 100, "accelerator": null },
                { "menu": "help", "command": "sd.greet", "group": "demo", "order": 100, "accelerator": null },
                { "menu": "file", "command": "sd.greet", "group": "demo", "order": 100, "accelerator": null },
                { "menu": "file", "command": "sd.reset", "group": "demo", "order": 200, "accelerator": "CmdOrCtrl+R" },
                { "menu": "file", "command": "bad id", "group": "demo", "order": 300, "accelerator": null }
            ]),
            json!([
                { "command": "sd.missing", "key": "Mod+M", "accelerator": "CmdOrCtrl+M", "scope": "window" },
                { "command": "sd.greet", "key": "Hyper+H", "accelerator": "Hyper+H", "scope": "window" },
                { "command": "sd.reset", "key": "Mod+R", "accelerator": " ", "scope": "page.home" }
            ]),
        );
        let problems = check(&m);
        let want = [
            "keybinding Mod+M names the missing command sd.missing",
            "keybinding Hyper+H of sd.greet has no native accelerator",
            "keybinding Mod+R of sd.reset has an empty accelerator",
            "menu item help/sd.gone does not route: no command sd.gone in the manifest",
            "command sd.greet is in two menus (help and file); the native menu bar shows a command once",
            "menu item file/sd.reset shows an accelerator but the command has no window-scope keybinding",
            "menu item file/bad id does not route: no command bad id in the manifest",
            "menu item file/bad id: the command id is not a native menu item id",
        ];
        assert_eq!(problems, want);
    }

    #[test]
    fn an_unreadable_or_newer_manifest_is_reported() {
        assert!(
            check("{")
                .first()
                .is_some_and(|p| p.starts_with("the manifest is not readable"))
        );
        let bad_item = json!({ "version": 1, "commands": [], "keybindings": [], "menus": [{ "menu": "x", "command": "y", "group": "g", "order": 1, "accelerator": null, "extra": 1 }] });
        assert!(check(&bad_item.to_string())[0].starts_with("the manifest is not readable"));
        let v2 = json!({ "version": 2, "commands": [], "keybindings": [], "menus": [] });
        assert_eq!(
            check(&v2.to_string()),
            ["the manifest is version 2; this crate reads version 1"]
        );
        let many: Vec<_> = (0..=MAX_ENTRIES)
            .map(|i| json!({ "menu": "file", "command": format!("sd.c{i}"), "group": "g", "order": i, "accelerator": null }))
            .collect();
        let commands: Vec<_> = (0..=MAX_ENTRIES)
            .map(|i| json!({ "id": format!("sd.c{i}"), "title": "t" }))
            .collect();
        let big = json!({ "version": 1, "commands": commands, "keybindings": [], "menus": many });
        assert_eq!(
            check(&big.to_string()),
            [format!(
                "the menus have {} items; at most {MAX_ENTRIES} are allowed",
                MAX_ENTRIES + 1
            )]
        );
    }
}
