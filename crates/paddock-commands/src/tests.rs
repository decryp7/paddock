//! Tests of the core, ported from Debrief's `commands.rs`, `dialog_windows.rs` and `generated_types.rs` tests with a
//! table of the same shape.

use crate::guard::DialogGrant;
use crate::labels::{Label, WindowClass, class_of, resolve};
use crate::table::{MAX_LIMIT, MIN_LIMIT};
use crate::testing::{self, FoundCommand, GeneratedError, Synced, WriteMode};
use crate::{
    CommandError, DEFAULT_LIMIT, ErrorCode, Kind, MIB, RefusalCode, Spec, Table, TableError,
    Windows, check, ts,
};
use std::path::{Path, PathBuf};

crate::command_table! {
    /// A table like an app's.
    static COMMANDS;
    Mutation [WINDOW] => workspace::open_paths,
    Query [WINDOW] => workspace::query_page,
    /// The session carries the UI state.
    Quiet [MAIN] limit 16 * MIB => session::save_session,
    Mutation [MAIN] => session::resolve_close,
    Mutation [MAIN] => app::restart_app,
    Query [WINDOW] => legal_text,
    Query [DIALOG] => dialogs::dialog_take_input,
    Mutation [DIALOG] => dialogs::dialog_close,
    Quiet [DIALOG] => dialogs::dialog_fit_height,
    Query [MAIN | POPOUT] => ui::ui_sync_layout,
    Query [WINDOW] => ui::ui_sync_theme,
}

/// A dialog's grant as `paddock-dialogs` gives it: the dialog's id, the base commands and its own (`prefix*` allowed).
struct Grant {
    id: &'static str,
    commands: &'static [&'static str],
}

impl DialogGrant for Grant {
    fn dialog(&self) -> &str {
        self.id
    }
    fn allows(&self, command: &str) -> bool {
        ["dialog_take_input", "dialog_close", "dialog_fit_height"]
            .iter()
            .chain(self.commands)
            .any(|d| {
                d.strip_suffix('*')
                    .map_or(*d == command, |p| command.starts_with(p))
            })
    }
}

const ABOUT: Grant = Grant {
    id: "about",
    commands: &["legal_text"],
};
const EXPORT: Grant = Grant {
    id: "export",
    commands: &["query_page", "ui_*"],
};

fn run(
    command: &str,
    label: &str,
    grant: Option<&Grant>,
    size: usize,
) -> Result<(), crate::Refusal> {
    check(
        &COMMANDS,
        command,
        label,
        grant.map(|g| g as &dyn DialogGrant),
        size,
    )
}

#[test]
fn every_command_has_one_entry_with_a_window_and_a_sane_limit() {
    let mut names: Vec<&str> = COMMANDS.specs().iter().map(Spec::name).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "a command is listed twice");
    for s in COMMANDS.specs() {
        assert!(
            !s.windows().is_empty(),
            "{} is callable from no window",
            s.name()
        );
        assert!(
            (MIN_LIMIT..=MAX_LIMIT).contains(&s.limit()),
            "{}: {}",
            s.name(),
            s.limit()
        );
        assert!(
            !s.name().contains(' ') && !s.name().contains(':'),
            "{}",
            s.path()
        );
    }
    assert_eq!(
        COMMANDS.spec("open_paths").map(|s| s.kind()),
        Some(Kind::Mutation)
    );
    assert_eq!(
        COMMANDS.spec("open_paths").map(|s| s.path()),
        Some("workspace :: open_paths")
    );
    assert_eq!(
        COMMANDS.spec("legal_text").map(|s| s.path()),
        Some("legal_text")
    );
    assert_eq!(
        COMMANDS.spec("save_session").map(|s| s.limit()),
        Some(16 * MIB)
    );
    assert_eq!(
        COMMANDS.spec("query_page").map(|s| s.limit()),
        Some(DEFAULT_LIMIT)
    );
    assert_eq!(COMMANDS.kind_of("query_page"), Kind::Query);
    assert_eq!(COMMANDS.kind_of("save_session"), Kind::Quiet);
    assert_eq!(COMMANDS.kind_of("no_such_command"), Kind::Mutation);
    assert_eq!(COMMANDS.kind_of("plugin:log|log"), Kind::Quiet);
    assert_eq!(COMMANDS.kind_of("plugin:event|listen"), Kind::Quiet);
    assert_eq!(COMMANDS.kind_of("plugin:dialog|message"), Kind::Mutation);
    assert_eq!(COMMANDS.kind_of("plugin:window|title"), Kind::Query);
    assert!(Table::EMPTY.specs().is_empty());
}

#[test]
fn a_spec_names_the_last_path_segment() {
    for (path, name) in [
        ("a :: b", "b"),
        ("a :: b :: c_d", "c_d"),
        ("plain", "plain"),
        ("a::b", "b"),
        ("", ""),
    ] {
        assert_eq!(
            Spec::new(path, Kind::Query, Windows::MAIN, DEFAULT_LIMIT).name(),
            name,
            "{path}"
        );
    }
}

