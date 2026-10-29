# 29. The shell owns the start: the tray and the main window open after every plugin is set up

- Status: Accepted (2026-10-29; the coordinator decided the design after an independent review of the fix)
- Date: 2026-10-29
- Related: ER-11, ER-14, ER-21 (recurring bugs mean a missing design rule), rules 40, 42, 43; ADR 0012 (the app shell),
  ADR 0026 (the shared dialog API); lesson "a window built inside a plugin hook deadlocks the main thread" (2026-10-29)
- Evidence: a symbolised `sample` of Spotter's start in a throwaway Tart VM (debug build of `29d65b6bd`), and the same
  deadlock on Tauri's mock runtime; Tauri 2.12.5 sources: `AppManager::initialize_plugins` (`manager/mod.rs`) and the
  plugin `on_event` dispatch (`app.rs`) hold `Mutex<PluginStore>`; `WebviewManager::prepare_pending_webview`
  (`manager/webview.rs`) locks it again to collect the plugins' initialisation scripts

## Context

Spotter 1.0's release build never opened a window: the process stayed up, nothing reached the log, and `sample` showed
the main thread waiting on a mutex. The symbolised stack:

```
tauri::Builder::build → AppManager::initialize_plugins → (paddock-shell plugin setup) → Builder::install
  → the app's set-up closure → paddock_window::tauri::start → create_main → WebviewWindowBuilder::build
  → WebviewManager::prepare_pending_webview → Mutex<PluginStore>::lock   (held by initialize_plugins: self-deadlock)
```

Tauri 2 holds its plugin store while it runs every plugin's `setup`, `on_event`, `on_page_load`, `on_window_ready` and
`on_webview_ready`, and building a webview locks that store again on the same thread. The shell's `Builder::setup` runs
the app's closure inside the shell plugin's `setup`, so an app that opened its tray and main window there (the obvious
place) deadlocked before the event loop ran. Five copies had the same mistake: Spotter, Dyno (`dyno/shell-1.0`), Pitcrew
(`pitcrew/shell-1.1`), the shell demo and the workbench demo; the shell's own run-event hook had a sixth (a Dock click
with no window and a file opened from Finder build windows under the same lock). The browser mock and the UI tests never
build a webview this way, so only an installed app showed it.

## Decision

1. **The shell opens the start.** `paddock_shell::Builder::tray(f)` (the app's tray: backend, model, actions) and
   `Builder::launch(LaunchArgs)` declare it; `Builder::add_to(tauri::Builder)` adds the plugin and Tauri's `setup`, which
   runs after every plugin is set up and outside the plugin store's lock: the tray, then `paddock_window::tauri::start`
   (no window for `--hidden` with a tray). A start that cannot open its window is logged and ends the app with code 1
   through the window layer's exit (`exit_with`), not a panic in the event loop. Apps no longer call `start` themselves;
   the shell's `Builder::setup` is for state only. `add_to` takes `tauri::Builder::setup`; an app that needs its own
   Tauri set-up adds it to the shell (a later need, not built now).
2. **Window work a hook triggers runs after the hook.** `paddock_window::tauri::after_plugin_hook` queues it; one task
   hands it to the main thread through the event loop, in the order given. The shell's run-event hook uses it for
   `Opened` and `Reopen`.
3. **Defence in depth: windows refuse to be built inside a hook.** `paddock_commands::plugin_hook` marks a thread inside a
   hook (`plugin_hook_scope`, not `Send`); the shell and the workbench plugin mark every hook they register. Every
   Paddock window builder checks it first and fails with a message that names the fix: `paddock-window` (windows, tray
   popover; error code `insidePluginHook`), `paddock-dialogs` (dialogs), `paddock-panels` (pop-outs, the drag ghost).
4. **Every app tests its start (rule 43).** `paddock_shell::testing::start` (feature `mock-runtime`) builds the app's own
   composition on the mock runtime with a `HeadlessTray`, runs Tauri's set-up once and reports whether the main window
   and the tray exist; a hang is reported as a deadlock after a hang guard. Each app has the start test and a `--hidden`
   case; the OS plugins (single instance, autostart, updater) stay out (rule 7). The demos carry the test, so a new app
   from `scripts/new-app.mjs` starts with it. Conformance **C50** fails an app on the shell without the test, or one that
   calls `paddock_window::tauri::start` itself.

## Consequences

- Spotter, the shell demo and the workbench demo compose their start in one `app(builder, shell, launch, tray_backend)`
  used by `run` and by the test. Dyno and Pitcrew adopt it on their branches (Dyno with its 1.0 release; Pitcrew when it
  resumes). Pitwall moves to the shell in 1.1 and gets the same composition and test then; Debrief keeps its own start
  (it does not use the shell plugin) until it does.
- Breaking (shared API, before v1): `WindowErrorCode` gains `insidePluginHook`; a window built inside a plugin hook now
  fails instead of hanging; the shell's `Builder::setup` no longer opens windows. No compatibility shim (rule: none
  before v1).
- A future hook in a shared plugin must open its scope; C50 and the start tests catch an app that regresses, the guard
  catches a shared part that does.
