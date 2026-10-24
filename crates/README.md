# Shared Rust crates

A Cargo workspace for Rust code the apps share. Members: `paddock-jobs` (ADR 0008) and the app shell crates of ADR 0012
(`paddock-commands`, `-dialogs`, `-download`, `-health`, `-menu`, `-settings`, `-startup`, `-updater`, `-window`; scaffolds
until their work packages in [docs/plans/workbench.md](../docs/plans/workbench.md) land). Crates are extracted from the apps one
at a time (a crate moves here when a second app needs it, rule of three for new abstractions, two for copied code).

Rules for a crate here:

- Pure logic and thin adapters, no dependency on any app crate. Edition and licence come from `[workspace.package]`.
- Library errors are typed, no `unwrap`/`expect` on user data or I/O, `cargo clippy -- -D warnings` clean,
  >= 99% line coverage.
- A pure core with no Tauri dependency, and a `tauri` feature for the adapter (`src/tauri.rs`, `pub mod tauri` behind
  `#[cfg(feature = "tauri")]`). Tauri comes from `[workspace.dependencies]` without default features (the app enables
  the runtime); CI builds every crate with `--all-features` on Linux, Windows and macOS.
- A crate that uses another one writes `paddock-jobs = { workspace = true }` (the workspace dependency carries path,
  version and registry, so the published crate points at the registry).
- `publish = ["decryp7"]` in every crate. A new crate starts with `[package.metadata.paddock] scaffold = true`, which keeps
  the publish job from uploading it; its work package removes the marker. A new member also needs its step in the `crate`
  job of `.gitea/workflows/ci.yml` (`scripts/publish.test.mjs` fails without it).
- Published to the Gitea cargo registry by the tag workflow only (`.gitea/workflows/publish.yml`, rule 9). One repo tag
  `vX.Y.Z` releases everything; each crate bumps its own `version` only when it changes, and a version the registry
  already has is skipped. `node scripts/publish.mjs` is the local dry run (lists every crate and npm package with what a
  tag would publish).

Apps consume the crates from the registry. In the app's `.cargo/config.toml`:

```toml
[registries.decryp7]
index = "sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"
```

and in its `Cargo.toml` (pre-1.0: `0.N` accepts patches of that minor only), with `Cargo.lock` committed:

```toml
paddock-jobs = { version = "0.1", registry = "decryp7", features = ["tauri"] }
```

