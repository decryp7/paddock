# 19. No start-up splash

- Status: accepted (2026-10-23); supersedes the splash added in e7708e0
- Related: docs/adr/0008-native-feel.md (window life cycle)

## Context

Closing the window destroys it and the tray (or the Dock) builds a new one (ADR 0008), because a hidden WKWebView came
back blank. A splash ("Starting Pitcrew...", fading in after 400 ms) was added in `index.html` as a guard against an
empty window on a busy machine. The owner prefers no splash unless it is needed.

## Decision

Remove the splash. The experiment that decided it (a test build of the app that closed and reopened its window 12
times with waits between 3 and 60 seconds, capturing the window each time): with the current window life cycle
(destroy on close, rebuild on open) every capture showed the rendered UI (screenshot sizes stayed at 124-128 KB, a blank
window is a few KB), so the window reliably paints without a splash. The variant that kept a hidden window alive and
re-showed it is the one that went blank, and Pitcrew does not use it.

What remains is the time between creating the window and Angular's first render. It is short (the bundle is local), the
start-up code never awaits IPC before the first paint (an initializer that did left a window blank for good, see
suite lessons), and the window background already follows the theme, so a slow start shows an empty themed window and
not a white flash.

## Consequences

- One less thing in `index.html`, and no `.pc-splash` styles.
- If a real machine ever shows a visibly empty window for seconds, measure it first; the fix would be a smaller first
  bundle, not a splash.
