# Paddock lessons

Lessons from Debrief and Pitcrew (2026-10-22). Apply in every app from the start. Source: the Paddock brief.

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
  one Playwright worker and CARGO_BUILD_JOBS=2 on the shared Mac; at most 5 agents at once.
- Never install over a running CI job: check idleness by the runner's exact path (test runners match too).
- No plugin systems unless third-party code/untrusted parsing/privilege separation needs one (user decision).
- Docs like Debrief: architecture.md, design.md with C4/UML draw.io + exported SVGs, user guide, features,
  roadmap, manual tests, README in Debrief's structure — and regenerate screenshots/diagrams at the end.
- When CI fails, fix and re-run only the failed tests locally, push once, then run the full suite locally and in CI in
  parallel as the final gate. A full CI round is 40–50 min; never push fix by fix to see what CI says.
- Lint and type-check the touched files before every push: a quoting typo in two e2e skip reasons cost a whole CI run.
- Agents stop and remove only the Docker containers, images and volumes they created (unique name or label), never
  `docker ps -q | xargs docker stop`: a blanket stop once destroyed a user's long-running container.

Releasing (Pitcrew, Pitwall and Spotter 1.0, 2026-10-23)
- Ship the approved icon set from `paddock/icons/<app>/tauri` and guard it in CI: Pitwall 1.0.0 shipped a placeholder
  whose artwork covered 10% of the canvas (the rest opaque white), so the app showed a blank icon. Check that the
  artwork's bounding box (alpha > 0 and not near-white) fills the canvas (Pitwall `scripts/ci/check-icons.mjs`).
- Strip whitespace from `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_UPDATER_PUBKEY` before bundling: a trailing newline in
  the secret fails with base64 "Invalid symbol 10".
- `[skip ci]` commits are unverified: run one CI build with bundles before tagging, and never tag a `[skip ci]` commit
  (Gitea skips the tag run too).
- Verify the artifact, not only `latest.json`: install the DMG/installer and look at the icon and the first launch.
- Without a Developer ID and notarization, macOS shows "Apple could not verify…" (Open Anyway in Privacy & Security);
  Windows SmartScreen needs a code-signing certificate. Say so in the release notes until the apps are signed.
  Decision 2026-10-23: Apple notarization and Windows code signing are deferred until after the releases settle. Every app's
  release notes carry a "First launch" section (macOS **Open Anyway**; Windows **More info > Run anyway**), appended by the
  release workflow after git-cliff (Debrief, Pitcrew, Pitwall, Spotter, Dyno).
- Seal the macOS bundle ad-hoc (`signingIdentity: "-"` in tauri.macos.conf.json) and verify it in CI with
  `codesign --verify --deep --strict` on the .app and on the .app inside the DMG: Debrief 1.0.0 shipped linker-signed
  only (no sealed resources), and a downloaded copy said "Debrief.app is damaged". Release verification runs codesign on
  the published DMG, not only a look at the icon and version.
