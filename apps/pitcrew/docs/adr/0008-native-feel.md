# 8. Native feel: Pitcrew's window behaves like a desktop application, not a web page

- Status: accepted (2026-10-22)
- Related: Debrief's ADR 0028 (the design this ports), `app/src/app/core/native-feel.ts`,
  `app/src-tauri/src/native_feel.rs`, ADR 0001

## Context

Pitcrew runs in a webview. Out of the box every label can be highlighted, a right-click shows the browser's menu (Back,
Reload, Save as, Print, Inspect), F5 reloads the window, Ctrl +/- zooms the page, middle-click starts autoscroll and a
file dropped on the window navigates away. A tray tool for build machines should feel like a native Windows, macOS or
Linux application. Debrief solved this in its ADR 0028; Pitcrew ports the same policy rather than inventing another.

## Decision

1. **Selection is opt-in.** The shell is `user-select: none`, `cursor: default`, no tap highlight,
   `-webkit-user-drag: none`, `overscroll-behavior: none`. Content the user may copy opts in with `.pc-selectable`, and
   is selectable by nature in text fields, `contenteditable`, `pre`/`code`/`.mono` (paths, runner names and IDs),
   callouts (error details) and log lines.
2. **One pure context-menu policy** (`contextMenuAction`): a menu Pitcrew opened wins; text fields keep the platform's
   edit menu; selected text opens Pitcrew's own Copy menu (`CopyMenu`); everywhere else nothing opens. Development
   builds show the browser menu on Shift+right-click.
3. **Browser keys and buttons are blocked** in every build: reload, print, save, view source, browser find, page zoom
   (keys, Ctrl+wheel, pinch), back/forward (Alt+arrows, mouse buttons 4/5), middle-click autoscroll outside text
   fields; release builds also devtools and hard reload. Only the default is prevented, from a bubble-phase window
   listener, so Pitcrew's own handlers see every event.
4. **The shell turns off what the webview allows**: `zoomHotkeysEnabled: false` in the window configuration, and on
   Windows (release builds) WebView2's accelerator keys. WebKit (macOS, Linux) has no such switch; the UI guard covers it.
5. **Dropped files never navigate**: file `dragover`/`drop` that no Pitcrew target handled is swallowed.
6. **OS typography and chrome**: the UI font is the system font; focus rings only on `:focus-visible`; thin scrollbars
   from theme tokens.

## Differences from Debrief

No theme-font setting (Pitcrew has one face), no pop-out windows or docked browser (one window declared in
`tauri.conf.json`, configured there plus `native_feel::apply`).

## Consequences

- New UI showing copyable content must be `.pc-selectable` (or `.mono`/`code`).
- Decision functions are pure and unit-tested; a Playwright test checks the behaviour in the browser build.