The app's `deny.toml` allows the registry: `allow-registry = ["https://github.com/rust-lang/crates.io-index",
"sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"]`.

Crates, built and planned, with their designs in [`docs/adr/`](../docs/adr/README.md):

| Crate(s) | Purpose | Three users today? |
|---|---|---|
| `paddock-jobs` | long operations: job/phase/progress model, throttled reporter, cancel token, hash-while-writing, parallel walk/map/pipeline, job pool (ADR 0008) | yes (all five apps); built |
| `paddock-commands`, `-health`, `-dialogs`, `-startup`, `-download`, `-settings`, `-updater`, `-window`, `-menu`, `-shell` | the app shell (ADR 0012) | yes; `paddock-commands` (S1), `paddock-health` (S3), `paddock-dialogs` (S5), `paddock-startup` (S7), `paddock-menu` (S13) and `paddock-window` (S12) built and the `paddock-jobs` adapter (S9) done, the rest scaffolds, plan S2-S17 |
| `paddock-pitwall` | Pitwall client: discovery, pairing, keychain keys, typed requests, optional-AI state (ADR 0004) | yes (Debrief, Pacenotes, Sighter; Spotter, Dyno later); built first |
| `paddock-ipc`, `-guard`, `-view`, `-plugin-api`, `-plugin-host`, `-plugin-package`, `-plugin-test` | the out-of-process plugin platform (ADRs 0001 to 0003) | yes (Debrief, Pacenotes, Dyno tier 3) |
| `paddock-mcp` | MCP protocol, tool schemas, limits, paging, audit log, local transports (ADR 0007) | three (Pacenotes, Sighter, Debrief); extracted with Debrief's server, after Debrief 1.0.2 |
| `paddock-testplan` | `.sighterplan` / `.sighterresult` (ADR 0005) | two (Pacenotes, Sighter); shared schema, so extracted on purpose |
| `paddock-env` | per-OS data/config dirs, portable mode | yes |
| `paddock-service` | background service install/control per OS | yes |
| `paddock-proc` | child processes with timeouts, caps, kill-tree | yes |
| `paddock-window` | native window feel, tray, single instance | yes |
| `paddock-notify` | OS notifications | borderline (two to three) |
| `chromium-driver` | CDP driver (today in Debrief) | two to three (Debrief, Spotter, Sighter) |
| `paddock-cli` | shared CLI conventions (`--json`, exit codes, timeouts) | yes |

## `paddock-jobs` Tauri adapter (plan S9)

With the `tauri` feature, `paddock_jobs::tauri` is the jobs part of the shell (ADR 0008 open question 2, ADR 0012 §3):

```rust
tauri::Builder::default()
    .setup(|app| {
        let jobs = paddock_jobs::tauri::manage(app.handle()); // Arc<JobRegistry>; snapshots go out on `paddock://jobs`
        app.manage(AppState { jobs, pool: JobPool::new(Workers::auto()) });
        Ok(())
    })
    .invoke_handler(paddock_jobs::tauri::route(
        paddock_jobs::tauri::handler(),                    // or handler_with_grants(lookup): dialogs by their contracts
        paddock_commands::tauri::Guard::new(&COMMANDS).wrap(handler!()),
    ))
```

- `Emit` is the `EventSink` (event `EVENT` = `paddock://jobs`, every snapshot to every webview); `Jobs` is the managed state.
- `COMMANDS` (a `command_table!`): `jobs_snapshot` (query: every known job, ordered by id) and `job_cancel { id }`
  (mutation: false for an unknown or ended job). Both are async, return at once and never wait for a job; an app without
  the state gets `{ code: 'notSetUp', message, retryable: false }`. Starting work stays with the app: its command registers
  a job, submits it to its `JobPool` and returns the id.
- `route(jobs, other)` sends the table's commands to the jobs handler and the rest to the app's, so the app keeps one
  `invoke_handler` until the `paddock-shell` plugin (S17) takes the commands over.
- Generated TypeScript in `ui/projects/paddock-ui/shell/jobs/generated/` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-jobs
  --features tauri --test tauri generated`): `jobs-commands.ts` (`JOBS_COMMANDS`, `JobsCommand`) and `jobs-fixtures.ts`
  (one snapshot of every shape, written by the real reporter, decoded by the UI's specs).

## `paddock-commands` (plan S1)

The command table of an app or a shell plugin, and its guard (ADR 0012 §3, ported from Debrief ADR 0016 and 0054):

```rust
paddock_commands::command_table! {
    pub(crate) static COMMANDS, macro handler;       // the validated `Table`, and `handler!()` = `::tauri::generate_handler![...]`
    Query [WINDOW] => workspace::summary,           // Kind: Query | Mutation | Quiet (how it is logged)
    Mutation [MAIN | DIALOG] limit 4 * MIB => workspace::open_paths,
    Quiet [MAIN] => session::save_session,          // classes: MAIN (`main`, `w-<n>`), POPOUT (`p-<host>-<n>`), DIALOG (`d-<host>-<n>`), WINDOW (all)
}
tauri::Builder::default().invoke_handler(
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(|app, label| dialogs_of(app).grant_of(label))  // impl DialogGrant: the per-dialog contract (ADR 0009)
        .wrap(handler!()),
)
```

- The table is validated at compile time: a duplicate name, a command callable from no class, a name that is not
  `lower_snake_case` (1-64) or a limit outside 1 KiB-64 MiB does not compile (default limit 256 KiB).
- The guard refuses, before the arguments are parsed: a class the entry does not name, a webview outside the label
  scheme, a dialog without a grant or whose grant does not cover the command, a request over the limit. The refusal is
  the command's error (`{ code: 'denied' | 'limit', message, retryable: false, command, dialog? }`) and is logged with
  target `paddock_commands` (keys `Command`, `Window`, `Dialog`, `ErrorCode`): an error naming the dialog and the
  command for a contract breach, else a warning. Plugin commands (`plugin:...`) and unknown commands pass.
- `Table::kind_of` classifies a command for the activity log; `Table::contract_problems(base, dialogs, host_only)`
  checks dialog contracts against the table; `labels` is the shell's window label scheme (`Label`, `class_of`,
  `resolve`).
- Tray popovers (`tray`, `tray-<item>`) are their own class `TRAY` (plan S12): not part of `WINDOW`, so a popover calls
  only the commands that name it (`[WINDOW | TRAY]`), it hosts no dialogs, and the start-up watchdog does not watch it.
- In the owning crate's tests: `paddock_commands::testing::assert_async(&COMMANDS, &[src])` fails on any sync
  `#[tauri::command]` and on table entries it cannot find; `testing::sync_generated(path, text, WriteMode::from_env())`
  fails on a stale generated file (`PADDOCK_WRITE_TS=1` rewrites it).