#[test]
fn window_sets_combine_and_list_their_classes() {
    let set = Windows::NONE.with(Windows::MAIN).with(Windows::DIALOG);
    assert!(
        set.contains(WindowClass::Main)
            && set.contains(WindowClass::Dialog)
            && !set.contains(WindowClass::Popout)
    );
    assert_eq!(set.classes(), [WindowClass::Main, WindowClass::Dialog]);
    assert_eq!(
        Windows::WINDOW.classes(),
        [WindowClass::Main, WindowClass::Popout, WindowClass::Dialog],
        "a tray popover is not a window"
    );
    assert!(!Windows::WINDOW.contains(WindowClass::Tray));
    assert_eq!(
        Windows::WINDOW.with(Windows::TRAY).classes(),
        WindowClass::ALL
    );
    assert!(Windows::NONE.is_empty() && Windows::NONE.classes().is_empty());
    for c in WindowClass::ALL {
        assert_eq!(Windows::of(c).classes(), [c]);
    }
    let info = COMMANDS.spec("ui_sync_layout").map(Spec::info).unwrap();
    assert_eq!(info.kind, Kind::Query);
    assert_eq!(info.windows, [WindowClass::Main, WindowClass::Popout]);
    assert_eq!(info.limit, DEFAULT_LIMIT);
    assert_eq!(crate::limit(&[]), DEFAULT_LIMIT);
    assert_eq!(crate::limit(&[MIB]), MIB);
    assert_eq!(
        [Kind::Query, Kind::Mutation, Kind::Quiet].map(Kind::as_str),
        ["query", "mutation", "quiet"]
    );
}

fn spec(path: &'static str, windows: Windows, limit: usize) -> Spec {
    Spec::new(path, Kind::Query, windows, limit)
}

#[test]
fn a_malformed_table_is_refused_with_the_entry_that_is_wrong() {
    let ok = spec("a :: one", Windows::MAIN, DEFAULT_LIMIT);
    assert_eq!(Table::validate(&[ok]), Ok(()));
    for (what, bad) in [
        ("empty name", spec("", Windows::MAIN, DEFAULT_LIMIT)),
        ("capital", spec("Open", Windows::MAIN, DEFAULT_LIMIT)),
        ("digit first", spec("1open", Windows::MAIN, DEFAULT_LIMIT)),
        ("dash", spec("open-paths", Windows::MAIN, DEFAULT_LIMIT)),
        (
            "too long",
            spec(
                "a_very_long_command_name_that_goes_on_and_on_and_on_past_sixty_four_x",
                Windows::MAIN,
                DEFAULT_LIMIT,
            ),
        ),
    ] {
        assert_eq!(
            Table::validate(&[ok, bad]),
            Err(TableError::BadName { index: 1 }),
            "{what}"
        );
    }
    assert_eq!(
        Table::validate(&[ok, spec("x", Windows::NONE, DEFAULT_LIMIT)]),
        Err(TableError::NoWindows { index: 1 })
    );
    for limit in [MIN_LIMIT - 1, MAX_LIMIT + 1] {
        assert_eq!(
            Table::validate(&[spec("x", Windows::MAIN, limit)]),
            Err(TableError::BadLimit { index: 0 })
        );
    }
    let dup = spec("b :: one", Windows::DIALOG, DEFAULT_LIMIT);
    assert_eq!(
        Table::validate(&[ok, spec("two", Windows::MAIN, DEFAULT_LIMIT), dup]),
        Err(TableError::Duplicate { first: 0, index: 2 })
    );
    assert_eq!(
        Table::validate(&[ok, spec("b :: one_more", Windows::MAIN, DEFAULT_LIMIT)]),
        Ok(())
    );
    // Same length, different bytes: not a duplicate.
    assert_eq!(
        Table::validate(&[ok, spec("b :: onf", Windows::MAIN, DEFAULT_LIMIT)]),
        Ok(())
    );
}

#[test]
fn a_valid_table_builds_at_run_time_too() {
    static SPECS: [Spec; 2] = [
        Spec::new("a :: x", Kind::Query, Windows::MAIN, DEFAULT_LIMIT),
        Spec::new("b :: y", Kind::Quiet, Windows::DIALOG, MIB),
    ];
    let table = Table::new(&SPECS);
    assert_eq!(table.specs().len(), 2);
    assert_eq!(table.kind_of("y"), Kind::Quiet);
}

#[test]
#[should_panic(expected = "listed twice")]
fn a_duplicate_panics_at_build() {
    static SPECS: [Spec; 2] = [
        Spec::new("a :: x", Kind::Query, Windows::MAIN, DEFAULT_LIMIT),
        Spec::new("b :: x", Kind::Query, Windows::MAIN, DEFAULT_LIMIT),
    ];
    let _ = Table::new(&SPECS);
}

#[test]
#[should_panic(expected = "no window class")]
fn a_command_for_no_window_panics_at_build() {
    static SPECS: [Spec; 1] = [Spec::new("x", Kind::Query, Windows::NONE, DEFAULT_LIMIT)];
    let _ = Table::new(&SPECS);
}

