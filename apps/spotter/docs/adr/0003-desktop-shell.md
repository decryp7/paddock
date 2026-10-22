# 3. The desktop shell: tray app, popup and a thin command layer

- Status: accepted (2026-10-23)

## Context
Spotter runs for days in the tray and must feel like a native app, never blank, never heavy, never a second copy.

## Decision
- **Thin Tauri shell over the engine.** `app/src-tauri` holds no watch state. `spotter-engine::Engine` owns it; each
  command in `commands.rs` is `async`, validates its arguments (the webview is not trusted) and maps the engine's
  answer to the DTOs of `dto.rs`, which mirror `app/src/app/core/api.types.ts` exactly. A unit test checks that every
  command the TypeScript backend calls has a handler. Changes are pushed on `spotter://state`.
- **Effects behind traits.** The engine's `Notifier` is implemented by `ShellNotifier`, which shows notifications
  through a small `OsToast` trait and runs actions (open, download with checksum verification, run a program without a
  shell and with a time limit) off the engine's task. `SecretStore` is implemented over the OS credential store
  (`Spotter` / `<scope>/<key>`) for source tokens, channel credentials and site cookies.
- **Window lifecycle.** Closing the main window destroys it (hidden webviews came back blank on macOS); the tray, the
  Dock icon and a second launch (single instance) create it again, on a route when asked (`index.html#<route>`).
  Quit is the only exit; with no tray (some Linux desktops) closing quits. Start at login passes `--hidden`: the window
  is destroyed at once and Spotter stays in the tray.
- **Never wait for the UI.** The engine opens in `setup`, before the event loop; the window shows the splash from
  `index.html` and the UI starts its calls in the background with timeouts (`call-timeout.ts`).
- **Tray popup.** A left click opens a frameless transparent window (`index.html#/tray`) anchored to the icon (pure
  `popup_position`), with the OS material (popover on macOS, acrylic on Windows); it closes when it loses focus. A click
  that closed it is not treated as a new click. A right click shows a native menu (unseen releases, check all,
  pause/resume notifications, open, quit).
- **Least privilege.** One capability for `main` and `tray`: `core:event:default` and the two window-drag permissions,
  nothing else (tests enforce it). The CSP has no `unsafe-eval`, no remote origin and no `http:`/`https:` source. Plugins
  (opener, dialog, notification, autostart, updater) are used from Rust only.
- **Data folder override** `SPOTTER_DATA_DIR` (absolute) for tests and second profiles, so nothing that sets it touches
  the user's real data.

## Consequences
The engine can be exercised without the shell and the shell without a webview. E-mail channels, toast action buttons
and click-through from a notification are not built yet (docs/roadmap.md).