**Generated TypeScript command names** (`paddock_commands::ts::names_module(&TABLE, "APP_COMMANDS", "AppCommand", how)`),
one file per table, written by a test of the crate that owns the table and checked on every run, so a stale file fails
CI:

```ts
// Generated from the command table by paddock-commands. Do not edit: <how to regenerate>.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const APP_COMMANDS = {
  open_paths: { kind: 'mutation', windows: ['main', 'dialog'], limit: 4194304 },
  summary: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AppCommand = keyof typeof APP_COMMANDS;
```

Entries are sorted by name; an empty table is `{} as const` (name type `never`). The shell's files live in
`ui/projects/paddock-ui/shell/generated/` (not yet re-exported; the `shell` entry point decides that):

| File | Content | Written by |
|---|---|---|
| `commands.ts` | the wire types (ts-rs): `CommandKind`, `WindowClass`, `CommandInfo`, `RefusalCode`, `CommandRefusal` | `paddock-commands` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-commands --lib generated`) |
| `shell-commands.ts` | `SHELL_COMMANDS` and `ShellCommand`, the shell plugin's command names | `paddock-commands` with an empty table until `paddock-shell` (S17) takes it over with its own table and test |
| `health.ts` | the events (`SERVICE_HEALTH_EVENT`, `NOTIFICATIONS_EVENT`, `NOTIFICATIONS_OPEN_EVENT`), `healthCause`, and the wire types (ts-rs) of service health, notifications and `HealthError` | `paddock-health` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-health --all-features --lib generated`) |
| `health-commands.ts` | `HEALTH_COMMANDS` and `HealthCommand`, the health and notification commands | `paddock-health` (same test) |

An app writes its own file the same way into its UI (for example `src/app/core/generated/app-commands.ts`); dialog
contracts and the contributions core type command names as `AppCommand | ShellCommand`.

## `paddock-dialogs` (plan S5)

Dialogs as real OS windows (ADR 0009, rule 17), ported from Debrief's `dialog_contracts.rs`, `dialog_placement.rs` and
`dialog_windows.rs` (Debrief ADRs 0050, 0054) without Debrief's dialog ids:

