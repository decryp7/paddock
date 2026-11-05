# Spike: drawing mechanisms in Playwright's browsers

The bench behind the results block "drawing mechanisms in Playwright's browsers" in
[apps/debrief/docs/performance.md](../../apps/debrief/docs/performance.md), quoted by
[the drawing note](../../docs/research/2026-11-debrief-drawing.md). It is a spike: no product code, not an npm workspace, not run in CI.

```sh
cd spikes/debrief-drawing
npm ci --ignore-scripts
PLOCK_LANE=fix plock run draw-research -- node run.mjs              # about 7 minutes, an 18-minute watchdog
PLOCK_LANE=fix plock run draw-research -- node run.mjs --caps-only  # the capability and CSP probe only
```

It uses Playwright's pinned browsers, headless (`npx playwright-core install webkit chromium chromium-headless-shell` if they are
missing), and never an installed browser. The data is generated; nothing is read from the machine. A local server on a free
127.0.0.1 port serves `www/` with Debrief's CSP as a header (`style-src` tightened to `'self'`). Results go to a new temporary
folder, whose path the last log line names.

It does not measure WKWebView, WebKitGTK, WebView2 or Tauri's IPC.
