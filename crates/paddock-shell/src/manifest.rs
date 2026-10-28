//! The Rust check of the app's generated `paddock-manifest.json` (ADR 0012 §2; written by the UI's manifest spec from
//! its contributions): the static parts the UI declares must agree with what the Rust side serves. Run it in a test of
//! the app (`Builder::problems(Some(manifest))`); every problem is one sentence, empty when they agree.
//!
//! - Menus and keys: [`paddock_menu::manifest::check`] (every menu item routes to a command, every key converts to an
//!   accelerator, one menu-bar menu per command).
//! - Dialogs: every command a dialog declares (and every base command) exists in the shell's table or the app's and is
//!   open to dialogs; every dialog of the manifest has a contract in the app's contract file and vice versa.
//! - Health: the UI's health services and the Rust services are the same set (every chip has a supervisor and every
//!   supervisor a chip).
//! - Settings: every app settings section has a page, and every page the shell's generic form shows has a section.
//! - The app's table never reuses a shell command's name (both would answer to the same dialog contract entry).

use crate::COMMANDS;
use paddock_commands::{Table, WindowClass};
use paddock_dialogs::Contracts;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// The manifest version this crate reads (`MANIFEST_VERSION` in `@decryp7/paddock-ui/shell`).
pub const MANIFEST_VERSION: u32 = 1;

/// What the Rust side serves, to check a manifest against.
pub struct Expect<'a> {
    /// The app's own command table.
    pub app_commands: &'a Table,
    /// The ids of the app's health services.
    pub services: &'a [&'a str],
    /// The ids of the app's own settings sections (the shell's are the shell's business).
    pub sections: &'a [&'a str],
}

// The sections this check reads; the others belong to other checks (menus: `paddock-menu`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u32,
    dialogs: BTreeMap<String, Dialog>,
    health_services: Vec<String>,
    settings_pages: Vec<String>,
    settings_forms: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Dialog {
    commands: Vec<String>,
}

/// Whether `name` (a command or a `prefix*`) is served to dialogs by one of `tables`; why not.
fn dialog_command_problem(who: &str, name: &str, tables: &[&Table]) -> Option<String> {
    let specs = tables.iter().flat_map(|t| t.specs().iter());
    if let Some(prefix) = name.strip_suffix('*') {
        let matched: Vec<_> = specs.filter(|s| s.name().starts_with(prefix)).collect();
        if matched.is_empty() {
            return Some(format!(
                "{who}: {name} matches no command of the shell or the app"
            ));
        }
        let closed: Vec<&str> = matched
            .iter()
            .filter(|s| !s.windows().contains(WindowClass::Dialog))
            .map(|s| s.name())
            .collect();
        return (!closed.is_empty()).then(|| {
            format!(
                "{who}: {name} matches {}, which the tables do not open to dialogs",
                closed.join(", ")
            )
        });
    }
    let found: Vec<_> = specs.filter(|s| s.name() == name).collect();
    if found.is_empty() {
        return Some(format!(
            "{who} declares {name}, which neither the shell nor the app serves"
        ));
    }
    (!found
        .iter()
        .any(|s| s.windows().contains(WindowClass::Dialog)))
    .then(|| format!("{who} declares {name}, which is not open to dialogs"))
}

/// The app's table against the shell's: a command name both serve is a problem.
pub fn table_problems(app: &Table) -> Vec<String> {
    app.specs()
        .iter()
        .filter(|s| COMMANDS.spec(s.name()).is_some())
        .map(|s| {
            format!(
                "the app's command {} has the name of a shell command; rename it",
                s.name()
            )
        })
        .collect()
}

/// The dialog contracts against the shell's table and the app's.
pub fn contract_problems(contracts: &Contracts, app: &Table) -> Vec<String> {
    let tables = [&COMMANDS, app];
    let base = contracts.base().iter().map(|c| ("base", c.as_str()));
    let declared = contracts
        .declared()
        .flat_map(|(id, cs)| cs.iter().map(move |c| (id, c.as_str())));
    base.chain(declared)
        .filter_map(|(who, c)| dialog_command_problem(who, c, &tables))
        .collect()
}