- `Contracts::parse(json)` reads the app's generated contract file once (written by `dialogContractFile` in
  `@decryp7/paddock-ui/shell/dialogs`; dialog ids are the shell's namespaced ids, `[a-z0-9.-]`) (version 1; `base` and per-dialog `kind`,
  `modality`, `requiredChoice`, `instanced`, `minSize`, `commands` with `prefix*`) and refuses one that contradicts
  itself (a modeless decision, a modal viewer or tool, a modeless required choice, bad ids or names, limits).
  `Contracts::allows(dialog, command)` covers `BASE_COMMANDS` (`dialog_take_input`, `dialog_close`, `dialog_fit_height`),
  the app's base set and the dialog's own list; `Contracts::problems(&TABLE, host_only)` checks them against the command
  table (run it in a test). `Kind::policy()` is the size rule per kind (viewer floor 680x560 and grows only, tool
  default 960x680, decision and form fitted to their content).
- `placement`: pure geometry in physical px. `monitor_of` picks the parent's monitor (most overlap, else nearest; never
  the primary display as such); `place` sizes and places on that monitor's work area (remembered offset from the parent
  only when the whole dialog stays there, else centred on the parent and moved inside; shrunk to fit; mixed DPI and
  negative coordinates); `screen_position` gives points on macOS and the exact physical place elsewhere;
  `memory_for` is what each kind keeps of a remembered place.
- `Dialogs` (managed state) records open dialogs by label `d-<host>-<n>` (at most 8 per host), answers the guard
  (`grant_of(label)`, the `paddock_commands::DialogGrant`) and decides close requests (`close_request(label, now)`:
  forwarded as the dialog's Close/Cancel, closed when it did not answer within 1.5 s, a required choice kept unless the
  watchdog marked its page unresponsive).
