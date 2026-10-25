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
- Run only the tests affected by a change while working (touched crates with `cargo test -p`/nextest filters, touched
  specs with `ng test --include`, related Playwright files); never full-workspace runs inside a branch. The full suite runs
  once on the combined result before a local build or release, plus CI (user rule).
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
- A valid signature is not enough on macOS: hardened-runtime ad-hoc sidecars refuse a bundled dylib that keeps its vendor's
  Developer ID ("different Team IDs"), so Debrief 1.0.1's store worker could not load libduckdb. Re-sign bundled
  third-party libraries ad-hoc before sealing; the verify step checks one TeamIdentifier across all Mach-Os and actually
  starts each sidecar from the built .app; CI runs a macOS real-app smoke test.
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
- Multi-stage work is a pipeline: producer threads → bounded queue → worker threads, with stage-specific thread counts
  (I/O stages few, CPU stages cores−1), back-pressure, cancel across stages and per-stage progress. Keep results
  deterministic with commit-time ordering: number only new items, in sorted canonical relative path order; existing IDs
  never change; a moved or renamed file is matched by path, then by fingerprint (paddock-jobs `pipeline_ordered`).
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
- Every dialog is a real OS window (Tauri window owned by and centred on the main window, modal where the dialog was
  modal, remembered size and position, Escape closes), never an in-app overlay popup: setup wizards, Settings, managers,
  What's new, About, export/share wizards and confirmations alike (user rule, all apps). Menus, dropdowns, autocomplete
  and tooltips stay in-app.
- Modality is an explicit property of each dialog: modal for confirmations, credentials, unsaved changes and pickers
  that feed the next step (parent disabled, dialog above it); modeless for tools kept open beside the work (Settings,
  managers, viewers, wizards with their own progress), owned by the parent but leaving it usable, one window per dialog.
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
- Work on one app at a time and finish it before starting the next (user rule); parallel agents only within that app.
  Juggling six apps at once overloaded the Mac and CI and left several releases half-done.
- Every agent that edits a repo works in its own git worktree; the main checkout stays on main. Two Debrief agents
  editing one checkout at once mixed their changes.
- Max ~5 heavy agents; check `uptime` before starting more (load 30–76 slowed every build to hours). Pause lower-priority
  work rather than overload; stopped agents' worktrees keep their work for a later resume.
- Every drawing, diagram and screenshot gets an independent review before release; every Paddock image or status change
  also refreshes the profile repo (decryp7/.profile).