/// Every problem of `manifest_json` against `expect` (menus, dialogs, health services, settings).
pub fn check(manifest_json: &str, expect: &Expect<'_>) -> Vec<String> {
    let mut out = paddock_menu::manifest::check(manifest_json);
    let m: Manifest = match serde_json::from_str(manifest_json) {
        Ok(m) => m,
        Err(e) => {
            out.push(format!("the manifest is not readable (regenerate it): {e}"));
            return out;
        }
    };
    if m.version != MANIFEST_VERSION {
        out.push(format!(
            "the manifest is version {}; paddock-shell reads version {MANIFEST_VERSION}",
            m.version
        ));
    }
    let tables = [&COMMANDS, expect.app_commands];
    for (id, d) in &m.dialogs {
        for c in &d.commands {
            out.extend(dialog_command_problem(id, c, &tables));
        }
    }
    let ui: BTreeSet<&str> = m.health_services.iter().map(String::as_str).collect();
    let rust: BTreeSet<&str> = expect.services.iter().copied().collect();
    for id in ui.difference(&rust) {
        out.push(format!(
            "the UI shows the health service {id}, which no Rust supervisor reports"
        ));
    }
    for id in rust.difference(&ui) {
        out.push(format!(
            "the Rust service {id} has no health service in the UI (defineHealthService)"
        ));
    }
    let pages: BTreeSet<&str> = m.settings_pages.iter().map(String::as_str).collect();
    for id in expect.sections.iter().filter(|s| !pages.contains(*s)) {
        out.push(format!(
            "the settings section {id} has no settings page in the UI (defineSettingsPage)"
        ));
    }
    let shell_sections = [
        paddock_settings::shell::APPEARANCE,
        paddock_settings::shell::UPDATES,
        paddock_diag::logging::LOGGING,
    ];
    for id in &m.settings_forms {
        if !expect.sections.contains(&id.as_str()) && !shell_sections.contains(&id.as_str()) {
            out.push(format!(
                "the settings page {id} shows the generic form, but no settings section {id} is registered"
            ));
        }
    }
    out
}