- `cargo deny` advisories can turn red overnight (a RUSTSEC advisory for a transitive crate blocked Spotter's release):
  run `cargo deny check advisories` right before tagging and update the crate (`cargo update -p <crate>`) in every app
  that has it.
- Read the version from its single source in tests (changelog / backend status) and assert only its shape: a hard-coded
  `0.1.0` broke the What's new e2e at the 1.0.0 bump and cost a release build, and a hard-coded "Debrief 0.1.0" in the
  status bar shipped. Mocks derive the app version from the same source.
- A tag that has not been published can be moved to the fixed commit; check that no release exists for it first.

Tests and CI environments (2026-10-23)
- Tests never launch the user's installed Google Chrome: every killed launch leaves a 0.7 GB code-sign clone in
  `/var/folders/…/X/com.google.Chrome.code_sign_clone` (620 clones filled 441 GB of the Mac). Browser tests use a pinned
  Chrome for Testing (installed by a script, SHA-256 checked) or an explicit env var, close it with CDP `Browser.close`
  before any kill, and fail instead of skipping when CI requires the browser (`*_REQUIRE_TEST_BROWSER=1`).
- Chrome refuses to run as root with its sandbox (CI containers run as root): pass `--no-sandbox` only through a
  test-only switch set in the Linux CI job; the product keeps the sandbox. Headful browser tests on Linux need a display:
  run them under `xvfb-run … dbus-run-session`. Capture the browser's stderr tail in launch errors so failures are
  diagnosable.
- sysinfo on Linux lists every thread as a process, and each reports its parent's full RSS: Spotter measured an idle
  Chrome as ~17 GB and its 2 GiB watchdog killed every render. Skip `thread_kind().is_some()` and measure PSS from
  `/proc/<pid>/smaps_rollup`. Check every place an app sums process memory.
- A test that relies on a background write finishing before a read is a race (Pitwall's cache expiry test): use fresh
  data per assertion or wait for the write explicitly; repeat a fixed flaky test 20–50 times before calling it fixed.
- Unit tests never open real windows (Tauri MockRuntime); agents pick test binaries from
  `cargo test --no-run --message-format=json`, never with `ls -t` (that picked the app executable and opened a blank
  window on the user's desktop).

Long operations, progress and parallelism (user, 2026-10-23; audit in docs/audits/2026-10-23-long-operations.md)
- Every operation that can take noticeable time shows live, phase-labelled progress from the first moment: a growing count
  ("Finding files… 12,345 found") when the total is unknown, never a static 0. Each phase has its own label and scale
  (Downloading → Checking → Unpacking); never restart one bar under the same label (Debrief's checksum pass looked like a
  second download).
- Hash while writing; never re-read a file you just downloaded. Cancel works in every phase and leaves state consistent
  (build new data, then swap; never delete first).
- The UI never times out a running job: it subscribes to progress events. UI call timeouts shorter than the engine's work
  showed errors while the work carried on (Pitcrew, Spotter, Dyno).
- Every operation runs on a background thread or worker pool, never on the UI thread, the Tauri main thread or an IPC
  handler's thread; commands return at once and report through events (user rule). Split work across threads wherever
  it can be split.
- Bulk file and data work is multi-threaded (bounded pools, parallel walks, parallel sniffing/hashing) and pipelined
  (process while discovery continues), deterministic, and within the app's CPU/memory limits. Shared mechanism:
  `paddock-jobs` (ADR in progress).

Configuration (user, 2026-10-23)
- Tunables (thresholds, intervals, limits, learning periods) are versioned Settings with defaults; knowledge lists
  (cleanup categories, detectors, known names and locations, catalogues, presets) are TOML packs: a built-in pack plus user
  packs in the data folder's `packs/`, parsed once with limits, never executing code, rejected whole with a named reason,
  only ever narrowing safety rules, and reloaded without a restart.

UX findings (2026-10-23)
- Panes lay out by container queries, not viewport media queries: Debrief's Home cut off "Needs attention" and Health
  in a narrow pane. Paths and file names truncate in the middle with the full value in a tooltip and copyable; long words
  wrap; buttons wrap below text; no horizontal scroll at any width ≥ 320 px.
- Search finds identifiers however they are written: index whole names, their parts (dots, camelCase, `_`, `-`, path
  segments) and adjacent-part joins; phrases ignore separators and case. The AI resolves the user's words to concrete
  values first (an entity catalogue with typo-tolerant lookup), then queries, and broadens when nothing matches ("graphic
  builder" vs `CompanyA.Graphic.Builder.exe`).
- Fit checks must use the same calculation before and after download: weights + KV cache for the context actually used +
  compute buffer, against free VRAM / RAM in use now, with "fits this computer" vs "fits right now" shown with numbers.
- An app that needs an engine installs it itself on first start (Pitwall's llama.cpp was not installable because a
  not-installed engine was listed as "planned"); never a message that points at the page it is shown on.
- macOS service registration needs visible repair: detect a Login Item that is off or a stale registration, retry it,
  and show a notice with Open Login Items (Pitcrew, then Pitwall).

Agents and the shared Mac (2026-10-23)
- Every agent that edits a repo works in its own git worktree; the main checkout stays on main. Two Debrief agents
  editing one checkout at once mixed their changes.
- Max ~5 heavy agents; check `uptime` before starting more (load 30–76 slowed every build to hours). Pause lower-priority
  work rather than overload; stopped agents' worktrees keep their work for a later resume.
- Every drawing, diagram and screenshot gets an independent review before release; every Paddock image or status change
  also refreshes the profile repo (decryp7/.profile).

## Platforms (user, 2026-10-22: "same thing for all apps, support linux, mac and windows")
Every app supports Windows, macOS and Linux as first-class platforms: the app, its service/agent/helper per OS
(Windows service, macOS launch agent via SMAppService, Linux systemd user service), installers (NSIS; .dmg; .deb +
AppImage), tray/menu-bar behaviour (Linux: appindicator, close quits when there is no tray), CI jobs on all three
(Linux container, the Windows runner, the macOS host runner), real verification on this Mac and in an OrbStack Linux
container (`--cpus 4`), Windows via CI and the user's VM. Platform-specific features degrade with a clear message
where an OS cannot do something (e.g. Dyno's Windows-only performance counters show the macOS/Linux equivalents).
- Never await IPC or the backend before the first paint: an app initializer that awaits a Tauri command with no timeout left Pitcrew's reopened window blank for good (the first IPC call in a freshly created window can go unanswered). Initializers start work in the background and return void; anything start-up must know gets a short timeout and a local fallback (e.g. platform from the user agent).
- Never show an empty window while the UI loads: put a plain splash (logo + "Starting <App>…") in index.html, styled from the global stylesheet (no inline style/script, CSP), replaced by Angular, fading in after ~400 ms. Do not create the window hidden and show it on "ready": macOS throttles hidden web views, so "ready" may never come.
- Tests never use the production names of anything: pipes, sockets, ports, data folders, keychain entries. The CI runner itself may run the installed app (Pitcrew supervises the runners), so a test that binds the real pipe fails with access denied. Derive names from the data folder (installed folder → production name; anything else → name-<hash>).
- Tray/menu-bar popups must look native: frameless transparent window with rounded corners (~12 px macOS, 8 px Windows 11 via DWMWCP_ROUND), the OS material (NSVisualEffectView popover/menu; Acrylic/Mica), hairline border, soft shadow, height fitted to content (no empty gap), anchored to the tray icon, close on blur/Escape, quick fade (none with reduced motion). A square opaque web page in a box looks wrong (Dyno feedback).
