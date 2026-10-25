# 9. Native feel: Dyno's window behaves like a desktop application, not a web page

- Status: accepted (2026-10-22)
- Related: Debrief's ADR 0028 (the design this ports), Pitcrew's ADR 0008, `app/src/app/core/native-feel.ts`,
  `app/src-tauri/src/native_feel.rs` (shell)

## Context

Dyno runs in a webview. Left alone, every label can be highlighted, a right-click shows the browser's menu (Back,
Reload, Save as, Inspect), F5 reloads, Ctrl +/- zooms the page and a dropped file navigates away. A system tool that
sits in the tray should feel like a native Windows, macOS or Linux application.

## Decision

Dyno adopts Debrief's ADR 0028 policy unchanged, rather than inventing another:

1. Selection is opt-in (`user-select: none` on the shell; text fields, code, paths and error details are selectable).
2. One pure context-menu policy: a menu Dyno opened wins, text fields keep the platform edit menu, selected text opens
   Dyno's own Copy menu, **Copy is offered only when the right-click lands inside the selection's rectangles**
   (the fix noted in docs/suite-lessons.md), elsewhere nothing opens.
3. Browser keys and buttons are blocked in every build (reload, print, save, view source, find, page zoom by keys,
   wheel and pinch, back/forward, middle-click autoscroll); release builds also block devtools.
4. The shell turns off what the webview allows (`zoomHotkeysEnabled: false`; WebView2 accelerator keys on Windows).
5. Dropped files never navigate.
6. System font stack and focus rings only on `:focus-visible` (ADR 0011).
7. The window chrome is native-feeling per OS: overlay title bar on macOS, a drag region on the frameless title bar
   (needs `core:window:allow-start-dragging` and `allow-internal-toggle-maximize`, enforced by
   `scripts/check-capabilities.mjs` in CI), translucent background where the OS offers Mica or vibrancy.

## Differences from Debrief and Pitcrew

Dyno has a tray popover view (`features/tray`) as well as the main window; both load the same guard. The guard's decision functions are
pure and unit-tested (`native-feel.spec.ts`); a Playwright flow checks the behaviour in the browser build.

## Consequences

- New UI showing copyable content must opt in to selection.
- Because the guard is a bubble-phase window listener that only prevents defaults, Dyno's own shortcuts and context
  menus keep working.