- `tauri` feature: `open` (window by kind, no title-bar buttons, owned or independent per OS, modal parent disabled
  natively except on macOS, `WindowHooks` for the app's webview settings), the commands `dialog_open`,
  `dialog_take_input`, `dialog_close`, `dialog_fit_height`, `on_window_event` (close request, move/resize, destroyed
  dialog or host), `page_unresponsive`, `close_all_of`, `set_visible`, `grant_of` for `Guard::dialog_grants`. Events
  `paddock://dialog-result`, `paddock://dialog-closed` (to the opener), `paddock://dialog-close-requested` (to the
  dialog).

```rust
paddock_commands::command_table! {
    static COMMANDS, macro handler;
    Mutation [MAIN | DIALOG] limit 9 * MIB => paddock_dialogs::tauri::dialog_open,
    Query [DIALOG] => paddock_dialogs::tauri::dialog_take_input,
    Mutation [DIALOG] limit 2 * MIB => paddock_dialogs::tauri::dialog_close,
    Quiet [DIALOG] => paddock_dialogs::tauri::dialog_fit_height,
    // ... the app's commands
}
tauri::Builder::default()
    .manage(paddock_dialogs::Dialogs::from_json(include_str!("../dialog-contracts.json"))?)
    .invoke_handler(Guard::new(&COMMANDS).dialog_grants(paddock_dialogs::tauri::grant_of).wrap(handler!()))
    .on_window_event(|w, e| paddock_dialogs::tauri::on_window_event(w, e))
```

Wire types are generated into `ui/projects/paddock-ui/shell/generated/dialogs.ts` (`DialogKind`, `DialogContract`,
`DialogContractFile`, `DialogRequest`, `DialogStart`, `DialogResult`, `DialogClosed`, `DialogError`, `DIALOG_EVENTS`,
`DIALOG_BASE_COMMANDS`; `PADDOCK_WRITE_TS=1 cargo test -p paddock-dialogs --lib generated`).
## `paddock-health` (plan S3)

Service health and the cause-key notification centre (ADR 0010, ADR 0012 §3), ported from Debrief ADR 0055
(`health/{model,registry}.rs`, the generic half of `health/hub.rs`, `notification_centre/`), without Debrief's
services, sources or actions:

```rust
// The app registers its services; the repair is the shell's code and never goes through the broken service (rule 19).
let health = paddock_health::tauri::init(app.handle(), paddock_health::tauri::Setup::new(vec![
    ServiceSpec::new("llm", "the model server").repair(move || supervisor.restart().map_err(|e| Cause::new("...").detail(e))),
]).notifications_file(data_dir.join("notifications.json")).os_notifications(|app, notice| show_os(app, notice)))?;
// Supervisors report; the same report again announces nothing.
health.down("llm", Cause::new("The model server stopped.").hint("Choose Restart.").code("llm.stopped"), Some(RepairAction::Restart));
// Commands behind the shell's guard (S17 lists them in the shell plugin's table).
.invoke_handler(Guard::new(&paddock_health::tauri::COMMANDS).wrap(paddock_health::tauri::invoke_handler()))
```

- One failure is one chip and one notification in every window: each real change is one `paddock://service-health`
  event (a `ServiceHealthSnapshot`) and one message with the cause `health:<id>` (posted on the outage, updated in
  place, resolved on the recovery), with a `paddock://notifications` event per window. New windows ask with
  `health_snapshot` and `notifications_list`.
- `health_repair` returns at once with the service `repairing`, runs the repair on a background thread (a panic is
  contained) and ends `ok`, or `down` with the repair's cause; `openSettings` repairs are opened by the UI instead.
- Notifications: sources and action types are the app's strings (`NoticeSource`, `NoticeAction { type, ...args }`);
  the shell's own are `health`, `repairService` and `openSettings`. App messages are saved debounced and atomically in a
  versioned file (missing, damaged or newer: empty, logged); a window's project messages are saved by the app with its
  project (`notifications_record` / `notifications_restore`). A dialog or pop-out sees its host window's list.

## `paddock-startup` (plan S7 and §11)

The start-up contract and the window watchdog (ADR 0012 §3, ported from Debrief ADR 0058), plus the liveness heartbeat
after start-up. Every window reports `window_ready` once it painted its frame; a window that does not within 10 s is
reloaded once, then the user is asked natively (Reload / Restart in Safe Mode / Keep Waiting). Once ready, each visible
window answers a ping every 2 s (`paddock://ping` → `window_pong`); after 5 missed in a row it is marked unresponsive,
logged, the unresponsive hook lets its dialog be closed by the OS close, and the user is asked (Reload / Keep Waiting); an
answer clears the mark. A gap of more than 3 s between ticks means the app process itself was paused: the clocks restart.
A terminated web content process (macOS) is reloaded at most three times a minute; the reloaded page hears why with its
last UI checkpoint.

```rust
let startup = paddock_startup::tauri::Startup::builder()
    .safe_mode(|app, _label| restart_in_safe_mode(app).map_err(|e| e.to_string()))
    .on_unresponsive(paddock_dialogs::tauri::page_unresponsive)    // a dialog whose page cannot answer may be closed
    .native_dialogs()                                               // feature `dialog`; the app registers tauri-plugin-dialog
    .build()?;                                                      // `Config` timings are checked
paddock_startup::tauri::install(tauri::Builder::default(), startup) // manage + on_page_load + content-process terminate
    .invoke_handler(Guard::new(&paddock_startup::COMMANDS).wrap(paddock_startup::tauri::handler()))
```

- The pure core (`Watchdog`, `Checkpoints`, `ready_reply`, `clean_error`, `Prompt`, `Config`) takes the time from the
  caller and is tested with a fake clock; the ticker thread (started by the first watched page load; never the main
  thread) only applies the `Action`s, reloading and asking through `run_on_main_thread`.
- `COMMANDS`: `window_ready`, `window_boot_error` (16 KiB, one bounded line, at most 10 per page load) and `window_pong`
  for every window class; `ui_checkpoint` (16 MiB, an object) for main windows only. An app with its own table lists
  `paddock_startup::tauri::window_ready` and the others there instead; `DIALOG_BASE` names the three every dialog
  contract includes.
- Generated TypeScript: `ui/projects/paddock-ui/shell/startup/generated/startup.ts` (wire types) and
  `startup-commands.ts` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-startup --lib generated`).

## `paddock-window` (plan S12)

Top-level windows, single instance and deep links, the bounded quit, native feel and the tray (ADR 0012 §3), ported
from Debrief's `window_manager.rs`, `windows.rs`, `instance.rs`, `lifecycle.rs` (exit watchdog), `native_feel.rs` and
`tray.rs`, Pitwall's `window.rs`/`tray.rs` and Dyno's `popover.rs`, with no app's names, settings or menu items:

```rust
let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &["pitwall"]);
if paddock_window::launch::wants_single_instance(&launch, safe_mode_env) {   // first plugin
    builder = builder.plugin(tauri_plugin_single_instance::init(paddock_window::tauri::second_instance));
}
builder.setup(move |app| {
        app.manage(Windows::builder(WindowConfig::new("Pitwall").schemes(&["pitwall"]))   // .multi_window(32) for Debrief
            .prefs(settings.clone())                          // impl Prefs: close setting, open-files setting, "remember"
            .bounds_file(data_dir.join("window-state.json"))
            .teardown("server client", |app| disconnect(app)) // bounded exit steps, in order
            .build());
        app.manage(Windows::dialog_hooks(app.handle()));      // dialogs get the same feel and builder hook
        app.manage(Popovers::new().popover(None, PopoverSpec::new("index.html#/tray", "Pitwall")));
        create_tray(app.handle(), Tray::new(NativeTray::new(icons).menu_on_left_click(false), model).popover(true));
        start(app.handle(), &launch)?;                        // main window, unless `--hidden` and a tray exists
        Ok(())
    })
    .on_window_event(|w, e| { paddock_dialogs::tauri::on_window_event(w, e); paddock_window::tauri::on_window_event(w, e) })
    .invoke_handler(Guard::new(&paddock_window::COMMANDS).wrap(paddock_window::tauri::handler()))
    .build(context)?
    .run(|app, event| paddock_window::tauri::on_run_event(app, &event));