/// Every dialog of the manifest without a contract, and every contract of the app's own tables (`own`) without a dialog
/// in the manifest (a shell table declares dialogs an app may not use).
pub fn contract_drift(manifest_json: &str, contracts: &Contracts, own: &[&str]) -> Vec<String> {
    let Ok(m) = serde_json::from_str::<Manifest>(manifest_json) else {
        return vec!["the manifest is not readable (regenerate it)".into()];
    };
    let ui: BTreeSet<&str> = m.dialogs.keys().map(String::as_str).collect();
    let file: BTreeSet<&str> = contracts.ids().collect();
    let mut out: Vec<String> = ui
        .difference(&file)
        .map(|id| format!("the dialog {id} has no contract (declare it in a dialogs! table)"))
        .collect();
    out.extend(file.difference(&ui).filter(|id| own.contains(id)).map(|id| {
        format!("the contract of {id} names no dialog of the manifest (register it, or remove it from the table)")
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    paddock_commands::command_table! {
        static APP;
        Query [MAIN | DIALOG] => app::app_status,
        Mutation [MAIN] => app::app_reset,
    }

    // One leaf table per module (each emits its own `dialog_callable` markers).
    mod clashing {
        paddock_commands::command_table! {
            pub(super) static CLASHING;
            Query [MAIN] => app::health_snapshot,
        }
    }
    use clashing::CLASHING;

    fn manifest(
        dialogs: serde_json::Value,
        services: &[&str],
        pages: &[&str],
        forms: &[&str],
    ) -> String {
        json!({
            "version": 1,
            "commands": [{ "id": "sd.greet", "title": "sd.greet" }],
            "keybindings": [],
            "menus": [{ "menu": "file", "command": "sd.greet", "group": "main", "order": 1, "accelerator": null }],
            "pages": [], "statusItems": [], "notificationKinds": {}, "messages": {},
            "dialogs": dialogs,
            "healthServices": services,
            "settingsPages": pages,
            "settingsForms": forms,
        })
        .to_string()
    }

    fn expect<'a>(services: &'a [&'a str], sections: &'a [&'a str]) -> Expect<'a> {
        Expect {
            app_commands: &APP,
            services,
            sections,
        }
    }

    #[test]
    fn a_manifest_that_agrees_has_no_problems() {
        let m = manifest(
            json!({ "sd.about": { "commands": ["about_info", "app_status", "updates_*"] } }),
            &["sd.engine"],
            &["sd.general", "pd.appearance"],
            &["sd.general", "pd.appearance", "pd.updates"],
        );
        assert_eq!(
            check(&m, &expect(&["sd.engine"], &["sd.general"])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn each_kind_of_disagreement_is_one_sentence() {
        let m = manifest(
            json!({ "sd.x": { "commands": ["nope", "app_reset", "nothing_*", "window_*"] } }),
            &["sd.engine", "sd.ui-only"],
            &["sd.general"],
            &["sd.form-only"],
        );
        let p = check(
            &m,
            &expect(
                &["sd.engine", "sd.rust-only"],
                &["sd.general", "sd.pageless"],
            ),
        );
        let has = |s: &str| p.iter().any(|x| x.contains(s));
        assert!(
            has("sd.x declares nope, which neither the shell nor the app serves"),
            "{p:?}"
        );
        assert!(
            has("sd.x declares app_reset, which is not open to dialogs"),
            "{p:?}"
        );
        assert!(has("nothing_* matches no command"), "{p:?}");
        assert!(
            has("window_* matches"),
            "window_close is for main windows only: {p:?}"
        );
        assert!(
            has("health service sd.ui-only, which no Rust supervisor reports"),
            "{p:?}"
        );
        assert!(
            has("Rust service sd.rust-only has no health service"),
            "{p:?}"
        );
        assert!(has("section sd.pageless has no settings page"), "{p:?}");
        assert!(has("page sd.form-only shows the generic form"), "{p:?}");
        assert_eq!(p.len(), 8, "{p:?}");
    }

    #[test]
    fn an_unreadable_or_newer_manifest_is_reported() {
        let p = check("{", &expect(&[], &[]));
        assert!(p.iter().any(|x| x.contains("not readable")), "{p:?}");
        let old =
            json!({ "version": 1, "commands": [], "keybindings": [], "menus": [] }).to_string();
        assert!(
            check(&old, &expect(&[], &[]))
                .iter()
                .any(|x| x.contains("regenerate"))
        );
        let newer = manifest(json!({}), &[], &[], &[]).replace("\"version\":1", "\"version\":2");
        assert!(
            check(&newer, &expect(&[], &[]))
                .iter()
                .any(|x| x.contains("version 2"))
        );
        let c = Contracts::empty();
        assert_eq!(
            contract_drift("{", &c, &[]),
            vec!["the manifest is not readable (regenerate it)".to_string()]
        );
    }

    #[test]
    fn the_apps_table_must_not_reuse_a_shell_name_and_contracts_name_served_commands() {
        assert_eq!(table_problems(&APP), Vec::<String>::new());
        assert_eq!(
            table_problems(&CLASHING),
            vec![
                "the app's command health_snapshot has the name of a shell command; rename it"
                    .to_string()
            ]
        );
        use paddock_commands::DialogCallable::{Command, Prefix};
        use paddock_dialogs::{Entry, Shape};
        let tool = |id, commands| Entry {
            commands,
            ..Entry::new(id, Shape::Tool)
        };
        let entries = [
            tool("sd.a", &[Command("app_status"), Prefix("settings_")]),
            tool("sd.b", &[]),
            tool("pd.shell-only", &[]),
        ];
        let c =
            Contracts::from_entries(&entries, &[Command("window_ready"), Command("gone")]).unwrap();
        assert_eq!(
            contract_problems(&c, &APP),
            vec!["base declares gone, which neither the shell nor the app serves".to_string()]
        );
        let m = manifest(
            json!({ "sd.a": { "commands": [] }, "sd.c": { "commands": [] } }),
            &[],
            &[],
            &[],
        );
        assert_eq!(
            contract_drift(&m, &c, &["sd.a", "sd.b"]),
            vec![
                "the dialog sd.c has no contract (declare it in a dialogs! table)".to_string(),
                "the contract of sd.b names no dialog of the manifest (register it, or remove it from the table)"
                    .to_string(),
            ]
        );
    }
}