#[test]
#[should_panic(expected = "limit")]
fn a_bad_limit_panics_at_build() {
    static SPECS: [Spec; 1] = [Spec::new("x", Kind::Query, Windows::MAIN, 1)];
    let _ = Table::new(&SPECS);
}

#[test]
#[should_panic(expected = "lower_snake_case")]
fn a_bad_name_panics_at_build() {
    static SPECS: [Spec; 1] = [Spec::new("X", Kind::Query, Windows::MAIN, DEFAULT_LIMIT)];
    let _ = Table::new(&SPECS);
}

#[test]
fn labels_follow_the_shell_scheme() {
    assert_eq!(Label::parse("main"), Some(Label::Main));
    assert_eq!(Label::parse("w-3"), Some(Label::Window(3)));
    assert_eq!(
        Label::parse("d-main-1"),
        Some(Label::Dialog { host: "main", n: 1 })
    );
    assert_eq!(
        Label::parse("d-w-2-13"),
        Some(Label::Dialog { host: "w-2", n: 13 })
    );
    assert_eq!(
        Label::parse("p-main-1"),
        Some(Label::Popout { host: "main", n: 1 })
    );
    assert_eq!(
        Label::parse("p-w-2-4"),
        Some(Label::Popout { host: "w-2", n: 4 })
    );
    for not in [
        "",
        "x",
        "main2",
        "w-",
        "w-x",
        "w-+1",
        "w-1-2",
        "d-",
        "d-main",
        "d-main-x",
        "d-main-",
        "d-d-main-1-2",
        "d-p-main-1-2",
        "p-p-main-1-2",
        "p-d-main-1-2",
        "d-foo-1",
        "x-main-1",
        "browser-main",
        "w-99999999999",
        "tray-",
        "tray-CPU",
        "tray--x",
        "tray-a b",
        "tray-../x",
        "trayx",
        "d-tray-1",
        "p-tray-1",
        &format!("tray-{}", "a".repeat(33)),
    ] {
        assert_eq!(Label::parse(not), None, "{not}");
    }
    assert_eq!(Label::parse("tray"), Some(Label::Tray { item: None }));
    assert_eq!(
        Label::parse("tray-cpu-1"),
        Some(Label::Tray {
            item: Some("cpu-1")
        })
    );
    assert_eq!(class_of("tray"), Some(WindowClass::Tray));
    assert_eq!(class_of("tray-x"), Some(WindowClass::Tray));
    assert_eq!(resolve("tray"), "tray", "a popover belongs to no window");
    assert!(crate::labels::valid_tray_item(&"a".repeat(32)));
    for label in [
        "main", "w-3", "d-main-1", "d-w-2-13", "p-main-1", "p-w-2-4", "tray", "tray-cpu",
    ] {
        assert_eq!(
            Label::parse(label).map(|l| l.to_string()).as_deref(),
            Some(label)
        );
    }
    assert_eq!(class_of("main"), Some(WindowClass::Main));
    assert_eq!(class_of("w-3"), Some(WindowClass::Main));
    assert_eq!(class_of("p-main-1"), Some(WindowClass::Popout));
    assert_eq!(class_of("d-main-1"), Some(WindowClass::Dialog));
    assert_eq!(class_of("bw-main"), None);
    assert_eq!(resolve("d-w-2-4"), "w-2");
    assert_eq!(resolve("p-main-1"), "main");
    assert_eq!(resolve("w-2"), "w-2");
    assert_eq!(resolve("browser-main"), "browser-main");
    assert_eq!(Label::Main.host(), None);
    assert_eq!(
        WindowClass::ALL.map(WindowClass::as_str),
        ["main", "popout", "dialog", "tray"]
    );
}