```

- Pure core: `close` (last window: quit / keep running in the tray / ask; no tray means quit; only an explicit exit
  ends a tray app; `--hidden` builds no window when a tray can bring it back), `launch` (flags `--new-instance`,
  `--new-window`, `--safe-mode`, `--hidden`, `--`; paths capped at 256; deep links of the app's schemes parsed once,
  bounded, no credentials; the open plan: an empty window is reused, links go to the window in front and never ask,
  the `openFilesIn` setting decides the rest), `bounds` (versioned file, `main` and `window` keys; restored on the
  monitor the rectangle overlaps most or the nearest, scaled, shrunk and moved inside the work area; new windows
  cascade on the parent's monitor; built on `paddock_dialogs::placement`), `exit` (`teardown_step` bounded per step,
  `end_stalled_exit`, once-only `ExitState`), `feel`, `tray` (menu model with validation and the shell's ids `pd.show`,
  `pd.new-window`, `pd.window:<label>`, `pd.quit`), `popover` (anchoring at the tray icon, fitted height, material per
  OS, the boot script with `data-popover-open` and Escape).
- `tauri` feature: `Windows` state and builder, `create_main` (from `tauri.conf.json` with `"create": false`, else
  `WindowConfig`), `open_new`, `reveal` (rebuilds a destroyed `main`), `list`, `target`, `quit`, `close_requested`,
  `on_window_event` (close setting, focus of pop-outs and dialogs for their host, bounds, destroyed windows, the
  popover's blur), `second_instance`, `opened` and `reopen` (macOS), `exit_requested`, `exit` (bounded teardown, once),
  `arm_exit_watchdog` (8 s, then teardown and process exit), `on_run_event`; `Tray` over a `TrayBackend` (redrawn only
  when the model changed), `Popovers` with `toggle_popover`, `fit_popover`, `close_popover`. `tray` feature:
  `NativeTray` (Tauri's tray icon; template images on macOS; no icon in a session without a taskbar). `wry` feature:
  WebView2's browser keys off in release builds.
- `COMMANDS`: `window_new` [MAIN | TRAY], `window_close`, `window_set_title` (4 KiB), `window_resolve_close`,
  `window_take_pending`, `window_resolve_open` [MAIN], `windows_list`, `window_focus`, `app_quit` [WINDOW | TRAY],
  `tray_fit_height`, `tray_close` [TRAY]. Events: `paddock://windows`, `paddock://window-close-requested`,
  `paddock://window-open-ask`, `paddock://window-open`.
