# Lessons from the suite

What Debrief and Pitcrew learned the hard way (2026-10-22). Apply these from the start; each one cost a bug, a
failed CI run or a broken install.


Build and bundle
- Anchor ignore rules (`/logs/`, not `logs/`): an unanchored rule silently kept `app/src/app/features/logs/` out of git.
- Angular production build: `"inlineCritical": false` (optimization.styles). Tauri adds a CSP nonce for inline
  `<style>`/`<script>`, which makes the webview ignore `'unsafe-inline'` and blocks Angular's runtime component styles.
  Add a bundle-time check that index.html has no inline elements (Pitcrew scripts/check-index.mjs).
- macOS: `signingIdentity: "-"` (ad-hoc seal) when there is no Developer ID — SMAppService/launch agents refuse a
  linker-signed bundle (-67056). Put `macos-private-api` in the main tauri feature list AND `macOSPrivateApi: true` in
  the shared tauri.conf.json (tauri-build checks both ways per target).
- An overlay/frameless title bar moves only through drag regions, which need `core:window:allow-start-dragging`
  (+ `allow-internal-toggle-maximize`) in the capability; keep a unit test that the capability grants them.
- Native libraries next to helpers: set rpath (`@executable_path/../Resources/lib`, `$ORIGIN`) and check it in CI.

Window, service and session behaviour
- Tray apps: closing the window destroys it (frees the webview); tray "Open" and macOS Dock Reopen rebuild it from
  config; only an explicit Quit exits; with no tray (some Linux desktops) closing quits. Hidden WKWebViews came back blank.
- Every backend request has a timeout and an inline error with Retry — never a skeleton that waits forever.
- Services started by launchd/systemd get a minimal PATH: pass the user's login-shell PATH (bounded, timed out,
  cached) to anything they start, plus a validated per-item env override.
- A service restarts itself when its own binary changed (app update), only when idle; launchd KeepAlive /
  systemd Restart=always bring it back.
- Windows: the user's session is not the console session when they work over Remote Desktop — enumerate sessions
  (WTSEnumerateSessionsW), prefer active, then console, then lowest id. WebView2/UI work needs the user's session.
- Windows console programs get a `conhost.exe` child: process counts and trees differ from Unix; never treat a
  helper started with the parent as the child's work.
- sysinfo on Linux lists threads as processes: filter `thread_kind()`; never adopt or signal a TID.
- Pure functions that take a target OS must build and check paths with that OS's rules, not the host's `Path`.

Native feel and UI quality
- Port Debrief ADR 0028 (selection opt-in, own context menu, browser keys blocked, OS font) — fix noted:
  offer Copy only when the right-click is inside the selection's rectangles.
- Run the units/format guard and the coverage guard before every push (raw numbers in templates broke main 3×).

Tests and measurements
- Tests never leave processes behind (kill children on harness Drop) and never touch real user folders
  (temp dirs / env overrides; a test once created ~/Library/Application Support/playbooks).
- Tests are timezone-independent (run the suite once with another TZ).
- Memory/speed budgets are measured on release builds, in CI on the target OS, and only at low machine load
  (check `sysctl -n vm.loadavg`); report the load next to every number.
- Give real-process tests generous deadlines (Windows `ping -n N` ≈ N s; loaded CI runners are slow).
- PowerShell scripts in strict mode: read optional properties via `$obj.PSObject.Properties[...]`.

Process and workflow
- Agents: merge origin/main before finishing; ADR numbers are allocated at merge time (renumber on clash);
- Never install over a running CI job: check idleness by the runner's exact path (test runners match too).
- No plugin systems unless third-party code/untrusted parsing/privilege separation needs one (user decision).
- Docs like Debrief: architecture.md, design.md with C4/UML draw.io + exported SVGs, user guide, features,
  roadmap, manual tests, README in Debrief's structure — and regenerate screenshots/diagrams at the end.

- Never await IPC or the backend before the first paint: an app initializer that awaits a Tauri command with no timeout left Pitcrew's reopened window blank for good (the first IPC call in a freshly created window can go unanswered). Initializers start work in the background and return void; anything start-up must know gets a short timeout and a local fallback (e.g. platform from the user agent).

- Never show an empty window while the UI loads: put a plain splash (logo + "Starting <App>…") in index.html, styled from the global stylesheet (no inline style/script, CSP), replaced by Angular, fading in after ~400 ms. Do not create the window hidden and show it on "ready": macOS throttles hidden web views, so "ready" may never come.

- Tests never use the production names of anything: pipes, sockets, ports, data folders, keychain entries. The CI runner itself may run the installed app (Pitcrew supervises the runners), so a test that binds the real pipe fails with access denied. Derive names from the data folder (installed folder → production name; anything else → name-<hash>).

- Tray/menu-bar popups must look native: frameless transparent window with rounded corners (~12 px macOS, 8 px Windows 11 via DWMWCP_ROUND), the OS material (NSVisualEffectView popover/menu; Acrylic/Mica), hairline border, soft shadow, height fitted to content (no empty gap), anchored to the tray icon, close on blur/Escape, quick fade (none with reduced motion). A square opaque web page in a box looks wrong (Dyno feedback).