#[test]
fn windows_get_their_own_set_and_requests_are_bounded() {
    // A dialog can call the base commands and its own, nothing else of the table (ADR 0009).
    assert!(run("dialog_take_input", "d-main-1", Some(&ABOUT), 10).is_ok());
    assert!(run("legal_text", "d-main-1", Some(&ABOUT), 10).is_ok());
    assert_eq!(
        run("query_page", "d-main-1", Some(&ABOUT), 10)
            .unwrap_err()
            .code,
        RefusalCode::Denied
    );
    assert!(run("query_page", "d-main-1", Some(&EXPORT), 10).is_ok());
    assert!(
        run("ui_sync_theme", "d-w-2-1", Some(&EXPORT), 10).is_ok(),
        "a prefix in a contract"
    );
    // A dialog window the dialog layer does not know may call nothing.
    let e = run("dialog_take_input", "d-main-1", None, 10).unwrap_err();
    assert_eq!((e.code, e.dialog.as_deref()), (RefusalCode::Denied, None));
    assert_eq!(
        e.message,
        "dialog_take_input is not available in an unknown dialog"
    );
    // A dialog cannot move the session or restart the app, whatever its contract.
    let anything = Grant {
        id: "any",
        commands: &["*"],
    };
    assert_eq!(
        run("save_session", "d-main-1", Some(&anything), 10)
            .unwrap_err()
            .code,
        RefusalCode::Denied
    );
    assert!(
        run("ui_sync_layout", "d-main-1", Some(&anything), 10).is_err(),
        "not open to dialogs"
    );
    assert!(
        run("dialog_close", "main", None, 10).is_err()
            && run("dialog_close", "d-main-1", Some(&ABOUT), 10).is_ok()
    );
    assert!(
        run("dialog_fit_height", "main", None, 10).is_err()
            && run("dialog_fit_height", "d-main-1", Some(&ABOUT), 10).is_ok()
    );
    assert!(run("query_page", "p-main-1", None, 10).is_ok());
    assert!(run("ui_sync_layout", "p-w-2-1", None, 10).is_ok());
    // A pop-out panel cannot restart the app or answer its host's prompts.
    assert_eq!(
        run("restart_app", "p-main-1", None, 10).unwrap_err().code,
        RefusalCode::Denied
    );
    assert!(
        run("restart_app", "main", None, 10).is_ok() && run("restart_app", "w-2", None, 10).is_ok()
    );
    // A webview outside the label scheme (a page, a plugin view) may call nothing of the table.
    for s in COMMANDS.specs() {
        let e = run(s.name(), "browser-main", None, 10).unwrap_err();
        assert_eq!(e.code, RefusalCode::Denied, "{}", s.name());
    }
    assert!(run("query_page", "unknown-window", None, 10).is_err());
    assert!(
        run("plugin:log|log", "browser-main", None, usize::MAX).is_ok(),
        "the capabilities govern plugins"
    );
    assert!(
        run("no_such_command", "main", None, usize::MAX).is_ok(),
        "Tauri answers unknown commands"
    );
    let e = run("query_page", "main", None, DEFAULT_LIMIT + 1).unwrap_err();
    assert_eq!(e.code, RefusalCode::Limit);
    assert_eq!(
        e.message,
        format!(
            "the request to query_page is too large ({} bytes; at most {DEFAULT_LIMIT})",
            DEFAULT_LIMIT + 1
        )
    );
    assert!(run("query_page", "main", None, DEFAULT_LIMIT).is_ok());
    assert!(
        run("save_session", "main", None, DEFAULT_LIMIT + 1).is_ok(),
        "the session carries the UI state"
    );
    let e = run("query_page", "d-main-1", Some(&EXPORT), DEFAULT_LIMIT + 1).unwrap_err();
    assert_eq!(
        (e.code, e.dialog.as_deref()),
        (RefusalCode::Limit, Some("export"))
    );
    assert!(!e.breaches_contract(), "too large is no contract breach");
}

#[test]
fn a_dialog_calling_an_undeclared_command_is_logged_as_an_error_naming_the_dialog_and_the_command()
{
    let e = run("query_page", "d-main-1", Some(&ABOUT), 10).unwrap_err();
    assert!(e.breaches_contract());
    assert_eq!(e.dialog.as_deref(), Some("about"));
    let line = e.log_message();
    assert!(
        line.contains("about") && line.contains("query_page"),
        "{line}"
    );
    // UIs recognise the refusal by this wording (Debrief's `core/denied-command.ts`).
    assert!(
        e.message.starts_with("query_page is not available in "),
        "{}",
        e.message
    );
    // A refusal outside a dialog (or for size) stays a warning.
    let main = run("dialog_close", "main", None, 10).unwrap_err();
    assert!(!main.breaches_contract());
    assert_eq!(
        main.log_message(),
        "A command was refused: dialog_close is not available in this window"
    );
    let unknown = run("dialog_close", "d-main-1", None, 10).unwrap_err();
    assert!(
        !unknown.breaches_contract(),
        "an unknown dialog window is a warning"
    );
}