More findings (2026-10-23, all apps)
- Service after an update (macOS): SMAppService keeps the old ad-hoc build's launch constraint (LWCR) while the launchd
  job lives, so after every update the agent fails with EX_CONFIG (78) and loops. Repair = unregister → `launchctl
  bootout` → wait until launchd drops the job → register (with retries), and do it proactively once per version change.
  Re-registering over the live job does nothing (Pitwall 1.0.3, Pitcrew, Dyno's agent).
- A control that repairs X must never depend on X: Pitwall's Settings ▸ Service rendered only after settings loaded from
  the server it was meant to start. Repair/install controls live in the app shell and always render; every "X is not
  running" message carries a working Repair button.
- Never list an installable thing as "planned": Pitwall's llama.cpp could not be installed because a not-installed engine
  fell into the read-only "planned" list. Not installed + installable = a card with Install (or install automatically).
- Parallel workers report progress out of order: a "found" count fed by several threads ticked backwards on Windows.
  Make each phase's `done` monotonic at the reporter (shared atomic or fetch_max) and test it with several threads.
- Race-free cancel tests: cancel from inside the progress callback (or a deterministic hook), not from a polling task
  that the operation can outrun on a fast runner.
- A fix branch must not share `CARGO_TARGET_DIR` with another checkout: Cargo fingerprints workspace crates by relative
  path, so a build in one worktree overwrote the other's artefacts mid-session. One target dir per worktree.
- Playwright `reuseExistingServer` silently tests whatever already listens on the port (another agent's app): give each
  worktree its own e2e port and check it is free before running.
- Regenerate i18n catalogs (`npm run i18n`) after merging branches instead of hand-merging messages.xlf.
- ADR numbers collide when branches add ADRs in parallel: allocate at merge time, rename the file and every reference.
- Stale e2e tests hide in suites nobody ran for a while (Pitcrew's Updates table, Resume menu, duplicate link): run the
  affected e2e files whenever a page changes.
- Local-only test failures can be environmental (plugin harness tests exit 71 under the agent sandbox's sandbox_init;
  macOS idle RSS above a Linux-tuned memory budget): check the same test on Linux CI and the previous release tag before
  calling it a regression; record environment-bound tests in docs/testing, don't skip them silently.
- Worktrees of a repo with a git-ignored local config that uses relative paths (Debrief's `.cargo/libduckdb.toml`) need
  absolute paths or env vars (DUCKDB_LIB_DIR/DUCKDB_INCLUDE_DIR) in each worktree.
- Model fit: a flat "file size + 1–2 GB" and "65% of total RAM" said "fits comfortably" for models that then spilled to
  the CPU; the shared fit check must use KV cache for the real context and free VRAM/RAM now (see UX findings).
- Engines and models go stale fast: the catalogue lists only current-generation models (verified on Hugging Face), and
  one engine (llama.cpp) installed automatically beats a menu of engines (llamafile caused the confusion).
- Segmented (multi-connection) downloads give ~4× on per-connection-throttled CDNs (measured 1→4 connections: 11.0 s →
  2.7 s for 64 MiB at 8 MiB/s per connection) while keeping one in-order checksum and per-segment resume; only for large
  files on hosts with Range support, with per-host caps and 429 back-off.
- Tests leak temp folders: 98k `debrief-*` folders (55 GB) piled up in $TMPDIR. Every test temp dir is a guard removed on
  Drop (also on panic), and a check after the test run counts leftovers. Time Machine local snapshots keep deleted
  space for about a day, so freed space shows up late.
- Every lesson goes here first, then into the apps' suite-lessons copies when each app is next touched.

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

## Angular and SignalStore (research 2026-10)
Source: [docs/research/2026-10-angular-and-signalstore.md](research/2026-10-angular-and-signalstore.md) (rules proposed there, awaiting approval).
- Angular 22 defaults components to OnPush and is zoneless by default; keep zone.js out and make state reactive (signals), because fields assigned in `subscribe` callbacks stop updating the view.
- `effect` is for side effects outside the signal graph; state derived from state belongs in `computed`/`linkedSignal`/`resource`. Debrief has 165 effects (156 `untracked`) that mostly re-implement derivations, which makes ordering bugs likely.
- `loading: boolean` + `error: string | null` can disagree; use one status union from a shared store feature (Debrief repeats the flags in 8 stores).
- Every Tauri window is its own webview and its own Angular bootstrap with its own stores; share state through the Rust engine (commands + events), never by sharing store instances, and give small dialog windows a small provider set instead of the whole `appConfig`.
- Stores are not registered by default; `providedIn: 'root'` everywhere (62 of 66 in Debrief) means every window pays for every store. Provide at the narrowest scope.
- `@defer` and lazy routes are the levers for webview start-up; measure the initial chunk before tightening budgets (Debrief uses one `@defer` in 918 files).
- `@Service`, `injectAsync` and the NgRx events plugin are new or experimental; do not adopt until documented stable.
- Angular's value is the first-party set (router, forms, DI, HTTP, i18n, CDK/Material, testing, CLI, `ng update`); reaching it elsewhere means 5-8 third-party packages with separate release trains. Performance is bound by the Rust engine and virtualised views, not DOM framework throughput.

## Recurring bugs (Debrief 1.0.x, 2026-10-24)
- One test round of Debrief 1.0.1/1.1 found the same bug kinds over and over: blank dialog windows (missing providers), a
  dialog denied the command it needed, dialogs too tall/short/tiny, missing Close buttons, three notifications for one
  store failure, and each layout fix squeezing the next pane (grid → Home → AI pane at 180 px → one-letter titles).
  Every instance was fixed on its own and the next one appeared, because each of the 51 dialogs and each pane was
  configured by hand with no shared contract. The fix is a design rule, not more patches: a dialog kind + declared
  contract per dialog in one checked registry, one notification and service-health model, and layout by declared
  min/preferred sizes and priorities. Rule 40: when a bug kind recurs, propose the redesign (ADR) that makes the class
  impossible or checked, then fix the instances through it.

## Problem catalogue findings (2026-10-24)
Added by the audit of all apps ([problem-catalogue.md](problem-catalogue.md), [audits/](audits/)). Each lesson names the check
that enforces it ([conformance-proposal.md](conformance-proposal.md)).
- A fix that lives on a side branch is not a fix: Debrief's macOS store-worker fix (and 85 other commits) sat on
  `fix/macos-store-and-dialogs` while main was already versioned 1.0.2. Merge a fix to main before the version bump, and let the
  tag preflight list every `fix/*` or `release/*` branch with commits newer than the last tag that are not in the tag (`C07`).
- While the bundle is ad-hoc signed, turn the hardened runtime off (`hardenedRuntime: false` next to `signingIdentity: "-"`):
  with the runtime on, library validation refuses a vendor-signed dylib even after an ad-hoc re-sign (reproduced with libduckdb).
  It is switched back on automatically when a Developer ID is configured. Applies to every app that bundles or loads a third-party
  library or ships helper binaries (`C04`).
- Start every bundled sidecar from the built `.app` in CI (`--self-test` or `--version`), on macOS too. Debrief's real-app smoke ran
  on Linux and Windows only, which is why the 1.0.1 crash on macOS reached users (`C04`).
- Test a service update as an update: install build N, then build N+1 over it, and assert the agent is running and not in
  EX_CONFIG. A unit test of the repair code does not prove the launchd state (`C21`).
- Quit must always end the process: arm an exit watchdog that ends it after a bounded teardown if the window close or a worker
  stalls (Debrief's session-0 hang; only Debrief has it). The real-app smoke asserts Quit exits within 10 s (`C20`).
- A release workflow separate from the checks workflow can publish from a red commit (Pitwall's `release.yml` had no gate). One
  workflow, `release` needs every gate; the preflight asks the Gitea API for a green run of this exact commit (`C07`).
- The postcard IPC cannot decode internally tagged enums, so a page worked against the mock and failed against the real agent
  (Dyno Network). Round-trip every IPC DTO through the real codec in a test, and have the mock return values decoded from the same
  fixtures (`C19b`).
- Mocks and specs derive the app version from its single source; a literal in a mock (`0.1.0`, `1.0.0`) is the same bug as one
  in a spec (`C16`).
- Build quirks seen on CI: `tauri-build` writes a placeholder over a sidecar of the same name (empty, non-executable engine
  binary); `copy_file_range` and hard links on the checkout volume can yield empty files; a `noexec` build mount blocks running
  a copied server. Check that every sidecar is a real, executable binary above a size floor (`C05`).
- macOS Finder stalls when a volume's free space is polled often: poll at most once a minute and put the poll cost in the
  benchmark budgets (Dyno, `a068435`).
- Playwright `retries` turn flakes green and invisible: list every retried test in the job summary and fail when the count grows
  against main (`C17`).
- Pasted logs from the user are confidential: never into samples, fixtures, docs or commits; invent same-shape data, and scan
  fixtures against a local denylist that is never committed (`C25`).
- Guards copied into each app drift (check-icons in three apps, key strip in four, codesign verify in three, nothing in
  Pitcrew): shared guards live in Paddock and are consumed by version, never copied (rule 39 applies to scripts too).
- macOS permission grants (Screen Recording, Accessibility) are tied to the code signature, and an ad-hoc re-sign on update
  changes it; verify in the first spike of Sighter and give the user a Repair that opens the right Settings pane (ADR 0010).


## External processes and windows (Debrief 1.1, 2026-10-24)
- "The browser is running" must include the page you drive: on macOS closing Chrome's last window does not quit Chrome,
  so a process-alive + socket-open check reused a dead session and research never worked again after the user closed
  the window. Every external process or window an app drives has one owner with an explicit lifecycle
  (none → launching → ready → closing → gone(reason)), detects exit, disconnect and page/target loss, and the next use
  starts fresh; a call on a gone resource fails at once with a plain message and Retry, never waits out a timeout.
- Folder names built from pid + clock collide when two launches start in the same microsecond on macOS; add a counter.
- Children of a SIGKILLed worker (e.g. llama-server) survive unless they die with their parent; kill the process group
  or watch for parent death.
- A webview's page process can be paused (e.g. App Tamer auto-stop) or killed: the window stays blank or dies.
  Every window reports ready, a watchdog reloads once and then shows a recovery state, and page errors go to the log.
  Debrief's contract: no initializer awaits IPC; startup calls retry (4 s × 4, first answer wins); each window sends
  `window_ready` after its first render; Rust reloads a window not ready in 10 s once, then asks natively
  (Reload / Safe Mode / Keep Waiting — not "Report": a stuck webview cannot run the in-app report); a paused app process
  (>3 s between watchdog ticks) restarts the timers instead of firing them all; uncaught page errors before ready go
  to the app log as one bounded line. Only the app process can notice a paused or dead webview.
- Dialogs on multi-display, mixed-scale Macs open on the wrong display or at the wrong size when position/size are
  absolute or converted with another monitor's scale; place relative to the parent, on the parent's monitor.
- Platform-only bugs hide when CI runs the Rust tests on one OS: every third-party plugin in Debrief failed to start on
  macOS (sandbox-exec resolves `/var` → `/private/var`, so profile paths and the IPC socket must be resolved and allowed
  exactly) and CI never saw it because the Rust tests ran on Linux only. Run the Rust test suite on every OS the app
  ships for (macOS host runner included). Also: don't run macOS tests under `nice` — it is SIP-protected and strips
  `DYLD_*` variables.
- Progress that can go backwards is fixed in the owner, not the callers: Spotter's parallel folder walkers reported the
  found count out of order; the Job now drops lower reports within a phase and calls its sink under the gate lock, so
  every caller with parallel reporters is fixed at once (rule 11). Replay the exact interleaving in a deterministic test.
- Every app's macOS verify script checks ad-hoc bundles for the hardened-runtime flag, one TeamIdentifier across all
  Mach-Os and a `--version`/self-test of every sidecar started from the built app; three of four apps had no macOS
  bundle check at all, and building the `.app` only when updater keys exist hid that.
- A stop/cancel token that is one-shot leaves a session unusable after the first Stop (Sighter POC: input stayed
  locked until a reconnect). Stop ends the run, not the session: give each run its own token and keep the session's
  input usable afterwards.
- When an app needs a shared component that isn't published yet, it vendors a copy and marks it (rule 39 debt):
  Sighter's POC copied paddock-ui's job view. Publish shared parts early (the shell's publish job) so apps never fork.
- Raw engine error strings reached the UI three times in one Sighter POC (connect failed, Python missing, Pitwall
  missing), each fixed per screen. Rule 40: one shared problem component (plain words, details folded, a working next
  step or Repair per rule 19, neutral styling for optional features) and a test that no page renders an engine string
  directly. The shell's notify entry (`PdServiceError`/`WaitingFor`) is the family's version; apps use it.
- Lime-vs-amber (and similar hue pairs) can be far apart for normal vision yet collapse under deuteranopia: check every
  accent against warning/success/danger in CVD simulation, and never let colour alone carry warning (icon + word).
- The CI image is x86-only, so on the Mac mini it runs under emulation: one container at `--cpus 6` is fine, but three
  at once plus native test runs drove the load to 107. Run at most one emulated CI container at a time, check
  `docker ps` before starting one, and keep it for the final local gate (rule 41). An arm64 build of the CI image would
  remove the emulation cost (CI-speed backlog).
- A fake that is kinder than the real thing hides bugs: Debrief's research browser never auto-closed because Chrome
  opens its own start tab and Debrief opened a second one, so "more than one tab" always meant "the user took part";
  the fake browser reported a fixed tab count and ignored `createTarget`. Fakes must model the real system's observable
  behaviour (start pages, created targets, recorded calls) and be checked against the real thing at least once; every
  keep/close style decision logs its reason, so a silent wrong branch shows up in the user's log.
- A Rust test binary that links `tauri-plugin-dialog` does not load on Windows (0xc0000139, STATUS_ENTRYPOINT_NOT_FOUND):
  the plugin imports `TaskDialogIndirect` from Common Controls v6, and Tauri embeds that manifest into app binaries
  only. Every crate whose tests can link the plugin embeds it in its `build.rs` (`/MANIFEST:EMBED` plus the
  Common-Controls `/MANIFESTDEPENDENCY`, windows-msvc only); `paddock-startup/tests/it/common_controls_manifest.rs` checks
  that on every OS, so a new crate fails locally instead of first on the Windows runner. Whether the binary needs it
  depends on what the linker keeps, so a crate can pass by luck (paddock-shell did) until a test reaches the dialog.
- An e2e assertion that matches a transient value passes on a fast machine and fails on a slow one: the narrow status
  bar test expected the bell's "1" while the demo's asynchronous commands were still posting, and the settled count is
  2. The fast native gate saw the "1" on its first poll; CI did not. Assert the settled state (exact values, full
  labels), never an intermediate one of asynchronous work.
- `cmd | grep -q pattern` under `set -o pipefail` can fail although the pattern matched: grep exits early, the writer
  gets SIGPIPE and the pipeline reports failure (the shell demo's macOS verify said "DMG is not signed" for a signed
  DMG). Capture the output first (`out=$(codesign …)`), then match. Debrief's and Pitwall's verify scripts use the same
  pattern and get the same fix when next touched.
- A SignalStore state key and a method of the same name shadow each other without a type error until a spec calls the
  wrong one: `withState({ save })` and `withMethods({ save() })` left `store.save` a function that is also a signal. Name
  state for what it is (`saveState`) and methods for what they do; members that start with `_` are private to the store
  (not on its type), so a spec drives them through the backend's events, as the window does.
- A scoped (thread-local) `tracing` subscriber does not see what a Tauri command logs: commands run on the async
  runtime's threads, not the test's, so the line vanished and the test read an empty file. A test of a command that logs
  installs the real log once, in a test binary of its own (`paddock-diag/tests/webview.rs`); `Logging::scoped` is for
  code that runs on the test's thread.
- Redaction cost grows with the input, so bound the input first: a section that handed over 100 MB was cut to the 4 MB
  limit only after every byte had been through the regexes (debug builds took 20 s for five such files). `Collected`
  cuts the content to the file limit before redacting and takes smaller `Limits` in tests.

## Tests that measure the wall clock fail under load (Debrief 1.1.1, 2026-10-25)
- Debrief's 1.1.1 release run failed on three tests that asserted wall-clock limits: a cancel seen within 578 ms, a chat
  cancel under 700 ms, and a "grandchild is gone" check polled for a fixed time. Each passed on an idle machine and failed
  on a loaded CI runner; each earlier instance had been fixed by raising its limit, and the next one appeared. Rule 40:
  time is a dependency. Code that waits, times out, throttles or retries takes a `paddock_jobs::time::Clock`; tests pass a
  `FakeClock`, wait until the work is blocked on it (`wait_for_waiters`) and advance it, so they assert outcomes and
  order, never durations. Promptness is counted in work (items after the cancel, `CANCEL_CHECK_ITEMS`), not in
  milliseconds; "gone" is waited for on an event (exit status, a closed pipe), not polled against a stopwatch. A real
  wall-clock limit in a test is only a hang guard (seconds to minutes, failing with a message), never the assertion. The
  conformance check C30 (plan shared-next, "Wall-clock assertions") flags the pattern.

## IPC (Paddock X13, 2026-10-25)
- Fuzz every decoder, also for round trips: a minute of fuzzing the frame codec found that a JSON number carried as text
  inside postcard changed by one unit in the last place each time it crossed (serde_json parses floats approximately
  unless `float_roundtrip` is on). A value must cross an IPC unchanged; the fuzz target asserts decode(encode(x)) == x.
- One oversized message must not take the connection down: read past it without allocating it (the length is in the
  prefix), and fail only the call its first bytes name. A length beyond the hard limit is garbage and ends the link.

## SMAppService on a fresh Mac (Paddock C21 VM smoke, 2026-10-25)
- `SMAppService.status` is `.notFound` for an agent the app has never registered: Background Task Management has no
  record of the app until the first `register()` (`backgroundtaskmanagementd`: "effectiveItemDisposition: record not
  found"; after `unregister()` it reads `.notRegistered`). paddock-service treated NotFound as "cannot be registered from
  here" and refused Install without ever calling `register()`, so the first install failed on every fresh Mac, not only in
  the VM (a drag-install to /Applications hits it too). NotFound is "not installed"; only a failing `register()` means
  "cannot register from here". The tart `exec` session, the helper (not the main executable) calling `register()`, ad-hoc
  signing and /Applications vs ~/Applications were all fine: a helper in `Contents/MacOS` registered at once from `tart
  exec`, which runs in the user's Aqua session (`launchctl managername` = Aqua).
- Read launchd's report by structure, not by `grep`: macOS 26's `launchctl print` adds `resource coalition` and
  `jetsam coalition` blocks with their own `state = active` after the job's `state = running`, so a line scan saw a
  stopped job and, with `has LWCR`, called a healthy agent "refused" (Repair offered for nothing). Only the job's own
  level counts. Pitwall's and Pitcrew's `service_repair.rs` have the same line scan; fix them when they are next touched.
- Before blaming the harness, look at what the OS was actually asked: the log had one `status` and no `register` call. A
  ten-line Swift probe in the bundle (`status`, `register`, `status`) answered in a minute what five harness changes had not.
- The simulation is only as good as its fidelity: the launchd simulation said "never registered = NotRegistered", so every
  scenario passed while the real first install failed. Fakes copy the real OS's answers, captured from a VM, including
  the odd ones.
- An update smoke needs both versions everywhere: the N+1 build bumped only the app's Cargo.toml, so the agent still said
  0.1.0 and could never report the new version. Check the built binaries' versions before booting a VM.

## Test and build speed (2026-10-24)

- Linking dominated the Rust test cycle: Debrief had 239 test binaries (181 integration). One `tests/it/main.rs` binary
  per crate (239 -> 99) cut "touch the engine, rebuild every test" from 145 s to 92 s and the nextest run from 301 s to
  196 s wall on the Mac mini. Tests that measure the process (resident memory, a global allocator) or install a
  process-wide logger keep a binary of their own; the rest serialise process-wide changes or re-run themselves in a child.
- Folding tests changes module paths (`scoped` becomes `it::scoped`), so any output that includes the target changes
  length: a log-budget test that passed by byte-count luck failed and exposed a real bug (a short line still fitted after
  the "only errors" notice). Assert the rule, not a byte count that happens to work.
- nextest needs every test binary to answer `--list --format terse`: a `harness = false` test answers it itself (a few
  lines in `main`) or nextest cannot list the crate.
- mold gains little in CI: x86_64 Linux links with rust-lld by default since Rust 1.90 (mold saves about 0.025 s per test
  binary), and an extra apt install costs more than it saves. It still pays on aarch64 Linux (GNU ld) and in jobs that
  already run apt.
- cargo-hakari must leave Tauri out: unifying its features puts the webview runtime (wry) into every crate, pure cores
  included. A workspace-hack crate also cannot be published; `cargo hakari publish` drops the dependency for the upload,
  which edits a manifest, so the lock file is checked separately instead of `--locked`.
- Measure under a quiet machine and in back-to-back pairs: at a 1-minute load of 30-50 the same build varied by 60%.

## Linux VM guests (Paddock C21, 2026-10-25)
- Linux guests: `tart exec` (like `ssh` without a PAM login) has no logind session, so there is no `systemd --user`
  manager and no session bus; `loginctl enable-linger <user>` starts one, as a desktop login would. A desktop smoke
  needs no full desktop: Xvfb, stalonetray (XEmbed tray; libayatana-appindicator falls back to it when no
  StatusNotifierWatcher is on the bus), dunst (`dunstctl history` shows what reached the notification server) and
  xdotool for the tray menu. Start the app with `--hidden`: a focused window counts as "seen" and suppresses the toast.
- Build Linux test binaries in the guest's own release (ubuntu:24.04 arm64 container for the Ubuntu 24.04 guest), so
  glibc and WebKitGTK match; `cargo build --features tauri/custom-protocol` embeds the UI, without it a release binary
  loads `devUrl`.
- Step a fake clock on waits that are pending (blocked, not yet due, not cancelled), not on a count of waits started:
  another thread's short wait (a finished call's timer, its token already cancelled) bumps the count, the test advances
  before the wait it meant exists, and that wait never ends (plugin-host hang, CI run 6319; reproduced 1 in 192 under
  CPU stress, 0 in 384 with `FakeClock::wait_for_pending`).
- WebKit (Safari, and WKWebView in the macOS apps) is a different engine, and its failures are macOS bugs, not test noise.
  (1) It has only `-webkit-user-select`: the unprefixed property is a separate one it ignores, so a rule that says only
  `user-select: text` cannot opt back in under the shell's `-webkit-user-select: none`; every rule sets both
  (`scripts/webkit-prefix.test.mjs`). (2) A click does not focus a button, so a key recorder that listens on the button
  hears nothing: focus it in the click handler. (3) Tab skips buttons and links unless macOS "Keyboard navigation" (full
  keyboard access) is on, Playwright's WebKit follows that default, and a test of what a keyboard user reaches presses
  Option+Tab there (`pressTab`, `tabKey` in paddock-e2e). Tauri's WKWebView takes the same system setting, so on a Mac
  without it a user also needs Option+Tab (or the setting) to reach buttons and links; Paddock keeps every action reachable
  by menu and shortcut for that reason. (4) `transition-duration: 0.01ms` is a real transition in WebKit: inherited colours
  arrive about 250 ms late after a theme change (axe saw the old scheme's colours); reduced motion sets `0s`. Run the three
  ui projects in WebKit before a build: `PADDOCK_WEBKIT=1 npx playwright test --project=gallery-webkit
  --project=shell-demo-webkit --project=workbench-demo-webkit` (in `ui/`).
