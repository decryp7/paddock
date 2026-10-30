# 21. What quitting Pitwall does about serving

- Status: accepted (2026-10-23)
- Related: ADR 0006 (memory), ADR 0012 (native feel), `docs/suite-lessons.md` (tray apps); code:
  [`quit.rs`](../../app/src-tauri/src/quit.rs), [`tray.rs`](../../app/src-tauri/src/tray.rs) (the 1.1 shell move removed `app_menu.rs`; the menu's Quit items come from the UI's contribution),
  [`Serving::stop_all`](../../crates/pitwall-server/src/serving.rs)

## Context

Pitwall's server is a background service; the window and the tray app are only a view of it. Closing the window never
stops serving, and until 1.0.2 quitting the app did not either. Some people want Quit to mean "stop": free the memory the
models use and end the engine processes.

## Decision

- The setting **When you quit Pitwall** (`general.onQuit`): `keepServing` (default), `stopServing`, or `ask`.
  The setting is part of the versioned settings file; a missing or unknown value reads as `keepServing`. The old
  `keepServingWhenClosed` flag, which nothing used, is removed (pre-v1).
- **Stop serving** is a server operation (`stop_serving`): pause new requests, unload every model, ask every engine
  process to end (SIGTERM on Unix), kill what is still alive after 10 seconds, and report the state `stopped`. Where the
  platform has no such signal (Windows) engines are killed at once. The service stays registered and keeps running, so
  Pitwall opened again can resume it.
- **Opening Pitwall resumes.** When the app starts and the server reports `stopped`, the app resumes serving; so does
  `set_serving(paused = false)` (the tray's Resume). A pause the user chose (`paused`) is never lifted by opening Pitwall.
- **Ask each time** shows a dialog in the window with both outcomes, Cancel and "Remember my choice" (which saves the
  setting). If no window is open the app opens one and the dialog appears when it loads.
- **Quit and stop serving** is also a tray / menu-bar item and an item of the macOS app menu, whatever the setting says.
  Quit itself follows the setting.
- Quitting never waits on an unreachable server: failures are logged and the app exits anyway.

## Consequences

- One more serving state (`stopped`) in the wire contract, shown as "Stopped" in the sidebar and the tray popover; a request
  made while stopped is refused like a paused one.
- On macOS the app now owns its menu bar (app, Edit and Window menus) so that the quit items can follow the setting.