#[test]
fn a_refusal_has_the_shape_of_a_command_error_on_the_wire() {
    let e = run("query_page", "d-main-1", Some(&ABOUT), 10).unwrap_err();
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "code": "denied",
            "message": "query_page is not available in the about dialog (its contract does not declare it)",
            "retryable": false,
            "command": "query_page",
            "dialog": "about",
        })
    );
    let e = run("query_page", "main", None, DEFAULT_LIMIT + 1).unwrap_err();
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["code"], "limit");
    assert!(v.get("dialog").is_none());
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// The dialog contracts and the command table agree: every command a dialog may call exists and is open to dialogs,
/// and none is one that must stay with the main window.
#[test]
fn dialog_contracts_name_only_commands_the_table_opens_to_dialogs() {
    const HOST_ONLY: &[&str] = &["resolve_close", "save_session"];
    let base = strings(&["dialog_close", "dialog_take_input"]);
    let about = strings(&["legal_text"]);
    let export = strings(&["query_page", "ui_sync_t*"]);
    assert!(
        COMMANDS
            .contract_problems(
                &base,
                [("about", &about[..]), ("export", &export[..])],
                HOST_ONLY
            )
            .is_empty()
    );
    assert!(
        COMMANDS.opens_to_dialogs("dialog_close") && !COMMANDS.opens_to_dialogs("resolve_close")
    );
    assert!(!COMMANDS.opens_to_dialogs("no_such_command"));

    let bad_base = strings(&["dialog_close", "restart_app"]);
    let missing = strings(&["no_such_command"]);
    let closed = strings(&["ui_sync_layout"]);
    let host = strings(&["resolve_close"]);
    let nothing = strings(&["nope_*"]);
    let wide = strings(&["ui_*"]);
    let host_prefix = strings(&["resolve_*"]);
    let problems = COMMANDS.contract_problems(
        &bad_base,
        [
            ("missing", &missing[..]),
            ("closed", &closed[..]),
            ("host", &host[..]),
            ("nothing", &nothing[..]),
            ("wide", &wide[..]),
            ("host-prefix", &host_prefix[..]),
        ],
        &["resolve_close"],
    );
    assert_eq!(
        problems,
        [
            "base declares restart_app, which the table does not open to dialogs",
            "missing declares no_such_command, which is not in the command table",
            "closed declares ui_sync_layout, which the table does not open to dialogs",
            "host declares resolve_close, which the table does not open to dialogs",
            "host declares resolve_close: that stays with the host window",
            "nothing: nope_* matches no command",
            "wide: ui_* matches ui_sync_layout, which the table does not open to dialogs",
            "host-prefix: resolve_* matches resolve_close, which the table does not open to dialogs",
            "host-prefix: resolve_* matches resolve_close, which stays with the host window",
        ]
    );
}

#[test]
fn the_names_module_lists_every_command_sorted_with_its_kind_windows_and_limit() {
    crate::command_table! {
        static SMALL;
        Mutation [MAIN | DIALOG] => b::open_paths,
        Quiet [POPOUT] limit 2 * MIB => a::autosave,
    }
    let text = ts::names_module(&SMALL, "APP_COMMANDS", "AppCommand", "run `cargo test`").unwrap();
    assert_eq!(
        text,
        "// Generated from the command table by paddock-commands. Do not edit: run `cargo test`.\n\
         \n\
         /** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */\n\
         export const APP_COMMANDS = {\n  \
           autosave: { kind: 'quiet', windows: ['popout'], limit: 2097152 },\n  \
           open_paths: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },\n\
         } as const;\n\
         \n\
         /** A command name of the table. */\n\
         export type AppCommand = keyof typeof APP_COMMANDS;\n"
    );
    let empty = ts::names_module(&Table::EMPTY, "SHELL_COMMANDS", "ShellCommand", "x").unwrap();
    assert!(
        empty.contains("export const SHELL_COMMANDS = {} as const;\n"),
        "{empty}"
    );
    for bad in ["", "1x", "a-b", "a b"] {
        let e = ts::names_module(&SMALL, bad, "T", "x").unwrap_err();
        assert_eq!(e, ts::InvalidIdentifier(bad.to_string()));
        assert!(e.to_string().contains("not a TypeScript identifier"));
        assert!(ts::names_module(&SMALL, "C", bad, "x").is_err());
    }
    assert!(ts::names_module(&SMALL, "$c_1", "_T", "x").is_ok());
}

/// Where the shell's generated TypeScript goes: the `shell` entry point of `@decryp7/paddock-ui`.
fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/generated")
}

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-commands --lib generated";

