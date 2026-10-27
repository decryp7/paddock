# 12. Native feel: Pitwall's windows behave like a desktop application, not a web page

- Status: accepted (2026-10-22)
- Related: Debrief ADR 0028 (the reference this ports), `app/src/app/core/native-feel.ts`, `app/src-tauri/src/native_feel.rs`

## Context

Pitwall runs in a webview. Out of the box every label can be highlighted, a right-click anywhere shows the browser's
menu (Back, Reload, Save as, Print, Inspect), F5 reloads the window, Ctrl/Cmd +/- zooms the page, middle-click starts
autoscroll and a file dropped on a page without a handler navigates away. A tray popover and a dashboard that look
like a web page do not belong next to the user's other desktop tools. Debrief solved this in its ADR 0028; Pitwall
adopts the same policy, including the fix noted in `docs/suite-lessons.md` (Copy is offered only when the right-click is
inside the selection's rectangles, never because a selection exists somewhere on the page).

## Decision

1. **Selection is opt-in.** The shell is `user-select: none`, `cursor: default`, no tap highlight, no dragging of
   images or links, `overscroll-behavior: none`. Content the user may want to copy opts in with `.pitwall-selectable`
   (or is selectable by nature: text fields, `contenteditable`, `[role=textbox]`, `pre`, `code`, alerts and notes).
   Applied to: playground messages, request details and bodies, the events feed text and the one-time key secret.
2. **One context-menu policy** (`contextMenuAction`, pure): a menu Pitwall opened itself wins; text fields and editable
   content keep the platform's edit menu; a right-click inside the selection's rectangles opens Pitwall's own menu with
   *Copy*; everywhere else nothing opens. Development builds show the browser menu on Shift+right-click.
3. **Browser keys and buttons are blocked** in every build: reload, print, save page, view source, browser find, page
   zoom (keys, Ctrl+wheel, pinch), back/forward (Alt+arrows, mouse buttons 4/5) and middle-click autoscroll outside text
   fields. Release builds also block devtools and hard reload. Only the browser's *default* is prevented, from a window
   listener in the bubble phase, so Pitwall's own shortcuts and handlers still see every event.
4. **The shell turns off what the webview lets it** (WebView2 accelerator keys in release builds, zoom hotkeys, no
   extensions) in `native_feel.rs`; on macOS and Linux only the UI guard applies, and it also covers the browser build
   used by `ng serve` and Playwright.
5. **Files dropped where no target takes them never navigate.**
6. **Native typography and chrome.** The UI font is the operating system's (San Francisco, Segoe UI Variable,
   `system-ui` with Cantarell/Ubuntu/Noto Sans); no web font is bundled. Focus rings appear for keyboard focus
   (`:focus-visible`) only; scrollbars are thin and drawn from theme tokens.

## Consequences

- New UI that shows copyable content adds `.pitwall-selectable`.
- The decision functions are pure and unit-tested (`native-feel.spec.ts`); a Playwright test (`e2e/native-feel.e2e.ts`)
  checks that chrome is not selectable, that right-clicking chrome is suppressed, that Copy is offered only on the
  selection, that reload and zoom keys are blocked and that a dropped file does not navigate.
- Screenshot baselines use the system font.