- Generated TypeScript: `ui/projects/paddock-ui/shell/generated/window.ts` (wire types, `WINDOW_EVENTS`) and
  `window-commands.ts` (`PADDOCK_WRITE_TS=1 cargo test -p paddock-window --lib generated`).
- Tests run on Tauri's mock runtime with a fake tray backend and hooks for Quit and the process exit: no window, tray
  icon or single-instance lock reaches the desktop (rule 7). `tests/native_menu.rs` has no harness, so the native menu
  is built on the main thread (macOS builds menus there only); the builder is made, never built.
## `paddock-menu` (plan S13)

One menu model for the native menu bar and the in-app menus (ADR 0012 §3, ported from Debrief's `menu.rs`, ADR 0008).
The UI builds the model from its contributions and draws it itself on Windows and Linux; on macOS it sends it with
`menu_set` once the app reported ready (later changes coalesced), and this crate shows it natively:

```rust
paddock_commands::command_table! {
    static COMMANDS, macro handler;
    Quiet [MAIN | POPOUT] limit 1 * MIB => paddock_menu::tauri::menu_set,
    // ... the app's commands
}
tauri::Builder::default()
    .setup(|app| Ok(paddock_menu::tauri::install(app.handle())))  // manages NativeMenu (product name), routes clicks
    .invoke_handler(Guard::new(&COMMANDS).wrap(handler!()))
    .on_window_event(|w, e| paddock_menu::tauri::on_window_event(w, e)) // the menu bar follows the focused window
```

- `parse` refuses a model over the limits (16 menus, 1000 entries, depth 4, ids of 128 `[A-Za-z0-9._:/-]`, labels of
  120 characters without control characters, no duplicate item id; unknown fields) and turns bindings into accelerators
  (`accelerator("Mod+Shift+O")` is `CmdOrCtrl+Shift+O`; a binding a menu cannot show keeps working in the page).
- `native_spec(model, app_name)` adds the standard macOS items; `outline` is its snapshot form (the crate's snapshot test
  pins it). `diff` answers `Unchanged`, `States` (check marks and enabled states updated in place) or `Rebuild`;
  `NativeMenu::builds` counts the builds (a start builds twice: the standard menus at `install`, then the UI's).
- A click goes as `paddock://menu` (the item id) to the focused top-level window only (a dialog or pop-out stands for its
  host); a model from a window that is not the target is answered `notFocused` and sent again on focus.
- `manifest::check(json)` over the app's `paddock-manifest.json`: every menu item routes to a command, every key converts
  to an accelerator, a command sits in one menu-bar menu. Run it in a test of the app.
- The native menu bar is a port (`NativeBar`: `TauriBar`, a fake in tests); on macOS native menus are created on the main
  thread only, so the Tauri menus are built in tests on Windows and Linux.
- Generated TypeScript: `ui/projects/paddock-ui/shell/menu/generated/menu.ts`
  (`PADDOCK_WRITE_TS=1 cargo test -p paddock-menu --lib generated`).

To add a crate: create `crates/<name>` (as one of the scaffolds), add it to `members` and `[workspace.dependencies]` in
`Cargo.toml`, and add its step to the `crate` job in `.gitea/workflows/ci.yml`.