/// The generated file of this crate: the wire types (ts-rs). The shell's command names (`shell-commands.ts`) are the
/// shell plugin's table and written by `paddock-shell`.
fn generated_files() -> Vec<(&'static str, String)> {
    use ts_rs::TS;
    let header = format!(
        "// Generated from the Rust wire types of paddock-commands (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        crate::Kind::decl(),
        crate::WindowClass::decl(),
        crate::CommandInfo::decl(),
        crate::RefusalCode::decl(),
        crate::Refusal::decl(),
    ];
    let types = decls
        .into_iter()
        .map(|d| format!("export {d}\n"))
        .collect::<Vec<_>>()
        .join("\n");
    // The IPC kit's types (`@decryp7/paddock-ui/ipc`, plan X28): the error envelope and the runtime list of its codes.
    let ipc_header = format!(
        "// Generated from the Rust wire types of paddock-commands (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let ipc_types = [ErrorCode::decl(), CommandError::decl()]
        .into_iter()
        .map(|d| format!("export {d}\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let codes = ErrorCode::ALL
        .iter()
        .map(|c| format!("'{}'", c.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let ipc = format!(
        "{ipc_header}{ipc_types}\n/** Every error code, for validating what arrives over the wire. */\nexport const ERROR_CODES: readonly ErrorCode[] = [{codes}];\n"
    );
    vec![
        ("commands.ts", format!("{header}{types}")),
        ("../../ipc/generated/ipc.ts", ipc),
    ]
}

#[test]
fn generated_typescript_is_current() {
    let mode = WriteMode::from_env();
    let stale: Vec<String> = generated_files()
        .into_iter()
        .filter_map(|(name, text)| {
            testing::sync_generated(&generated_dir().join(name), &text, mode).err()
        })
        .map(|e| e.to_string())
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn a_stale_generated_file_fails_and_write_mode_rewrites_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/names.ts");
    let e = testing::sync_generated(&path, "a\n", WriteMode::Check).unwrap_err();
    assert!(matches!(&e, GeneratedError::Stale { path: p } if *p == path));
    assert!(e.to_string().contains("PADDOCK_WRITE_TS=1"), "{e}");
    assert_eq!(
        testing::sync_generated(&path, "a\nb\n", WriteMode::Write).unwrap(),
        Synced::Written
    );
    assert_eq!(
        testing::sync_generated(&path, "a\nb\n", WriteMode::Check).unwrap(),
        Synced::Current
    );
    std::fs::write(&path, "a\r\nb\r\n").unwrap();
    assert_eq!(
        testing::sync_generated(&path, "a\nb\n", WriteMode::Check).unwrap(),
        Synced::Current,
        "CRLF is not stale"
    );
    assert!(testing::sync_generated(&path, "a\nc\n", WriteMode::Check).is_err());
    // A path under a file cannot be written.
    let blocked = path.join("x.ts");
    let e = testing::sync_generated(&blocked, "a", WriteMode::Write).unwrap_err();
    assert!(
        matches!(e, GeneratedError::Io { .. }) && e.to_string().starts_with("cannot write "),
        "{e}"
    );
    let readonly_dir = dir.path().join("ro.ts");
    std::fs::create_dir(&readonly_dir).unwrap();
    let e = testing::sync_generated(&readonly_dir, "a", WriteMode::Write).unwrap_err();
    assert!(
        matches!(e, GeneratedError::Io { .. }),
        "writing over a folder fails"
    );
}

#[test]
fn the_write_mode_comes_from_the_environment() {
    use std::ffi::OsString;
    assert_eq!(WriteMode::from_var(None), WriteMode::Check);
    assert_eq!(WriteMode::from_var(Some(OsString::new())), WriteMode::Check);
    assert_eq!(WriteMode::from_var(Some("0".into())), WriteMode::Check);
    assert_eq!(WriteMode::from_var(Some("1".into())), WriteMode::Write);
    assert_eq!(testing::WRITE_VAR, "PADDOCK_WRITE_TS");
}

/// A source tree with one sync command among async ones.
fn planted() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    let async_ones = [
        "#[tauri::command]",
        "pub async fn open_paths() {}",
        "",
        "#[::tauri::command(rename_all = \"snake_case\")]",
        "/// Docs between the attribute and the fn.",
        "#[allow(clippy::too_many_arguments)]",
        "pub(crate) async fn query_page<R: Runtime>(app: AppHandle<R>) {}",
        "#[command]",
        "async fn legal_text() {}",
        "#[commander]",
        "fn not_a_command() {}",
        "#[tauri::command]",
        "// no fn follows",
    ]
    .join("\n");
    std::fs::write(dir.path().join("src/lib.rs"), async_ones).unwrap();
    std::fs::write(
        dir.path().join("src/nested/sync.rs"),
        [
            "",
            "#[tauri::command]",
            "pub fn save_session(state: State<'_, S>) {}",
        ]
        .join("\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/nested/notes.txt"),
        "#[tauri::command]\nfn ignored() {}",
    )
    .unwrap();
    dir
}

#[test]
fn a_sync_command_fails_the_async_check() {
    let dir = planted();
    let src = dir.path().join("src");
    let found = testing::scan(&[&src]).unwrap();
    let names: Vec<(&str, bool)> = found
        .iter()
        .map(|c| (c.name.as_str(), c.is_async))
        .collect();
    assert_eq!(
        names,
        [
            ("open_paths", true),
            ("query_page", true),
            ("legal_text", true),
            ("save_session", false)
        ]
    );
    assert_eq!(found[1].line, 7);
    crate::command_table! {
        static PLANTED;
        Mutation [WINDOW] => open_paths,
        Query [WINDOW] => query_page,
        Query [WINDOW] => legal_text,
        Quiet [MAIN] => save_session,
        Query [MAIN] => elsewhere,
    }
    let problems = testing::async_problems(&PLANTED, &found);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0].ends_with("sync.rs:3: save_session is a sync command (make it async)"),
        "{}",
        problems[0]
    );
    assert_eq!(
        problems[1],
        "elsewhere (elsewhere) is in the table but no command of that name was found"
    );
    let caught = std::panic::catch_unwind(|| testing::assert_async(&PLANTED, &[&src])).unwrap_err();
    let message = caught.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.contains("save_session is a sync command"),
        "{message}"
    );

    // Made async (and the stray entry dropped), the same tree passes.
    std::fs::write(
        src.join("nested/sync.rs"),
        "#[tauri::command]\npub async fn save_session() {}",
    )
    .unwrap();
    // A block of its own: each leaf table emits a `dialog_callable` module, one per scope.
    let fixed: &crate::Table = {
        crate::command_table! {
            static FIXED;
            Mutation [WINDOW] => open_paths,
            Quiet [MAIN] => save_session,
        }
        &FIXED
    };
    testing::assert_async(fixed, &[&src]);
    assert!(
        std::panic::catch_unwind(|| testing::assert_async(fixed, &[&dir.path().join("missing")]))
            .is_err()
    );
}

#[test]
fn commands_are_found_with_their_line() {
    let found = testing::commands_in(
        Path::new("x.rs"),
        "#[tauri::command]\nasync fn a() {}\n#[tauri::command]\nfn b<T>() {}",
    );
    assert_eq!(
        found,
        [
            FoundCommand {
                name: "a".into(),
                is_async: true,
                file: "x.rs".into(),
                line: 2
            },
            FoundCommand {
                name: "b".into(),
                is_async: false,
                file: "x.rs".into(),
                line: 4
            },
        ]
    );
    assert!(
        testing::commands_in(Path::new("x.rs"), "#[tauri::command]\nfn (").is_empty(),
        "no name"
    );
}

// Each leaf table emits its own `dialog_callable` module: one table per module.
mod tray_table {
    crate::command_table! {
        /// A table with commands for the tray popover.
        pub(super) static TRAY_COMMANDS;
        Query [WINDOW] => status,
        Mutation [WINDOW | TRAY] => window_focus,
        Quiet [TRAY] => tray_fit_height,
    }
}
use tray_table::TRAY_COMMANDS;

#[test]
fn a_tray_popover_calls_only_the_commands_that_name_its_class() {
    let call = |command: &str, label: &str| check(&TRAY_COMMANDS, command, label, None, 10);
    assert!(call("window_focus", "tray").is_ok());
    assert!(call("tray_fit_height", "tray-cpu").is_ok());
    let refused = call("status", "tray").unwrap_err();
    assert_eq!(refused.code, RefusalCode::Denied);
    assert!(
        call("tray_fit_height", "main").is_err(),
        "the popover's own"
    );
    assert!(
        call("tray_fit_height", "tray-CPU").is_err(),
        "outside the scheme"
    );
    assert_eq!(
        TRAY_COMMANDS.spec("window_focus").map(|s| s.info().windows),
        Some(WindowClass::ALL.to_vec())
    );
}

mod part_a {
    crate::command_table! {
        pub(super) static PART_A;
        Query [WINDOW] => a::first,
        Mutation [MAIN] limit 2 * crate::MIB => a::second,
    }
}
use part_a::PART_A;

mod part_b {
    crate::command_table! {
        pub(super) static PART_B;
        Quiet [TRAY] => b::third,
    }
}
use part_b::PART_B;

crate::command_table! {
    /// Both parts.
    static COMPOSED = [PART_A, PART_B,];
}

crate::command_table! {
    static NOTHING = [];
}

#[test]
fn a_composed_table_lists_every_part_in_order_with_its_specs() {
    let names: Vec<&str> = COMPOSED.specs().iter().map(|s| s.name()).collect();
    assert_eq!(names, ["first", "second", "third"]);
    assert_eq!(
        COMPOSED.spec("second").map(|s| s.limit()),
        Some(2 * crate::MIB)
    );
    assert_eq!(COMPOSED.kind_of("third"), Kind::Quiet);
    assert_eq!(
        COMPOSED.spec("third").map(|s| s.windows()),
        Some(Windows::TRAY)
    );
    assert!(NOTHING.specs().is_empty());
    assert_eq!(crate::table::total(&[&PART_A, &PART_B]), 3);
    // The same at run time.
    let specs = crate::table::concat::<3>(&[&PART_A, &PART_B]);
    assert_eq!(specs.map(|s| s.name()), ["first", "second", "third"]);
}

#[test]
#[should_panic(expected = "composed length")]
fn a_composed_table_of_the_wrong_length_is_refused() {
    let _ = crate::table::concat::<2>(&[&PART_A, &PART_B]);
}

// ---- the error envelope (plan X28) ----

#[test]
fn an_error_serialises_to_the_wire_shape_and_skips_what_it_does_not_have() {
    let plain = serde_json::to_value(CommandError::invalid(
        "The port must be between 1 and 65535.",
    ))
    .unwrap();
    assert_eq!(
        plain,
        serde_json::json!({"code": "invalid", "message": "The port must be between 1 and 65535.", "retryable": false})
    );
    let full = CommandError::unavailable("Pitwall is not running.")
        .hint("Start it from the tray.")
        .field("engine")
        .cause("health:pitwall")
        .kind("noEngine")
        .retry(false);
    assert_eq!(
        serde_json::to_value(&full).unwrap(),
        serde_json::json!({
            "code": "unavailable", "message": "Pitwall is not running.", "retryable": false,
            "hint": "Start it from the tray.", "field": "engine", "cause": "health:pitwall", "kind": "noEngine"
        })
    );
    assert_eq!(full.to_string(), "Pitwall is not running.");
}

#[test]
fn every_code_has_a_constructor_a_wire_name_and_a_retry_default() {
    let all: Vec<CommandError> = vec![
        CommandError::failed("m"),
        CommandError::invalid("m"),
        CommandError::not_found("m"),
        CommandError::not_granted("m"),
        CommandError::busy("m"),
        CommandError::cancelled("m"),
        CommandError::io("m"),
        CommandError::limit("m"),
        CommandError::unsupported("m"),
        CommandError::unavailable("m"),
        CommandError::denied("m"),
        CommandError::missing("m"),
        CommandError::timeout("m"),
        CommandError::internal("m"),
    ];
    let codes: Vec<ErrorCode> = all.iter().map(|e| e.code).collect();
    assert_eq!(codes, ErrorCode::ALL, "the constructors follow the list");
    for e in &all {
        let wire = serde_json::to_value(e.code).unwrap();
        assert_eq!(wire, e.code.as_str(), "as_str is the serde name");
        assert_eq!(e.retryable, e.code.retryable_by_default());
    }
    let retryable: Vec<&str> = all
        .iter()
        .filter(|e| e.retryable)
        .map(|e| e.code.as_str())
        .collect();
    assert_eq!(retryable, ["busy", "io", "unavailable", "timeout"]);
}

#[test]
fn foreign_errors_convert_at_the_edge() {
    use std::io::{Error, ErrorKind};
    assert_eq!(
        CommandError::from(Error::from(ErrorKind::NotFound)).code,
        ErrorCode::NotFound
    );
    assert_eq!(
        CommandError::from(Error::from(ErrorKind::PermissionDenied)).code,
        ErrorCode::Denied
    );
    let other = CommandError::from(Error::other("disk on fire"));
    assert_eq!(
        (other.code, other.message.as_str(), other.retryable),
        (ErrorCode::Io, "disk on fire", true)
    );
    assert_eq!(CommandError::from("old helper").code, ErrorCode::Failed);
    assert_eq!(
        CommandError::from(String::from("old helper")).message,
        "old helper"
    );
    let denied = run("query_page", "main", None, DEFAULT_LIMIT + 1).unwrap_err();
    let e = CommandError::from(denied);
    assert_eq!((e.code, e.retryable), (ErrorCode::Limit, false));
    let refused = run("dialog_close", "main", None, 0).unwrap_err();
    assert_eq!(CommandError::from(refused).code, ErrorCode::Denied);
}

mod marked {
    crate::command_table! {
        pub(super) static MARKED;
        Query [MAIN | DIALOG] => app::app_status,
        Query [WINDOW] => app::app_legal,
        Mutation [MAIN] => app::app_quit,
        Quiet [TRAY | DIALOG] => app::tray_fit,
        Quiet [POPOUT] => app::popout_only,
    }
}

#[test]
fn a_leaf_table_marks_exactly_the_commands_open_to_dialogs() {
    use crate::{DialogCallable, plugin, prefix};
    assert_eq!(marked::MARKED.specs().len(), 5);
    assert_eq!(
        marked::dialog_callable::ALL,
        ["app_status", "app_legal", "tray_fit"]
    );
    assert_eq!(
        marked::dialog_callable::app_legal,
        DialogCallable::__command("app_legal")
    );
    assert_eq!(marked::dialog_callable::tray_fit.wire(), "tray_fit");
    assert_eq!(prefix(marked::dialog_callable::ALL, "app_").wire(), "app_*");
    assert_eq!(plugin("dialog", "open").wire(), "plugin:dialog|open");
    for (prefix_text, why) in [
        ("", "empty"),
        ("Bad", "not snake"),
        ("zzz_", "matches nothing"),
    ] {
        let r = std::panic::catch_unwind(|| prefix(marked::dialog_callable::ALL, prefix_text));
        assert!(r.is_err(), "{why}");
    }
    assert!(std::panic::catch_unwind(|| plugin("", "open")).is_err());
    // A prefix matches a name only from its start, and never one shorter than itself.
    assert!(std::panic::catch_unwind(|| prefix(&["ab"], "abc")).is_err());
    assert!(std::panic::catch_unwind(|| prefix(&["xab"], "ab")).is_err());
}

#[test]
fn the_dialog_helpers_also_work_at_run_time() {
    use crate::DialogCallable;
    use crate::table::{dialog_count, dialog_names};
    let specs = [
        spec("a :: one", Windows::MAIN, DEFAULT_LIMIT),
        spec("a :: two", Windows::DIALOG, DEFAULT_LIMIT),
        spec("a :: three", Windows::MAIN.with(Windows::DIALOG), DEFAULT_LIMIT),
    ];
    assert_eq!(dialog_count(&specs), 2);
    assert_eq!(dialog_names::<2>(&specs), ["two", "three"]);
    assert_eq!(dialog_count(&[]), 0);
    assert_eq!(DialogCallable::__prefix("app_").wire(), "app_*");
}
