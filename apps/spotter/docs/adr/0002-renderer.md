# 2. Rendering JavaScript pages: Chromium over the DevTools protocol

- Status: accepted (2026-10-22)

## Context
Some release pages only exist after JavaScript runs. An OS webview differs per platform and cannot be supervised or
isolated well.

## Decision
A Rust driver speaks the Chrome DevTools protocol to a separate Chromium-family process. Order of preference: an
installed Edge, Chrome or Chromium; else, on request, a pinned Chromium downloaded and verified by SHA-256; last
resort the OS webview, labelled as such. The browser is headful but off-screen, uses a throwaway per-watch profile,
runs supervised with time and memory caps and is killed on timeout. Ready means: load finished, the selected element
present, DOM quiet (not network idle). No stealth. A challenge page becomes a "Needs you" state handed to the user.
The app uses the driver only through the `PageRenderer` trait so the driver can later move to the shared paddock repo.

## Consequences
One behaviour on every OS; a missing browser is a clear, actionable status, not a failure. See docs/roadmap.md for
what is not built yet.
