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
- Tests that pass on the Mac fail on Windows when they bake in the author's OS: `/tmp` and `/definitely/not/here` are not absolute there, CRLF checkouts change `\n`, 50,000 files time out on NTFS + Defender, hand-written JSON breaks on backslashes, `File::open` cannot set a file time, a wait equal to a 10 s product interval races it (CI 6419–6430; fixed one at a time on three branches plus two commits). **Rule:** ask `paddock-testkit` for paths, text comparison, quoting, fixtures, waits and tools; limits are parameters; C38 checks every test; `cargo xwin clippy` before push (ADR 0021).
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
- Never put a `.cmd`/`.bat` between cargo and rustc on Windows (`RUSTC_WRAPPER`): cmd.exe caps a command line at 8191
  characters and rustc's arguments for windows-sys are longer ("The command line is too long", CI 6365). To keep a build
  folder out of sccache's key (it hashes every `CARGO_*` variable) set it as cargo configuration
  (`--config build.target-dir='...'`), not with `CARGO_TARGET_DIR`. nextest still writes its own store (junit) under the
  default `target/nextest`.
- Moving a suite from `cargo test` to nextest changes one thing tests can notice: every test is its own process. A static
  `Mutex` that made tests take turns on an outside resource (Spotter's real-browser tests, which starve each other when
  several Chromes start at once) serialises nothing any more; the turn-taking becomes a nextest test group
  (`max-threads = 1`) in the workspace's `.config/nextest.toml`. Grep for `static .*Mutex<()>` in tests before switching.
- Tests that need a build in a particular order (Pitwall: tauri-build copies a placeholder sidecar over
  `target/debug/pitwall-server`, which the server's tests start) stay two nextest runs, the shell's last.
- The imported apps' workflows (2026-10-25): installers, the real-app smoke and WebKit only on full runs (main, the app's
  tag, nightly, a manual run with `bundles: true`); the image's Chrome for Testing and Playwright browsers instead of a
  download per job; one port per e2e shard across every workflow (ci-actions.test.mjs checks it). `tauri build` asks
  cargo where the build is, so a `--config` flag on the cargo commands does not reach it; the shared build-folder action
  writes the folder to the workspace's `.cargo/ci.toml` (included by its `.cargo/config.toml`), which every cargo command
  and `tauri build` read, and nothing sets CARGO_TARGET_DIR.
- A live history view whose window ends at `now()`, exclusive, misses what was recorded in the same millisecond (Dyno's
  network timeline flake): a live window ends just after now. An accept loop with short windows must give a client that
  connected late the whole handshake time, or it drops the client unread and the client sees a reset (Dyno's helper).
- Socket I/O waits with `poll`, never inside the socket (2026-10-31, Dyno's agent tests, 1 run in 15 under load). On
  macOS, closing a copy of a socket while another process inspects that copy (`proc_pidfdinfo`: netstat2, lsof, Activity
  Monitor, Dyno's own socket lister) makes the kernel drain the descriptor, which sets `SS_DRAINING` on the SOCKET that
  every copy shares, for good. From then on every `read` or `write` that has to WAIT inside the socket fails at once with
  EBADF although the descriptor is open and healthy (`fcntl` and `SO_ERROR` fine, nothing of ours closed anything, "the
  same call again worked" only because data had arrived). Calls that do not wait, and `poll`, keep working. A listener
  marked this way hands every accepted connection the mark ("a fresh pair starts broken"). A `posix_spawn` child holds a
  copy of EVERY descriptor, `O_CLOEXEC` ones included, until its exec closes them, so any spawn is a chance for it. An
  immediate retry of the call cannot work (the mark never clears, and `F_GETFD` also passes for a reused number). So
  paddock-ipc and dyno-ipc streams are non-blocking and wait on `poll` (the read timeout is the poll timeout), and a
  partial write continues after `poll` says writable. The regression test puts a socket into the state on purpose.
  Agent tests also stopped running the real socket lister, counters and hardware readers (ER-14): the platform owns
  those ports and the fake supplies them. A forking test in a shared binary was a lesser trigger of the same thing.
- A just-released lock can look held (2026-10-31, Dyno restart flakes): on macOS ANY spawn, `posix_spawn` included, gives
  the child a copy of the descriptor table until it execs (measured: a thread spawning `true` in a loop against a thread
  taking and dropping an `InstanceLock` fails every run). Closing the spawn path does not close this, so the lock judges:
  `InstanceLock::claim` reports `Held` only after `HELD_CHECKS` consecutive checks a tick apart (a real holder stays held,
  a copy clears). Tests and callers no longer retry on "already running". Separately, `pre_exec` is out of the shared
  crates and the app preset (it forces fork+exec; priority is lowered on the held child after the spawn), and a test that
  forks on purpose lives in a test binary of its own: in a shared one its child copied its neighbours' locks and was a
  trigger of the random EBADF above (more copies of descriptors closed under inspection; see the entry on `poll`).
  The judgement belongs to one layer only: a supervisor that already judges the lock over its own paced looks (Dyno's
  link) probes it with `is_held_now`, one check and no wait. Probing with the re-checking `is_held` put a hidden 400 ms
  wall-clock wait into every look, so the link's looks no longer followed its pace (CI 7257: a counted wait of 50 looks took
  20-43 s, and a slow first look showed "the system is not answering" before the holder was known). No test caught the
  wrong choice (the link's tests only got slower), so the wrong choice is made impossible to write: Dyno has no waiting
  probe at all (`dyno_agent::instance::is_held` is gone, only `is_held_now`), and the shared one is named for its wait
  (`is_held_settled`). Rule: `is_held_now` only where a held reading leads to waiting and looking again; a one-shot
  decision uses `claim` or `is_held_settled`. When a fix is "pick the right one of two calls", make the wrong one fail to
  compile or carry its cost in its name; a test that only gets slower proves nothing.
- A Node script's "am I the main module" check must compare real paths: Node gives `import.meta.url` the resolved path
  and leaves `process.argv[1]` as given, so through a symlink (a CI work folder linked to a stable one, `node_modules`)
  `import.meta.url === pathToFileURL(process.argv[1]).href` is false and the script exits 0 doing nothing (the Chrome for
  Testing installer on macOS from CI 6353 on). Use `isMain` (conformance/src/util/is-main.mjs); a step that relies on a
  script's output also checks that the output is there.

- **`CARGO_TARGET_DIR` in the environment defeats sccache 0.19 (2026-11-16, feat/local-sccache).** sccache puts every `CARGO_*`
  variable into its cache key and cargo hands the variable to rustc, so a per-worktree build folder given as `CARGO_TARGET_DIR`
  (or `CARGO_BUILD_TARGET_DIR`) gives 0% hits across worktrees; the same folder as `--config build.target-dir='<dir>'` gives about
  82%. ADR 0014 §1's 6.5% was this, not the checkout path. The same holds for `CARGO_INCREMENTAL=0` set as a variable (see
  docs/benchmarks.md, "Local sccache"): put such a setting in cargo configuration, where it is not part of the key. Every tool
  that starts cargo now passes the folder through one accessor, `tools/paddock-dev/src/cargo_target.rs`, and drops the variables
  from the child's environment (the variable would outrank the configuration). A stand-in cargo in the tests fails when
  `CARGO_TARGET_DIR` reaches it, and a source audit (`tests/it/cargo_target_lint.rs`) fails when a tool sets the variable on a child
  outside its short list.
- **A server a tool starts is the machine's, not the test's or the job's (2026-11-16/17, feat/local-sccache).** Three review
  findings, one class. (1) A tool that starts the shared sccache server from a test starts it on the developer's machine (ER-45):
  the wrapper is prepared on first use by a cargo step (`local_sccache::Lazy`), never when the tool starts, and every test spawn of
  `paddock-dev` goes through `tests/it/dev_bin.rs`, which sets `PADDOCK_SCCACHE=off` (an audit fails on a direct use of the binary's
  path). (2) A server inherits the starter's `TMPDIR` and outlives it: builds failed once the starter's scratch folder was deleted, so
  the server now starts with a tool-owned `TMPDIR` and working folder. (3) Whether a running server is "ours" is decided by an
  exact comparison of the printed folder (`Local disk: "<dir>"`) or bucket and prefix, never by `contains`. CI keeps its per-slot
  sccache; a first version let the Mac runner join the agents' server, and the review found `CARGO_INCREMENTAL=0` (a `CARGO_*`
  variable, so part of the key) gave 4% hits against the agents' entries and that a shared server is one more thing that can stop a
  runner; the user dropped it.
- **sccache 0.19.0 latches a failed startup write check (2026-11-17, found for the RustFS cache).** At startup the server writes one test
  object (`RemoteStorage::check`); when that one PUT fails the server runs read-only for its whole life, every later write fails
  ("Cannot write to read-only storage") and the stats show 0 hits with a write error per compile (the first RustFS series: 138 of 138).
  The tool now reads `Effective cache mode` from `--show-adv-stats`, restarts a read-only server of its own cache once, and turns the
  wrapper off with the server's log path in the warning if it stays read-only. The server always logs its errors to a file in its run folder
  and never idles out; S3 without keys fails at once instead of waiting about 75 s in the metadata lookup.

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


## A CI job container whose PID 1 is `sleep` never reaps orphans (2026-10-25, Debrief 1.1.1)

Gitea's Linux job containers run `sleep` as PID 1. A process killed after its parent died is reparented to PID 1 and
stays a zombie for ever, and `kill(pid, 0)` still reports it. A test that checks "the grandchild is gone" passed only
when the shell happened to reap it first. **Rule:** "process gone" checks treat a zombie as gone (read the state from
`/proc/<pid>/stat` on Linux, `proc_pidinfo` on macOS); `paddock_proc::is_alive` already does.

The same holds for a group: Linux's `killpg(group, 0)` counts zombies too (macOS's does not), so a guardian that waited for a
SIGKILLed group to "empty" looked its whole bound (10 s) per group in such a container, and its test passed at 32 s
(2026-11-10, run 7904). **Rule:** "group gone" checks read the members' state too (`paddock_proc::tree::group_exists`
reads `/proc/*/stat` on Linux), and a test of a bounded wait asserts the outcome the wait logs ("emptied", not "still had
members"), because a hang guard longer than the bound cannot tell.

## Build a command line in one function (2026-10-25, Debrief 1.1.1)

Chromium's arguments were assembled in three places; after two branches merged, `--no-sandbox` landed after the start
page on Linux as root (only CI runs as root, so the macOS gate never saw it). **Rule:** one function builds the whole
command line, puts positional arguments last, and is tested for every combination of options on any OS (inject
"is root"/sandbox as a value).

## On macOS, an accepted socket inherits a non-blocking listener's flag (2026-10-25, Debrief)

A blocking test fake used a non-blocking listener; on macOS each accepted connection inherited the flag, so a request
that arrived a moment late read as "would block" and the fake dropped the connection — a client then saw "not
running". Linux does not inherit the flag, so only the Mac flaked. **Rule:** a blocking fake sets every accepted
connection back to blocking, and its test sends the request only after the fake has started reading.

## Compare mechanisms in tests, timings in benchmarks (2026-10-25, Debrief)

"Four connections are faster than one" failed 30 of 30 times under load. **Rule:** a test asserts the mechanism (four
requests held open at once), and the speed comparison lives in a benchmark with a budget (rule 16, `paddock-bench`).

## Many agents waiting on "load below N" start together (2026-10-25)

With a dozen agents each told to build only while the 1-minute load was under 20, they all waited, saw the load drop,
and started at once — load swung between 10 and 70 all night. **Rule:** heavy steps take a slot from one machine-wide
semaphore (a few mkdir-lock slots under the shared build volume, stale slots reclaimed by pid) instead of each watching
the load; the slot count is the rule-41 cap. Load watching stays for deciding how many agents to run, not when to build.

## Resident size counts freed pages on macOS (2026-10-27, Debrief)

A Debrief store worker with a physical footprint of 535 MB showed 1,965 MB of resident size (RSS) and was killed at its
2 GiB cap. mimalloc gives freed pages back with `MADV_FREE_REUSABLE` and tags them (vmmap: "IOAccelerator", tag 100);
they stay in the resident size until the kernel needs them, and are not in the footprint. Rule 26 only covered Linux
(PSS), so macOS fell back to RSS. **Rule:** every cap, budget and display uses `paddock_proc::process_memory` /
`tree_memory` (Linux PSS, macOS `ri_phys_footprint`, Windows private working set), never `sysinfo`'s `memory()`; a test
frees pages the way mimalloc does and asserts the figure is far below the resident size and the child survives a cap
between the two.

## `net::ERR_ABORTED` from `Page.navigate` names no cause (2026-10-28, Spotter)

A Windows CI run failed `a_missing_element_times_out_as_failed` with "the page did not load: net::ERR_ABORTED"; 20 earlier
Windows runs and 240 stressed local renders never showed it, and the error said nothing about why. Measured on Chrome for
Testing 156: Chrome answers `Page.navigate` with `ERR_ABORTED` for every load it cancels before the page commits: a
response that is a download (`isDownload: true`) or a 204, and a load that another navigation in the same frame replaced.
A dead renderer is reloaded, not aborted; a closed or reset connection is `ERR_EMPTY_RESPONSE`. The replacing navigation's
`Page.frameStartedNavigating` arrives just *after* the cancelled one's reply, so a check right at the reply misses it.
**Rule:** the driver turns `errorText` into a reason (download; replaced by a navigation to <url>, found after one more
browser-side call as an ordering barrier; otherwise the code), so the next failure names its cause instead of being
retried or guessed at.

**Follow-up (same day):** Windows run 6563 failed the same test with the plain code again, so the cause is neither a
download nor a replacement the driver saw. It still does not reproduce on the Mac. Two findings while instrumenting it:
(1) an unexplained abort now carries what the browser reported about the tab from its creation on (lifecycle steps, the
document request's `Network.requestWillBeSent`/`responseReceived`/`loadingFailed` with `canceled` and `blockedReason`,
requested and started navigations, crashes, detaches), and the real-browser tests write Chrome's net log
(`--log-net-log`) and print its page-load, cancellation and error events in a failed assertion. `Network.enable` sent
while a `Page.navigate` is unanswered gets no answer within 10 s, so it is enabled once when the tab is attached, and
the connection keeps only documents' network events. (2) The net log showed the "hermetic" test browser talking to the
internet: Chrome for Testing 156 with `--disable-background-networking` still connected to www.google.com,
accounts.google.com, update.googleapis.com and clients2.google.com. **Do:** give a test browser
`--no-proxy-server` and `--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE 127.0.0.1` (the rule also
maps IP literals, so loopback must be excluded), and assert from the net log that it connected to loopback only, so
"no real hosts" is checked rather than assumed.

**Likeliest cause, now guarded:** Chrome's `Page.enable` reports a navigation of the tab that is still running as
`Page.frameStartedNavigating` before its reply (Chromium's PageHandler::Enable, read 2026-10-28). A new tab's own first
navigation (`about:blank` from `Target.createTarget`) can still be running on a loaded Windows runner when the first
`Page.navigate` is sent. **Do:** note the navigation `Page.enable` reported, and let the first load wait (within the
caller's timeout, on events, no sleep) until that navigation committed or stopped loading. Both drivers
(paddock-chromium and Spotter's copy) now do this; the fake browser's `SlowStartPage` reproduces the abort.

## A local model over every item turns a background task into a machine hog (2026-10-28, Debrief)
Debrief's background "Learning human names" ran the local model over up to 2,000 names after every import. It held about
8 GB on the Mac and made a Windows PC sluggish for the whole run, for a result that rules, a names pack and the logs' own
metadata match or beat. **Rule:** work over many items never calls an LLM automatically or once per item; it uses
deterministic rules, TOML packs and static embeddings, and an LLM only answers bounded requests the user starts (rule 46).

## Rule 1 allows small releases alongside a large app (2026-10-28)
Consolidating CI, Dyno 1.0.0 and Spotter 1.0.0 had to run together, which "one app at a time" forbade, though the two
releases are small, finished apps in their own folders with their own tags. The rule's reason is attention and machine
load, and a release prep of a finished app costs little of either. **Rule:** one *large* app at a time; small,
independent releases (separate app folders, serialized tags) may run alongside it (rule 1, amended 2026-10-28).

## Rule 1 is one release at a time, not one app (2026-10-28)
Even "one large app at a time" held back work that cost the release nothing, while the real limits that day were two
single-job CI runners, the machine's load and conflicts in shared code (Dyno and Spotter both wait on the shared dialog
API). **Rule:** one release at a time; release-critical work first; other apps progress in parallel only while load and
CI capacity allow and only if they don't change shared code the release depends on (rule 1 and ER-1, amended
2026-10-28).

## A turn budget in a test wait is a timeout in disguise (2026-10-28, paddock-ui, Debrief)
The boot-every-dialog harness gave a dialog window 200 macrotask turns to become stable, and Debrief's dialog spec 400
render rounds to leave Loading. On a busy CI runner a first `import()` of the settings screen took longer, and both
reported "never leaves Loading" for a dialog that was fine. **Rule:** a test waits on the condition itself (the promise the
code awaits, `whenStable` over pending tasks), never on a count of turns, rounds or milliseconds; a wait that can hang is
left to the runner's test timeout, which names the test (rule 5).

## Material first, because it is tested (2026-10-28)
Hand-rolled components (for example `pd-table`'s sticky header over virtual scroll) cost us bugs that the Angular
Material and CDK equivalents already solve and test. **Rule:** use a Material component wherever one fits and CDK for
the behaviour Material lacks; hand-roll only what neither provides, with the reason in the doc comment (rule 47).

## aws-lc's jitter entropy aborts on Windows VMs; build with AWS_LC_SYS_NO_JITTER_ENTROPY=1 (2026-10-28, Spotter)

Spotter's one test that completes a TLS ClientHello died twice on the Windows runner (a VM) with 0xc0000409, no panic
and an empty stderr (runs 6457 and 6628). The probable cause: the first random draw in a process seeds aws-lc's
generator from CPU Jitter Entropy, which calls `abort()` when the jitter source fails its health tests three times
(aws/aws-lc-rs#1072, open, seen on Windows VMs and at early boot), and the MSVC CRT turns `abort()` into a fast fail
with that code, which reads like a stack overrun. Not caught in a dump; on the Mac 8,680 fresh processes under load
never failed. The shipped app takes the same path for mail and HTTPS. **Rule:** an app that links aws-lc (rustls'
default provider, reqwest's `rustls`) sets `AWS_LC_SYS_NO_JITTER_ENTROPY = { value = "1", force = true }` in its
`.cargo/config.toml` `[env]`, with a test that reads it through `option_env!`, and never sets the target-suffixed
variant, which the build script reads first. Windows 0xc0000409 is any self-triggered abort (`abort()`, `__fastfail`,
`/GS`), not proof of a buffer overrun: look for a C `abort()` first.

## Per-host CI state keyed by app and job breaks with two jobs on one host (2026-10-28, CI kit)

Planning several jobs at once on one host (more runners per host) showed that every kept folder of the kit was named
after the app and job only: a second run of the same job would have replaced the first one's checkout
(`stable-workspace`), killed its test processes and deleted its temp folders (`host-cleanup`), pruned its build folder,
and stopped its sccache server (one fixed port). The runner already gives every job a folder of its own
(`<workdir_parent>/<16 hex>/`, `RUNNER_TEMP` in it, removed after the job). **Rule (ADR 0025):** what need not outlive the
job goes in the runner's job folder; what must stay warm is a kept folder of the job's slot, declared once in
`conformance/ci/kept-folders.txt` and reached only through `conformance/ci/job-isolation.sh`; process patterns are
anchored on `{tmp}`, `{build}` or `{slot}`, never on a shared name.

## A lock that can be reclaimed races; take an OS lock and act while holding it (2026-10-28, CI kit)

The first job-slot lock was a `mkdir` with an owner file and a reclaim when the owner looked dead; the review found two
jobs could both hold a slot (rename away, check, move back), and the disk sweep checked "slot idle" and then deleted, so a
job could take the slot in between. **Rule:** a lock between processes is an OS file lock held by a process that lives as
long as the owner (`paddock-slots`, `./dev ci slot`: the kernel frees it when the holder dies, so nothing is reclaimed),
and work that needs "nobody holds X" runs while holding X's lock (`./dev ci slot idle-run`), never after a check.

## A CI script tested only on the Mac's bash 3.2 can break in bash 5 (2026-10-28, CI kit)

`job-isolation.sh` passed every test on the Mac and failed in CI's containers (bash 5.2): `patsub_replacement` made a
backslash in `${v//p/r}`'s replacement an escape (every scoped process pattern became an invalid regular expression), and
command substitutions dropped `-e` (a missing CI home became an empty one). **Rule:** a CI shell script never puts a
computed value into `${v//p/r}`'s replacement (concatenate instead), sets `shopt -s inherit_errexit`, checks a value a
substitution must produce, and its tests are also run under the container's bash before pushing (`docker run node:22`).

## A fake clock next to real processes must not run away from the real one (2026-10-28, Pitcrew)
Pitcrew's supervision tests drive the supervisor with a fake clock but real runner processes, and failed or timed out
only on a loaded CI runner (run 6628): (1) a test took the runner's "task started" line for the job's process, then
jumped the clock 120 s with no look at the processes in between, so the job counted as over (its process started
after the line under load); (2) the job's start is the fake clock's time while its processes' start times come from
the real clock, so a clock that ran ahead while the machine was slow put the leftovers outside the job
(`ATTRIBUTION_SLACK`, 10 s) and they were never found; (3) a 60 s clock step during a leftover's stop passed the stop
timeout before a slow process could exit; (4) real-time job lengths (`sleep 3`) and 80 clock steps, each a full
process-table read, scaled with the load. **Rule:** a test double's job lasts until the test releases it, never a real
time; a test waits for what the system under test has *seen* (its usage, its list), not for the input it was given;
while the system compares the fake clock with real OS timestamps, the clock follows the real one (`until_real`) and
holds still while a real process reacts to a signal (step it only where the product needs time to pass, such as a
job's end after `IDLE_CONFIRM`); a tool's timeout ends its whole tree: on Unix its own session (`setsid`, so an
interactive login shell gets no `SIGTTIN` either) and `killpg`, on Windows a kill-on-close job object, which also holds
helpers that left the tree (`taskkill /T` misses those); otherwise `cmd /c`'s and a login shell's children outlive it
(nextest's "leaky"). A test of a timeout gives up on a condition (the tree is complete), never on a time budget.
Reproduce with a delay knob in the fake (`spawn_delay_ms`), not by hoping for load.

## Unrecorded benchmark conditions made 182k and 1.3M events/s look like a contradiction (2026-10-29)
Two quoted ingest numbers, about 182k and 1.3M events/s, were argued over as if one was wrong. Neither came with machine,
load, build profile, corpus or command, so nobody could tell whether they differed by a release build, a quiet machine, a
different corpus or a real regression, and stale figures kept being quoted from ADRs and old job logs. **Rule:** record
every benchmark you quote as a results block (date, commit, machine, load, build, corpus and generator, exact command,
runs, numbers with budget and verdict, physical-footprint memory) in the owning `docs/performance.md` or
`docs/benchmarks.md`, and link it from wherever the number is quoted; a number without a block is not evidence (rule 48).

## A sticky header needs a z-index above everything that scrolls under it (2026-10-28, all apps)
"The cell covers the header when scrolling": a `position: sticky` header with `z-index: auto` paints in tree order with the
rows, so a later cell that is positioned or makes a stacking context (opacity on muted rows or hover actions, a relative
badge, a transform) paints over it, and a sticky column at the header's own z-index wins the tie. Fourteen tables in three
apps and the shared shell had it (ADR 0027 lists them); none showed with the mock data, so a run-time sweep alone would not have found them.
**Rule:** every sticky header sets a z-index from the table ladder (`--pd-z-table-header` above sticky columns above row
actions) and an opaque background; C43 checks every instance and `expectHeaderOnTop` checks a grid at run time (ADR 0027,
rule 40).


## Stale servers and stale builds (e2e and screenshots, 2026-10-28)

Three times in one night tests and screenshots ran against old code: (1) `reuseExistingServer: true` on fixed ports
(4300 to 4302 and the apps' own) silently reused another worktree's or an old dev server, so the tests "passed on the old
code"; (2) `ui/dist` and the generated, untracked `ui/projects/paddock-ui/css/*.css` were older than their sources, so
screenshots showed old text and a reviewer reported a bug that existed only in stale CSS. **Rule:** one mechanism in `@decryp7/paddock-e2e`
(ER-21, rule 42), not another warning. A running server is reused only with `PW_REUSE_SERVER=1` (default never, and
never on CI; `reuseServer()`, `paddockConfig`); `e2ePort` derives the local default port from the worktree (`20000 +
(hash(git top level) mod 160) * 100 +` the config's last two digits: disjoint blocks) and still refuses a default on CI
(`e2ePortOrCiDefault` keeps the apps' fixed CI ports); and the config checks, at load and so before any server starts,
that what it consumes of ui is built from the current sources. The build writes a content stamp (`css/.source-hash` by
`npm run build:css`, `ui/dist/.source-hash` by `npm run build -w ui`); the check recomputes the hash of `scss/**` (and
every file of the package for the dist, except tests, bin/ and the generated css) and fails with "rebuild ui: npm run build -w ui" on a mismatch, a missing stamp or
a missing css entry. A content hash, not mtimes: unchanged rebuilds, checkouts and touches never false-fail. Which
guard: `paddockConfig({ buildGuard: 'none' | 'ui' | 'ui-dist', uiRoot })`; an app's own config calls
`appBuildGuard(__dirname, uiRoot)`, which picks `ui-dist` only when the app's tsconfig maps `@decryp7/paddock-ui` to
`ui/dist` (today Spotter), else nothing. `PW_SKIP_BUILD_GUARD=1` skips it. The ui config checks the css only with `PW_REUSE_SERVER=1` (its webServers run `build:css` themselves). Dot-files (`.DS_Store`) are ignored; README and doc edits under `projects/paddock-ui` mark the dist stale on purpose (every non-test file counts). A direct `ng build paddock-ui` leaves `ui/dist` unstamped, so the guard fails: build with `npm run build -w ui`. Tests: `build-guard.test.ts`, `env.test.ts`
(including every app config on a fresh checkout). Proposed binding rule (the user decides): every Playwright config
uses these three, and `scripts/new-app.mjs` templates do.

## 2026-10-29: a harness that stands in for a plugin must stand in for its routing and its ACL

The ADR 0026 real-window probe ended "no report within 90s: the dialog page never became ready" on macOS, and it looked like
a product fault (a hidden window that never readies). It was the harness: the page sends the shell's commands as
`plugin:paddock-shell|dialog_take_input`, but the probe registered them as bare app commands and had no ACL, so Tauri
refused every call ("paddock-shell.window_pong not allowed. Plugin not found") before the probe's handler saw it. The page
booted fine (the DOM was there, and the ready of ADR 0026 §4.2 took about 10 ms once the calls arrived). "Refused" was told
from "never made" only by silence in the probe's own log, and confirmed by calling a command from the webview by hand; the
probe could not tell them apart. **Rule (guidance):** a probe or test that replaces the app's plugin registers a plugin of
the same name and an ACL resolved from the app's real capability file, so a wrong capability fails it; it prints every call
it receives, with its time, and the calls seen when it gives up. A refusal itself shows only in the webview console or as a
`RuntimeAuthority` check, so a harness that needs to name it asks the authority, not the log. C48 checks the capability file
of every app on the shared shell.

## 2026-10-29: a row's actions must fit their own box, or the tooltip anchored to it covers them (Dyno)

A row-action cluster got one tooltip on its container, placed left of the container. In the active-alarms table the cluster's
cell was 40 px wide for three 28 px buttons that overflowed to the left, so the tooltip opened over the buttons, took the
pointer (a `mouseleave` after 30 ms) and the second and third actions never showed one. The reverse also happened: widening
the cell pushed the buttons past the clipped edge of the grid at 1280 px, out of reach. **Rule (guidance):** a cluster's box
holds all its buttons (a column as wide as the actions), and the row-actions e2e (every action must show its tooltip, whose
text is its name, and the tooltip must overlap no other action) is the check; a tooltip on an icon-only button is never left
to a per-button `matTooltip` inside a cluster.

## Chromium tests compared a loaded machine with a product timeout (2026-10-28, paddock-chromium)

Three `paddock-chromium` tests failed on CI runs of several branches and never on a quiet machine: (1) `a_real_browser_reads_a_page_and_closes_cleanly` and `a_cancelled_load_says_whether_it_was_a_download_or_replaced` asserted `Browser.close()`, whose 5 s grace (`CLOSE_GRACE`, the product's bound before it kills the tree) is a fine product choice but a time budget as a test assertion: a loaded runner let Chrome take longer (macOS 6662, Windows 6726, 13 s test); (2) `a_page_is_read_snapshot_by_snapshot` pushed a 16 MiB page (`MAX_HTML` + 1) through the fake browser in a debug build to see the size cap refuse it, which outlasted the 10 s call timeout on a macOS runner and gave `Timeout` instead of "larger". **Rule:** a test asserts the mechanism (the browser exits by itself and is not killed; the cap refuses a page one byte over it), never that it happened within a product timeout; give the code under test the cap as a parameter so a few bytes prove it; wait on the condition (`close_gracefully` returns the moment Chrome exits) with a hang guard that only stops a browser that never exits. A failing wait names what the thing it waited for reported (the load's own result), so the next failure carries its cause.

This is the same lesson as "A turn budget in a test wait is a timeout in disguise" for assertions on a product's own grace period: wait on the condition, and let a generous hang guard (never an expectation) stop only a thing that never ends. A third cause was found in the same test by review: its server took one connection at a time with a blocking read, so an idle socket Chrome opened (a preconnect or backup connect) could hide the real request; each connection now has its thread and a failed wait prints what the server saw. C30 now flags a non-negated `assert!` over a bare `.close()` or `.close_gracefully(..)` (the calls that wait out a grace). Waive `close_gracefully` with `// conformance: hang-guard` and a grace of at least 10 s, or either call with `// conformance: grace-free — <why>` when the thing had ended already; a forced end (`assert!(!b.close_gracefully(Duration::ZERO))`) is not flagged.

## 2026-10-29: the Tauri template's crate types cost more than the linker (all apps)

An incremental debug rebuild of Debrief spent 0.61 s linking the binary, but about 1.5 s writing a 1.0 GB static archive
(about 1.25 s) and linking an unused cdylib (0.24 s), both from Tauri's template
`crate-type = ["staticlib", "cdylib", "rlib"]`, which only iOS and Android use. `rlib` alone cut the rebuild by 25%
(Debrief) and 38% (Dyno); lld was slower than ld-prime and cannot read the macOS 27 SDK. Dropping the crate types also
changes release output, though not its behaviour (verified function by function on Dyno): cargo hashes each unit's LTO
mode into `-C metadata`, so the dependencies are compiled bitcode-only under new hashes (other `TypeId` values, folding
and inlining). Also: cargo fingerprints the linker, so a timing wrapper set as the linker after a build recompiles
everything. **Guidance:** app crates build an rlib only; measure link time with a linker wrapper
([`scripts/perf/link-time/cc-timed.sh`](../scripts/perf/link-time/cc-timed.sh)) set before the first build, and compare
release binaries function by function ([`classify.py`](../scripts/perf/link-time/classify.py)), not by checksum.
Results: [Debrief](../apps/debrief/docs/performance.md#2026-10-29-incremental-rebuild-and-link-time-docslink-time),
[Dyno](../apps/dyno/docs/performance.md#2026-10-29-incremental-rebuild-and-link-time-docslink-time),
[Dyno release identity](../apps/dyno/docs/performance.md#2026-10-29-release-identity-of-the-rlib-only-crate-type-docslink-time);
details: [research/2026-10-29-link-time.md](research/2026-10-29-link-time.md).

## Test isolation (Debrief, 2026-10-29)
- `import-range.spec.ts` failed now and then ("expected undefined to be ImportRangeDialog") in full `ng test` runs only.
  `ImportRange` opens its dialog through `openLazy`, whose chunk arrives after an unknown number of event-loop turns; the
  spec waited `settle(undefined, 2)`. On a loaded machine the dialog was not open yet. Earlier the same day `live/components.spec`
  (commit a78de2d86) had patched `FakeDock.prototype.setTitle` and restored it only on the last line, so a failing `expect` leaked the patch.
- **Wait on the condition, never on a count of event-loop turns.** `openDialog(dialogs)` (`testing/helpers.ts`) waits with
  `vi.waitFor` and fails when no dialog opens. To prove a fix, model the slow case (a chunk that takes 25 turns) so the
  old wait fails every time, not now and then.
- **An optional chain never performs a test's action.** `dialogs.stack().at(-1)?.ref.close()` does nothing when the
  dialog is missing, so the test passes or hangs on the next `await`.
- **A prototype patch is always a `vi.spyOn` under `restoreMocks`,** also restored after each test by a global
  `afterEach(vi.restoreAllMocks)` in `test-setup.ts` (specs share a page, `isolate: false`). Assigning to a prototype, or
  `Object.defineProperty`/`assign` on one, is banned in specs by ESLint (rule 49, ER-21).
- **Known gaps of the lint (Debrief only, today):** a prototype reached through a variable (`const proto = X.prototype;
  proto.y = …`) and a patch on a shared object (`Object.defineProperty(navigator, …)`, `document`, `window`,
  `globalThis`) are not flagged. **Follow-up branch (planned):** move the selectors into the shared
  `@decryp7/paddock-eslint` preset, add a shared-object rule (fix: `vi.spyOn(obj, name, 'get')` or `vi.stubGlobal`) and
  convert Debrief, Dyno and Pitcrew.
  Non-dialog optional-chain actions (about 54 in Debrief specs, e.g. `buttons[1]?.click()`, `handlers[0]?.(…)`) are not
  flagged yet; the follow-up bans `ExpressionStatement > ChainExpression` in specs, keeps cleanups such as
  `getSelection()?.removeAllRanges()` with a disable comment, and converts the rest to `!` or `el()`. It also catches
  `menu?.items[0]?.run()`.

## 2026-10-29: a "global" input must mean every build, or the branch runs everything (affected planner)

`scripts/affected.mjs` treated the whole of `.gitea/`, the planner's own files, `.cargo/`, `lint/` and `conformance/(ci|release)/`
as global, so a branch that added one id to the conformance `--only` list in `ci.yml` (feat/icons-mat-registry) or one line
to `affected.test.mjs` (dyno/shell-1.0) ran every app, crate and UI job, against the rule that branches get affected runs
and the full gates run once on main. **Guidance:** a file is global only when every build reads it (`rust-toolchain`, the
root `package.json`, `.npmrc`, `.nvmrc`, `tsconfig/`). Narrow the rest to what it reaches and write the reasoning as a table
next to `GLOBAL`: the planner's files mark tools, ui and paddock-dev (the `plan` and `tools` jobs run `affected.test.mjs`), `ci.yml` marks the jobs whose text changed (line-based split; everything when it cannot
be split or the header changed), a shared action marks the jobs and workflows that use it, `.cargo/` and `lint/` the Rust
builds. Anything that cannot be narrowed still fails safe to everything.

Review found six places where the first narrowing selected too little: a second implementation of the same rules (the Rust
port in `tools/paddock-dev`, compared by a golden test) kept the old list; `--files` mode diffed HEAD with a working tree that
equals it, so a committed change looked unchanged; the shared scripts under `conformance/` reach the crate job through the
actions that run them; the `plan` job wires every other job; a line-based YAML split is only safe if it refuses anything it
does not understand (comments on keys, flow mappings, anchors); and mutants survived because tests asserted `every(...)` over
possibly empty lists. **Guidance:** when narrowing a trigger, grep for every consumer of the old list, make a narrowing fail
safe to everything on any input it cannot model, and assert exact lists in tests, never `every` on a list that may be empty.

## Stale, duplicate and global-trigger CI runs wasted the runners (2026-10-29)
One day of CI went on runs for superseded branch heads, the same head dispatched twice, runs for branches already merged
into another branch, and global triggers in the affected planner that made every run build everything (fixed in
b618b5c97). **Rule:** cancel older queued runs before dispatching, dispatch once after the last review fix, give merged
branches and docs-only changes no CI, and keep branch runs on the affected plan (rule 50).

## Slow imports hid cheap wins in threads and per-item cost (2026-10-29)
Debrief's import was bound by 2 Tantivy writer threads and a single-threaded catalogue, and regex was 38% of parse time.
Nobody asked whether more workers or a cheaper operation would help until the numbers were read by accident. **Rule:** for
any slow operation, measure both: can more threads or workers run it (bounded by the kind of work), and can each item cost
less? Make thread counts Settings with an "Auto" default, and give long or bulk designs a "Parallelism and cost" section
(rule 51).

## Long work in an agent needs the shell's job mechanism, not a poll of our own (2026-10-29, Dyno)
Dyno's first jobs (6c73efd75) polled the agent for their status with a runner of their own: the shell's Jobs view stayed empty,
the pages had two mechanisms, every UI request still had a 15 s timer, and a log read could be cancelled only between logs.
**Rule:** work that runs in another process pushes its `paddock_jobs::wire::WireSnapshot`s, the shell relays them as
`paddock://jobs`, and the pages follow with `JobsStore` and `JobFollow`; the UI has no timer at all (the shared
`uiTimeoutProblems` check runs over the app's sources as a test). A tool that streams its output is read line by line with
`OnCancel` killing the child, so Cancel does not wait for the next line, and the reading happens on a thread of its own: a
killed shell's own children keep its pipe open. Anything that waits for another read waits on a `Condvar` that Cancel also wakes.
**Also:** a store that every dialog window shares must not ask for the shell's jobs store at construction (it sends a request the
dialog's contract may not declare); resolve it when a job starts.

## 2026-10-29: a window built inside a plugin hook deadlocks the main thread (Spotter, Dyno, Pitcrew, the demos)

Spotter 1.0's release build never opened a window: the process kept running, `sample` showed the main thread waiting
on a mutex and nothing reached the log. Symbolised (a debug build in a throwaway Tart VM), the main thread was in
`tauri::Builder::build` → `AppManager::initialize_plugins` → the shell plugin's `setup` → the app's set-up closure →
`paddock_window::tauri::start` → `create_main` → `WebviewManager::prepare_pending_webview` →
`Mutex<PluginStore>::lock`. Tauri 2 holds its plugin store while it runs every plugin's `setup`, `on_event`,
`on_page_load`, `on_window_ready` and `on_webview_ready`, and building a webview locks that store again on the same
thread. Not VM-specific: the same deadlock reproduces on Tauri's mock runtime. Five apps had the same mistake (Spotter,
Dyno, Pitcrew's shell branch and both demos) and the shell's run-event hook a sixth (Dock click, Finder "Open with").
**Rule (ADR 0029):** the shell opens the start (`Builder::tray`, `Builder::launch`, `Builder::add_to`); apps never call
`paddock_window::tauri::start`; window work a hook triggers runs after it (`after_plugin_hook`); every shared plugin
marks its hooks with `plugin_hook_scope`, and Paddock's window builders refuse to build inside one. Each app tests its
composed start with `paddock_shell::testing::start` (C50). Neither Spotter nor Dyno installed a backend for the `log`
crate, so every shared crate's line (the start's error among them) was lost: every app installs `paddock-log` first
(`paddock_shell::logging::install`). Also: a start-up hang with an empty log is checked first with a symbolised stack,
not by guessing at the VM.

**2026-10-30, Pitwall 1.1 on the shell.** Three things the move found. (1) The shared updater had no Offline switch: an app
with a fail-closed Offline mode (Pitwall) would have let About's Updates and the daily check reach the network, and Dyno's
and Spotter's update checks have no Offline switch at all. Local first means every network path takes the app's Offline
predicate and fails closed when the app does not know (`UpdatesBuilder::offline_mode`); a shared crate that opens a
connection without one is a bug. (2) An app's command may share a name with a shell command (`job_cancel`,
`open_login_items`): the plugin prefix keeps them apart at run time, but the generated names and the guard do not; the
shell's `Builder::problems` reports it, so every app's tests call it. (3) Hiding a "constant" column across two rows hid a
key's dates and a preset's defaults that no other place showed: a column is constant only across three or more rows (UI
review 2 rule 5), and a row's own facts (its dates, its settings) are never hidden.

## A work-stealing test picked its victim by position (2026-10-29, paddock-download)
`a_slow_range_is_split_so_neither_the_tail_nor_the_hash_waits_on_it` made the range at offset 0 slow and expected it to be
split. On a loaded Windows runner (CI 7110) its worker was descheduled before sending the request, and the other workers
split the unstarted range below the split threshold, so nothing was left to split. **Guidance:** a test about work stealing
must not pick the victim by its position; gate the fake so the victim is in flight before a stealer can run (ADR 0031,
rule 49).

## 2026-10-29: a host runner's temp folder is the user's temp folder (CI hermeticity, ER-14)

On the Mac host runner, `os.tmpdir()` and Rust's `temp_dir()` are the runner user's `/var/folders/.../T`, the same folder
the agents' local test runs on that Mac use. Debrief's leak check snapshotted and checked it, so a local run that died
during a CI job made the job fail with leaks it did not cause, and the CI tests wrote there too. **Guidance:** every job
on a host runner that runs tests or checks a folder gets a folder of its own first: the shared `job-tmpdir` action
(`RUNNER_TEMP/t` as `TMPDIR`, `TMP`/`TEMP` on Windows; removed as the last step), and checks name that folder explicitly
(`--dir "$PADDOCK_JOB_TEMP"`). A conformance test holds every job that runs `check-temp-leaks.mjs` to it.

Review of the fix (2026-10-29): a job's own temp folder is not enough. **Sockets fall back to `/tmp`** when the temp path is
too long (the runner's per-job folder is), so the leak check and the tests still met other runs there. A short socket
folder per job (`PADDOCK_SOCKET_BASE`, `/tmp/pd-XXXXXXXX` from mktemp) comes before `/tmp`; scanning all of `/tmp` (`--shared`) is only
right in a Linux container, where `/tmp` is the job's. **One owner:** job-isolation's acquire already pointed `TMPDIR` at
the runner's folder; two mechanisms would disagree, so `conformance/ci/job-tmpdir.sh` is the only code that sets
`TMPDIR`, `TMP` or `TEMP` (a test fails on any other writer), and every host job gets it through acquire. **macOS'
bash 3.2** (what `shell: bash` runs on the Mac host) cannot parse an apostrophe inside `${var:?word}`, `${var:-word}`,
`${var:+word}` or `${var:=word}` within double quotes, nor a `case` with unparenthesised patterns (`a)`) inside `$(...)`:
write `(a)`. A `$(...)` inside a case branch is fine (an earlier version of this lesson said otherwise). `bash -n` does not
always catch the second, so a conformance check greps every CI script for both and also runs `/bin/bash -n` where it exists.

## A fake server's polling timeout belongs after the handshake (2026-10-29)
Spotter's and `paddock-chromium`'s fake DevTools servers set their 10 ms polling read timeout before
`tungstenite::accept`, so on a loaded runner a client whose upgrade request came a tick late was dropped mid-handshake
(Windows os error 10053). Debrief's copy had hit the same bug earlier ("Handshake not finished") and was fixed alone.
**Rule:** a fake server completes its protocol handshake under a read bound far above any scheduling delay (seconds,
not ticks) and only then sets its polling timeout; the
regression test keeps the client quiet for many ticks before it shakes hands. A fix in one copy of duplicated code is
searched for in the others the same day.

## An unexplained browser abort needs its cause in the message (2026-10-29)
`net::ERR_ABORTED` covers every load Chrome cancels before commit: a download, a replacing navigation, the tab's own first
navigation, and a restart of the browser's network service. Fixing them one guess at a time cost three rounds of Windows
CI. Ending the network service locally reproduced the Windows trace exactly. **Rule:** a driver error that can have
several causes says which ones it ruled out (the trace, the network service's process), and a real-browser test drives
each named cause on purpose (ADR 0030). Diagnostics land before a fix that would hide the cause.

## 2026-10-29: `ControlOrMeta` follows the test machine, the app follows the page (Dyno's WebKit leg)

Dyno's WebKit e2e on the Linux runner failed every shortcut test (command palette, Settings with `Mod+,`, and the axe
pass over the palette): `devices['Desktop Safari']` gives the page a Mac user agent, so the shell's `currentPlatform`
binds `Mod` to Cmd, while Playwright's `ControlOrMeta` resolves on the host (Linux) and pressed Ctrl. The branch runs are
Chromium-only and the macOS runner has a Mac host and a Mac page, so only the WebKit-on-Linux leg (main pushes) saw it,
and the Settings tests had been red there since that leg began. **Rule:** e2e tests press the app's shortcuts with
`pressMod(page, 'Mod+…')` from `@decryp7/paddock-e2e`, which reads the page's navigator; never `ControlOrMeta` for an
app shortcut. The selftest `mod-key.shot.ts` makes the mismatch on any host by giving the page the other platform.

## 2026-10-30: CI runners have the VC++ runtime, so only a clean PC or an import check shows a missing one (Dyno, all apps)

On a clean Windows 11 VM, Dyno 1.0's `dyno-agent.exe` exited with 0xC0000135 (`STATUS_DLL_NOT_FOUND`) until Microsoft's
VC++ redistributable was installed: it imported `VCRUNTIME140.dll`. Every app with a sidecar shipped the same import.
Tauri's `staticVCRuntime` covers only the main binary, and each sidecar is a crate of its own. No CI job could see it:
the Windows runners have Visual Studio's build tools and therefore the redistributable, so every build, test, smoke
run and installer test passed. **Rule (ADR 0032):** every Windows binary links the C runtime statically (`+crt-static`
in the root `.cargo/config.toml`; every app sets Tauri's `staticVCRuntime` to false, because the two together do not
link), and every Windows installer build runs `conformance/release/windows-crt-imports.mjs` over the files it ships. More
generally, a dependency that the build machine has and a user's PC may lack (a runtime, a framework, a font, a driver)
is checked on the artifact, with an import or bundle check, or on a clean machine, never inferred from a green CI.

## A pid is not a process: one test's close could kill another test's browser (2026-10-30)
`paddock_proc::kill_tree` read the process table, ran `taskkill /T /F`, then killed every pid from that table again. By then
those pids were free, and Windows hands a freed pid on at once: with four real-browser tests in parallel, the next owner was
likely another test's Chrome helper, and killing its network service gives exactly the `ERR_ABORTED` trace of the Windows
flake. That was the suspected cause, and **the suspected cause was disproved by run 7158; the flake has another cause
(WP-4)**: run 7158 failed with `ERR_ABORTED` on Windows with every identity fix in place and none of the ADR 0030 code
(handled apart, with a local Windows reproduction). Do not call a cause confirmed until a run shows it, and keep the fix
for a real mistake apart from the hunt for a flake's cause. The sweep
found the same mistake in four shapes across the family: a snapshot killed later, a tree built from parent pids (which outlive
their processes on Windows; an "explorer orphan" then joins a browser's tree and its memory cap), a `killpg(child.id())`
after `try_wait` had reaped the child, and a pid trusted from a file. The first fix had a gap of its own: macOS's
`proc_pidinfo` refuses a setuid child (EPERM), so such a child had no identity and ran unsupervised; the start time comes
from `sysctl(KERN_PROC_PID)` instead, which answers for every process.
**Rule 53 (ADR 0033):** never act on a reusable OS name (pid, port, temp path) read earlier: hold it (handle, job, listener)
or verify its identity (`paddock_proc::ProcessId`, pid plus start time) right before acting; trees follow parents by age or
job membership. Clippy's `disallowed-methods` and check C53 enforce it. **Also:** when a flaky test fails right after
another test in the same binary ended something, suspect the ending before the failing test; and test an identity source
with a process of another user, not only your own.

## 2026-10-30: a caller that waits on macOS `BACKGROUND` threads waits as long as the machine is busy (Debrief)

`debrief-cli`'s `the_report_is_written_as_html_and_never_overwritten_silently` took 3 to 9 s on the Mac runner most of
the time, 36 s, 63 s and 102 s (timed out) on busy runs, and timed out again in debrief.yml run 7150; on Linux it never
passed 10 s. The report's analyses run on Debrief's shared mining pool, whose threads
`debrief_core::threads::lower_priority` put in macOS's QoS class `BACKGROUND`, the lowest class: it is kept to the
efficiency cores and runs only after default and utility work, with its I/O throttled. While the shared Mac was loaded,
the CLI's foreground thread waited on workers that barely ran. Measured on the Mac mini with 14 `nice -n 20` CPU hogs,
which normal-priority threads preempt: the report build went from 0.16 s to as much as 12.7 s while the import beside it
did not change; with `UTILITY` the same build stayed at 0.09-0.33 s
([performance.md](../apps/debrief/docs/performance.md), 2026-10-30 block). **Lesson, now rule 52:** work a caller may
wait for (a report, an analysis, an import blocked on a lock, a download) never runs at an OS's starvable background
level; lowered means `UTILITY` on macOS, nice 10 on Linux and below-normal on Windows. Family ADR 0020 was amended the
same day: its `Background` class uses those levels, and only `Idle` work, which nobody waits on, may use the starvable
ones (check C54).

## A launchd `Background` job is DARWIN_BG as a whole: thread QoS cannot lift it (2026-10-30, Dyno)
After Debrief's report waited on `QOS_CLASS_BACKGROUND` pool threads (100 s instead of 3 s on a loaded Mac), Dyno's disk scan
had the same class, and the agent itself ran as a launch agent with `ProcessType Background`. launchd spawns such a job as
`POSIX_SPAWN_PROC_TYPE_DAEMON_BACKGROUND`; xnu makes that task DARWIN_BG "always, no exceptions" and clamps every thread of it
to the `BACKGROUND` band (`osfmk/kern/task_policy.c`, `thread_policy.c`; `ps -M` shows priority 4 on every thread of such an
agent). Every request of Dyno's app is answered inside the agent, so the whole UI waited on starvable threads, and a thread set
to `UTILITY` there stays clamped.
**Rule:** a process that answers requests somebody waits for is never a launchd `Background` (or `Adaptive` without XPC) job;
use `Standard` and lower individual threads instead, at the gentle levels of rule 52 (`QOS_CLASS_UTILITY`, nice 10,
`THREAD_PRIORITY_BELOW_NORMAL`). Tests read the class back (`pthread_get_qos_class_np`) and the plist's `ProcessType`.
On Linux the same goes for the idle I/O class: Dyno's systemd unit now says `IOSchedulingClass=best-effort` with
`IOSchedulingPriority=7`. **Follow-up (ADR 0020):** Dyno's `dyno_platform::priority::lower_current_thread` copies Debrief's
`debrief_core::threads::lower_priority`; both go once the shared `paddock_jobs::exec::priority::apply_current(Class)` exists,
and the shared `paddock-service` renderer's `low_priority()` output moves off the starvable levels then: the systemd
text (`IOSchedulingClass=idle`) to best effort 7, and the launchd text's `LowPriorityIO` (I/O throttled like the background
band), both still in its golden files.

## 2026-10-30: on Windows a `BELOW_NORMAL` thread does not run while normal threads are ready (Dyno)
The batch that lowered every Dyno agent thread to `THREAD_PRIORITY_BELOW_NORMAL` (rule 52's gentle Windows level) turned
three Windows CI tests red (runs 7166 and 7168): the baselines test found no sample because the sampler had not ticked
yet, and the counters listing passed its 10 s deadline (2.5 to 7.4 s at normal priority on main). Windows schedules
strictly by priority: a ready thread of a higher priority always runs first, and a lower one is preempted the moment one
becomes ready ([Scheduling Priorities](https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities)).
The dynamic boosts are for foreground windows, input and finished waits
([Priority Boosts](https://learn.microsoft.com/en-us/windows/win32/procthread/priority-boosts)); the anti-starvation boost
for a ready thread that has not run comes only after seconds. That figure is from Windows Internals, not checked against
Microsoft Learn: its [Priority Inversion](https://learn.microsoft.com/en-us/windows/win32/procthread/priority-inversion)
page (fetched 2026-10-30) describes AutoBoost, priority floors for the owner of a lock a higher thread waits on, not a
timed boost. None of this shares the CPU the way macOS QoS or Linux nice do, so on a machine whose cores are busy with
normal work, a lowered thread waits seconds at a time. A review then found more threads a person waits on that ran
lowered: the pinned counters reading (it holds one of the two slots the Counters page's requests use, so stalled readings
would make a request come back Busy), the reverse DNS behind Look up names, the Network page's traffic refresh and the
deadline probes at the start of a scan. **Rule (rule 52, amended):** every thread has a role. Live work is what a person
waits on within a deadline or for a screen to fill (requests, probes with a deadline, a sampler, a page refresh) and stays
at `THREAD_PRIORITY_NORMAL` on Windows; bulk work is long work with progress and Cancel (walks, scans, compaction,
exports), is lowered on every OS, and on Windows accepts stalls of seconds. Dyno decides the level in one place,
`dyno_platform::priority::level(ThreadRole, Os)`, and a test per call site reads the role back
(`priority::current_role`).

## 2026-10-30: newer Windows 11 builds list a listener with `dwState` 0; the table says it listens (Dyno)
Dyno's Windows socket tests failed on the CI runner from 2026-10-29 on, with the same commit that had passed hours before.
A first fix assumed a short-lived BOUND state (100) and re-read the tables for ten seconds; the real value, printed only
after the reader kept undocumented states as `UNKNOWN (n)` (CI job 22533), was 0, which `MIB_TCP_STATE` does not define,
for the test's own listener, while the accepted connection on the same port was ESTABLISHED. The runner had just
installed the optional preview update KB5127653 (Windows 11 26H1, build 28000.3365, 2026-10-28); its release notes say
nothing about the TCP tables
([Release Preview build 28000.3351](https://learn.microsoft.com/en-us/windows-insider/release-notes/release-preview-26h1/build-28000-3351)),
and another project saw the same on build 26340.9596
([Hyact/workbuddy-switch-plus#2](https://github.com/Hyact/workbuddy-switch-plus/pull/2)); a build 26300 VM still reports 2.
**Rule:** never decide what an OS record is from an undocumented or drifting field when a documented source says it:
Windows documents `TCP_TABLE_OWNER_PID_LISTENER` as all listening endpoints
([TCP_TABLE_CLASS](https://learn.microsoft.com/en-us/windows/win32/api/iprtrmib/ne-iprtrmib-tcp_table_class)), so Dyno
reads the listener and connections tables and keeps a value it cannot name as `UNKNOWN (n)`. And when a test fails on
a value nobody can see, first make the product report the raw value, before guessing a fix.

## 2026-10-30: a stored UTC offset nobody filled in made every "local" day a UTC day (Dyno)
The Issues e2e test failed on CI at 00:07 UTC: it ran on the wall clock against mock issues dated relative to it (the newest
35 minutes ago), so there was nothing "Today" in the first 35 minutes after midnight (a test bug, fixed by pinning the clock
and zone). The audit behind it found a real one: the engine took local days from a setting, `utcOffsetMinutes`, that defaulted
to 0 and that nothing in the product set, so today's score, quiet hours, "until tomorrow", maintenance windows, baseline words
and Time Machine dates were all in UTC while the UI showed local days. **Rule:** local time is a port read from the OS for each
instant (daylight saving included), never a stored offset; every day boundary goes through one set of day functions; and a test
of anything with "today" in it pins both its clock and its time zone, and runs once in a zone far from UTC (Dyno ADR 0024).

## Read the failing environment's own logs before building a reproduction (2026-10-30)
Windows CI failed one real-browser load in nearly every crate job with `net::ERR_ABORTED` and no connection at the test
server. Four rounds guessed at the driver (first navigation, pid reuse, a network-service restart during the load), and
the local Windows VM never reproduced it: it is Arm, and Chrome never sandboxes the network service on Arm, so the VM
cannot take the code path CI takes. The Chrome logs that CI had already kept as artifacts showed what differs on Windows:
in every browser the testing config's sandboxed network service could not start (Chrome for Testing in a user folder
grants app containers nothing) and was started again unsandboxed. The lost loads came 3 to 4 s later, when the profile's
own requests failed too, so the restart was a lead, not a proven cause, until CI run 7170 (testing config off) ran 25
Windows browsers with no second start, no failed profile request and no lost load. **Rule:**
for a flake, read the evidence the failing environment already wrote (logs, artifacts, the program's own counters) before
building a local reproduction; check that a reproduction can take the same code path (architecture, feature flags,
sandbox); and write a cause down as confirmed only after a run without it. Test the suspected cause directly (the
browser's own count of failed helper starts is zero), so the check fails on every run, not only on the unlucky ones.
**Follow-up:** the apps' runtime store keeps Chrome for Testing under the user's local app data, which grants app
containers nothing either; granting S-1-15-2-1 and S-1-15-2-2 read and execute there at install time is still to do.

## A count of a queue's contents is a reservation, never a separate copy (2026-10-30, Debrief)
The Event Log follower kept its queue length in an atomic counter beside the bounded channel: readers added after a send,
the writer subtracted after taking a batch. The two updates were never atomic with the channel, so the count raced both
ways: first it dipped below zero and showed 18446744073709551615 (7d38d47a0 clamped it, which hid the cause), then it
showed 5 queued with a capacity of 4 when a reader refilled the slots the writer had freed before the writer counted them
out (debrief.yml run 7175, at load 40; fixed in b935d03a6). This is the second instance of one pattern (ER-21). **Rule:**
a count of a queue's contents is a reservation taken before the send and given back after the take, never a separate copy
updated after; then it stays between what the queue holds and its capacity by construction, needs no clamp, and doubles
as the back-pressure. Test such a race by stalling one thread at the hand-over point (a probe or barrier), never by load.

## An unbuilt UI hid a broken installer build because tests passed (2026-10-30, Dyno)
Dyno's release workflow had no `prepare: npm run build -w ui`. Every test passed, but every installer build failed on a
clean runner; a leftover `ui/dist` in the working trees had hidden it. **Rule:** a change to an app's release path
(release workflow, shared app-release workflow, bundle and prepare steps, Tauri config, imports of shared packages) is
proved by building that app's installers (`<app>-release.yml` with gates on and the bundle built) before it lands on
main (rule 54). The ci-actions test now checks that every release workflow whose app imports `@decryp7/paddock-ui`
builds the UI.

## A discovery file is a hint, never proof (2026-10-30, Dyno)
Dyno's `agent.json` (the file that names the agent's pid and port) outlived its agent. The app read it, took it for a
live agent, showed a false "Agent stopped" and looped on exit code 3. Separately, `Plan::Replace` deleted a
file it no longer owned: it had read the file earlier and removed it later, after a newer agent had written its own.
**Rule (53, amended):** a file that names a pid or port is only a hint. Act on it after a held lock or a live connection
proves the process is there, and remove it only if it is still the same file (same pid and start time). This is rule 53's
reusable-name rule applied to files.

## A bug-fix review runs a mutation check (2026-10-30)
Rule 5 asks for a failing test first, but a reviewer reading the diff cannot tell whether the new test would have
failed without the fix: a test written after the fix may never have failed. **Rule (5, amended; user, 2026-10-30):**
the reviewer of a bug fix runs a mutation check on the fix (`cargo mutants --in-diff <diff>` for Rust where available,
otherwise, and for TypeScript, revert the fix and run the new test), in the reviewer's own worktree with a bounded run
(`-p <crate>`, `--timeout-multiplier`, `CARGO_BUILD_JOBS=2`), and reports whether the test fails. **Follow-up, not
built:** a CI job that runs `cargo mutants --in-diff` over the shared crates' changes, to be added after the releases.

## A start window on a wall clock doubled Dyno's agent on a cold disk (2026-10-30, Dyno)
In a fresh macOS VM (a 32 MB binary took 50 s to read) the first launch of Dyno 1.0 ran two `dyno-agent run` children
on one `dyno.db`; after Quit one stayed as an orphan. The app's log showed the cause: the link gave up on its agent after
a fixed 5 s (`SPAWN_WAIT`), started another after a 2 s backoff, and its launcher dropped the first process's handle when
it stored the second, so Quit stopped only the newest. Nothing stopped a second agent either: no lock on the data folder,
and a busy database looked damaged to the store, which moves a damaged file aside. **Rule:** a single-instance process
holds an OS lock for its whole life (a file lock on its data folder) and a second one exits before it opens anything; a
host waits for a child it started on the child's run state (ADR 0031), never starts another while it holds one, and ends
it through the handle it holds (rule 53). Reproduce with a start gated by the test, not with load.

**Follow-up (review of the hand-over, 2026-10-30):** a wait judged on run state still needs a way out the user can see.
Three waits could show only "Starting..." forever: a service that "supervises" on paper (switched off in Login Items,
job dropped), a child that runs but never writes its discovery file, and another agent holding the lock after its file
was deleted. **Rule:** every wait without a time limit reports a cause and a Retry once it is long (ER-19), a "ready"
discovery file must be newer than the wait or answer (a stale file caused exit-3 launch loops and a false "Agent
stopped"), and a supervisor's claim is checked against the lock, which nothing can fake. Open: kickstart after hand-over,
re-probing `RequiresApproval`, the `is_held` probe rate, the `/tmp/Dyno` fallback (ADR 0010 §11).

## Five fixes in two days meant a supervisor with no owner and no state (2026-10-30, Dyno)
Dyno's agent link needed five rounds of fixes on the release path (a fixed wait that doubled the agent, endless silent waits,
a stale discovery file taken as ready, a launch before a Windows lock was released, a quit outcome read twice), each approved in
review and each followed by a failure on another OS. The cause was not any one interleaving: the supervisor had no single
owner and no explicit state. Five threads wrote 12 flags, "which wait are we in" was the function being executed, and each
decision was assembled from several reads that other threads and the OS changed in between. **Rule:** a supervisor of a
process (start, find, hand over, stop) is a pure `step(event) -> commands` machine with one owner thread; its phase says what
it waits for; every answer from the world carries the number of the thing it is about (look, connect, child generation,
connection) and a stale one is dropped; a child is held until its end is reported; a decision that must not change (the quit
outcome) is taken once, in `step`, and stored; the owner never blocks (slow ports answer as events); every transition is
published and tests wait on it, not on flags or sleeps. Prove the machine with a bounded model check of every interleaving,
and prove the check by making each historical bug a mutant it must catch (Dyno ADR 0025). Two things the first prototype got
wrong and a review caught: releasing the child before it is gone (a second agent while the first still runs), and an event
without the generation it is about (an old child's exit taken for the new one's).

## A fix in the shared crate reached nobody; the app copy had the bug (2026-10-31, family)
Dyno 1.0's login blocker B4 (the app replaced its healthy launchd agent four times at every login) came from Dyno's
`service_repair.rs`, ported from Pitwall. `paddock-service` already had the fix (its parser skips launchd's nested blocks),
but no app used that part of it, and the copies in Pitwall and Pitcrew had the same defect. In the same week the Windows
network-sandbox switch went into `paddock-chromium`, which no app depends on, and the EBADF fix turned out to be needed in two
IPC endpoints. 37 of the 51 shared crates had no direct dependency from any app. Rules 42 and 43 said what to do once code is
shared; nothing tracked the copies in between, the freeze sent quick fixes to the app copy only, and "deleted later" had no
date. **Rule (55, ADR 0034):** every mechanism that exists more than once is listed in `docs/known-copies.toml`; a fix goes to
the canonical implementation and every copy in the same change (a freeze does not stop it) or a copy is waived with a reason;
each copy has an owner and a tag by which it is deleted, and that tag is refused while the copy exists. Before fixing a bug in
any service, IPC, driver or UI helper, look it up in the register first.

## The fixed start wait hid in a helper named `connect` (2026-10-31, Debrief chromium-driver)
Debrief's `chromium-driver` real-browser tests failed three times on the loaded Mac runner (CI 7213, 7250 and the run after)
with `launch: Timeout("the browser start")` after 20 s: two Chrome for Testing cold starts side by side, beside the heavy
suites, did not write `DevToolsActivePort` within the fixed 20 s of `launch.rs`'s `connect`. ADR 0031's start judge had
already replaced the same limit in `paddock-chromium` (the canonical), but the fix never reached the copy (rule 55), and C51
did not flag it because it looks for fixed waits only inside functions named `start*`, `spawn*`, `launch*` or `boot*`.
**Rule 55 and ADR 0031 applied:** a start's readiness wait is judged by `paddock_proc::StartWatch` wherever it lives,
whatever the helper is called; a fix in a canonical implementation goes to every copy in `docs/known-copies.toml` in the
same change, or the copy is waived with a `Copies-waived:` trailer (a register note is not a waiver). Real-browser tests take
turns in a nextest `real-browser` group (max-threads 1) in every workspace that runs them, with a CI budget waiver longer
than the spin patience.
**Endpoint folders leak a third way (CI 7278):** a test process that ends normally while a plugin still starts leaves
`debrief-plugin-<hex>` behind, because the exit clean-up of the test helpers removed their own folders but not the live
endpoints. Each fix so far armed one more path (lifeline exit, SIGTERM, now normal exit); the class ends when
`debrief_ipc` registers its own `atexit` removal when the first endpoint is made, so every binary has it, not only those
that arm the helpers.

## Asking the kernel about another program's descriptors can break that program (2026-10-31, Dyno)

Dyno's macOS socket list called `proc_pidfdinfo` on every socket of every process (`netstat2`, and `lsof -p` / `lsof --` for
the details page and "what locks this file"). Each call holds an I/O reference on the other program's descriptor; if that
program, or a child it spawned that still holds a copy of the descriptor until its exec, closes it meanwhile, XNU drains
the descriptor and marks the shared socket `SS_DRAINING` for good, and every later waiting read or write on it fails with
EBADF (12 in 100 loaded runs with the sweep, 0 in 100 without). A monitor must not change the thing it watches, and a read
that takes a reference on a descriptor is a change. **Lesson:** read other programs' sockets from the kernel's own lists
(`sysctl net.inet.{tcp,udp}.pcblist_n`: nothing is held), and ask about another process's descriptors only for files, whose
file operations have no drain (`fo_no_drain`; sockets, pipes and kqueues do). Never `lsof` on macOS (Dyno ADR 0026). Three
things found on the way: (1) the kernel hands the whole list only to some processes (macOS 27.2: a code-signing identifier
with a dot), so a list is checked against its own announced count and owners, and a cut-down list is an error with the fix,
never an empty page; (2) Tauri re-signs sidecars and resets a file's identifier, so the identifier comes from an embedded
Info.plist and the built bundle is checked; (3) the header count includes dead entries, so it can only be used together
with something else (here: no socket of another process in the list).

## tauri-build copies every sidecar over the binary cargo just linked (2026-11-01, Dyno)

Dyno's signing-identifier test for the dyno command failed in a release run and passed in CI on the same commit and
runner: codesign named the copy `dyno-da39a3ee…` (the SHA-1 of nothing), `Format=generic`, 0 code pages. The app's
build script writes empty sidecar placeholders (`binaries/<name>-<triple>`) so tauri-build accepts `bundle.externalBin`,
and tauri-build then removes `target/<profile>/<name>` and copies each sidecar there: the 0-byte placeholder replaced the
`dyno` and `dyno-agent` that cargo had just put there. Whether a test saw the real binary depended on whether the build
script ran before or after cargo placed the binaries in that cargo run (20 of 20 failures in a loop that reruns the build
script with the binaries fresh; 0 of 50 with the fix). Causes ruled out: a concurrent rewrite by another job (each slot has
its own build folder), `std::fs::copy` truncating (the source itself was 0 bytes), codesign racing the linker (the
binary was not relinked). **Lesson:** a build script must never write into the build folder next to cargo's own outputs;
while the sidecars are placeholders, Dyno hides `externalBin` from tauri-build (a `TAURI_CONFIG` merge patch), and a test
that copies a built program first checks it is not empty. Pitwall and Pitcrew wrote the same placeholders; the mechanism now lives
once in `paddock-build` (the next lesson), and Dyno's local copy goes after its 1.0 tag.

## A build script that tauri-build follows must not leave placeholders where cargo puts programs (2026-11-01, shared fix)

tauri-build 2.7.4 copies every `bundle.externalBin` file to `target/<profile>/<name>`, replacing the program of that name that cargo
linked. The apps' build scripts write empty placeholders (`binaries/<name>-<triple>`) so tauri-build accepts the list for tests and
`cargo check`, so whenever the build script ran after cargo placed the sidecar, the program in the build folder was 0 bytes (Dyno's
signing test saw `dyno-da39a3ee...`, the SHA-1 of nothing, in one of two runs of the same commit). Pitwall and Pitcrew had the same
build script and the same bug: with `pitwall-server` / `pitcrew-service` built first, `cargo test -p <app>-app` left them empty (the new
`tests/sidecar_placeholders.rs` failed before the fix). **Lesson:** this is a mechanism, so it lives once, in `paddock-build`
(`sidecars` module: hide `externalBin` from tauri-build while a sidecar is a placeholder, delete empty copies of older builds), and an
app's `build.rs` is `paddock_build::Build::new().sidecars([..]).run()`. A third app with sidecars gets it by calling that, never by
copying Dyno's or Pitwall's lines. Dyno's local copy is replaced by the helper after its 1.0 tag.

## What the browser e2e cannot see: three Dyno 1.0 features that passed it and failed in the real app (2026-11-01, ER-21)

The Dyno 1.0 VM tests found three tray/Settings features that were green in the browser build and the Playwright e2e and broken in the installed app:
(1) a click on the tray icon did nothing: the shell's `toggle_popover` looks up a `Popovers` spec that Dyno never registered (the browser build
never calls it; unit tests used a `Popovers` they built themselves); (2) Settings' "Popup widgets" and menu-bar editors loaded for ever: the
`pd.settings` dialog contract did not declare `widget_layout`, `widget_choices` and `popover_preview`, which the shell refuses to a dialog window
(the mock backend has no command names); (3) "Customize widgets…" did not re-target an open Settings window: the new inputs travelled on a
`BroadcastChannel`, which the browser e2e shares between its pages and the other webview of the real app did not hear. The pattern: **the browser build
replaces the part of the app that is the real contract** (Tauri state, command contracts, native events), so a feature can be complete there and
unwired in the app. Fixes through the class, not the instances: a Rust start test that the app's builder registers the tray popover
(`the_start_registers_the_tray_popover_a_click_opens`); a spec that runs the Settings widget editors over the real `TauriBackend` with a recording
`invoke` and checks every command against the generated `pd.settings` contract (`settings-dialog-commands.spec.ts`; a new command used by those pages now fails
it until the contract names it); and a native window-system event (`paddock://dialog-inputs`) for re-targeting, with a unit test of both ends. What is still
open: ADR 0026's recorder-based `dev check dialog-commands` (every command a dialog calls against its contract, in every app) does not exist yet, and a
tray click is only tested up to "a popover is registered", not "a window opens". Before calling a window, tray or dialog feature done, read the log of the
installed app once (the three above were each one line of the app's own log).

## A warm module made a chart draw before teardown, and dispose threw on jsdom's missing canvas (2026-11-01, Dyno)

Dyno's `widgets-ui.spec.ts` board test failed now and then in full runs on a busy machine with "1 fixture threw errors
during cleanup": the test body passed, then `dy-chart`'s destroy called ECharts' `dispose()`, which cleared a canvas layer
whose 2D context was null (jsdom has no canvas) and threw. That needs ECharts to have loaded and painted a frame before the
fixture is destroyed. Alone, the dynamic `import('./echarts-setup')` is cold and the test ends first; with `isolate: false`
an earlier spec in the same worker (`chart.spec.ts`) leaves the module warm, so it loads in a turn, and a loaded machine
stretches the test's `vi.waitFor` polls long enough for a frame. Reproduced every time with one worker and the victim last
(`chart.spec` then `widgets-ui.spec`: red; `format.spec` then `widgets-ui.spec`, or alone: green). Causes ruled out: leaked
fake timers (the AnimatedNumber block restores them; the error is a canvas TypeError), a waitFor bound (the assertions had
passed), an unawaited effect in the store or edit path (the layout and edit counts were right). **Cause in product code:**
the chart contained a failed draw but not a failed dispose, and drew on a canvas with no 2D context (WebKit gives none when
canvas memory runs out), where ECharts also throws from its frame loop. The chart now checks for a 2D context before it
starts ECharts (otherwise it shows "This chart could not be drawn") and logs, instead of throwing, a failed dispose.
**Lesson:** a component's destroy never throws; specs that render a real third-party renderer depend on module warmth and
load under `isolate: false`, so the renderer's missing-platform path must be a designed state (rule 49, ER-29).

## A test that copies the current status inherits whichever state the race left (2026-11-01, Dyno)

Dyno's disk-map ghost test failed now and then in full shuffled runs under load (seeds 207 and 221 of 22): the "Not counted
yet" arc was drawn, but so was "Hidden space". The test pushed `{ ...store.status(), seq: 70, state: 'running' }`, and only a
finished scan carries `hiddenBytes`, so the copy was of the finished status: the mock's step timers (faked) had already run
when it was taken. Whether they had depended on timing, not on the code. Letting the scan finish first made it fail every
time with the same message; pushing a complete running status (no hidden space, no scan time) made it pass in both orders.
The jobs test in `mock-backend.spec.ts` had the plain form of the same thing: a fixed 30 ms wait for chained timers.
**Lesson:** a test builds the state it pushes from scratch, or after waiting for a known state, never by copying whatever is
current. `vi.waitFor` also moves faked timers on every poll, so a fake-timer spec cannot use it to hold a moment still.

## A spy on a faked timer outlived the clock and froze every later spec in the worker (2026-10-31, paddock-ui)
`panel-snapshot.spec.ts` timed out at 5000 ms in CI runs 7257, 7274 and 7276 (with the notify reference app in 7276),
though the whole spec takes 0.4 s alone and a snapshot 1 to 4 ms. Measured inside full `ng test paddock-ui` runs, the page
CSS was not the cause (one style sheet, 46 to 118 characters, at every file start; every phase under 0.5 ms). The cause
was a dead `setTimeout`: a new spec on that branch (`settings-close.spec.ts`) made `vi.spyOn(globalThis, 'setTimeout')`
under `vi.useFakeTimers()`, its own `afterEach` then called `vi.useRealTimers()`, and the kit's `restoreAllMocks()` put
back what the spy had wrapped: the fake, bound to a clock that was gone. Vitest 5.0 keeps a spy's restore in its registry
after `mockRestore()`, so this happens even when the spec restores its spy itself. Specs share one window per worker
(`isolate: false`), so every later spec in that worker that waited on a real timer (the snapshot's 5 ms "too slow" case,
the notify reference app's two repair tests in run 7276) hung until the test timeout; with one worker, 16 tests in 8 files
failed. Which spec failed depended only on which files the scheduler put after it. **Kit behaviour (no new rule):** the
test kit (`installTestKit`) restores spies before switching to real timers, records the real timer globals at the first
install in a worker, puts back any it finds replaced around every test, and fails the test that left one replaced by
something other than a spy, after every `afterEach` (a throw inside the hook chain skipped Angular's TestBed clean-up and
failed the next TestBed test too). Never `vi.spyOn` a timer while fake timers are on; count timers with
`vi.getTimerCount()`. Measure inside the failing run before fixing the suspected cost: the leading hypothesis (CSS piling
up) was wrong.

## Pitcrew 1.1 could download an update it could never install (2026-10-31, family)
Pitcrew 1.1 built the shared `UpdatesBuilder` with a public key but no `.installer(..)`. The update checked and downloaded; then
"Restart to update" failed with "this build has no installer", and "Install when the app quits" recorded a failed install the user
saw at the next start. A second app had the same wiring, and Dyno, Pitwall and Spotter had wired it by hand. The builder allowed
the state and nothing but a reviewer's memory prevented it. **Rule (ER-21, no new number):** a shared builder makes the wiring
choice explicit and refuses the unchosen one at the app's start, independent of the CI's secrets (`tauri::install` returns
`InstallerUndeclared` unless `.installer(..)` or `.no_install()` was called), the status says what the build can do
(`canInstall`) so the UI offers only that, and a conformance check (C59) covers every app.

## A detached chart host resized to 0 stayed at height 0 (2026-11-03, Dyno; open question)

Dyno 1.0.0: switching the history range left the chart empty until the view was reopened. The panel's skeleton detaches the
projected `dy-chart` between two answers; the `ResizeObserver` then resized ECharts to 0 px. After the host came back the
canvas kept `height="0"` (width 982) although the observer saw the 0 to N change, and ECharts warned "Can't get DOM width or
height". **Why the later resize did not restore the height is not explained** (not reproduced outside the gated e2e, only in
Chromium). The fix avoids the cause instead of explaining it: `fit()` ignores a container with no size and resizes only
when the size differs from the chart's own. **Lesson:** do not resize a third-party renderer to a size of 0; if it happens
again with the fix in place, find the ECharts/zrender step that skips the restore before adding more guards.

## "Day 1 of 7" on the second day: elapsed 24-hour periods shown as a calendar day (2026-11-03, Dyno)

A user installed Dyno 1.0.0 on a Monday and, after Tuesday's boot, the "Is this normal?" card still said "day 1 of 7", which
read as a reset. Suspected first: a different data folder for the Windows `Run` entry's agent than for the app's child, so each
start saw a fresh database. The code refuted it: both run `dyno-agent run` as the same user with no extra environment or
working folder, and `Dirs::current` reads only `DYNO_DATA_DIR` and `%LOCALAPPDATA%`; `firstSeen` is never deleted or rewritten.
The cause was the rule: the engine counted complete 24-hour periods since the first sighting (0 for the first 24 hours, 1 for
the next 24) and the card showed `max(1, daysDone)`, so both of the first two days said "day 1", and with an evening install
a morning boot two calendar days later did too. A restart test over one database file on a fake clock reproduced it (Tuesday
shown as 1, expected 2). Contributing: `firstSeen` was stored lazily on the first Baselines request, so an agent started at
logon without a window did not start the count, and nothing in the log said what the learning counted from. **Lesson:** a
progress count shown to people is defined once in the engine in the unit people read (local calendar days, 1-based) and sent
as such; the UI never clamps or offsets it, and the start of the count is recorded and logged when the process starts.
The decided rule (user, 2026-11-03): the card counts local calendar days, and Ready also needs 7 times 24 hours since the
first sighting; on the last day the card says when it will be ready, and the engine sends that time (`ready_at`), so a clock
change moves it by exactly the hour the clock moved.

## A sidebar spec counted the links once and indexed past the end (2026-11-03, shell-demo; CI run 7452)

`shell-demo-framenav.spec.ts` read `links.count()` right after the frame opened and later asked for `links.nth(count - 1)`; one run
found the list shorter by then ("element(s) not found" at index 9, light theme only, passed in the runs before). The spec now uses
`links.last()`, which is resolved against the page when it is asserted. **Lesson:** never freeze a locator's count in a number and
index with it later; use the locator that names the position. (Same day, run 7453: the Angular dev server of the Dyno chromium
shard 2 died with `lmdb` "Cannot read properties of null (reading 'refCount')" in its build cache before the first test; the cause
is not found, the job passed on the other shard and in earlier runs; if it repeats, set `NG_BUILD_CACHE=false` for e2e.)

## A stall test read the runner's log right after the exit event (2026-11-03, Pitcrew; CI run 7459)

`a_stalled_runner_is_stopped_gracefully_and_restarted` failed once ("the runner shut down by itself after the stop request", 434 of
435 passed, nothing in Pitcrew changed). Candidates and what refuted them: the supervisor treats a self-exit after a graceful stop
differently (no: the `Exited { expected: true }` event the test had already seen is that path); the fake never logs the line (no:
it logs before it exits; 100 stress runs under four busy loops at nice 19 all passed); the log file is flushed late (no:
`write_line` runs in `line()`). The cause: the runner's output reaches the supervisor through a reader thread and a channel, the
exit through a probe of the process, so the exit can be handled before the last line; the test waited for the exit and then read
the log once. Confirmed by putting the cause back: a temporary 300 ms delay per line in `read_lines` failed the old test with the
reported message every time, and the waiting test passes 10 of 10 with it. The test now waits on the line with the real clock.
**Lesson (class: two event sources, one assumed order):** a test that waits for event A on one path never reads state fed by
event B on another path without waiting for B; to measure a rare order race, inject the delay on the path that is assumed fast.

## A baseline read after an async step hides what changed during it (2026-11-03, Dyno dialogs)

Dyno 1.0.0's Updates window showed a scrollbar beside one line. `DialogFrame.watch()` took its "last measure" from a fresh
measure read after the `dialog_ready` round trip, so content that arrived during the round trip was taken as already sent and
never refitted. **Confirmed on Windows 11 (VM, 2026-11-03, 100 % and 150 %):** the 1.0.0 code ended at inner 105 px (the first,
loading state) with the scrollbar of the user's screenshot, and the change at 195 px. Ruled out by the new fit log:
frame rounding (the frame matched `AdjustWindowRectExForDpi`, the inner size is rounded up), the viewport delta (0) and
fonts (same content height in the later fit). Found on the way: tao resizes the window itself after `ScaleFactorChanged`,
so a refit inside that event is overwritten (now scheduled); and a hand-resized window must stop being fitted. **Lesson:** a change detector's baseline is what was last sent, never what
is measured now; and a fix for a user report is claimed only after the report's own scene (Windows, 100 % and 150 %) is run.
Follow-up: Spotter and Pitwall have no dialog-fit e2e like Dyno's and the shell demo's.

## `maxminddb` `Reader::verify` never returns on a search tree with a cycle (2026-11-03, Dyno)

Dyno's location databases are downloaded, so their bytes are untrusted. A byte-damage scan over a tiny valid file (every
position, four values) hung on one case: the first record of the root node set to 0, which makes the root its own child.
`Reader::verify` (`maxminddb` 0.27.3) then loops and grows memory until the process is killed (SIGKILL after 5 minutes); a
property test showed the same as a test that "ran for over 60 seconds" with nothing printed. In debug builds the same crate also
panics on arithmetic overflow in `resolve_data_pointer` for a damaged pointer (release wraps and its next bounds check rejects
it). **Fix** (`dyno-agent/src/geo_db.rs`): before `verify`, a bounded walk of our own proves the tree is acyclic and no deeper
than 128 bits (one byte of state per node, `O(nodes)`); opening and lookups run under `catch_unwind`; tests pin both; a fuzz
target (`apps/dyno/fuzz`) runs the open-and-verify path. **Lesson:** a library's "verify" is not a safe parser for untrusted
bytes; scan single-byte damage of a valid file with a time limit per case, and do not trust a green proptest that never
finishes.

## A window's size is a state with one owner, not a sequence of events (2026-11-04, ADR 0043)

Sixteen dialog-size fixes in two weeks each closed one gap between two baselines (a handshake measure, a watch started later, a
scale-change hook). The class is "nothing states what the size should be at any moment". ADR 0043 makes the size a pure state
machine (`paddock_dialogs::size`) fed by numbered measures `(epoch, seq)` and the OS's own events, carried out on the main
thread; the page has one measure loop with a last-sent baseline and a guard of two sends per change. **Lessons:** (1) an
echo of our own `set_size` is told from the user's resize by a queue of what was asked, and by the OS's signal where it has one
(`WM_SIZING`, live resize), never by comparing sizes alone; (2) anything that touches a native window (`ns_window`) must be
left out under the mock runtime, which hands out a dangling pointer (SIGSEGV, not an error); (3) an e2e that fixes the
viewport to a width other than the contract's asserts the wrong thing: at another width the content wraps differently and
the loop rightly measures again.

## Tests never wait on `requestAnimationFrame`; they wait on a state the page exposes (2026-11-04, ADR 0043)

The dialog size e2e declared the page quiet after two animation frames. WebKit withholds frames from an occluded window, and
with four parallel workers a window can get none for seconds: 32 of 200 runs saw one height sent and the loop already quiet,
then hung waiting for frames that did not come. Refuted first: an oscillating loop (the height history was a single value)
and load alone. **Fix:** the page exposes `data-fit-pending` (the follow-up frames still owed) and `quiet()` waits for 0
pending and a sequence number that moved past its value before the change and then held (0 of 200). **Lesson:** a test
that waits on a rendering callback depends on whether the browser renders that window; wait on a condition the page states
itself (ER-12), and measure the failure rate before and after (ER-28).

## A fix to a cfg-gated test file went out unformatted (2026-11-04, Dyno; CI runs 7475 and 7476)

I rewrote three lines of `apps/dyno/crates/dyno-agent/tests/bench_units.rs` (a `#![cfg(target_os = "linux")]` file) and checked it with clippy and nextest on the Mac, which skip the file, and never ran `cargo fmt --all --check` after the edit; the local gates I ran were clippy, tests and conformance, not the formatter. The Linux job's fmt step found the long command chains. **Lesson:** after editing a Rust file, run `cargo fmt --all --check` in that workspace (rustfmt formats cfg-gated files too); a green local clippy or test run says nothing about a file the host does not compile.

## land-f's first CI run: eight causes, none visible on the Mac (2026-11-04; runs 7485 and 7486)

Everything below passed locally on the macOS dev machine and failed on the Linux or Windows runners. **Lesson:** a Mac-only local run
proves little for code that depends on the user id, the path separator, the font or the rasteriser; the checks that would have
caught each one are named.
- `paddock-dialogs` `any_interleaving_keeps_the_size_rules` (proptest, seed `ef445c8e`): the OS echoes requests A, B, A in order, but the
  echo of the first A matched the newest A (`rposition`) and took B and the second A off the queue, so B's echo arrived with nothing
  pending and was read as the user resizing. The state now remembers the requests an equal-sized echo took off the queue (`late`) and
  explains their own echoes. Regression: `the_late_echo_of_an_older_request_after_an_identical_newer_one_is_not_the_user`.
  A proptest with a random seed is a flaky test until its counterexample is a fixed test; run it with `PROPTEST_CASES=30000` after a change.
- `paddock-ipc` `a_client_finding_no_folder_...`: the second half assumed the test user is not root; the CI containers run as root,
  where a folder the client owns is the service's folder. Guarded as the test above it already was (`geteuid() != 0`).
- `paddock-service` and `dyno-agent` tests built paths with a hard-coded `/` (`"/Library/LaunchDaemons/x.plist"`, `rsplit('/')`),
  which differ from the code's `Path::join` on Windows: use `Path::join` and `file_name` in tests that compare paths.
- `dyno-plugin-host`: a plugin that exits at once may be gone before the bootstrap line is written ("exited before it took its
  bootstrap") or after ("exited before it connected"): a race, so the test accepts both.
- Clippy `assertions_on_constants` (Linux-only `edge/linux.rs`), `-webkit-user-select` missing on one rule (`webkit-prefix` test),
  `THIRD_PARTY_NOTICES.md` out of date for Dyno (`scripts/notices.sh dyno`): run the CI's cheap checks (`notices.sh <app> --check`,
  `node --test scripts/*.test.mjs`) before pushing a batch that adds crates or CSS.
- Dyno e2e at 500 and 900x600: the "Is this normal?" sentence ended in an ellipsis with its tooltip on the parent, and the network
  row's `flagged` and country chips could not shrink inside a clipping cell. The sentence now wraps and both chips ellipsize with their
  titles. Both pass on the Mac with the old CSS (narrower font): layout checks need the Linux run.
- `widget-grid.spec` "frame budget": ADR 0044 §7 sets 16.7 ms for the CI Mac; the Linux runner's software rasteriser measured 31.6 ms
  for the same drag. The budget is now asserted on macOS and only recorded (annotation) elsewhere.

## The real-window probe is outside the coverage gate (2026-11-04, paddock-dialogs)

`paddock-dialogs/src/probe.rs` (feature `probe`, 554 lines) is the ADR 0026 section 6 harness that opens real webview windows. It runs only
with `PADDOCK_REAL_WINDOWS=1` where there is a window system, so the line-coverage gate of the `crates` job (98.67% with it, over the 99%
line) never sees it, like the other harnesses it joins in the ignore regex (`paddock-proc` test child, `paddock-chromium` fake browser,
`paddock-ipc` test child). Its check is the `dialog-window-probe` job instead. **Lesson:** a harness is excluded by name with its reason beside
the regex, never by lowering the gate; and a job that stops at its first failing step (here coverage, before `cargo deny check`) hides the
steps after it: read a red job's later steps as not run, not as passed.

## Windows-only assumptions in tests that came in with land-f (2026-11-04, Dyno gates run 7494)

Five Dyno tests failed only on the Windows leg (2147 passed, 5 failed); each was a Unix assumption in the test, and one was a real checkout bug.
- `dyno-helper-service` unit, maintainer-script and polkit tests read `app/src-tauri/linux/*`; `* text=auto` checked them out with CRLF on
  Windows (`#!/bin/sh\r`, a unit that differs from its render). The packaging files are now `text eol=lf` in `.gitattributes`
  (`/apps/*/app/src-tauri/linux/**`, the macOS plists). The same files built on a Windows checkout would ship scripts that sh cannot run.
- `bench::export::tests::the_destination_is_checked_before_anything_is_written`: `/Users/x/results.csv` is not absolute on Windows (no drive). The product
  is right; the test now builds its paths from `std::env::temp_dir()` and adds a real `C:\...` case and a drive-less one on Windows.
- `the_helper_installs_and_removes_itself_on_linux_from_the_package` models Linux paths with the fake system's string keys: `Path::join` spells them
  with backslashes on Windows. `#[cfg(unix)]`, with the reason beside it.
- `helper_counters::a_sysfs_tree_...`: the fake sysfs tree has `intel-rapl:0`; a colon is not valid in a Windows file name. `#[cfg(unix)]`, with the reason.
**Lesson:** a test that builds a fake Linux or macOS tree runs on all three runners: build paths from the OS (`temp_dir`, `Path::join`), keep
test inputs LF in `.gitattributes`, and gate a test by `cfg` only with the reason (never `#[ignore]`). The check before pushing is `cargo xwin clippy
--all-targets` (an import used only by a gated test is an unused-import error on Windows).

## The macOS runner's stable-workspace step copies the checkout across volumes (2026-11-04, Dyno gates run 7501, job 28671)

`stable-workspace` timed out after 300 s on the Mac runner and its message blamed "a process left from an earlier job holds a file or lock". That guess
was wrong for this run: the step `mv`s the checkout from `/Users/decryp7/gitea-runner/work/...` to `/Volumes/d7sst7/ci/home/src/...`, and across volumes
`mv` is a full copy, which is slow while other jobs copy their checkouts at the same time (the same `mv` was running for another leg when this one timed
out). **Lesson:** a timeout message that names one cause needs the step to log the other likely one (bytes and time of the move, the source and target
volume); until then read "did not finish within N s" on the Mac runner as "check for a cross-volume copy under load" first. Runner fix: with the user.

## Two Dyno vitest specs that lose to a loaded runner (2026-11-04, Dyno gates runs 7494 and 7501)

- `widget-choosers.spec` "disables every add button once the menu bar holds its 32 items" added 28 items through the page, re-rendering a growing list
  of 5 to 32 rows of Material fields each time (5254 ms against the 5 s limit on the runner). The engine now takes items 5 to 31 directly and the
  page's button adds the last one: work that scales with the item count belongs in the arrangement, not in the step under test.
- `processes-page-actions.spec` "opens the row menu with Shift+F10 ...": the row's context menu is disabled while a row has no actions
  (`matContextMenuTriggerDisabled`) and live updates redraw the rows, so one key press sent at that moment is lost and a `vi.waitFor` after it never
  succeeds. The test now sends the key again to the row that is there, inside the `vi.waitFor`, until the menu is open. A real person's key press may be lost
  in the same way (the focused row replaced by a redraw, or a row whose actions have not arrived): **unverified, a follow-up** to check that live updates keep
  the focused row's DOM node and that Shift+F10 on a row without actions says why instead of doing nothing; not fixed here.
Proof, honestly (Linux, OrbStack `paddock-gate-arm`, 6 cores, 12 busy loops started and stopped by pid, 50 runs of each test):
- 32 items: the old test passed 50 of 50 but took 94.97 s (1.9 s a run); the fixed one passed 50 of 50 in 23.5 s (0.47 s a run). It was **not reproduced as a failure**
  (the CI runner needed 5.25 s), but the old test costs four times the work, which is what the 5 s limit trips on; the cause is the cost that grows with the item count.
- Shift+F10: old and fixed both passed 50 of 50 (15.4 s and 15.7 s); the failure was **not reproduced and the lost-key cause is from reading the code and the CI log, not proven**.
  The resend is harmless to keep, but if this spec fails again the cause is something else: record the row's `disabled` state and the actions at the moment of the key before guessing again.

## A CDK virtual viewport measures its height once; a body that grows later is blank (2026-11-04, paddock-ui `pd-table`, Dyno 1.1.0 RC)
Dyno's Processes page (458 rows) showed blank space under the last drawn row. `CdkVirtualScrollViewport` measures its own
height at start and on a window resize only; `pd-table`'s body height follows its frame (`maxHeight`) and its rows, so a body
that grew after the first measure drew only the rows its old height plus the buffer cover. Confirmed: a gallery spec grows
the body to 1600 px and found an 800 px gap before the fix and none after. **Rule:** whatever sizes a CDK viewport from
outside re-measures it when it resizes (`pd-table` calls `checkViewportSize()` from its body ResizeObserver); a virtual
list spec asserts the last drawn row meets the body's bottom edge after the body changed height.

## A request's answer was visible before its slot was freed: a phantom Busy (2026-11-04, Dyno `counters::tests::requests_and_the_pinned_reading_run_as_live_work`)
The test failed once in a full nextest run in 0.010 s (so not a timeout) and passed 6 of 6 alone. Causes listed, each with what rules it out:
- Thread roles shared across tests (a global registry or a leaked thread-local): ruled out by reading `priority.rs`: the role is a `thread_local` set inside each spawned thread, and the test reads only its own three threads.
- Other tests creating threads the test reads: ruled out, the `Roles` source belongs to this test's own `CounterCentre`.
- An ordering assumption between tests: ruled out, the test shares nothing with the others.
- The hang-guard loop leaving early on a stale value: ruled out, the loop only runs after `poll_pinned` returned; the failure came before any sleep.
- **Confirmed:** `CounterCentre::start` sent a request's result (`within`'s channel, the listing's `notify_all`) inside the thread's closure, and freed the `Slot` after it. A caller that had its answer could still see `in_flight` at 1, so with the catalog thread and the values thread not yet released, `poll_pinned` met `MAX_IN_FLIGHT` and returned false. The real effect: the page's next request, or the pinned reading, can get a Busy right after another request answered.
Fix: `start` frees the slot first, then runs a hand-over closure that publishes the result. Failing-first test: `an_answered_request_has_already_freed_its_slot` (3000 calls, `in_flight` must be 0 after each).
Proof (macOS, 8 `yes` loops started and stopped by pid, built once, 100 runs of all `counters::` tests at 8 threads, 100 of the new test, 300 of the original): before the fix, the new test failed 100 of 100 with and without load, and the original test failed 0 of 300 without load and 18 of 300 under load; after the fix 0 failures in every row.
**Rule:** whatever tells a waiter "done" is published after the resource it counts is released. Class check: no other slot or semaphore in Dyno or the shared crates (`paddock-jobs`, `paddock-ipc` permits, `paddock-download` hosts, the disk engine queue) sends before it releases.

## A test counted every job as a finished one; the agent has its own start-up job (2026-11-04, Dyno gates run 7510, Windows)

`only_the_last_few_finished_jobs_are_kept_and_an_unknown_id_is_not_found` listed 17 jobs against `KEPT_FINISHED` 16 (1 of 2166 on the Windows leg).
The agent's network watcher starts a job of its own at start (`JobSpec::RebuildIntel`, the match index of the user's lists), and `Request::Jobs`
lists the jobs that have not ended besides the last 16 that have: on a slow machine that job is still running when the test counts. Code reading,
not a reproduction (0 of 60 on the Mac under load): the first line of defence in the test was the assumption that no other job exists. The test now
registers a job that has not ended on purpose (it failed first with the same 17 against 16), counts only the ended jobs against `KEPT_FINISHED` and
asserts the live one is listed. **Lesson:** a count over a shared list names what it counts (`is_terminal`), and a feature that starts work at
start-up (here the network watcher) invalidates every test that assumed an empty agent: arrange the other work instead of hoping it is done.

## A missing-folder test failed because the CI Mac's temp folder is on an external volume (2026-11-04, Spotter `folder::tests::missing_folders_files_and_shares_explain_themselves`)

Runs 7509 and 7515 (macOS): the test expected `NotFound` for `<tempdir>/nope`, got `Unreachable`. Not the permission problem it looked like: the runner's job
temp folder sits under `/Volumes/d7sst7/...`, and `is_network_path` treats everything under `/Volumes/` as a mount point, where a missing folder means "not
mounted" by design. The test now uses a path that is not under a mount point. **Lesson:** a temp folder's location is an environment input: a test of
path classification must not take its path from `temp_dir()`. (A re-run of an old macOS job after the runner's workdir moved failed differently: "failed to
read action.yml ... no such file": a stale `hostexecutor` path, so re-dispatch instead of re-running after a runner change.)

## TCC blocked self-responsible processes on the external work volume (2026-11-04, Mac runner)

With the runner's work folder on the USB volume `/Volumes/d7sst7`, Pitcrew's disclaim and supervision tests failed with
`getcwd: ... Operation not permitted` and Dyno's `check-bundle.sh` saw empty `launchctl submit` output files, which it blamed on
the code-signing identifier. The unified log showed the cause: macOS TCC (Removable Volumes) lets the runner read the job's temp
folder (`<workdir>/<hash>/tmp`, which `job-tmpdir.sh` used as `TMPDIR`) but denies any process that is its own responsible
process: shells spawned with responsibility disclaim, and `launchctl submit` jobs (xpcproxy cannot open the `-o`/`-e` files).
Full Disk Access for the runner does not help, because the denied process is not the runner. **Lesson (class: a permission
belongs to the responsible process, not the parent):** keep the work folder and caches on the external disk if wanted, but put
each job's `TMPDIR` on the internal disk (`job-tmpdir.sh` now does so on macOS when `diskutil info -plist` says Internal=false),
and a check that finds empty output from a launchd job prints `launchctl print` and the exit state before it guesses a cause.

## A check that could not see the thing passed, and a wrong cause was called confirmed (2026-11-05, Dyno 1.1.0 VM checks)
The tray diagnosis for Dyno 1.1.0 was confirmed in 12 of 12 runs and then retracted: the VM had no menu bar, so the probe
could not have seen a tray icon either way, and a tray count had matched the app's own window buttons. The audit of the
VM suites found the same shape elsewhere: the Windows autostart judge passed on the agent alone, Lifespan passed on
"helper did not answer", a reading row passed on a dead helper, and `renderResult` printed PASS on zero steps. What exposed
it was the user asking why iStat Menus, on the same Mac, did not have the problem: nobody had compared with a known-good
peer. **Lesson (class: a check that cannot fail):** before calling a cause confirmed or a check green, verify the
precondition (the menu bar exists, the helper runs, at least one step ran), give the probe a positive control (show it sees
a thing known to be there), and compare with a known-good peer. Otherwise the result is `not measured` or `check broken`
and the release does not pass (rule 71, ER-33).

## First-parent lint on main needs a pre-landing branch lint (2026-11-08, Debrief 1.2 batch 1)
A push to main lints only the commits main itself gets (`--first-parent`), so a branch's own subjects and trailers (two `wip:`
commits, five first lines over 100 characters) were never checked once the branch landed without a pull request.
**Lesson (class: a check that skips what it cannot see):** lint the branch before landing it (`commit-lint --range
origin/main..<branch>`, rule 50(g)); C23 in the conformance run is the backstop. Waivers for commits are exact per commit
(`commit <10 or 40 hex>`, no patterns) and expire.

## A delayed service looked like a broken one (2026-11-04, Dyno helper after a Windows restart)

After a restart the Dyno helper card said "installed but did not answer, Repair" until the person ran Repair (UAC), though nothing was damaged. By the code, four things added up: the service was `delayed-auto` (it starts a minute or two after the boot), the agent turned "program present, pipe absent" straight into Silent and Repair, nothing started a stopped service although its security descriptor already lets interactive users start it, and the card read the state once and never again. A start that is merely late, or a service that is merely stopped, is not damage. **Lesson (class: a state that fixes itself is not a fault):** a status that can be transient is polled with a backoff and says "starting", an unelevated start is tried before an elevated repair is offered, and Repair is for a service that stopped with an error, with the service's exit code and reason in the detail. `paddock_service::start::revive` is the one implementation; a refusal at the service's start leaves its reason in the Event Log and in its service-specific exit code (ER-27), because a service that stops silently can only be repaired by guessing.

## The Linux "Turn off" reported done and nothing proved it: a result dropped at the OS edge, a removal with no read-back (2026-11-04, ADR 0054 RC-2 and E-3/E-4)

The Dyno 1.1.0 review found `Systemd::unregister` running `let _ = self.ctl(&["disable", "--now", ..])` and returning `Ok(())` for a packaged unit
without looking, and `plan_uninstall_self` (the daemon removing itself after the `Uninstall` request) with no `ConfirmGone`: the app was told `Bye`
whatever systemd did afterwards. Both are defects in the code and each now has a test that failed first (`cargo nextest run -p paddock-service --lib -E
'test(packaged_unit) | test(service_removing_itself)'` on `a391ebda5` plus the tests: 7 run, 3 failed). The reported symptom (75 s later still `active`
and `enabled`, restart counter 1, `/var/lib/dyno-helper` left) was **not reproduced** (ER-28): on the OrbStack `paddock-gate-arm` machine (Ubuntu 24.04,
systemd 255) the base code with the real hardened unit, the real `dyno-helper serve` and the real `Uninstall` request, and the elevated
`dyno-helper uninstall`, both left the unit `inactive`/`disabled` with `NRestarts=0` and the data folder gone, and `disable --now` called from inside a
`ProtectSystem=strict` unit works (PID 1 edits `/etc`, not the caller). Plausible causes still open, each with the observation that rules it out:
`disable --now` failing on the Tart GNOME image (its journal line from the same run); the daemon exiting non-zero before the removal thread reached
`disable` (the unit's `Result=` and `ExecMainStatus`); the elevated path taken instead of the daemon's (the app log's "turning the helper off" line).
The fix makes every one of them visible: `unregister` fails with the tool's words or `StillPresent(inspection)` and logs the read-back, the self-removal
ends in `ConfirmGone`, and the next lifecycle run (ADR 0054 L7) shows which it was. **Lesson (class: errors discarded at OS edges):** a removal is
judged by what the OS says afterwards, never by the call's exit status or by a reply sent before the work; C93 (ADR 0054) lists the edge modules
(`crates/wiring.toml`, `apps/dyno/wiring.toml`) and the `// silent:` hatches that remain are counted. The helper-log race (`68889a16e`, a log thread
recreating `helper.log` between the unlink and the folder's removal) is on this line and its test passes.

## Retracted: A guessed fix made the missing menu-bar items worse; the measured cause was an activation gate (2026-11-05, Dyno 1.1.0, macOS 26)

Symptom: after a login start Dyno's menu-bar items were missing. First hypothesis (65c4c8fc4, "Verify"): the add was dropped, so check and re-make. It was refuted by the VM runs: the items were made every time, and the re-make after ControlCenter was up re-hid them and crashed Dyno with SIGTRAP 1 of 2 times on a fresh guest. Cause (12 of 12 runs red without it, 6 of 6 green with it, ER-28): on macOS 26 ControlCenter keeps status items an app makes after ControlCenter started off screen (`.ephemeral`) until the app has been active once in the session; a hidden login start is never active. Adopted at ControlCenter start: shown. The AppKit calls `activate()` and `activateIgnoringOtherApps` are refused for a background launch; an Apple event `activate` (osascript) works. Fix: activate once at a hidden start, give focus back, no re-make on macOS. The logs did not show it before: "created" was logged, never whether ControlCenter showed it; now a window-list test per autosave name, ControlCenter's pid/start and a diagnose line 5 s after creation say so. **Class:** a call that succeeds is not proof the OS shows the result; log the OS's own view, and never add a "repair" before the cause is measured.

**Retracted:** follow-up to the entry above (review of `fix/dyno-tray-login`): the sequence runs only on macOS 26 or later (`activate()` and `yieldActivationToApplication` are macOS 14+ and the minimum is 13: an unrecognized selector raises on the main thread; both are also guarded with `respondsToSelector`). It first looks about 2 s at whether ControlCenter already shows the items (window list matched by ControlCenter's pid, not its localized name) and activates only if they are parked, so a login where ControlCenter adopted the items keeps its focus. Focus goes back only to the same process (pid, launch time, bundle id), only if Dyno is still active and no Dyno window is open. Logs carry pids, never app names. **To look for on a real Mac** (the VM guest cannot show a TCC prompt): at a hidden login start, (1) the menu-bar items appear within a few seconds; (2) the menu bar and focus switch for about a second and return; (3) no "Dyno wants to control ..." Automation prompt - if one appears, the log line `tray activation: attempt apple_event: call failed (...)` says so, and the items stay hidden until Dyno is opened by hand; the Info.plist carries `NSAppleEventsUsageDescription` so such a prompt would at least explain itself. The `tray diagnose:` lines in the Dyno log give the rest.

## Retraction: the menu-bar activation "cause" was a guest without a menu bar (2026-11-05, Dyno 1.1.0, macOS)

The two entries above (2026-11-05, "A guessed fix made the missing menu-bar items worse…" and its follow-up) are **retracted**. They said ControlCenter keeps status items that an app makes after it started off screen until the app is activated, "12 of 12 runs red without it, 6 of 6 green with it", and the fix activated Dyno once at a hidden start. That claim was wrong, and the activation was removed.

What was measured: every failing run was in a guest session whose whole menu bar was off screen (0 Control Center windows and 0 Window Server `Menubar` windows; `AccessibilityUIServer` frontmost). The parked frame y=768 was the menu bar being away, not Dyno being held back. Activating any regular app, Finder included, brought the bar up, and with it Dyno's items, so the "green" runs showed that a menu bar appeared, not that activating Dyno helped. With a menu bar present, Dyno shows its items at a hidden start and at a login-item start without any activation, as Stats and a plain AppKit probe do. The user's real-machine symptom is **not reproduced**; the Tart guest's missing menu bar is unexplained and may not exist on a real Mac. The activation code, its tests and the `NSAppleEventsUsageDescription` plist key are gone. The autosave name set before the status item connects stays as housekeeping, with no measured effect.

What stays true: Verify's remove-and-re-make on macOS was removed (the check reads the local window, which is wrong while the menu bar or ControlCenter is not ready, and it re-made all four items one second after a good login; one test run then crashed after the re-make: a candidate for the real symptom, not a confirmed cause, no stack was captured). For the next real failing login the log now says: whether a `Menubar` window exists, ControlCenter's pid and start, the shown test by ControlCenter's pid with the status item's own window as a diagnostic fallback, a line before every icon removal, the start signal, and the system's crash reports for Dyno by name and time (Report a problem includes them too).

**Lesson (class: a cause confirmed against an unchecked precondition, ER-28):** before calling a cause confirmed, check that the failing state is the state under study (here: is there a menu bar at all?) and run a known-good peer (another status-item app) in the same state; a fix that "works" must be shown to work through the cause, not through anything else the step also does (activating an app also brings up the menu bar).

## The first run registered the login service after the app had started its own agent: three agents in 20 s (2026-11-05, Dyno 1.1.0 VM checks on build 7538)

The macOS cold-start check saw 2 to 3 distinct `dyno-agent` processes where 1 is allowed; the Linux check saw the agent under `systemd --user`, not the app. Hypotheses and what refuted them: a respawn loop in the supervisor (the app log shows one launch, one `Shutdown`, one launch, no loop); a duplicate from the login item (one `dyno-app` pid in every run); a check that counts wrongly (partly: see below, but the logs show the extra processes). Cause, from the app and agent logs of a 7538 run: start at login became on by default (`91d703d81`), and the first run registered the agent service from `settings_changed`, i.e. after the app had started and connected to its own agent. The registration started the service's agent at once, which exited 3 because the app's agent held the lock; the repair then saw a "stale" registration and re-registered it, the app asked its own agent to exit (the documented hand-over, Dyno ADR 0010 section 11), waited for the service (launchd's 10 s throttle, systemd's `RestartSec`), and on macOS fell back to its own agent after the fallback wait, with the service agent refused every 10 s until Quit. Fix: the first run is decided before the link starts any agent, from the settings file the app already reads (`login::apply_first_run_at_start`), so the link finds a supervising service and starts none. Verified on a macOS VM with a local build: one `Agent starting`, no `Agent refused`, no hand-over. Review follow-up (same day): the registration can open System Settings (SMAppService) or wait on `systemctl` for up to 40 s, so it does not run on the setup thread: `AgentLink::before_first_launch` runs it on a worker and the link begins when it returns (a test fixes the order, red against a link that begins first); one function (`login::run_first_run`) serves the start and the first sight of the settings, once per process (`FirstRunAttempt`), so a failed try is not repeated in the same run; a damaged settings file is logged. The Linux check on the updated vm-smoke checks (`00760d0e5`) was run against the 7538 package only: a package of this branch (CI) is needed to see the systemd side without the exit-3 restart.

Not product bugs, found on the way (the checks, branch `test/dyno-release-vm-checks-v2`): (1) `instancesOf` matches processes by the bundle path in `ps` args, but the agent that SMAppService starts shows as `dyno-agent run` without a path, so a service-started agent is invisible (cold start "not ready", discovery file "stale", Quit "agent.json still there" while the service's agent keeps running by design); (2) the Linux check expects the agent to be the app's child, but with Launch at login on (the default) systemd supervises it (`dyno-agent.service`, `ppid` of `systemd --user`) and Quit leaves it running (user guide: the agent keeps measuring until you stop it). **Lesson (class: an ordering decision taken after the thing it governs has started; a check written for a mode the default no longer is):** decide who supervises a process before the first one is started, and when a default changes, re-read every check that assumed the old mode.

## A report that cannot explain the failure it was made for: the Dyno bundle audit (2026-11-05, Dyno 1.1.0, ER-27)

An independent audit fed 38 invented hostile lines and a broken helper to Dyno's two bundles. Findings, with their class. (1) **Two bundles, two redactors (class: copies drift).**
The Diagnostics page's "Save bundle" used Dyno's own redactor and zip writer, which had none of the secret layer Report a problem has; the register already said
the copy should be gone by 1.1.0 (`personal-data-redaction`). Now there is one report: the agent hands over plain files, `paddock-diag` redacts, previews and zips.
(2) **A fix that reads the environment cannot work in a GUI app (class: the test machine is not the user's machine).** `Redactor::for_machine` read `HOSTNAME`, which
a macOS GUI app does not have; the host name now comes from the OS. (3) **A root-only log explains nothing to a user-run report.** `helper.log` is 0600 root, so a
broken helper left no evidence; the helper now writes a small world-readable lifecycle file with no user content, and the report adds the OS's view of the
service, the Event Log entries of its source and the helper's own status and detail. (4) **A partial failure said the wrong thing.** An unreadable log folder reported
"There are no log files yet"; a log tail with no newline was dropped; a crash marker was never cleared. Each now says what happened. (5) **A verdict for every hostile
line** (`crates/paddock-diag/tests/it/hostile.rs`): a line is either removed or listed as deliberately left, and a stale "left" fails, so the list the dialog shows
stays true. Deliberately left: file and folder names in paths, other machines' names and identifiers, text an app imported or showed. Not reproduced before the fix:
the red run of the hostile table is the audit's own probe log (20 of the lines leaked); the other new tests were written with their fix (ER-11 gap, noted in the PR).

## sysinfo's macOS available memory includes active pages, so "used" read 17 % for 54 % (2026-11-05, Dyno 1.1.0, macOS)

Dyno computed used as total minus `available_memory()`. On macOS sysinfo 0.39 defines available as active + inactive + free pages (`unix/apple/system.rs`), which counts memory apps are using, so used came out as roughly wired + compressed (4.3 + 6.2 of 64 GB) where Activity Monitor said 34.8 GB. sysinfo's `used_memory()` there is internal - purgeable + wired + compressor, which is what Activity Monitor shows. Fix: `split_memory` in `dyno-platform/src/sensors.rs` takes used from `used_memory()` on macOS and keeps total minus available elsewhere (Linux reads `MemAvailable`, Windows available physical memory: both right). Class: a library's "available" is not "not used" on every OS; check what a cross-platform figure means on each OS before subtracting. Other users of `available_memory()` (Debrief, Pitwall, `paddock-metrics`) have the same macOS reading; not changed here.

## A first import of an external package can time out whichever test is running (2026-11-06, Dyno 1.1.0, Mac)

Symptom: Dyno's `opens the shared chooser` spec reported "Test timed out in 5000ms" at 5569 ms and 6228 ms on a busy machine, then passed on repeat runs. First hypothesis: a real hang in the chooser or side sheet. The `vi.waitFor` limits (1 s each) and the late timeout refuted a pending promise, and CPU profiles of a failing run showed about 7.1 s of 8.1 s in the module loader's file reads.

Root cause: `dy-chart` runs `import('./echarts-setup')` on first render. The Angular test builder keeps packages external, so Node reads about 688 ECharts files (6.5 MB) synchronously in one event-loop turn, inside the running test. A slow disk or a starved CPU stretches that past the test timeout. The spec's `PD_ECHARTS` fake did not help, because `dy-chart` does not use that token.

Fix: `src/test-setup.ts` imports `echarts-setup`, so the read happens before any test starts. `chart.spec.ts` fails if a test run reads a module for that import.

Logs: the run output showed only the timeout, and the one clue was a duration well past 5000 ms. Class: lazy imports in specs are blocking disk reads, so preload heavy ones in setup. `PD_ECHARTS`'s default `import('echarts')` in `ui/projects/paddock-ui/chart` has the same exposure wherever a spec has no fake.

## Service repair was an undeclared state machine; three review rounds patched instances (2026-11-05, Dyno 1.1.0, ADR 0056)

Moving Dyno's macOS service code into `paddock-service` took three review rounds that each found the same kind of bug in a different entry point: a registration the OS accepted but had not answered was forgotten and replaced (a second "Background Items Added"); a quit during the 120 s wait left no record; a job that stayed refused was re-registered at every start; the resume marker could be lost behind a refused location; a crash between unregister and register lost the service. State was spread over a version file, an attempt counter, a marker file and a lock; five entry points decided for themselves; it was saved after long waits, not before. The cure is structural (ER-21): one persisted state with phases, written before each OS step; one pure transition function; one executor; holds derived from the attempts; the model explorer as the guard.

- Write the record before the OS call, never after the wait; a thread that waits dies with the app.
- Never hold a mutex across a wait the user can press a button during: `ServiceControl::run` and the health repair now try the manager and answer Busy at once, with words.
- Every hold names what happened and a next step that exists for that state: unanswered, refused and register failed are three facts with three messages, and a failed register is not "not installed".
- A start that cannot save its record changes nothing and tells the UI, with the way out (Repair goes on, loudly).
- Tests that only restate what the explorer proves are deleted, not rewritten; the ones that pin exact commands, files and words stay.

## A newer Dyno kept talking to an older agent of the same version (2026-11-07, Dyno 1.1.0)

Symptom: after replacing Dyno.app, an agent started on 5 Nov kept running and the new app used it; killing agents by hand then showed "crashed 3 times, Restart loop". First hypothesis: the app never asks an old agent to exit. Refuted: the link already asks an agent of another version or protocol to exit (`Request::Shutdown`), waits for the data folder and starts its own. Root cause: "another build" meant a different `CARGO_PKG_VERSION`, and a release candidate and the release carry the same version. Contributing: nothing watched the agent's own file, so an agent whose bundle was replaced ran on (on Linux a deb upgrade, on macOS a replaced bundle); the issue readers called any `.ips` report a crash, so a SIGKILL or SIGTERM counted toward the crash loop.

Fix: the discovery file carries a build id (SHA-256 prefix of the agent file), the app compares it with the id of the agent file it would start, and the agent exits (code 0, logged) when its own file was replaced or removed on two looks 30 s apart; signals others send are `StoppedFromOutside`, no crash and no stability points. Logs: the connect line now carries the agent's version and build and the app's key. Class: identity by a label that releases share; compare content.

- A test fake built from the real agent carries its self-healing (the discovery guard writes `agent.json` again at the 30 s watch): tests that tamper with a live agent's files must hold only the lock and start the agent when told (`SlowHolder`; CI run 7672, Dyno link test connected at 94 of 100 looks).

## One failed first read left "could not be read" up for good in 14 shell stores (2026-11-08, Dyno 1.1.0, ADR 0058)

Dyno's Jobs panel kept "The jobs could not be read" after the app started while its agent restarted, even while later
events listed live jobs. The 1.1.0 fix gave `JobsStore` its own retry; the search for the pattern then found eight more
shell stores whose event path never touches `requestStatus`, and six whose snapshot events clear it but which never read
again by themselves, so an engine that comes back unchanged leaves the error up.

- A status is state too: every path that brings data (an event, a command's reply) must say what it means for the status,
  or the store keeps an error that is no longer true.
- "An event clears it" is not enough: the engine that comes back may have nothing to announce. A read of engine state
  retries itself (bounded backoff on an injected clock), keeping the error up (no loading flash) until a read succeeds.
- Make the declaration part of the API (a required `ReadPolicy`) and check it with one spec that enumerates every call
  site from the sources, so a new store cannot opt out silently.

## A brief without a named scratch folder let an agent clobber the shared scratchpad (2026-11-08)

Symptom: an agent told to "clean up" its scratch files removed the shared scratchpad, including other agents' files (the same shape as the 2026-10-31 reviewer incident). Root cause: the brief named no folder, so the agent chose the root. Fix: every brief names the agent's own subfolder of the scratch area and tells it to touch nothing else; an agent never globs the root. Class: a cleanup without a named path deletes more than it made.

## Agents ran git in the main checkout after a failed cd (2026-11-08)

Twice, a `cd` into a worktree failed and the next git command ran in the main checkout: once a sparse checkout and a merge, once a fast-forward of local main. Root cause: the shell stays where it was when `cd` fails, and git does not care where it runs. Fix for now: check `git rev-parse --show-toplevel` before every git command and use absolute paths. A guard (a hook that refuses mutating git in the main checkout) is an ER-21 candidate, not built yet.

## A shared redaction default changed for one app silently changed another app's outgoing text (2026-11-08)

A redaction default in a shared crate was widened for one app's problem report. Another app's web-search text, which leaves the machine, then changed with it and nothing failed. Root cause: the shared default was the only switch. Fix: a shared behaviour change that alters what leaves the machine is opt-in per caller; the new behaviour is a parameter the one app sets, and each other caller keeps its test of what it sends.

## Gitea tokens cannot mint narrower tokens, and agents must not create bot users (2026-11-08)

An agent looking for a narrower CI token found that a Gitea token cannot create a token with fewer rights, and the only other route was creating a bot user. Agents must not create users. The user creates scoped CI tokens (one per purpose, least privilege) and the agent is given the one it needs; if none exists, the agent stops and asks.

## Two Debrief tests failed on main run 7697: a harness stall nobody could see and a click lost to a layout shift (2026-11-08)

Symptoms: `debrief-plugin-test::it harness::a_hung_import_is_cut_off_and_fails_the_cancellation_check` timed out at 90 s on the loaded macOS runner (30 s on Linux), and WebKit `live-tail.e2e.ts:66` failed with `resume-following` not found (also on green run 7581, with retry #2 hiding it).

Harness: first hypothesis a regression from the paddock-ipc merge, refuted (the diff of `peer.rs`, `launch.rs`, `handshake.rs` and `link/server.rs` only logs ignored results and adds lint attributes; 29.6 s alone, 30.5 to 31.4 s under 24 CPU burners). Slow 16 MiB read and slow start stayed open (0.94 s and 0.13 s unloaded). Root cause not isolated: the test ran the whole conformance set against a hung plugin, about 26 s of fixed waits in a 90 s budget, with a 300 s read guard and no phase output. Fix: phase lines with elapsed ms (CI, nextest or `DEBRIEF_PLUGIN_TEST_LOG`), a 20 s no-progress read guard that fails with the bytes read, `Launch::import_timeout` and `Launch::read_stall`, a fault hook that stops the fake host's reads, and a hang judged on the call's "did not answer" error instead of elapsed time. The next 90 s kill will name its phase.

Layout: first hypotheses (zoom ignored because the summary or view is null; the live window not yet resolved; the follow effect dropping the range) were each refuted by a log. Root cause, from document-level mousedown/mouseup/click logs plus the header children's boxes: the header row was full on a narrow pane, so a 22 px spinner appearing between mouse press and release moved the Zoom in button and the click never reached it. Rate 10 to 15 % in 6 workers under `taskpolicy -b`. The spinner slot alone left the class open (the paused chip alone moves Zoom in 107 px), so the header is now a `.lead` that takes the free space and clips and a `.tools` group that is `flex: none` and anchored right, with a geometry e2e that samples the buttons' edges every animation frame (it failed on the old layout). After the slot-only fix 1 of 120 runs still failed; no trace was kept, so whether it was the same symptom is unclassified. After the restructure 60 of 60 runs were green at the default workers and 60 of 60 under starvation; load 21 to 69. Known limit: while paused, or on panes narrower than about 1100 px, the headline's own width no longer fits and its end is clipped (still tabbable).

Why the logs did not show it: neither the harness nor the app logged the phase or the click; both were found by adding probes. Class: a test or control whose verdict or target depends on something that moves or waits (fixed waits in a budget, a control that shifts under the pointer). Debrief's Playwright retries were 2 on CI, so a failure could still go green; now 1 with `failOnFlakyTests`, and traces are kept on failure. Other apps' retry settings and Debrief's local nextest retries are left to a separate change.

## A rule that counts sites needs a baseline by text, not by count (2026-11-04, C71)

C71 (tests do not settle on real time, ADR 0041) found 654 sites on `main` the day it ran: 339 Rust sleeps in test code, 181
`setTimeout` waits, 57 `sleep`/`wait(n)` helpers, 35 `waitForTimeout`, 30 `requestAnimationFrame` waits and 12 `.count()`-then-`.nth()`
uses. They sit in `conformance/baselines/C71.json` as file plus a hash of the whitespace-collapsed line, so moving a line keeps it
known, editing it makes it new, and a second copy of a known line is new too (a per-file count would let a new site hide behind
a fixed old one). A shrink-only rule that only the baseline tool enforced was not enforced: the check itself now compares the file with
the merge base's and errors on an added entry, and the baseline has no expiry (the migration is open-ended, ADR 0041 WP-6). A Rust "test code" scope of
"everything after the first `#[cfg(test)]`" and a masker that kept string contents flagged 27 non-sites; scope the region to its item and blank strings.
**Lesson:** when a check is added to a codebase with existing violations, make the allowance a shrink-only list keyed by
what the line says, with a date, and say in the check's fix text how to waive a real case.

## Dyno helper_seam: the agent closed a client's connection in the handshake, cause open (2026-11-09, run 7763; resolved, see the next entry on file descriptors)

Run 7763 (train 1, Linux container): `dyno-agent::helper_seam the_cached_inventory_reads_the_memory_slots_again_when_the_helper_comes_up` failed after 5.25 s at `helper_seam.rs:138` (`connect(&self.dirs).unwrap()`) with "The agent in .../agent.json did not answer (the connection ended (the host closed the connection during the handshake))". It did not reproduce on macOS. A first change (a 100 ms poll cadence, aca067f7f) assumed the fast poll's connection rate was the trigger; the review measured 21,000 back-to-back sequential connect-and-ask calls on macOS (normal priority and starved), 0 failures, so that cause is unproven, and the change was reverted (9a08a1ca2). Candidate causes, none measured yet: the handshake watchdog thread failed to spawn (`bounded`, paddock-ipc `handshake.rs:104-114`, the stream is dropped without an answer); the server's read of the Hello failed and the host closed without a `Fail` frame; the 10 s watchdog fired (unlikely in 5.25 s); a rig artefact (the discovery file rewritten mid-test). The agent's own log line (`dyno-agent: refused a connection: ...`, `server.rs:75`) would decide it, but the rig does not show the agent log on failure. Resolved on `fix/dyno-agent-handshake-close`: the cause was the rig running the agent out of file descriptors; the lesson is the entry "A polling test ran its in-process agent out of file descriptors" below. **Lesson:** a test that fails because a product closed a well-formed connection is a product question first; do not slow the client until the cause is measured, and make the rig print the product's own log on failure.

## A test that kept every vCPU of the Windows test VM busy cut its SSH and left nothing to clean up with (2026-11-09)

To provoke a rare close race in paddock-ipc, an agent started one busy process per core (4 of 4) in the Paddock Windows 11
test VM over SSH, with the cleanup in the same PowerShell session. sshd stopped answering at once, the session dropped, and
the cleanup could not run; the VM was later powered off by someone else. The same series without the extra load had
already reproduced the race (1 in 2,000 runs). Rule of thumb: in the test VM, load at most cores minus one, run the load
from a scheduled task with its own time limit (not from the SSH session), and stop it by the process ids it recorded.

## A polling test ran its in-process agent out of file descriptors, and the handshake closed without a word (2026-11-09, Dyno 1.1.0)

Symptom: CI run 7763 (Linux), `helper_seam::the_cached_inventory_reads_the_memory_slots_again_when_the_helper_comes_up` failed after
5.25 s with "the host closed the connection during the handshake"; an independent reviewer could not reproduce it on macOS with
21,000 sequential connections. First hypotheses: a thread failed to start (container thread or pid limit), the server failed to
read the hello, or a rig artefact. Measured in the CI image (a synthetic in-process agent and a probe loop of connect, ask, drop): a `BodyPeer` that is dropped
without `close()` keeps its connection, so each poll leaked 6 descriptors and 5 threads (agent and client share the test process);
the train's new back-off poll made about 250 connections in 5 s, 1,500 descriptors. Under a 1,024 limit the agent's accept took the
last descriptor and the handshake's watchdog could not copy the stream: `IpcError::Io(EMFILE)` before the hello was read, the
stream dropped, the client saw the end of the stream. A thread limit (`--pids-limit 700`) instead failed after the welcome or on the
client side ("cannot start an IPC thread") in 5 of 5 runs, never with this message; CPU starvation with closed connections gave no
failure. Not measured: the CI container's own open-files limit (nextest does not raise it on Linux; the local image had 20,480).

Root cause: a test rig that dropped peers instead of closing them, plus a handshake that answered local resource failures with
silence. Fix: `paddock-ipc` answers a host that cannot start a handshake with `busy` naming the cause, a malformed hello with
`invalid`, and keeps one descriptor in reserve so a client the accept could not take (Linux queues it; it waited 10 s while the
host logged a refusal every 50 ms) is told `busy` too; the helper-seam rig closes each connection and prints the agent's log, the
open files and the limit when a test fails; a 120-connection stress test checks that the agent gives back what a closed
connection held.

- A test that starts the product in its own process shares its limits: a leak in the test's client exhausts the product.
- Logs explain the failure only if the rig shows them: an in-process agent's `log` lines went nowhere until the rig captured them.
- Class: a handle whose drop does not release its resource (`BodyPeer`, `Peer`); reported for an ER-21 decision.

## A background-lister test asserted a call count that a second trigger can change (2026-11-09)

Symptom: `disk::centre::tests::the_cached_volumes_never_list_on_the_request` failed in CI (`ci.yml` run 7767, Linux Dyno leg, nextest, twice): `a cached read within the
freshness window never asks the system again`, left 2, right 1 (`crates/dyno-agent/src/disk/centre.rs:1616`). It passed five times in a row on a Mac. The cause is **not yet
confirmed** (ER-28), so it is an open bug, routed by the coordinator, not a fix. Lesson so far: a test that counts calls into a fake after waiting for a background job must
wait for the job to end, not only for its first result, and must name every trigger that can start another job. Candidates and the observation that would rule each out:
(1) the first request and the mount-event thread's first trigger each start a listing: log each call's trigger and thread in the failing run; (2) the freshness window reads the
real clock somewhere while the test advances only the fake: grep the window's `Instant` and `SystemTime` use in `centre.rs`; (3) a second listing from the volume watcher
(`mount_events` has macOS, Linux and Windows adapters, so this is not Linux-only): run the test with the watcher stubbed out; (4) a shared static between tests in one process:
run it alone 200 times in the CI container. No sleep or raised timeout (ER-12).

## A popover test failed once under load: disk-space was missing before the volume cache was filled (2026-11-08)

Dyno's `widget_layouts::popover_values_answer_only_the_slow_kinds_the_layout_has` (apps/dyno/crates/dyno-agent/tests/agent.rs) failed once, right after a heavy build, with `disk-space` missing (`["active-alarms"]` instead of `["active-alarms", "disk-space"]`). Rerun alone it passed 3 of 3 times, it passed in the full run, and it passed on a clean origin/main. Not fixed here (ER-12: the fix is to wait on the condition, never a sleep).

Causes considered (ER-28), with what would rule each out:
- The test asks before the disk list is cached. `popover.rs` answers a disk widget only from `shared.disk.cached_volumes()` and treats an empty cache as "not yet", so the widget is left out. Rules out if: a run that waits for the cache never fails. Not measured yet; this is the leading candidate.
- The first volume listing was slow under the build's load. Same code path as above, one step earlier; rules out if the listing time in the failing run is short.
- The branch under test (the data-folder trust change) changed the answer. Ruled out so far: the popover code reads no folder, and the test passed 3 of 3 alone on the same tree.
Second sighting the next day, same code area: `disk::centre::tests::cached_volumes_refresh_once_they_are_older_than_the_freshness_window` (dyno-agent lib) failed once in a full run (left `Some(589000000000)`, right `Some(1256000000000)`: the refresh had not landed when the test looked) and passed 3 of 3 alone. It is the same shape: the test reads the cache right after asking for a refresh, without waiting for the refresh to finish. Both point at the volume-cache tests racing the background refresh under load.
Class: a test that asks a cached answer before its cache is filled.

## Debrief store tests remove a store folder while its index threads still write into it (2026-11-09)

Under a full nextest run two store-duck tests failed once each with "Directory not empty": `segments::reopening_recovers_from_a_crash_and_refuses_damage` (`tests/it/segments.rs:240`, `remove_dir_all` of the moved store) and `search::indexing_many_segments_in_batches_is_complete_and_repeatable` (`tests/it/search.rs:220`, `remove_dir_all` of the `index` folder right after `drop(store)`). Both passed 30 of 30 times alone under `--stress-count 30 -j 6`, on this branch and on the base, so the cause is not the temp-folder change. Probable cause (not measured): the store's index threads finish writing after `drop`, so the removal races them. Not fixed in the LINT-1 branch: a separate branch should make the store's drop wait for its index threads (or the test wait on the condition), not retry or sleep (ER-12).

## A Debrief test process crashed at exit: the catalogue's checkpoint ran outside the exit handler's count (2026-11-09, CI 7837)

- **Symptom:** `debrief-app ai::tests::use_cases_route_through_ipc_to_the_model_and_the_engine` printed "ok", then the process
  died with SIGSEGV (macOS, run 7837); the temp-leak check then failed on `debrief-test-stores-*` and `debrief-bundles-*`.
- **Evidence:** the runner's crash report (`~/Library/Logs/DiagnosticReports/Retired/debrief_app_lib-*-2026-11-09-213925.ips`): the
  main thread in `exit` -> `test_support::remove_process_scratch_at_exit` -> `remove_dir_all`, the faulting thread
  `debrief-catalogue` in `catalogue::settle` -> `CHECKPOINT` -> `QueryResult(ErrorData)`. Ten `debrief_app_lib` reports of
  2026-11-08/09 on the Mac have the same thread and `settle` or `commit` frames. The leaked folders are the ones only that
  `atexit` handler removes: it was the handler running when the process died (confirmed, not assumed).
- **Causes listed:** the catalogue thread's checkpoint at exit (confirmed); the fts thread (ruled out: every statement is in its
  guarded `step`); tokio or the AI fakes (ruled out: their threads idle in the report); a child or socket left (ruled out: the
  faulting thread is ours, in DuckDB); ADR 0072 endings and the orphan guardian (ruled out: not on the stacks, not on the train).
- **Root cause:** the exit handler (`shutdown.rs`) waits only for statements under an `Active` guard. `catalogue::step` took one,
  but `run` called `settle` (the `CHECKPOINT`) after the guard was gone, and `settle` did not check `down()`. So at exit the
  handler found nothing running, the checkpoint went on (or started after the interrupt), and C++ destroyed DuckDB's statics
  under it while the test helpers removed the store's folder.
- **Fix:** one `Active` per turn covers the step and the checkpoint; `step` and `settle` take `&Active`, so a statement outside
  the count no longer compiles; the thread ends when no guard is given (exiting). Regression test
  `catalogue::tests::the_exit_handler_waits_for_the_catalogue_checkpoint` (records, in test builds, whether each checkpoint ran
  under a guard): failed with "1 catalogue checkpoint(s) ran outside ...", then 30 of 30 green.
- **Reproduction:** the crash itself did not reproduce locally (the app test 40 of 40 green, a child process exiting mid-build
  20 of 20 green although a probe saw the unguarded checkpoint run after `down()` in every round); the invariant test is
  deterministic.
- **Open, same class:** `debrief-store-worker` crash reports of 2026-11-05 to 09 show the catalogue thread crashing inside a
  guarded `step` while the main thread still drains in `shutdown::at_exit`: DuckDB statics registered after the handler's
  last registration are destroyed before it runs. This is the second fix of "a background thread in DuckDB at exit" (CI 6734,
  CI 7837): a candidate for a redesign ADR (ER-21), for example stopping and joining the store threads before `exit`.

## ADR 0072 M0: an ending test is only as good as the OSes it ran on, and a reset is a race (2026-11-09)

The ending harness (`paddock_ipc::testing::endings`, register `docs/ipc-endpoints.toml`, ratchet `docs/ipc-endpoint-ratchet.toml`)
drove every listen site of the family through the ways a connection ends. What the first runs taught, with the measurements in
`docs/benchmarks.md` (2026-11-09, how a connection ends on each OS):

- **A green run on the developer's Mac hid eight of the ratchet's entries.** macOS does not reset an AF_UNIX socket closed with
  unread input and delivers the reason; Linux resets it (I8, I9, I10, I11 on `ipc-host` and the link server, the Dyno agent and
  Pitcrew), and Windows TCP loses the reason altogether. The ratchet has an `os` field for the faults that are not on every OS;
  the endings were run on the Mac, in the native arm64 Linux container as root with 1,024 open files, and (paddock-ipc) on the
  Windows 11 Arm VM. The Dyno, MCP and Pitcrew endings were not run on Windows (cross-built only: `cargo xwin clippy`).
- **"Reset or lost reason" is one fault, not two.** A close with unread input resets, and whether the reason reaches the client
  before the reset is a race (the MCP scenario read `reset` on one run and `no-reason` on the next, then both at once on Linux).
  Asserting either would have been a flaky test; the harness reports a missing reason of a client whose input was never read
  as `reset` (`Expect::unread_input`), and the ratchet records that.
- **A named pipe closed with `CloseHandle` keeps what was written** (200/200), so Windows pipes need no drain (ADR 0072 D4);
  `DisconnectNamedPipe` throws it away. Decided by measurement, not by the documentation alone.
- **The positive controls found a bug in the first harness.** The broken endpoint "drop without a reason" was not seen by the
  scenario meant to see it: the broken endpoint's reader thread kept a clone of the socket, so the socket never closed and the
  client waited for its guard instead of reading the end. A control that fails for the wrong reason proves nothing; each control
  asserts the fault kind it is meant to show (`hang`, `drain-not-eof`, `cancelled-after-owner-close`, `no-reason`).
- **A guard has a direction.** The reference endpoint failed once in the pre-push run of the committed tree while two other
  builds ran. Its output was not kept, so the cause is probable, not proven: the only 3-second wait on the reference path was
  the one meant to detect that a broken endpoint **never** waits on the clock, and a starved thread can miss 3 seconds. With
  that guard cut to 1 microsecond the reference fails in exactly that scenario with `hang`. A wait for something that will
  happen is a hang guard (30 s); a short wait to learn that something will not happen belongs to the positive control that
  expects "never" and nowhere else. Two more defects of the first harness, found by timing each control: it waited for an
  end line of a connection that was never accepted (30 s for nothing), and its raw client took a refused read timeout (macOS
  answers `EINVAL` on a reset socket) for the end of the stream instead of reading on and reporting the reset. Stress after all the fixes of this
  entry: 40 runs of all endings tests with 14 busy loops, 1-minute load 55 to 68 on 14 cores, 0 failures (25 runs before the
  accept fix: 1 failure).
- **A `connect` that returned is not an accepted connection.** At load 58 (14 busy loops) the control "drop without a reason"
  failed once in 25 runs with `unanswered`: the harness accepted "what is in the accept queue" right after the client's
  `connect` returned, and a loaded kernel had not yet moved the connection there, so nobody ever served the client. The
  endpoint now accepts exactly the expected number of clients (a bounded poll of that condition), and the listener-stop
  scenario says how many clients wait in the backlog. AF_UNIX connects are synchronous and had no such race.
- **Small traps met:** `Deadline::remaining()` is `Some(0)` once passed, not `None` (a cap loop that waited for `None` never
  ended); on macOS `set_read_timeout` fails with `EINVAL` on a socket the peer has already reset, so set it before anything can
  close; a Unix socket path under the macOS temp folder is near the 104-byte limit, so a test folder name must be short; the
  busy loops of a load test must be forked by a supervisor that leads its own process group (a child cannot join the group of a
  process in another session), or the load cannot be stopped as a group.
- **Rigs show the product's log now:** the Dyno agent's `agent.rs` harness prints the agent's log when a test fails, as the
  helper-seam rig does; the ending tests of the agent, the helper and the MCP listener print the product's lines under every
  scenario that has a fault. Rigs that start the agent without the harness (`extras.rs`, `jobs.rs`, `disk_map.rs` and others) do not
  yet: open.
- Class: ADR 0072 (a connection ends without the peer learning why, or in the wrong order); the register and the ratchet make it
  checkable, M1 to M6 remove the entries.

- **Second review round (2026-11-09):** the Windows 11 Arm VM ran the endings of paddock-ipc, paddock-mcp, the Dyno agent
  and Pitcrew (the MCP and ipc binaries aarch64, the agent and Pitcrew x86_64 under emulation). Only Pitcrew differed: at 32
  connections the named pipe's instance cap (33) is reached, creating the next instance fails (ERROR_PIPE_BUSY) and the
  service stops serving, so the 33rd client gets no answer there (ratchet entry I26, `os = "windows"`); on Unix it waits
  unanswered. The Dyno helper's ending test is Unix-only and was not run on Windows. A run through the VM must wait for host
  load under 22: at load 90 `vm-win.sh start` refuses.

## Local checks overlap with CI (2026-11-09)

Serial local-then-CI added a full local cycle to every landing, while CI found nothing the local checks could not. Fast static gates now run first and in parallel; CI is dispatched and the slower local checks (affected tests, the `train3-linux-dev:1` container, load runs) run alongside. A local failure cancels the CI run at once and everything is fixed in one push (rule 50h, ER-9).

## Fix rounds after a red CI run rerun only the fix (2026-11-09)

Each full re-run of a combined train cost about 35 minutes of every runner, while the fixes touched a few jobs. The user asked: keep running only the failed tests and builds, then the full CI once everything passed. Rule 50(i): fix rounds dispatch with `base` = the last tested commit; the full affected plan runs once on the final tree before landing.

## Known instances for C103 and ADR 0089 WP-4: a failure read as "nothing to report" (2026-11-10)

Found by the review of the outcome type (`feat/outcome-type`). None is fixed one by one; each goes through the ADR's mechanism (the `Outcome` type and C103) in WP-4.
- `conformance/src/checks/C57-tracked-symlinks.mjs:33-34` treats any `git` failure as a clean tree.
- C34 (sidecar self-test) treats invalid JSON as "no sidecars".
- `conformance/src/util/keychain-graph.mjs:202` drops files it cannot read instead of reporting the check as not measured.
- `ctx.read(...) ?? ''` reads a missing file as an empty one in the checks that use it, so a missing input passes.
Fixed in the same change as the type: the runner's crash path (a `path = "*"` waiver hid a crashed check), a non-Error throw (which made `runChecks` reject and exit 2), a malformed finding and an unknown severity, and the stale-waiver warning for a crashed check.

## A Debrief store worker crashed at exit with two threads in `exit` and a live thread in DuckDB (2026-11-10, ADR 0076 exit-phase amendment, WP-X0)

The `debrief-store-worker` crash (21 reports in six days, all in the catalogue thread inside DuckDB while another thread ran
the exit handlers) had two candidate causes, listed under ER-28: (a) a native thread still alive during static
destruction (as `601858de8`, `5740cb6b6`, `e3f6cdead` before it), (b) two threads in `exit` at once. A deterministic
reproducer does not need DuckDB: a test-only `atexit` pair (a holder registered last, a witness before it) shows on macOS
that a second thread enters `exit` while the first is in the handlers, since libc marks the entry it runs as called and the
second thread runs the entries after it (`two_exits_never_run_the_handlers_together`, red 6 of 6 before the fix in
`debrief-ipc`, 20 of 20 in `paddock-ipc`; a first version of the probe registered only one handler and stayed green, because
the second thread skips the held entry: the probe has to watch the entries after it).

What the measurement said (`apps/debrief/docs/performance.md`, "Store worker exit"): with a thread held inside DuckDB, one exit
route alone still crashed (22 of 100), so (a) is a cause on its own; a join in `main` with both exit routes kept crashed
100 of 100, because the lifeline's `exit` runs the static destructors while `main` is still joining; both fixes together 0 of
200 where the unfixed code crashed 12 of 100. Without the hook the unfixed worker did not crash in 107 natural runs, so the
natural per-run rate could not be measured; the arms needed a hook that holds a thread inside a statement.

What changed: both lifelines (`debrief-ipc` and `paddock-ipc`, rule 55) close the peer instead of calling `exit`, so `main`
is the only exit (with a bounded fallback to `exit(0)` if the exit phase has not begun within the child's stop budget plus
a margin); the worker's `main` stops and joins the store threads within `STOP_BUDGET` (interrupting their statements) and
on overrun logs what it was waiting on and ends with `_exit`; Debrief's `KILL_GRACE` is the child's stop budget plus the
margin (1.5 s, was 150 ms, which killed a worker that needed a few hundred ms).

Lessons: (1) a test double for a race must watch the code that runs after the held point, not the held one; (2) a join does
not protect a process that has a second route to `exit`; fix the route before the join; (3) when the natural rate is
unmeasurable, hold the suspected cause in place with a hook and measure the arms against that, saying so in the record.
Open: the two repaired routes are checked by the barrier test only on macOS (std's `unique_thread_exit` hides a second exit on
Linux, and Windows' `ExitProcess` is a different mechanism); the shared `paddock_jobs::exit` phase (WP-X1) replaces the
hand-made pieces.
Class: a background thread in native code at process exit (CI 6734, CI 7837, instance 5 above).

Review round (2026-11-10): the worker's `stop_for_exit` stops the import sessions (cancel, then wait: dropping an `ImportSession`
detaches its `debrief-import` thread), the store threads and the stores; **open owners it does not stop, for WP-X1's ratchet:**
the threads of calls in flight (`spawn_blocking` work of the peer's handler is cancelled by the peer's close but not joined, and
its `DuckStore` clones keep the databases open), the plugin processes' threads, the IPC runtime's threads. The lifeline's
fallback claims the exit atomically (serving, main exiting, lifeline exiting; a late `main` waits at most `STOP_MARGIN`, then ends the process itself), and in `debrief-ipc` ends with
`end_without_destructors`. The D-E4 gate for instance 5 is not met (see `apps/debrief/docs/performance.md`).
For WP-X3: `BuiltinPlugins::shutdown` stops plugin processes one after the other, so two hung plugins can overrun the 3 s teardown;
stopping them in parallel is done now (`BuiltinPlugins::shutdown` and `WorkerPlugins::shutdown` run together, after the stores close). A child's kill grace must fit the parent's split (0.5 s per plugin by the amendment's §3.1), not the 1.5 s `KILL_GRACE`: a hung plugin still outlasts the 1.25 s budget, ends the worker with `_exit`, and is left to its own kill thread (WP-X3).
Two flakes seen, not fixed here: `debrief-ipc` `launch::a_locked_child_cannot_write_files_or_start_programs` on Linux (about 3 of 6
runs on the base tree, 5 of 6 in one container run; the "before the lockdown" note, probably the handshake thread still being
listed), and the store worker's client-RSS budget tests (`imports_and_queries_within_the_budgets`, `low_memory_mode_...`), which
measure the test process's own growth while other tests of the same binary run in it.

## An uncapped test container ran OrbStack out of memory, and restarting it took Docker down (2026-11-11)

A test container without CPU or memory limits ran OrbStack's VM out of memory; the agent then restarted OrbStack, which also
restarts every container of the user's, and Docker was down for about 15 minutes. Rule 7 now says: test containers run with
`--rm` and CPU and memory limits, and agents never restart, stop or reconfigure shared infrastructure; they stop and report.

## A 23-31-day design lost to a half-day warning that common practice already used (2026-11-11)

Debrief's elevated-helper amendment to ADR 0073 went through four review rounds and 22.75 to 31.25 days of estimate before
the user compared it with Visual Studio and VS Code and chose a half-day risk warning instead; several accepted process
ADRs were then cut as more than a personal tool needs (`docs/adr/trims-2026-11-11.md`). Rule 76 (ER-37): a design states
how comparable software and the platform vendor handle the problem, with sources and the cost in agent-days, and always
offers the cheaper common-practice option.

## A LocalSystem service trusted its pipe ACL and started any program a user process named (2026-11-11)

A survey found that Pitcrew's LocalSystem service (shipped in pitcrew-v1.0.1) starts any program a same-user process names
over its pipe, with no prompt, because ADR 0004 authorised by the pipe ACL alone; Dyno's helper already checked the caller
per connection. Rule 77 (ER-38): privileged services authorise changing requests by the caller's OS identity, and a
request that changes what runs elevated needs an administrator caller; user-chosen elevation needs only a risk warning.

## The user had to ask "any redesign?" three times in one morning (2026-11-12)

Every agent report carried a `Recurring:` line, yet the coordinator relayed reports and kept proposals for a later batch, so
classes seen twice reached the user only when they asked. Cause: relaying reports and batching proposals for later. Fix: rule
40 (ER-21) now asks for the proposal in the same reply that relays the report (a question, or a start under an earlier
approval); a Stop hook, `.claude/hooks/recurring-guard.sh`, is planned on `fix/scratch-root-guard-and-runner-busy`.

## A downloaded engine needed the VC++ runtime and no installer check could see it (Pitwall, 2026-11-11)

On a clean Windows PC the llama.cpp `llama-server.exe` Pitwall downloads (pin b11669) exits with `0xC0000135`
(`STATUS_DLL_NOT_FOUND`): its archive imports `vcruntime140.dll` and `msvcp140.dll` and ships neither, so the engine's `--version`
self-test failed and the install was discarded with "it could not start" or an empty `--version failed:`. Measured on the Paddock
test VM `clean-baseline` (speech spike 2026-11-11, section 6); shipped since pitwall-v1.0.3 (same pin, same self-test). The first
hypothesis, that the self-test or the pinned asset was wrong, was refuted by the same binary starting after Microsoft's
`vc_redist` was installed. Root cause: ADR 0032's import check (`conformance/ci/tauri-bundle.sh` runs it for every NSIS bundle) sees only
the binaries an installer ships, never one fetched at run time; CI runners have Visual Studio's runtime, so every run was green (the 2026-10-30 lesson
above, reached through a download). **Fix:** the Windows installer runs the pinned `vc_redist.x64.exe` (Microsoft recommends central
deployment because it services the libraries: redistributing-visual-cpp-files), the engine's exit status `0xC0000135` is
explained by `engines::exit_problem` with the next step, and a fresh install of the CI-built installer (run 7931 artifact, sha256 00bbd3a93b44c8927320f29ef45c7cb98449ef82f2e309b4690c5faa09c22291) on the Paddock test VM clean-baseline (Windows 11 ARM64 build 26300, 2026-11-11) ran the pinned vc_redist (exit 0, 25 s) and the engine self-test passed; the update over 1.0.3 is not yet proven. **Class:** a
dependency the build machine has and a user's PC lacks, introduced through a binary no build step sees (second instance after the
Dyno sidecar lesson); proposed for a redesign ADR: every downloaded or staged executable has a declared runtime-dependency check
at install time.
**Update over 1.0.3 (open, ER-28).** The silent 1.1.0 installer over an installed 1.0.3 hung in three early VM runs (4 to 13+ minutes; its
`vc_redist.x64.exe` children were alive with no `msiexec` yet, next to `MicrosoftEdgeUpdateSetup`). Plausible causes and what each
measurement showed: a busy Windows Installer (the `_MSIExecute` mutex or `msiexec`: not held in the later runs, and the redist
finished in 20 to 36 s while `TiWorker`, `TrustedInstaller` and `MicrosoftEdgeUpdate` were running); a pending reboot
(`PendingFileRenameOperations` after the 1.0.3 install, present in the runs that passed); Burn's package cache (the log shows it
caches and installs normally); the Edge update (running in the passing runs too). Four later runs did not reproduce it (the redist
directly right after the 1.0.3 install, the 1.1.0 installer right after it, and after a 5-minute wait): 20 to 36 s each. The hung runs
had a very slow VM (SSH ready after 6 to 20+ minutes, against about 1 minute later), so host disk contention is the best guess; it is
not confirmed. The installer therefore bounds its wait at 5 minutes and says what to do (the hook comment has the numbers).

## A test's disclaimed fake runner on an external TMPDIR prompted the user once per test (2026-11-11)

The Mac mini showed "gitea-runner would like to access files on a removable volume" again and again. The prompts looked
like the CI runners', but the tccd log (`AUTHREQ_PROMPTING`, matched to `AUTHREQ_ATTRIBUTION` by msgID) named one
subject for all 18 prompts from 06:06 on: `<TMPDIR on the external volume>/pitcrew-it-<case>-<pid>/runner/bin/gitea-runner`.
That is Pitcrew's ad-hoc signed `examples/fake_runner.rs`, which `tests/supervision.rs` copies as `gitea-runner` and the
supervisor starts disclaimed (ADR 0020), so it is its own responsible process. An agent had run the tests locally with
`TMPDIR` on the external volume, and each test's new path was a new TCC identity, so each test prompted once. CI was not
affected: `job-tmpdir.sh` already puts the job's `TMPDIR` on the internal disk (lesson of 2026-11-04). Chrome for Testing
and the build tools never prompted: they run with the runner (which has Full Disk Access) as their responsible process.
**Lesson (class: a permission belongs to the responsible process, not the parent; second instance after 2026-11-04):** a
test helper that launches a program disclaimed, or through launchd, keeps that program and its files off removable
volumes itself, and does not rely on the caller's `TMPDIR`. The mechanism is one shared rule (ER-21):
`conformance/ci/boot-volume.sh` (`diskutil` says the folder's volume, links resolved, is not internal; then `getconf
DARWIN_USER_TEMP_DIR`), used by `job-tmpdir.sh` and Dyno's `check-bundle.sh` and, for Rust tests, through
`paddock_testkit::boot_volume::boot_volume_temp()` (Pitcrew's supervision harness and its `launch.rs` and `disclaim.rs`
tests). Measure prompts with `AUTHREQ_PROMPTING` before you guess the binary from the prompt's title: the title shows the
file name, and a test fake can share the real program's name. Measured on the Mac mini (`log show` of `tccd`, which kept
entries from 06:06 that day): 18 prompts from 06:06 to 17:44 and 17 more at 17:46-17:47, all for this fake runner, one per
test path; 0 from the CI runners, the test browser or the build tools (Chrome for Testing's requests were attributed to
the runner or the terminal; 80 chrome-headless-shell requests were allowed from a stored consent; node's 8 went to the
runner or the terminal). After the change, two runs of the Pitcrew service tests with `TMPDIR` on the external volume
made 0 prompts and 0 requests for the test folder, while an unchanged run in the same window prompted.

## Start waits kept coming back as fixed budgets: wait for ready or exit, warm a fresh binary once (2026-11-11)

Tests on busy macOS runners failed because they gave a real process, browser or scan a fixed time: dyno-agent's start (CI
7918), Spotter's and paddock-chromium's real browser (7912, 7918, 7929, 7932), Debrief's store-worker test (the product's 15 s
connect window, "the store worker did not connect within 15 s", main 7919 and 7925) and paddock-dev's whole-repository live
scans (budgets only raised). The first run of a freshly installed binary is held by the OS for 42 s on an idle Mac and over
240 s under I/O load, with no CPU and no output, so neither a clock nor a CPU judge can tell it from a hang. What landed under
ADR 0071 (no new ADR): `paddock_proc::warm_up` (a first run paid before the start that matters, memoised by file identity, its
duration printed; macOS only for a browser, since `chrome --version` starts the browser on Windows), `WorkerConfig::for_tests`
and the plugin-test harness waiting until the child connects or exits (`UNTIL_CONNECTED`, ADR 0071 rows 2 and 4; the product
keeps its 15 s window, because the supervisor holds its lock while a launch waits and an exit already ends the wait at once),
and `paddock_testkit::guarded` for a slow job judged on units finished (the live scans). The shared wait `wait_ready_or_exit`
of ADR 0071 section 1 is NOT built yet: `paddock_proc::StartWatch` is the judge it is to drive, and a second wait beside it
would be a copy (rule 55). Still open: ADR 0071 row 13 (the 20-40 s receives and deadlines in the real-browser tests of
paddock-chromium, Spotter and Debrief's chromium-driver; only the first-run warm-up is done) and rows 1, 5-12 and 14-15.

Update 2026-11-12 (`fix/debrief-train5-waits`): `paddock_testkit_start::wait_ready_or_exit` is built (feature `start`, on
`StartWatch`), after Debrief's renderer real-browser tests failed with `navigate: Timeout` on the CI Mac (run 7963): the driver
copy still had the fixed 5 s and 10 s command deadlines that `paddock-chromium` had replaced in `8d8424805`. Debrief's
chromium-driver now judges the start's answers like the canonical, and its tests set `Answers::Judged` (every answer and page
load waited for while the browser is there, until the kit's hang guard); the renderer's real-browser tests take turns with the
driver's. A registered copy that falls behind its canonical's fix fails the same way later: port the fix to the copies or say
why not (rule 55), and a copy's `Copies-waived` reason "adopts the canonical later" does not cover a failure that is already
there. Row 13 of ADR 0071 is done for Debrief; paddock-chromium's and Spotter's real-browser tests still count polls.

## Every dispatch handed the updater key to npm ci, and bundler tools came unpinned (2026-11-12)

ADR 0085 found that `tauri-bundle.sh:55` ran `npm ci` with `TAURI_SIGNING_PRIVATE_KEY` set on every rule 54 dispatch, so
every install script of every branch could read the key that signs updates (not rotated: user, 2026-11-10), and that Tauri
fetched linuxdeploy, AppRun and NSIS at bundle time into a kept per-user folder. Rule 78: the key reaches `tauri build` only
and the real key only on a tag (other runs sign with a throw-away key made in the job; `conformance/ci/updater-key.sh`), and
the bundler tools come from pinned URLs and SHA-256 values into `<build folder>/.tauri` (`conformance/ci/tauri-tools.mjs`,
`useLocalToolsDir`). A secret given to a whole step reaches every program the step starts; hand it to the one command that
needs it.

A review of that fix (2026-11-12) found the key moved into `env`'s command line (`env K=V npx tauri build`), where the process
list shows it to every user of the machine, while the app's `beforeBuildCommand` still ran inside `tauri build` with it, and a
Debrief Windows tag still reused a branch-written `node_modules`. Hand a secret over by `export` and `exec`, never as an
argument; check what a tool starts on its own (a build hook) before calling a step "the one command"; and list every kept
input a tag reads, not only the one that was found.

## Process rules grew faster than the product: cap cost, retire what is not kept up (2026-11-12)

A cut review of all apps and the family rules found the same pattern as the elevated-helper design of 2026-11-11: each
rule and each review round was right alone, and nothing bounded the total. Debrief 1.2 had grown to about 656 raw days,
Pitcrew's 1-2 day caller check was in its 8th review round, and the wiki rule (ER-30) had a writer, a fact-check and a lint
for pages last ingested on 2026-11-09. **Lesson (class: process growth without a cost bound; first instance 2026-11-11):**
rule 76 (ER-37) now needs the user's yes above 1 agent-day beyond common practice, a budget line per release plan with
growth over 20% going back to the user, and one cut review per release. Rules 14, 15 and 51 merged into 13, rule 71 into
ER-33, ER-30 retired, rules 22 and 55 and 54 narrowed, and hooks that judge an agent's text only warn. The review's
proposed cap on review rounds was not adopted: security code keeps its rounds. Mechanism: text rules and the reviewer
line, no tool.

## Store threads still inside DuckDB when the process exits: stop them, do not only order the drain (Debrief, 2026-11-12)

CI 6734 (SIGSEGV) and CI 7997 (SIGBUS) were one class: a background thread of the store (the catalogue builder, the indexer)
still ran a statement while `exit()` destroyed DuckDB's statics. `9dba20d4b` made the exit drain run first of all (on the
main thread, from a thread-local destructor armed before `main`), which cured the order; the cause, threads nobody joined
(a worker thread's strong reference, a leaked store, a test that ended while the catalogue still built), stayed. Measured
on that fix: 6 crashes in 200 runs of the failing `debrief-app` test before it, 0 in 300 after it.

The fix for the cause: every store thread registers itself in `debrief_store_duck::shutdown` while it runs (the thread drops
the registration last); the exit drain asks every registered thread to stop and waits for the list to empty, in every binary
that opens a store; `StoreThreadsGuard` and `stop_store_threads_when_this_thread_ends` (the app's `test_app`) do the same
when a test ends and fail it, naming the thread, if one stays. Windows (reasoning, not measured): libduckdb is a DLL whose
statics die in `DLL_PROCESS_DETACH`, after `ExitProcess` ended the other threads, so the destroyed-static crash cannot happen
there; a thread ended inside DuckDB holding a lock is what stays open, and the same drain, run by `atexit` before
`ExitProcess`, now stops those threads first.

Rule of thumb: a destructor order that cures a crash is not the cause while a thread can still be running native code; look
for who owns the thread and make the owner end it.

## A toast's Show missed its click when the next live alert moved the stack (Debrief, 2026-11-12)

WebKit `live-tail.e2e.ts:100` failed on main run 8004 (job 36718) with the Error level's `aria-pressed` still "false" after
clicking an alert toast's Show, and `:121` failed twice on run 7941 (job 35690) with the Error spike toast still there after
"Mute this kind for 15 min". Hypotheses: (a) live follow re-applies the levels after Show; (b) the 5 s expect is too short on
a slow runner; (c) a WebKit-only event order. The failing traces refuted all three: `show()` never ran (no "Live alert
evidence shown" log, and live refreshes kept saying `Grid: follow`, which Show would have paused), so (a) and (b) had nothing
to act on. In all three failures a live alert was logged between "performing click action" and "click action done" (rule
alerts at +314 ms, +78 ms and +0 ms of clicks lasting 369, 80 and 116 ms). The stack is anchored at the bottom and
`visibleNotifications` puts a new or updated cause last, so the rule toast took the bottom slot under the pointer (run
8004's screencast shows the stack reorder mid-click; on 7941 the new rule toast pushed the error spike up): mousedown hit one
toast's button, mouseup another, and neither got a click. The spec installed Playwright's clock but let it run in real time,
so alerts every 1 to 1.5 s kept coming during the axe scan and the click. Rate on the Mac: 0 of 100 for both tests on WebKit,
also 0 of 100 with six CPU burners. Raised with a probe that holds Show down for 2 s of real time: clock running 20 of 20
failed (10 WebKit, 10 Chromium, the CI symptom), clock paused 20 of 20 passed. So (c) is refuted too: the race is the same in
both engines; WebKit's slower clicks only widen the window. Fix: the spec clicks toast buttons with the clock paused
(`clickInToast`: `pauseAt`, click, `resume`), the pattern of `timeline-header-geometry.e2e.ts`. The product still moves a
toast under the pointer when an alert arrives; holding the stack still while the pointer is on it is left as a decision.
Why the logs did not show it: the app logs alerts and Show, but not the click's target; the trace's console and screencast
were enough. **Class: a control that moves under the pointer during a click (second instance after the timeline header of
2026-11-08, run 7697).**

## A detached Windows child inherits every inheritable handle, not only the standard ones (CI kit, 2026-11-12)
Run 8019 (jobs 37039 and 37040): two Windows jobs shared slot s1 and the second removed the first's checkout mid-build. The
slot holder that `./dev ci slot acquire` starts had died 10 s after the acquire step ended. Cause, confirmed on the test VM
with the real binary (base: the step pipe stayed open; fixed: closed, holder alive): once `./dev ci isolation acquire` (Rust)
started `ci slot acquire`, the step's output pipe reached `acquire` as a plain inherited handle (std spawns with inheritance on),
and `acquire` cleared the inherit flag of its standard handles only, so the holder kept the pipe. The Gitea runner waits 10 s
for a step's pipe after the step ends and then ends every process of the step's job object (gitea/runner
`internal/pkg/process/treekill.go`). The shell path started `acquire` from Git bash, which passes no such handle. Fix:
`acquire` clears the inherit flag of every handle it has before it starts the holder, and refuses to start one when it cannot
(`tools/paddock-dev/src/fds.rs`). Lesson: a process meant to outlive its step must carry nothing of the step; on Windows that
means the whole handle table, and a check of the standard handles alone passes while the real chain fails.

## Eight combination failures on train 8 (2026-11-13, ADR 0098)
Train 8's integrator made eight fixes after the merges (run 8050: 13 failing tests or jobs, 12 new against main). Each branch
had passed its own checks against its merge base: a new rule met new code (`cd0c3aa14` canonicalize ban, `6394bc365` and
`b51ef0308` C71, `ecb6f33ad` the safe_remove allow list), or a test read another area's text or layout (`11c365fc4` a moved
file, `0308201b4` a span ended at a remembered comment, `bddd2947c` a `sed` over ci.yml while `ci_line::conformance_args`
existed, `be69a31f6` a hand list that had to equal `cargo metadata`). Three of the eight were siblings in review at the same
time, so a trial before approval onto the open train could not have seen them; only a check at the merge does. What changed:
the member gate at each merge (`./dev merge check --gates`), the blame command (`./dev ci triage --members`), the one-reader
rule 79 with C121 (report-only), and the trial dispatch (`./dev ci dispatch --trial`). **Class: train-time combination
failures (the queue's Q3 and ER-8 already named it; ADR 0098).**

## An agent killed another agent's process because it assumed the pid was its own (rule 53, 2026-11-14)
The train-11 integrator killed `lefthook run pre-commit` (pid 34522), which belonged to another agent's worktree: it took the
pid for its own hook run without checking the process's working folder or start time. Earlier instances of the same class: a
test's close killed another test's browser by a reused pid (2026-10-30, above), Dyno read `agent.json` and acted on a pid that
had outlived its agent, and Dyno's launcher dropped the handle of its first child and could end only the newest (both
2026-10-30). Rule 53 covers an agent's own process handling too, not only the product's code: a pid read earlier or found in
`ps` or `pgrep` is a hint, never an owner. **Rule for agents:** end only a process whose handle you hold (you spawned it and
kept the child), or, when you must act on a pid, check immediately before the kill that its cwd is inside your own worktree and
that its start time is later than your own command's start; if either is unknown or differs, do not kill: report it to its
owner or the coordinator. Never `pkill -f` or `kill` by a name pattern (`lefthook`, `cargo`, `node`) because every agent runs
the same tools. **Class: acting on a reusable OS name without identity (rule 53).** Enforced, once wired in the main checkout's
`.claude/settings.json`, by the blocking hook `.claude/hooks/kill-guard.sh` (with its `.test.sh`): an agent's `pkill`, `killall`,
`fuser -k`, `xargs kill` and `kill <pid>` are refused
unless the pid is a `%job`, `$!` from the same command, or a live descendant of this Claude session whose working folder is in
the agent's own worktree.

## A cached tool vanished under a long run, and a hard link's age is the file's age (2026-11-14)
CI run 7773's job-isolation release found its pinned paddock-dev gone, and on train 10 six generators of one `gen check` found
heavy gone: `./dev`'s prune of `~/.paddock-dev/bin` counted `heavy-*` folders as its own hashes and never knew a run still needed
an entry, and both `./dev` and heavy's resolver removed a cached binary whose `--version` probe failed (a slow probe on a loaded
machine is enough). The first fix held heavy only, and its sweep judged a hold's age by the file's modification time, which a
hard link shares with the cached binary: a fresh hold of a heavy built a week ago was removed by the next run's sweep (a failing
test showed it). Fix: one mechanism for every cached tool (`tools/paddock-dev/src/tool_cache.rs`): a run holds its own hard link
under `held/` (paddock-dev holds itself whenever it runs from the cache, and starts its children from that hold), a hold's age
comes from its name, a hit touches its entry, and `./dev`'s prune keeps five entries per tool and never removes one used within
a day or with a file linked more than once; a binary that fails its probe is replaced by an atomic rename, never removed first.
**Class: a shared cached tool binary is removed while a long run needs it (ER-21).**

## Member gates passed, the train's own tests did not: run the full tool suites before a train push (ER-8, ER-36, 2026-11-14)
Every member of train 11 passed its member gate (fmt, Clippy on three targets, conformance, lockfiles), and the integrator ran the
tests of the modules its two merge ports touched (229 of them). A full paddock-dev nextest on the merged tree, first in a Linux
container and then on the host, still found three failures that only the combination made: the 0089 merge added C122 to the
check lists of both hosts but not to the registry test (`check_engine`) or the hand-kept ci-run golden, the Rust C122 lacked the
`root` scope its JavaScript host declares (the registry test compares them), and a test in `pre_push/heavy_tool.rs` (from
fix/gen-check-build-first) used `remove_dir_all`, which `safe_remove_lint` refuses under tools/ (resolved by merging feat/kill-guard-and-tool-hold, whose `tool_cache.rs` took that
test over without it). The ci.yml run had not started
yet, so no runner time went on it. A member gate runs no tests by design (ADR 0098); the train's one test run was meant to be CI.
**What changed:** before a train is pushed, the integrator runs the full nextest of `tools/paddock-dev` and of `crates` on the
merged tree (through the heavy slot), fixes what fails in one combination commit, and pushes once; touched-module tests are not
enough for a merge that changes shared lists (check registries, goldens, lint allow lists). **Class: train-time combination
failures (ADR 0098; train 8's eight fixes, above).**

## The guards kept a disk path that no longer existed (2026-11-14)
After all dev data moved to `/Volumes/vault-dev`, `worktree-guard.py` still defaulted its main checkout to the old `d8sst7` path
and `docker-guard.py` its allowed folders to the old `d8sst7`/`d7sst7` dev-targets, and no env override was set. The worktree
guard stopped recognising the real main checkout, so the ER-2 protection (no subagent git write in main) was silently off; their
tests hard-coded the same paths, so they stayed green. Fix: the guards derive the main checkout from git (the common dir of
`$CLAUDE_PROJECT_DIR`, else of the hook's own folder) and the allowed folders from `git config paddock.worktreeRoot`, keep their
env overrides, and refuse with a `Blocked: ... Next step: ...` message when they cannot derive
(`.claude/hooks/lib/paddockpaths.py`). Tests build temp repos at run time. **Rule:** a path that can be derived (from git, a
config key or the script's own location) is never a literal default, and a guard whose input is missing fails closed.
**Class: stale machine paths after a move** (earlier: the tart symlink and plists in paddock-bench/RESUME-AFTER-REBOOT.md).

## A test child that inherits the machine's environment reads the machine (tests, 2026-11-13)
Twice in one day a new environment lookup in a tool reached real machine state through tests: the pre-push integration tests ran
the real heavy steps (`PATH` found `heavy.sh`), and on `fix/precommit-heavy-slot` the helper `dev()` found the real
engineering-rules `heavy.sh` through `ENGINEERING_RULES` and the checkout beside the repository, and took machine-wide slots in
`~/.cache/heavy-slots` (ER-14). Patching each test (`2ff5f5072`) leaves every other spawn site open for the next lookup, because
a deny-list never contains the variable that bites next. Fix (ADR 0100): `paddock_testkit::TestCommand` starts `./dev`, `cargo` and
`node` from an empty environment plus an allow-list (`allowed_names()`), with a private `HOME` and temp folder and a curated
`PATH`; check C123 (report-only) flags a test that starts one of them any other way. Two things the cleaner environment exposed
at once, both real: `lsof` lives in `/usr/sbin`, which a curated `PATH` must name, and `ps -o lstart=` prints another date format
in a clean locale, so a test that compares a stamp must start `ps` through the same builder as the program that reads it.
Lesson: give a test child what the test names and nothing else; Cargo's own test support removes a list of variables and
keeps the rest, which is the shape that failed here.

the same tools. **Class: acting on a reusable OS name without identity (rule 53).**

## Tests summed their steps under one bound and failed on the loaded Mac (2026-11-14, ADR 0103)
CI runs 8050, 8060, 8061, 8066, 8090 and 8099 failed tests that were never hung: the kept-clone parity tests ran 20-24 jobs one
after the other under nextest's 60 s (25.6 s quiet), a Debrief test made 50 001 files to reach a product limit (4.8 s quiet, 90 s
in CI), the crash model's CPU work slowed 9x under other jobs' load, and a VM lock test that failed partway left a detached
`slot hold` that failed nine tests after it. Each fix on train 11 was right and by hand. **Rule:** a test's verdict depends on each
step's progress, not on a sum of steps; independent cases run side by side (`paddock_testkit::both` / `run_cases`); a wait for
several steps is `wait_progressing` (Rust) or `waitProgressing` (scripts/test-kit.mjs), which back off; every process a test
starts goes through a `Reaper` (Rust `paddock_testkit_start`, Node `scripts/test-kit.mjs`), which ends detached grandchildren too;
a limit is tested with a small limit; a CPU-bound test gets a measured size class, not a raised default. C126 (ADR
0103) lists the shapes, report-only. **Class: tests time out on the loaded Mac (ADR 0102 is the machine side).**

## A red test of the Reaper ended other processes on the machine (2026-11-15, ADR 0103)
Writing the failing-first test for "a short marker is refused", the designer called `Reaper::marked("/tmp")` before the guard
existed. The red run (three runs between 00:12:04 and 00:12:40 +08, from file times) did what the guard was for: its drop ended every process whose command line contained `/tmp`, by identity,
three runs in a row; that included the agent's own shell and very likely other agents' running shell commands (their wrappers
name `/tmp/claude-…` files). **Rule:** a failing-first test must be safe in its failing state: when the missing behaviour is a
guard against a destructive action, the red test feeds an input that the action cannot harm (here a short marker that matches no
process), never the real dangerous one. A tool that ends processes never acts on a pattern match alone: the rebuilt reaper
(ADR 0103) acts only on its own children, what it saw below them, its groups, its token, and orphans naming a folder it made
with a random name. **Class: acting on processes by a name or pattern
(rule 53; the lefthook kill of 2026-11-14 above).**

## The local Linux gate could not judge a worktree, and amd64 emulation hid failed execs (2026-11-14)
Train 10's local Linux container run (the CI image, `--platform linux/amd64`, on the Apple Silicon Mac) failed in four ways that
also failed on main, none of them in the code: a worktree's `.git` file named a git folder the container did not have (about 35
paddock-dev tests: "not a git repository"); the read-only `/work` refused `dev check --pre-push`'s logs in
`<git common dir>/paddock-pre-push` (os error 30); under qemu-user a failed exec is not reported to the parent, so every
missing-program test failed (paddock-proc's six, heavy's exit-127 test, paddock-dev's slotflow, input_hash, slots and idle_run);
the kept-clone parity test ran past its one-second phase limits under emulation (it passes natively); and the prune parity test
differs on OrbStack's btrfs, where an empty folder has no blocks and a few bytes of length: GNU `du -sk --apparent-size` prints
0 for 18 bytes, the Rust port (`ci_host::prune::size_in_units`) rounds up to 1, so only the Rust side removes the folder at a
1 KB cap (CI's ext4 gives every folder a 4 KB block, which hides it; open, owned by paddock-dev). The emulation class had been seen twice before:
f08362a29 (Spotter's notifier, 2026-10-30) and feat/heavy-priority-lane (2026-11-13). What changed: `tools/linux-container.sh`
mounts a worktree's git folders read-only at their own paths with a tmpfs pre-push state folder, and `--ci-image` runs the CI
image natively (linux/arm64 on the Mac, built locally from `ci-image/Dockerfile` on `ci-image/garage-arm64.Dockerfile`: garage's
base tag and tool versions as recorded in `ci-image/garage-release.txt` and checked by verify.sh, paddock-ci's own pins on top;
apt packages float, and there is no Chrome for Testing or draw.io) and warns whenever amd64 is emulated. CI's Linux jobs stay on their native amd64 runners. **Rule for agents:** run the local Linux
gate with `--ci-image` and no `--platform`; a missing-program or timing failure seen only under emulation is not a finding.
**Class: environment-only failures of the local Linux gate (amd64 emulation on Apple Silicon).**

## The push gate ran out of budget on work, not on queue time; a local base would have hidden commits (ADR 0102 amendment, 2026-11-15)
Five pushes on 2026-11-14/15 were "not measured" by the 15-minute budget. The blamed cause was queue time; the logs said
otherwise (queued 0 to 194 s, run 766 to 815 s): branches stacked on a train that was built locally and not yet pushed (or only
partly) planned Clippy for every workspace the train touched (280 and 461 files), and a person's run always measured from
origin/main. The first fix measured such a branch from the local train; review found that this drops the train's unpushed
commits from the check while `git push` still sends them, and that any local ref named `train/*` could shrink a check. The fix
that landed uses one definition, git's own: check the commits the remote lacks (`rev-list <tip> --not --remotes`), from the
nearest commit of the remote the push contains, verified live; a person's run uses the same choice. Pass records name their
base commit and count only against a base the remote has. The queue keeps its own bound, now for the whole push, and heavy
steps of one push run one at a time while light rows go on.
**Rule:** before fixing "it ran out of time", read the run and queue time each row reports; and a check's scope is defined by
what the push sends, never by local names. Pushing a train before stacking work on it keeps the checks small.
**Class: the pre-push gate's scope differs from what the push sends (an old base measured more; a local base would have
measured less); earlier: the 2026-11-12 base choice from origin/main for train-based branches.**

## A zombie answers kill(pid, 0): CI's job container never collects orphans (2026-11-15, CI run 8110)
The VM lock test "a holder killed by its test leaves no `slot hold` process" passed on the Mac and failed in ci.yml's ui job:
the detached holder had exited, but in the job container PID 1 is `sleep 21600` (the runner's entrypoint), which never collects
an orphan, so the holder stayed a zombie under PID 1 and `kill(pid, 0)` still answered for it; the guard polled its whole 15 s
bound. Reproduced in `node:24-bookworm` with entrypoint `sleep`: stat Z, ppid 1, kill0 alive. **Rule for tests:** a process
check that means "has it ended" treats a zombie as ended (`running()` in scripts/vm-test-support.mjs: exists and `ps` stat is
not `Z`); a bare `kill(pid, 0)` is only "the pid exists". Tests that start detached processes run once in a container whose PID 1
is a plain `sleep` before a train push, as CI's is. **Class: a Linux CI container differs from the dev host (ER-9; run 7485's
root user and software rasteriser, above).**

## An open menu ran its neighbour's command after a lazy load hid a row (Debrief, 2026-11-15, CI run 8116)
`ux-demo-tour.e2e.ts:58` flaked (2 of 156): after a reload, File ▸ Close workspace ran Close window. The lazy share commands
loaded while the menu was open, `report.export` hid itself, and the rows below moved up; the list was rendered with
`track $index`, so the button Playwright had resolved kept its place and took over the next row's label and command (its
hit-target check passed, as it was the same element). First hypotheses (async tour state, shared storage, an overlay, a fixed
wait) were refuted by the trace: the Activity log named the command that ran. 14 of 100 failed in the CI image before, 0 of
100 after. **Rule:** `track $index` on interactive rows that can change while shown reuses a row for its neighbour; key by
identity (command id; position only for rows that run nothing), and keep ids unique per level. Fixed in Debrief's menu bar,
paddock-ui's `pd-menu-bar`/`pd-context-menu` and `pd-tray-popover`. **Class: stale element identity after a list changes
under an action** (earlier: the toast whose Show missed its click when the next alert moved the stack, above).

## A test took a helper's identity after the code under test had already ended it (2026-11-15, CI run 8118)
Spotter's `a_browser_that_exits_leaves_no_helper_behind` failed once on the macOS runner after 10.09 s with "timed out waiting
for the helper's pid file" and passed on its retry in 0.15 s. First hypotheses: the helper started late (a cold exec or a
first-launch scan, the "macOS host stalls a test" class), a torn pid file, a reused temp folder. Refuted: the stand-in and its
helper are `/bin/sh` and `/bin/sleep` (signed system binaries, no first-launch scan); with the timeout naming its last look, 10
of 200 runs under 6 CPU burners failed, and every one had read a complete pid file whose process had already ended. **Cause:**
the stand-in wrote the helper's pid and exited at once; `Chromium::launch` saw the exit within its 50 ms poll and ended the
tree, so a test thread that the loaded host kept off the CPU for that long took `ProcessId::of` of a pid that was gone, and
polled for a "pid file" that was there all along. **Fix:** the stand-in waits on a FIFO the test opens only after it has the
helper's identity (a real signal, no sleep, no longer bound); the pid file is written by temp file plus rename; the poll's
timeout says what it saw last. A second test makes the slow path certain (the helper announces itself a second late): red
before (the tree is ended before the helper writes), green after; 200 of 200 green under the same burners. **Rule for tests:** a test that must observe a process
the code under test is about to end holds the stand-in until it has observed it, and a wait's failure message names its last
observation. **Class: an observation raced the action under test (rule 53's "act on a name read earlier", seen from the test side).**

## The two hosts of a check gave different answers on error paths; a shared helper now owns them (ER-21, 2026-11-15)
C121, C124 and C125 read their allow lists through one shared helper, but each check still handled the helper's error paths
itself, and the Rust and JavaScript hosts handled them differently. The parity tests compared findings only on trees where
everything could be read, so the differences went unseen. C125 was fixed first (865498235). On train 12 the same bug showed in
C121 and C124. For a broken allow file, the Rust host reported an unmet precondition and went on scanning, while the JavaScript
host gave one check-broken finding. For an unreadable file, the Rust host marked the run incomplete, while the JavaScript host
skipped the file without a word. The fix (`fix/allow-list-shared-reader`) moves both outcomes into the helper, in both hosts
(`conformance/src/util/allow-list.mjs` and `tools/paddock-dev/src/check/allow_list.rs`):
- a broken allow file gives one unwaivable check-broken finding and nothing else;
- a file the check cannot read gives a not-measured error at that file.
Each check declares the reader's `Spec` as its allow list (`Check::allow_list`, `allowList`).
`tools/paddock-dev/tests/it/check_allow_list.rs` runs the error-path parity tests for every check that declares one. It
uses each check's own control tree and compares both hosts.
**Rule:** a helper that several checks share owns its error paths. Its parity test runs over every user the helper declares, not
over a hand-picked check, and it covers the error paths as well as the clean run.
Two limits remain, both opt-in:
- A check that calls the reader without declaring its `Spec` skips the parity tests, and nothing notices.
- The JavaScript error-path test uses a hand-made table of samples (`SAMPLE` in `conformance/test/allow-list.test.mjs`). The
  test fails when a declared check has no sample, but the table is still a hand list.
**Class: the two hosts diverge on an error path; earlier: C125 (865498235), then C121 and C124 (train 12).**

## A recorded pid is read once, as an identity or as "ended" (2026-11-15, the review of f224dd0b4)
The review of Spotter's helper fix found the same shape in more tests: a test parses a pid from a file or a child's output and
calls `ProcessId::of` on it later, after the code under test may have ended that process. `of` answers `None` for an ended
process, which a poll reads as "not yet" (Spotter's never-ready test with a 3 s guard, paddock-proc's 3 s timeout test: a test
thread kept off the CPU for the guard waits out its own bound) and an `unwrap` turns into a panic (paddock-chromium's
launcher-gone test, whose fake ends on purpose); and once the pid is handed on, `of` gives the new holder, which a failure
path then ends (tools/heavy's background-sleep tests, ER-14). **Fix:** `paddock_testkit_start::recorded` reads the record
once it is complete (a pid and a newline) and takes the identity through `ProcessId::of_recorded` with the file's modification
time, or, for a printed pid, a `Stamp` taken before the spawn; it answers `Recorded::Ended`, which is final. Each site got a
test that forces the late look (the test looks only after the run returned, or the stand-in has ended before its pid is
printed), red before. C128 (report-only) flags `ProcessId::of` on a pid parsed in the same function. **Class: an observation
raced the action under test (rule 53, seen from the test side; run 8118 above).**

## Two branches that each added a lesson conflicted at the end of the file (2026-11-15)
Every train that merged two branches with a new lesson (or a new benchmark block) stopped on a conflict in `docs/lessons.md`:
both sides added lines after the same last line, so git saw one hunk changed twice, and the integrator resolved it by hand each
time. The lessons log now merges with git's union driver (`.gitattributes`: `docs/lessons.md merge=union`), which keeps both
sides' added lines; `conformance/test/union-merge.test.mjs` merges two such branches in a scratch repository.
`docs/benchmarks.md` and the apps' `docs/performance.md` stay on the normal merge: their results blocks are corrected in place
(review of 2026-11-15), and the test checks that they are not union. **When union is not safe:** it never reports a conflict, so a change that
edits or deletes lines in place (a reworded lesson, a corrected benchmark row) merged with another branch's edit of the same
lines keeps both versions, silently. Rewording an earlier entry goes in its own change, merged alone, and the integrator reads
the merged hunk of these files like any other. Registers, tables whose rows must be unique and generated files (`gates.toml`,
`known-copies.toml`, lockfiles) are never `merge=union`: a duplicated row there is a bug, not a merge.

## The nightly disk summary measured a folder CI runners do not have (2026-11-15)
The nightly's `disk-summary` job ran `./dev disk scan` and `./dev disk report --summary` on the Mac runner. Those read the
dev-targets folder (worktrees, build folders, agent scratch), which lives on the development machine, not in a runner's
environment, so it summarised folders a runner does not have (or wrote "not measured") while it held the single Mac runner.
The user asked whether it was needed; it was removed (with its README line). `./dev disk report` gives the same picture on the machine, and the
`--summary` option stays for use there. `conformance/test/ci-actions.test.mjs` fails on any workflow step that runs
`./dev disk scan` or `./dev disk report`. **Rule:** a CI job reads only what a runner has; a check of the development machine
runs on the machine.

## Waiters stacked and polled without a bound although the pgrep guard existed (2026-11-15)
The `no-pgrep-wait` hook only refused a `pgrep -f` loop, by a regex over the whole command text (it also refused such a loop
written into a heredoc or a commit message). Agents still started a second `./dev wait` for a run another one already waited
on, and polled logs with `while ! grep ...; do sleep ...; done`, which has no maximum duration. The hook now runs on
`lib/shellscan.py` (the loop's condition, not any text), keeps the `pgrep -f` refusal, and warns (never blocks: ER-36, a hook
that judges text only warns) on a grep polling loop, on two waits for one target in one command, and on a wait for a target a
running `dev wait` already has (read from `ps`; it cannot tell which agent started it, so the warning says so). Rows are in
`lib/shell-forms.tsv`, whose runner now knows a `warn` verdict. **Class: unbounded or stacked waiters (ER-32; earlier: the 36
stuck waiters of 2026-11-03/04).**

## The shell scripts deleted computed paths with a bare rm -rf while Rust had a guard (2026-11-15)
paddock-dev's deletes go through `safe_remove::remove_under` (strictly under a vetted root, no `..`, no link on the way), but
the shell scripts, which still run as the CI kit's fallback, on tag runs and on the development machine, had no such helper:
each computed `rm -rf "$x"` trusted its variable. The class has bitten or nearly bitten four times: the 2026-10-31 scratchpad
wipe, the 2026-11-10 live-worktree removal, the prune script's IFS near-miss and its stat fallback. `conformance/ci/safe-rm.sh`
(`safe_rm <root> <path>...`, POSIX sh) is the shell copy of the Rust guard (docs/known-copies.toml, `safe-remove-under`): it
refuses an empty path, `/`, a top-level or home root, `..`, a relative path, a path outside the root and a link on the way,
and removes nothing when one path is refused. The prune, host-cleanup, node-caches, stable-workspace, kept-clone and vm-win
prepare sites use it with the same roots as their Rust twins. C127 (report-only) lists every other recursive `rm` of a computed
path in a shell script. **Rule:** a shell script deletes a computed path only through `safe_rm`, with the root it owns; a
mktemp folder of its own, or a site that cannot name a root two folders deep (a guest VM's home, Linux's `/tmp`), carries
`# conformance: computed-rm — <why>`. Review then found that both copies checked one spelling and removed another: `root/link/`
(a final link with a trailing slash) passed as a path strictly under the root, and the OS read it as the folder the link names, so
the delete emptied a folder outside the root; the Rust guard had the same hole. Both now remove the path rebuilt from its checked
names, and one table (`tools/paddock-dev/tests/fixtures/safe-remove-parity.tsv`) is answered by both copies' tests. **Class:
computed deletes beyond their folder (C115's third sighting, ER-21); check and act on the same normalised value.**

## A train waiting to dispatch was starved by scheduled runs (2026-11-15)
`./dev ci dispatch` waited while another run held the single Mac or Windows runner, and `ci schedule-skip` ends a scheduled run
early only while a train's run is queued or running. A train whose dispatcher was still waiting had nothing queued, so each new
scheduled run took the runner first: on 2026-11-14 around 11:00 and on 2026-11-15 from 05:03 to 07:07 (runs 8112 to 8114). A
first fix recorded a "waiting" marker in a repository Actions variable; review found that it needs a repo-admin-scoped token,
and the common-practice answer is a queued run, as CI systems do (a job waits on a free runner, not the client). It was
reverted. Now `ci dispatch` sends a `train/*` run of a workflow whose first job runs the `runner-wait` action at once, and that
Linux job waits (`./dev ci runner-wait`, at most 2h30m, logged) until no OLDER run has an unfinished job on its Mac or Windows class (an older run's later Linux jobs do not hold it:
review round 2 found that waiting for the whole run could fill the Linux slots with waiters).
Older only: two runs of one train never wait for each other, so the oldest always goes. The queued run is what schedule-skip
already yields to, with its bounds (never twice in a row, never past the gap). Conformance checks that every macOS or Windows
job of such a workflow needs the runner-wait job. **Rule:** a wait for a shared resource lives in the queued job, not in the
client that has not queued yet. **Class: a scheduler that cannot see work that has not been queued.**

## safe_rm's per-call programs pushed kept-clone's checkout phase past its 1 s test limit (2026-11-15)
`host_steps::kept_clone_the_checkout_phase_is_bounded_by_its_own_limit` failed under load after `kept-clone.sh` moved its deletes
onto `safe_rm` (997ab3a4e): the shell and Rust kept clones differed in their config keys (the shell had no `core.sparsecheckout`),
so the shell's 1 s checkout limit had ended the phase in `sanitize`, before `apply_profile` and before the git the test hangs. First
hypotheses and what refuted them: bounded.sh's polling missing the last output, buffering of the last line, and a comparison of
differently buffered outputs: the two stdouts were equal in every failing run; only where the shell's phase was killed differed. The
cause: each `safe_rm` call started 16 programs (three same-case calls on the parent: 12 `tr` and 36 `uname`, plus subshells):
`sanitize` (two calls) took 236 to 268 ms on average, at most 675 ms, at 1-minute load 25 to 40, and about 35 ms with the fix (30
calls each, timer included). With 6 burners: 4 of 40 runs failed with safe_rm at load 40 to 60, 0 of 40 with the plain `rm -rf`
before it (load 30 to 46), 0 of 60 with the fix (load 17 to 30), and an interleaved run of both at load 18 to 39 passed 32 of 32
each (too little load to tell them apart). With 6 burners and ten loops of the test at once (load up to 38): 79 of 80 passed; the
one failure was another cause, on the Rust side: a checkout phase abandoned at its bound (limit + 1 s) finished its git after the
next phase had started and set that phase's shared `timed_out` flag, so `create the clone` reported "did not finish within 900 s" at
once (the phases share one `Kc` state). The fix: the OS is asked once when the helper is sourced, and plain ASCII case is folded by
the shell, so a call starts no program but `rm` (`tr` only for names beyond ASCII); the limit is unchanged. **Rule:** a helper that
runs inside a bounded phase costs no program per call that it can compute once or in the shell itself. **Class: work before the hang
point of a 1 s hung-phase test meeting its limit under load (earlier: fe1bd861d, a `git rev-parse` over 1 s in 4 of 204 runs;
e0fdd0286, run 8099).**

## An abandoned kept-clone phase marked the next one hung, and hung-phase tests timed the work before the hang (2026-11-15)
Under load (6 burners and ten loops of the checkout parity test, load up to 38), 1 of 80 runs failed with the Rust side printing
`'create the clone' did not finish within 900 s` at once. The Rust phases shared one error buffer, log and timed-out flag; a
phase abandoned at its bound (limit + 1 s) keeps its thread, and its git, ended at its own 1 s limit, returned after the next phase
had started and set that phase's flag. Each phase now writes to its own state, and what an abandoned phase writes later goes
nowhere (failing first: `a_phase_abandoned_at_its_bound_writes_nothing_into_the_next_phase`). The same tests had met their 1 s
limit three times through work before the hang (a `git rev-parse` outside the phases, five phases in one test, `sanitize` once
`safe_rm` started 16 programs per call), so the limit they assert now counts from a mark the stand-in git writes as it enters its
hang, in the shell and in Rust (ADR 0103, "Amendment"). **Rule:** a test of a time limit times the thing that hangs, from a
signal it gives, not the setup before it; state that an abandoned worker can still write is that worker's own. **Class: work before
the hang point of a hung-phase test meeting its limit under load (third sighting; ADR 0103 amendment).**

## A git-fixture property test timed out on Windows: count a fixture's processes (2026-11-15)
The pre-push property test built 24 random git stacks in one test: 1,651 git processes, 17 s on the Mac, and past nextest's
60 s limit on the Windows runner (run 8138, job 39335), where a process spawn costs about 100 ms. Enumerating the shapes (each
train state once, each flag on and off in at least one) cut it to 267 processes and 2.9 s; each shape is now its own test, so
nextest runs them in parallel and a failure names its shape. The test files are planned through the shared `-z --no-renames`
reader, which the review found the production code had not used (a `git mv` out of a workspace and a non-ASCII path were missed;
fixed with failing shapes first).
**Rule:** before adding cases to a test that builds git repositories or many files, count the processes (or files) one case
costs and multiply by Windows' spawn and file cost; split cases into separate tests so they run in parallel.
**Class: tests that pass on the Mac and time out on Windows because of per-process or per-file cost; earlier: 50,000 files
timed out on NTFS + Defender (Platforms, "Tests that pass on the Mac fail on Windows").**

## Dyno's row-menu e2e failed once in about 100 runs: a live update redrew the focused row (tests, 2026-11-16)
Symptom: in WebKit `tables.spec.ts` "after switching Apps to All, Shift+F10 opens the focused row's menu" failed once with
`getByRole('menu')` not found and passed on its retry (run 8161, job 39628; three flaky marks in 368 runs of history).
Causes listed with what would rule each out: the key reached the wrong element (a `keydown` listener showed the key on the row
every time: ruled out); the menu opened before the list settled (no: it was added, then removed); the clock stood still (it
did not: `page.clock.install()` lets time flow, a probe read 1,523 ms in 1.5 s); a re-render replaced the row. Root cause,
measured: a live update redrew the row's element (an `Element.remove` hook showed `MatTable.renderRows` > `forEachIdentityChange` >
`viewContainer.remove`). pd-table's virtual-scroll viewport puts the CDK table on its recycle strategy; a view reused from the
row cache comes back with a new ViewRef (`ViewContainerRef.detach`, @angular/core `_debug_node-chunk.mjs` 9697-9701); `renderRows`
records the row definition only for INSERTED views, not for REPLACED ones (cdk `table.mjs` 1602-1606); so the next identity-change
render (1608-1617) recreates each reused view. It happens on any update where rows come and go, not only after a swap (20 views
reused on the swap, the cache size; the row indexes matched, 0 mismatches). The row's trigger went with the element, so an open menu closed and
the focus fell to the page. Probe (ticks forced during the key, WebKit and Chromium, 15 runs each): red 15 of 15 (WebKit 14) without a prior
update, green 15 of 15 with one `clock.runFor(1000)` after the swap. Fix in the spec: take that update before the row gets the focus,
and its comment no longer says the clock stands still. **Open (product):** the redraw is real for a user too (a row menu opened in
the first second after a list swap closes on the next update); a fix needs a workaround in `pd-table` around the CDK table. **Resolved (2026-11-16, user: local workaround, no upstream issue):** the cause is an upstream regression since @angular/cdk 22.1.8 (angular/components#33670; 22.2.4 installed, unfixed upstream as of 22.2.7 per the research, 2026-11-16); `pd-table` records the views of its body table after each render (`recordRecycledRows`), the spec "keeps every row element through live updates after rows were removed and added again" was red on stock CDK (9 of 12 rows redrawn) and the Dyno e2e fixme is a test again; the extra `clock.runFor(1000)` in the Shift+F10 spec is gone; a canary spec (workaround switched off, stock CDK must still redraw) fails when a CDK upgrade fixes the bug and says to delete the workaround. Dyno row keys carry a start time the OS gave earlier (`carryStarts`), so a start time that comes and goes does not flip a key. Remove the workaround when the CDK fixes it (the spec stays). **Class:
a test assumed a quiet page while live updates ran (earlier: Debrief's Stop-button probe, 2026-11-15; this file's Escape fix, 0315ca827).**

## The AI browsing chip's Stop moved when the jobs pill appeared, and the click was lost (Debrief, 2026-11-15, CI run 8136)
WebKit `browser.e2e.ts:71` flaked on main (2 of 147 runs): after "Stop the AI's browser" the chip was still there. The trace
had no "Stop pressed on the AI browsing chip" log, so `stop()` never ran: the click did not reach the button. Its DOM
snapshots show the jobs pill (`status-jobs`, "1 job running or waiting") inserted between the click's resolve and its end;
the pill sits to the right of the chip in the right-anchored status bar and appears 2 s after the question is queued (the
engine's and the mock's `visibleMs`, emitted on a 400 ms tick), as does the problems badge after its scan. Hypotheses: the
chip re-created by the status bar (refuted: items are tracked by id), Stop disabled while `stopping` (refuted: no log at
all), the mock dropping Stop (refuted: nothing called it). Measured in the CI image (WebKit): pressing Stop, letting the
pill appear and releasing lost the click 10 of 10 times, Stop moved from x 1140.5 to 1092.9 (the failing trace's box is at
1140.5). **Fix (spec):** `clickWithTimePaused` (`e2e/app.ts`) clicks with the page clock paused (`pauseAhead`, which
`live-tail`, `resilience-context-live` and `timeline-header-geometry` now use too), so no timer can move the bar during the
click. Proof that matches the rate: a probe that forces the pill to fall due during the click (clock running: `runFor(3000)`
between press and release; paused: `pauseAhead`'s 5 s jump inside the helper) lost the click 6 of 6 times with the clock running
(Stop from x 1140.5 or 1180.2 to 1092.9) and landed it 6 of 6 times through `clickWithTimePaused`; the two Stop tests were then
60 of 60 green (`--repeat-each=30`), all in the CI image on WebKit. At CI's rate (2 in 147) 60 runs alone would not prove it.
The probe, to rerun it: a spec that installs the clock, opens with `openAiApp`, switches the chat's browser to Agent, asks,
waits for the chip and for no `status-jobs`, then either presses the Stop button with `page.mouse.down()`, runs
`page.clock.runFor(3000)`, waits for `status-jobs` and releases (expect the chip to stay: the lost click), or calls
`clickWithTimePaused(page, stop)` (expect the chip gone); run it with
`npx playwright test <probe> --project=webkit --repeat-each=6` in `apps/debrief/app` (CI image, `DEBRIEF_E2E_PREBUILT=1`).
The product still moves Stop when a status item to its right appears: the user approved ADR 0061 for the status bar and the
toast stack (2026-11-15), which will replace the helper. Why the logs did not show it: the app logs Stop, so its absence named
the lost click; the click's target is not logged. **Class: a control that moves under the pointer during a click (third instance after the
timeline header, run 7697, and the toast stack, run 8004).**

## axe read a table row half-way into the dark theme although the transitions had been settled (Pitwall, 2026-11-15, CI run 8134)
WebKit `a11y.e2e.ts` "benchmarks" failed on main with `dark: color-contrast` on every row's muted cells: #a0a7b2 on #5c5d60,
a background 70 % of the way from white to the dark surface; the cells' `transition: background 100ms` was running while axe
read them. Train 11 changed nothing in Pitwall's styles or the tokens (refuted by the diff). The trace: the DOM did not change
from the theme switch until after the dark scan, the screencast still showed the light theme 80 ms after `emulateMedia` and
the grey rows 375 ms after it, during the scan, so the transition began after `settleTransitions` had looked. Not reproduced:
0 of 20 runs in the CI image, and probes of emulate plus settle (40 switches, with the page kept busy 0 to 450 ms) always
found the transition and finished it; with six CPU burners the runs timed out instead. So why the transition started late
on the runner is open; the likeliest cause is that WebKit applies an emulated scheme only at its next rendering update, which a
loaded runner delays past settling. **Fix (helper), after three rounds:** first a detect-and-rescan (record colour transitions
and animations that start during a scan, scan again); its review found partial name matches (`stroke-dashoffset` read as
`stroke`) and unreadable keyframes counted as colour, and the next check found short animations (30 to 90 ms) and `::before`
ones that its lookup after `animationstart` never saw, so false contrast failures came back in both engines. The user then
chose common practice: `axeViolations` holds the page still for the scan (a constructed stylesheet with `transition: none`
and finite animations jumped to their end, `animation-duration: 0s; animation-delay: -1ms`, on `*, *::before, *::after`, so a
tooltip that fades in with `forwards` is read open where `animation: none` left it invisible; plus `prefers-reduced-motion:
reduce`), waits two frames, scans once and undoes both; `axeInThemes` and `axeTargets` also wait until the switched scheme is live (`matchMedia`, two
frames; ER-33). Selftest `selftest/axe-themes.spec.ts`, run in CI (ui's e2e job, Chromium; e2e-webkit, WebKit), starts motion
from inside axe's scan: a 90 ms colour animation, the same on `::before`, and a `fill-opacity` transition were red against the
rescan version in both engines and are green now. **Rule:** a check that reads a moving page stops the motion; it does not try
to detect every kind of motion. **Class: axe read colours that were still changing (fourth instance: the theme switch transition that `settleTransitions`
was made for, the hover tooltip fading in, run 7872, this run, and the rescan's own misses).**

## Two shared-crate tests failed once each in run 8174: a harness that let the peer read after the deadline, and `ETXTBSY` (tests, 2026-11-16)
Symptoms (crates job, Linux, each green on its retry): `paddock-ipc` `the_reference_endpoint_passes_every_scenario_it_supports`
failed with `mini-reference owner-close-while-peer-not-reading: drain-not-eof` after 0.010 s, and `paddock-plugin-host`
`a_program_changed_after_install_is_a_quarantine_grade_fault_and_is_not_started` panicked on `Os { code: 26, "Text file busy" }`
after 0.94 s. Neither crate had changed since run 8163.
**ipc.** Causes listed: the endpoint's drain ends early (no: its end line said `reason=reasonUnsent drain=none`, the path of a peer that
still does not read, not a drain); the fake clock lost a wake-up (no: both waits were counted); the harness let the peer read only
after it had advanced the reason deadline (confirmed). The endpoint tries the reason once more when its wait ends, so which peer
it found, still stopped or reading again, depended on which thread ran first. A new `mini::run_with` hook makes the harness wait
for the endpoint's `reason decided` line before it goes on: the old order then fails 1 of 1 with the CI message, the fixed order
(the peer reads before the deadline is advanced) passes 400 of 400 on Linux under 4 CPU spinners.
**plugin-host.** Causes listed: a process still runs the file (no: `/proc` showed no process with the file as exe, a map or a
descriptor when it failed); a leaked write descriptor in a forked child (no: the failing call is an open for writing, and that
descriptor would make an exec fail); the kernel keeps the file text-busy for a moment after the process was reaped (the same
refusal Go's tests retry, golang/go 22315, 56811; reproduced in 7 of 1,000 runs (3 of 100, 1 of 150, 3 of 300, 0 of 450) under 4 to 14 CPU spinners in a Linux container;
0 of 300 with the fix). The product path has the same refusal in the other direction (a program written or installed a moment ago
and started at once), where it was reported as a counted crash.
**Fix, one mechanism.** `paddock_env::text_busy::retry` retries only `ETXTBSY`, at most 8 times with doubling pauses (about
1.3 s in all, logged and counted), and every start in `paddock-proc` goes through it. `paddock_testkit::exe::write_program` writes a
program through a short-lived `install` process (the parent never holds its write handle, and a running program is replaced,
not rewritten) and `exe::spawn`, `output` and `status` retry for programs a test did not write. 52 test sites that wrote a
script and then set its mode now call `write_program`.
**Rule:** a test writes a program with `paddock_testkit::exe::write_program` and starts one it did not write with `exe::spawn`,
`output` or `status`; product code starts programs through `paddock-proc`.
**Class: a test or harness whose outcome depended on which thread ran first (earlier: the Debrief Stop-button probe and the row-menu e2e above), and Linux's text-busy refusal of a file written or run a moment ago (no earlier lesson).**

## Rules ER-43 to ER-47 and the ER-9, ER-14, ER-24 amendments (user, 2026-11-15 and 2026-11-16)
Waiter holding a slot the waited-for build needed (2026-11-14/15, ER-43); a slowdown fix split into a small part and a deferred root fix (2026-11-16, ER-44); a probe that would have started installs and a reviewer's probe that installed into `~/.rustup` (ER-45); growable exception lists and Mac-only goldens, Spotter e2e after midnight UTC (run 8154, ER-46); cleared child environments (run 8138, ER-47); train 12's red rounds (runs 8138, 8147, 8152, 8154, ER-9). The rules and reasons are in `docs/rules.md` 80 to 84 and the engineering rules. **Class: a mechanism changed the thing it checked, waited on or started.**

## A gate on the wrong machine and an assertion over nothing (2026-11-16, ER-9 and ER-12)
The Windows-on-Arm VM and an amd64-emulated container failed 22 paddock-dev tests on a base the real runner passed, so a member check on another kind of machine measured the machine, not the branch. Dyno's layout check passed on zero rows (an `every()` over an empty set) and failed 31 of 240 runs under the pinned clock. **Rules:** a branch-deciding gate runs on CI's kind of machine, else it is not measured (rule 50 (k), the member gate); a set assertion requires a non-empty set and a layout check needs two agreeing reads (rule 49 (d)). **Class: a check that could not see what it checked still reported.**

## Unaffected release gates held the Mac runner (2026-11-16, train 12; rule 54 (a2))
Train 12 dispatched every app's release gate whether or not its diff reached that app: runs 8164, 8165, 8167 and 8169 cost about 77 minutes of the single Mac runner, up to 34 minutes each, and delayed the gates that mattered. **Rule:** the plan over the train's diff against main decides which release gates run; a skipped gate records the plan output in the landing report, and the counting full `ci.yml` run always runs. **Class: a fixed sequence ran work the change did not need (rule 50).**

## Gates repeated, reviews before the merge, assembly after the green run (2026-11-16, train 12; rules 50 (k) and 54 (a3))
Train 12 ran each affected release gate with the tests that `ci.yml` had just run on the same sha (about 77 minutes of the Mac runner in total, runs 8164, 8165, 8167, 8169), reviewed each member before its merge, and assembled the next train only after the current one was green, so the runner or the people waited with nothing running. **Rules:** release gates run with `gates=false` once `ci.yml` is green on that sha (the installers job needs no `gates` job, so its bundle checks still run; the demo release workflows cannot); the member review runs beside the train's CI; the next train is assembled on the running train's head during its CI. **Class: a fixed order serialised work that could overlap (rule 50, ER-36).**

## Old races in unchanged code held a train (2026-11-16, run 8174; rule 50 (k) item 4)
Two old races in shared crates that train 12 did not touch (`paddock-ipc` endings and the plugin-host ETXTBSY) passed on their one retry and held the train for hours. **Rule:** a FLAKY test blocks only when the change reaches its code; otherwise it becomes a fix item with its log, fixed next train with a failing reproduction first; a test failing both tries always blocks; the run stays marked FLAKY. Large projects (Chromium, Google) quarantine such flakes instead of blocking unrelated changes. **Class: an unrelated old defect held a landing (ER-12, rule 5).**

## "3 of 3 green" was 3 of 7 red (2026-11-16, Debrief live tail; rule 49 (e))
A live-tail fix was reported "3 of 3" and was 3 of 7 red; a later "12 of 12" was 4 of 150 red (`fix/debrief-events-grid-live-tail`, `fix/debrief-live-tail-adr`). A small green count says little when the failure rate is a few percent and depends on workers and engine. **Rule:** a stability claim states runs x workers x engines (or OS); e2e timing fixes run at least 20 per engine at 4 workers, more when the measured rate needs it; a bare "N of N green" is rejected in review. **Class: a claim without its sample size (ER-28, ER-12).**

## Linux-only Rust was never linted before a push (pre-push, 2026-11-16)
Symptom: CI's Linux legs went red on lints in code only Linux compiles: `zombie_processes` in paddock-env `tests/it/text_busy.rs` and an unused import in dyno-agent `tests/bench_units.rs` (fix/shared-crate-flakes-8174, 2026-11-16). Cause: the Mac Clippy and the xwin Clippy do not compile `cfg(target_os = "linux")` code, and the pre-push Linux row was opt-in (`--linux`) and for `crates/` only. Fix: the row runs by default for every touched Cargo workspace (not only those a text scan finds Linux-only code in: the scan missed `cfg(unix)`, `not(macos)`, target dependencies and build scripts), as one container run on the native architecture with a kept target folder per workspace and CI image version; the Tauri app crates are left out, found from `cargo metadata` and named in the row text, so a pass never claims them; no container runtime or a cold volume is "not measured", exit 6, with `./dev check --warm-linux` as the next step. Tests: the row is chosen for apps and crates and names what it leaves out, the real script runs against a stub cargo (per-workspace folder, exclusion, a failing workspace does not hide the next), a planted Linux-only lint was red in the real container. **Class:** a platform-only code path that no local check compiles (earlier instances not re-verified here: Windows-only assumptions in tests, ADR 0021).

## Design note: a read-only container plus build scripts that write into the source tree (2026-11-16)
Two instances in one day. (1) The pre-push Linux Clippy of `apps/dyno` failed in the Tauri app crate: `paddock-build`'s build script writes the sidecar placeholders into `apps/dyno/app/src-tauri/binaries/` and tauri-build writes `gen/`, and `tools/linux-container.sh` mounts the repository read-only (ER-14), so `dyno-app` fails with "Read-only file system" before Clippy runs. (2) Another agent hit the same in the Linux container for `debrief-app` and the other app crates and had to use `--workspace --exclude <app>-app`. Options: **(A) a writable overlay** of the repository in the container (copy-on-write or a tmpfs over `src-tauri/binaries` and `gen`), so the app crates are linted too; **(B) declared container-lintable crates**: the row leaves out the packages whose build dependencies include `tauri-build` or `paddock-build` (read on the host through the repository's cargo-metadata accessor, not a path match), names them in the row text and in the NOT_RUN table, and CI lints them. **Chosen: B.** An overlay needs the helper to mount more than the repository read-only (a writable layer per app, a copy of the placeholders, and a check that nothing else leaks into the host tree), which is more than one crate per app is worth, and CI's Linux legs already lint those crates; B costs one metadata read and a row suffix. Revisit A when more than the app crates need to write into the tree, or when a Linux-only lint is found in an app crate in CI that this gate left out (then lint `src-tauri` through an overlay, not a bypass). Mechanism: `linux_inputs::tauri_crates`, `gates::linux_gate`; tests `the_tauri_crates_are_the_packages_with_a_writing_build_dependency` and `a_passing_linux_clippy_names_the_crates_it_never_linted`.

## A test blamed for what an earlier test left behind: a shared leak detector (tests, 2026-11-16)

The apps' and ui's Vitest suites run with `isolate: false` (one page per worker), so state one test leaves reaches a later
test and the wrong test fails: debrief.yml run 8170 (a chunk loaded after teardown), a leaked mock-jobs singleton (its running
job held the import slot of the next spec), nodes left in the body, a fake timer that hung unrelated specs (ER-29) and a
module-level record a hook kept waiting on. Each was fixed where it showed, with a one-off hook in one app's `test-setup.ts`.
**Fix (user decision, 2026-11-16):** one shared detector, `installLeakDetector` in `@decryp7/paddock-ui/testing`
(`ui/projects/paddock-ui/testing/src/leaks.ts`). It records at the end of every test, after Angular's clean-up, what the test
left (live timers and intervals, fake timers, body nodes, registered singletons, tracked in-flight work) and names the leaking
test: a warning in `report` mode, a failure of that test in `fail` mode. Its hook never throws inside the `afterEach` chain (a
throw there skips Angular's clean-up and the next TestBed test fails too: the kit's own lesson). **Rule:** anything
module-level that a hook reads is drained per test (a reader takes what is recorded and clears it), so a stuck item fails only
its own test; a new page-wide singleton is registered with `registerSingleton`; a new gate starts report-only (ER-41).
**Roll-out:** report-only first; the first full runs found timers started by stores that outlive their injector (Toast, Spotter's
dashboard and editor, Pitcrew's and Pitwall's service store, Pitwall's settings and presets), a wait in Pitcrew's
`TauriBackend.init` and its mock, and the Debrief and Dyno findings listed in `docs/benchmarks.md`; the stores now clear their
timers with `DestroyRef`, each with a test that was red first. The ui, Pitcrew, Pitwall and Spotter switched to `fail` with zero
findings; Dyno and then Debrief joined the failing suites after its `MockBackend.dispose()` and two store fixes. **Class: a test fails for what an earlier test left in
a shared page (earlier: runs 7257/7274/7276 fake-timer spy, run 8170 teardown chunk, the mock-jobs singleton, the leftover DOM).**

## Debrief: the live tail "holds the rows while not following" read 120052 for 120028 in about one full run in three (tests, 2026-11-16)

`events-grid.spec.ts:913` failed once in four full runs of the leak-detector branch (120052 against 120028). Not fixed here; a fix item (ER-28). Causes to rule out, with the observation that would rule each out: (1) the detector (it wraps `setTimeout` and `setInterval` and clears timers only in fail mode; Debrief is report-only, so it changes nothing: rule out by running the spec 50 times with the detector off); (2) a live-tail push landing between the two reads (the 24-row difference equals one batch: check the tail's batch size and whether the test advances the mock clock); (3) a leaked mock-clock timer from an earlier test firing into this one (`core/mock/mock-clock.ts:30,32,34` are in the report list: the findings of the preceding tests in that worker show it); (4) a render frame (`requestAnimationFrame`) landing between reads. Measure all four in the failing run before changing anything. Class: **a test that reads a moving value without holding the source still** (earlier: the axe theme read, Dyno's row-menu redraw).

Positive controls (2026-11-17, ER-33)
**What happened.** Three checks passed without ever being able to fail. A Debrief dock spec's "nothing moves after planned"
check never ran (24 of 24 runs green, the signal it waited for was never logged). The shared `expectIdle` helper passed before
the stream had started. Conformance C137 passed on the text of an unrelated ci.yml job.
**Rule:** every new check, gate, test assertion over a signal and shared wait helper lands with a positive control: a test that
removes or hides what it watches (the signal, element, attribute or job step) and shows it goes red. A review rejects one
that has none (rule 71 merged into ER-33).
**Class: a check that cannot see the thing it guards (earlier: the Dyno 1.1.0 menu-bar probe and tray count, ER-33).**

## Goldens captured on one machine failed on another; they are now checked when they are made (2026-11-15, train 14 WP-2; C131 added 2026-11-16)
On WP-2 alone six goldens captured on the Mac failed elsewhere: macOS-only git config keys, the CI image's `/sccache` mount, RAM
from `sysctl` against a file, the host OS paired with its pin, root against a normal uid, and a guardian golden with the Mac's uid
501 baked in that would have turned the Linux leg red. Each was found by CI or by a later review, after the golden had been
committed. Now the check runs when the golden is made: conformance C131 (report-only, ER-41) reads the golden folders for uid and
gid numbers, `/Users`, `/home` and `C:\Users` paths and temp paths; a rewrite with `PADDOCK_UPDATE_GOLDEN` prints the command that
runs the test on Linux as root in the CI image; and `./dev golden confirm <test filter>` runs both and passes only if both pass.
The first version also read the user, host, RAM and CPU count of the machine that ran it; review found that made its result differ
per machine (a waiver for the account name was stale on CI, a RAM figure in KiB collided with other numbers, a non-ASCII host name
panicked the Rust host), so it reads fixed patterns only. The first run on the tree found no real value, only invented users. The same review found the JS and Rust hosts of C131 could differ on text shape; the shared parity corpus (conformance/test/parity-corpus, run over every dual-host check through `tests/it/parity_corpus.rs`) is the mechanism that keeps them equal, and it found three more divergences (C119, C121, C124: a BOM, `\r` in a waiver, prepended lines). A lint that asks for the `u` flag was tried and dropped: `u` alone does not make the hosts agree (`\w`, `\b`, `\d`, `\s`, `trim()` and U+FEFF, `\r` and line handling differ even with it), so its fix text misled; ESLint's `require-unicode-regexp` is the standard rule if conformance ever gets ESLint.
**Rule:** a golden holds nothing of the machine that captured it; normalise the value in the test's `norm()`, and confirm a rewritten
golden on Linux as root before committing it; a check's result never depends on the machine that runs it. **Class: a golden captured
on one machine fails on another (second and later sightings: the six WP-2 cases above; ER-21).**

## Pitcrew's stop took a pid with an optional start time and signalled the bare pid (2026-11-16)
`Request::StopProcess` carried `started_at: Option<Timestamp>`; `check_stop_target` compared start times only when both sides had one, so
`None` (or a process whose start time sysinfo gave as 0) passed for any pid, and `OsTerminator` then signalled the pid with `nix::kill` /
`taskkill /PID`, outside `paddock_proc`, after the tree read of an earlier moment (rule 53, ADR 0033; two `expect(clippy::disallowed_methods)`
marked it as WP-6 work). Failing first: `a_stop_without_a_verified_start_time_is_refused` and `a_stop_request_without_a_start_time_does_not_decode`.
The fix: the start time is required on the wire (protocol 5, named JSON fields, no layout change beyond the field), a member without one is
refused (`Refusal::NoStartTime`) in the engine and the UI, the service reads `ProcessId::of(pid)` when it lists a process and holds that exact
start (the list shows its whole milliseconds, a stop must match within 1 ms: the whole-second slack and its three copies are gone), and
`Terminator::ask/end` take the `ProcessId` and go through `paddock_proc::ask_outcome/end`, which compare the holder immediately before the
signal (`a_reused_process_id_is_neither_asked_nor_ended`). A review found the same gap one level down (a console process on Windows refused the
polite ask and the forced end never ran, and `end`'s outcome was dropped): a signal's answer is recorded, and a refused polite ask is not the
end of a stop. **Rule:** a kill action names a
`ProcessId`, never a pid plus an optional time. **Class: an OS name acted on without identity (earlier: ADR 0033 WP-6 sites; the
family `ProcessRef` follows).**

## The midnight/noon sweep: six of seven e2e failures were not about the time (2026-11-17)
The pinned-clock sweep (`PADDOCK_E2E_NOW` at 2026-11-15T23:59Z, 2026-11-16T00:19Z and 12:00Z, every app, both engines, once each) failed
seven specs: Debrief `ux-welcome` (before and after), `ai-chat` (before), `dock-default-split` (noon); ui `gallery`
(after), `material` twice and `workbench-demo` (noon). Each was run 40 to 240 times at its failing instant and at the others. **None depends on
the instant** (rates within noise: 8, 8 and 9 of 100; 7, 3 and 0 of 120; 3, 0 and 1 of 40); the instant only decided which run of a
roughly 5 to 10 % failure was the one the sweep saw (the two WebKit-only ones passed 120 of 120 in Chromium). Hypotheses for "time-dependent" (a date or hour in the code under test, a day boundary
in a format, a timezone) were refuted by those equal rates and by failures that show no date. The causes, each confirmed by a run without it:
- **`ux-welcome` palette order (WebKit, 3 of 40 at before, 1 of 40 noon, 0 of 40 after).** The spec read the options as soon as "an option is
  visible"; the palette still showed the unfiltered list (recent and common commands, where "Connect to Pitwall" is the 9th), so the index
  was 8 on every failure, and the final DOM of every failing run was correct. Fix: read after the first option has a `mark` (the filter's highlight, which the
  unfiltered list has none of). After: 0 of 100 failed (100 runs, 4 workers, WebKit).
- **`ai-chat` pipeline proposal (WebKit, 8, 9 and 8 of 100).** The spec pressed "Add to profile" while the answer's text still streamed in above
  the card; the card moved between press and release, so no click arrived (the probe saw the same element, no running turn, no Undo
  entry: `apply` never ran). With the answer done first: 0 of 100 (100 runs, 5 workers, WebKit); with the fix in the spec: 0 of 100 (same). A press on a moving target is not a click.
- **`dock-default-split` (both engines, 7 and 3 of 120 at before and noon, 0 at after).** A product fault the spec found: the dock sat 36 px from the top
  until the lazy filter bar arrived, its placeholder was 30 px where the bar is 51 px, and the bar was 59 px for a frame (its status line host
  had no `empty` class before the first update, so its 8 px gap counted). The dock top moved 66, 87, 95, 87 px after "ready" and the groups re-scaled
  (279/502/781 then 282/507/789). The review of the first fix found that a fixed placeholder is right in wide windows only (the bar wraps to 87 px
  at 800 to 1100 px and WebKit showed 123 then 87 at 900 px), and that `settledLayout` is a proxy for the dock's own re-plan. Fixed in the product:
  the filter bar is no longer deferred (it is above the fold and its height depends on the window width; no placeholder), `debrief-query-status` is
  hidden until it has something to show, the bar measures its width before its first render, and the dock has a real ready signal,
  `data-dock-planned` on `debrief-dock-host` (`DockService.planned`: the first fit has run, no resize waits for its debounced re-plan, and two
  frames have passed since the last plan started, because what a plan's listeners do resizes the dock once more). The spec records the dock's top and
  the groups' heights from the first moment with resize observers, waits on the signal, and asserts that nothing changes after it, at 1440 and
  900 px: red in 20 of 20 runs on the old code (10 runs per engine, 5 workers), 80 of 80 green after (20 runs, 4 workers, both engines, both widths).
  **Narrow windows:** below 1000 px the Explorer was open for the first frames and `NarrowWindow` closed it by the dock's first plan, so the bar widened
  (123 to 87 px) and the dock top moved; a first fix started windows under 920 px closed by a constant (`EXPLORER_MIN_WINDOW_PX`), which the review found
  wrong at 920 to 934 px (the plan closes the Explorer after the first draw, the dock's left edge 320 to 48). Three attempts to ask the plan before
  the dock exists failed, and the cause turned out to be two: (1) the plan's input includes the dock's height, which the filter bar of the first data takes
  part of, so only a decision after the render that draws the bar is right; (2) after any change of the container dockview's own inline size followed three
  frames later in drawn frames (`requestAnimationFrame` samples: stage 48+880 with the dock 48+580, 49 ms later 48+852). **Fix (fix/debrief-dock-same-frame):**
  `DockLayout.followContainer` lays dockview out at its container's size from our own resize observer, in the same frame; `NarrowWindow` decides the
  Explorer once, from the plan's own `explorerVerdict` (the same call as every re-plan, so no constant), after the render that first draws the Explorer with
  the filter bar and before the browser draws, and the render that removes it needs no dock frame at the old size. The spec asserts a constant dock top and left
  edge at 1440, 1100, 1000, 936, 928, 920, 900 and 700 px (red on 7 of 20 runs at 920 and 928 px before; 320 of 320 green after: 20 runs, 4 workers, 2 engines,
  8 sizes). Cost: docs/performance.md (+2 ms script time per resize, within the noise; lagged frames per resize about halved). The Explorer's open flag was never
  saved, so there is no saved choice to honour.
- **A reader that compares a read with the one two polls before (found in review).** `settledLayout` kept `previous` and `last` and compared the new
  read with `previous`, so A, B, A passed as "two reads in a row". It now compares with the read just before; the test is red on the old code (settled on
  the third read) and green after (the fourth).
- **`material` select and Tab walk (Chromium, 4 and 6 of 40 per theme at after; the sweep's noon pair).** The spec drove the section as soon as its first
  button was in the DOM. That is the creation pass; the update pass (the select's `aria-expanded`, the form field's control, the picker
  inputs' bindings) comes later, up to 60 ms in the probe. Meanwhile a click hit "onContainerClick of undefined", a keydown hit NG0950
  "timepicker is required", and the Tab walk left the section before the select. Fix: wait for the Level select's `aria-expanded`. After: 0 of 160 (160 runs, 1 worker, Chromium; 10 of 160 before).
- **`gallery` open select list (Chromium, 5 of 40, 12 of 60, always 536.09 px off).** `toBeVisible` of the list is true a frame or two before the
  overlay positions it (at x 0, 131 px wide; 40 ms later at the field's x and 212 px). Fix: wait for the alignment (`expect.poll`, a failing
  list stays red with the last offset). After: 0 of 100 (100 runs, 1 worker, Chromium).
- **`workbench-demo` Jobs view: not reproduced.** 0 of 100 at noon (40 idle, 60 with 8 CPU burners), once in the sweep; the failure page shows the palette open
  with "View: Show jobs" selected after the second `run`. Cause not found, so no fix (ER-33: not measured); the palette helper's failure message
  now names the label it was running. Next step: run the spec under the full suite's load with `--trace on` until it fails once.
**Why the logs did not show it:** the specs read the page once and compared; none printed what the page showed at that moment. The material
errors were in the dev server's console only.
A second find of the review: the dock spec imported the kit by package name, which Debrief's e2e jobs never build, so every shard ran no test; C137 (report-only) flags it.
**Class: a spec that acts or reads on "it is in the DOM" while the page is not ready or still moving** (earlier: the toast whose Show missed
its click, the Dyno Firewall layout read before it settled (`settledLayout`), the Debrief menu that ran its neighbour's command; ER-21 candidate:
a shared "ready" per kind of check, with a lint over `waitForTimeout` and first-visible reads).

## A broad `pkill -f` by an agent, with the kill guard not wired (2026-11-17)
While cleaning up its own e2e servers an implementer ran `pkill -f "ng serve"` (and later `pkill -f "ng serve gallery --port 4701"`, own ports). The first
pattern matches every agent's dev server on the machine, so it may have ended other agents' runs; nobody reported one, and none can be ruled out.
Hypotheses: the guard did not fire because it only covers `kill PID` (refuted: `.claude/hooks/kill-guard.py` refuses `pkill`, `killall` and `killall5`
for agents, with a test) or because the pattern was inside a `&&` chain (refuted: lib/shellscan.py scans every command of a chain). **Cause:** the
main checkout's local `.claude/settings.json` wires six hooks (docker-guard, no-force-push, no-pgrep-wait, recurring-guard, scratch-root-guard,
worktree-guard) and not `kill-guard.sh`, whose header says the coordinator wires it after the train lands. **Rule for agents:** end only what you
started, through the tool that started it (`run_in_background` with TaskStop, or `kill %job`), or by the port you picked after `lsof` shows your own
process in your own folder; never by a pattern on a program name. **Class: a guard that exists, is tested and is not wired** (earlier: the coordinator-hook
rollout of 2026-11-12).

## Shared "ready" waits and a lint for reads after a bare visibility wait (2026-11-17)
The class of the sweep entry (a spec reads or presses on "it is in the DOM") now has one wait per kind in `ui/projects/paddock-e2e/src/ready.ts`
(`expectListFiltered`, `expectOverlayAlignedWith`, `expectWired`, `expectStreamDone` (the chat log is `aria-busy` while a turn runs; an `expectIdle` over "no Stop button" passed before the stream started and was dropped), `expectDockPlanned`, `expectNotMoving`), and an ESLint rule,
`paddock/e2e-ready-reads` (in the `e2ePinnedClock` block, AST, not a regex check; first off by default, since 2026-11-18 an error: see the entry below), that reports a read (`boundingBox`, `allInnerTexts`,
`allTextContents`, `innerText`, `textContent`, `evaluate`, `evaluateAll`) in the statement right after a bare `toBeVisible()` or `waitFor()`. The
sweep's five specs moved onto the helpers. First count (`node scripts/e2e-ready-report.mjs`, after the evaluate rule): 74 sites in the other specs (Debrief 26, Dyno 17, ui 23, Pitwall 5, Spotter 3, Pitcrew 0; an earlier version of the report script used a `--rule` override, which fails with "could not find plugin paddock" where the plugin is registered only inside the e2e block, and measured 2 of 6 projects; it now sets `PADDOCK_E2E_READY_LINT=warn`, and a fixture project of that shape is in its test);
no waiver was added (ER-46), so the count only goes down as specs move or the rule flips to error after its first green run on the real tree. Many of
the 74 are reads of a static element; a waiver `// conformance: e2e-ready — <why>` was meant for those (replaced by suppressions, see the entry below).
Debrief's `openApp` and `openAppWith` now wait on the dock signal (`expectDockPlanned`). Suite time on 9 related files (68 tests, Chromium, 4 workers, one run each, machine idle): 1.1 min with the wait, 1.0 min without; the difference is inside the run-to-run noise of one run each.

## The ready-reads sites are gone and the rule is an ordinary lint error (2026-11-18)
All 74 sites were moved onto the helpers (`expectNotMoving` for a box, a retrying assertion or `expect.poll` for text, colours and attributes) or are one of six reads that are immediate by design (the page's own ready signal, a faked-clock read, a focus loop, a "nothing happened" check, the CSP violation list, a "did not move" check). `data.spec.ts` "loading and empty" (FLAKY in run 8215) took the settled position of one box and then polled the other three for it; a box that agrees with itself while the others still settle gave a wrong `loaded`. It now reads all four boxes in one reading and settles when two readings agree and all four are at the same position. A `for` loop after a bare wait counts as the next statement: put the helper before the loop.
**The first gate was the wrong shape, and so was the second.** A JavaScript script with baseline, waiver and switch files (d5ad6babd to 1e7c273a5) never ran on app-only changes, passed silently in a shallow clone and could be turned off by deleting its switch. The redo (6fa33ef59 to 15c1702bf) made the rule an error and the six exceptions `eslint-disable-next-line` directives counted by a C139 that parsed ESLint's comment grammar with its own regex. Review measured that rule switch off, unseen by both ESLint and C139, three ways with the real configs: an inline `/* eslint paddock/e2e-ready-reads: "off" */` (or `: 0`), a block comment with the directive on its second line, and the description separators ESLint accepts (`/\s-{2,}\s/`: a tab before `--`, `---`, `--\t`); the string literal that held the directive text was a false positive. **The cause is parsing a grammar that the tool already owns.** The fix leaves the grammar to ESLint: the e2e lint block sets `linterOptions.noInlineConfig`, so every inline config comment is ignored (and reported), and an exception is an entry of the project's `eslint-suppressions.json` (ESLint bulk suppressions, applied by the Angular lint target through `applySuppressions` and by the ui's CLI by itself). The CLI fails on an unused entry (the API the Angular builder uses does not, so `eslint/test/wiring.test.cjs` runs the CLI over each project with a suppressions file and forbids the flag that ignores unused ones). C139 shrank to the part ESLint cannot do: each suppressions file against the merge base through the shared baseline reader (not measured is an error in CI), and a run that reads no e2e spec is not measured. The six entries (Debrief 3, Dyno 3) were approved by the user on 2026-11-18. A run of the checks on a change that only touched an app's `eslint.config.js` marked no eslint project, so nothing would have caught `e2ePinnedClock({ ready: 'off' })`: `scripts/affected.mjs` and `./dev plan` now mark the eslint project for the lint wiring files (an app's or the ui's `eslint.config.*`, `eslint-suppressions.json`, `angular.json`, and the ui's `package.json`), and the ui's lint script has `--max-warnings 0`. The wiring test resolves the rule's severity (and survives an inline `off`) in the six real configs (ER-33). **Class: a new gate built as a separate script, or as our own parser of another tool's grammar, instead of on the tool's own mechanism and the existing baseline reader** (earlier: the hand-written baselines that C53, C115 and the allow lists replaced).
**Second review of the redesign (3dd841ed8): a new exception list was silent.** A new `apps/pitwall/app/eslint-suppressions.json` with count 5 gave C139 no finding, because the shared baseline reader treated a list absent at the merge base as "the first introduction" and returned nothing added. This is the second time the class appeared (the first: the six entries of `c139-allow.json` in round 1), so the fix is in the shared reader, on both hosts (`baselineGrowth` in `conformance/src/util/baseline.mjs`, `baseline::growth` in paddock-dev): **a list absent at the merge base is an empty base, so every entry is growth (ER-46).** It reaches every user (C121, C124, C125, C130, C53, C71, C104, C115, C129, C139); their tests that encoded "introduction adds nothing" now say the opposite (C71, C104, the reader's, the allow list's), and a reader test on each host pins it (red against the old reader: a Rust test failed, the JS reader tests failed). A branch that introduces a list now gets one unwaivable growth finding until the list is on the target branch, which is the intended meaning of "needs the user's approval". Second, a non-default file was invisible: `suppressionsLocation: "lint-exceptions.json"` in an angular.json, or `--suppressions-location` in a lint script. The wiring test now reads every lint command through the accessors of the files that declare them (package.json scripts, the lint target of angular.json, the contract paddock.ci.json, the workflows through a YAML parser) and fails on a non-default location or the flag that ignores unused suppressions, replacing the text grep; `apps/*/app/package.json` and the contracts mark the eslint project in `scripts/affected.mjs` and `./dev plan`. Third, `ng lint` (the ESLint API) is silent about an unused suppression, so each app's lint script now ends in the ESLint CLI over e2e (`ng lint && eslint e2e --max-warnings 0`), CI runs `npm run lint` (Dyno's contract called `npx ng lint`), and the wiring test requires both. Red then green: wiring 1 fail then 23 pass, affected 3 fail then 130 pass, the Rust reader test failed against the old reader.
**Third review (ccc9ad06f): a different ESLint config.** In Pitwall an `eslint.lax.cjs` with the rule off, `"eslintConfig": "eslint.lax.cjs"` in the lint target and `-c eslint.lax.cjs` in the script left the wiring test green (23 of 23) and `affected` silent. This is the third instance of one class (round 1: an app's own config override; round 2: a non-default suppressions file; round 3: another config), so the fix is not a fourth forbidden spelling. **The control now runs the real command:** for each of the six projects, `eslint/test/wiring.test.cjs` takes the lint command CI runs (the contract's `npm run lint`, which is the package.json script) and runs exactly that in the project folder, once against a planted spec with an unready read (it must fail with the rule's message) and once against a planted suppression that no longer occurs (it must fail with ESLint's unused-suppression error); the planted files are removed or restored in `finally`. Whatever config, flag, builder option, `configurations` entry or composite action the command picks up is covered, because the command itself is shown to go red. The reviewer's three Pitwall edits turn it red (1 failure, 28 others still green, then 29 green after the revert). Any `eslint*.js|cjs|mjs` file in an app or the ui folder now marks the eslint project in `scripts/affected.mjs` and `./dev plan` (red then green: 3 failures, then 133 pass), and the wiring test refuses `configurations` and `eslintConfig` in a lint target. Time added (this Mac, 6 projects concurrently, load about 10): 35 s for the whole wiring test file; per project unready read / unused suppression: Spotter 3.8 / 3.8 s, Pitcrew 4.2 / 4.3, Pitwall 5.3 / 4.8, Dyno 7.3 / 5.4, ui 10.2 / 6.5, Debrief 16.8 / 12.6; run one after another that is about 85 s, under the 2 minute line. **A check's list has to land on main while the check is still report-only** (a list created on a branch is all growth): written in the baseline reader's header on both hosts and in the gate register's header.
**Round-4 review: the control wrote into the real worktree (ER-45).** The real-command control planted `e2e/zz-planted-*.spec.ts` and edited `eslint-suppressions.json` in place with cleanup only in `finally`; a SIGINT mid-run left 9 stray files in `git status` (measured: 4 `zz-planted` specs and 3 new `eslint-suppressions.json` among them), and two concurrent runs (pre-push plus an agent's `npm test`) would corrupt each other's "original". The class (a check that writes into the real repository) now has one mechanism: `scripts/test-helpers/scratch-project.cjs` (`scratchProject(repo, 'apps/dyno/app')`), importable from the eslint and conformance tests. It copies the project outside the repository (without node_modules, dist, .angular, test-results, target, snapshots), symlinks node_modules, eslint/, tsconfig/ and the root package.json (an app also gets `ui`; the ui copy links `projects`; `src-tauri` is linked), runs the command in the copy, and removes it in `finally`, by a process exit, SIGINT, SIGTERM and SIGHUP handler that also kills what it started, and, for the one signal that cannot be handled (`node --test` ends an interrupted file that way), by a sweep of `paddock-scratch-<pid>-*` folders whose process is gone at the next call. The wiring file also asserts `git status --porcelain` is the same before and after. Red then green: the old control left 9 files after SIGINT; the new one, interrupted mid-run, leaves `git status` unchanged (the 4 copies it could not remove stay in the OS temp folder and the next run removed them: 5 before, 0 after). The control still passes 6 of 6; wall time for the wiring file is 34 s idle and 58 s under load (Debrief 25 to 48 s of it), the copy itself adds about 1 s per project. No other test in eslint/ or conformance/ writes into the real repository (every write goes to a mkdtemp folder or a copy of one).
**Open fix item: two Debrief WebKit timeouts.** In the 20-repeat run of the 25 changed Debrief specs on WebKit (20 repeats x 4 workers x WebKit, 3540 tests, load about 30), two tests failed in the same repeat, `view-areas.e2e.ts:76` ("a view open behind another shows a dot on its activity button") with "Test timeout of 30000ms exceeded", and `view-controls-stay.e2e.ts:10` with "Test timeout of 30000ms exceeded while setting up \"page\"". Logs: `test-results/view-areas.e2e.ts-view-hom-013cc--bar-is-for-a-view-in-front-webkit-repeat8/` and `test-results/view-controls-stay.e2e.ts--76b22-s-the-load-opens-the-dialog-webkit-repeat8/` (error-context.md and trace.zip, in the worktree that ran it; not kept). The cause is not confirmed (ER-28): the two specs alone passed 120 of 120 (6 tests x 20 repeats, 4 workers, WebKit) and a full rerun passed 3360 of 3360 with 180 skipped. Chromium: 3420 passed, 160 skipped, 0 failed. Next step: if it shows on a real runner, reproduce with a stress of those two specs under load and read the trace for the step that hangs.

**Open fix item: three live-tree comparison tests hang under load.** In `cargo test --test it check_` in tools/paddock-dev (2 build jobs, 2026-11-18, machine load about 30 and above from other agents and a train's CI), `check_c125::live_c125_tree_findings_equal_the_js_host`, `check_c129::live_c129_tree_findings_equal_the_js_host` and `check_c131::live_c131_tree_findings_equal_the_js_host` failed with "the Rust C125 over the live tree is hung: no progress for 120s after 11471 units" (C131: after 312 units); the whole filter took 1239 s and 1187 s in two runs. Alone at load 3 to 5 the same three passed together with the other two live-tree tests in 16.45 s. No limit was changed (ER-12). Probable cause, not confirmed (ER-28): the same quadratic walk over a large text that `fix/paddock-dev-load-flakes` fixed for C122 and C53 with `jsre::Positions`; the first measurement to take is the time per unit of each check over this tree at load 3 to 5, to see which grows with the file size.

## The start-up benchmark was red for starts a busy host had not slowed (Debrief, 2026-11-16, CI run 8175)

Run 8175 said "NOT MEASURED: 1 of 5 starts ran with host load <= 12 (14 discarded in 15 attempts, peak 1-minute load 49.4)". The
load average on the shared Mac counts OrbStack and build threads, and the gate threw away starts that were fine: under 14, 28
and 84 CPU-bound `yes` loops (with `dd`) Debrief's start moved by under 5% and its tree CPU time not at all. **Two plans did not
hold.** (1) Scaling wall time by a fixed Node reference workload: the reference ran 2.2 to 5.2 times slower under the same loops, so a
regression of two times would have passed. (2) "CPU time barely moves with load", from those `yes` loops: under real build load (load
97 to 177, 0% idle) the CPU time rose by up to 40% (530 to 720 ms against 515) and the wall time of a single start by up to 2.5 times,
so a tight CPU threshold (561 ms) failed a busy host. The coarse limit that a busy host needs (875 ms) would let a 70% regression through on a quiet host, so the user chose "tighter when quiet": a 600 ms limit when the median host CPU idle is 30% or more. The first proposed cutoff, 50%, would never have applied (no invocation of 38 reached a median idle of 50%: other agents are always working on the host), so a cutoff is chosen from the readings the benchmark really sees. **Rule:** measure a gate in the condition it must survive, with the real
neighbours (other agents' builds), not only a synthetic stress; thresholds come from that, and a quantity that is noisy under load is
a note, with a ceiling only a broken run reaches. The user's rule (2026-11-17): always measure, record host load and CPU idle, never
refuse for a busy host, fail only on a real regression. Results: apps/debrief/docs/performance.md, "Start-up benchmark on a busy
host". **Class: a proxy gate (load average) stood in for a measured effect (second instance: ER-12's raised timeouts without a
measured cause), and a synthetic stress stood in for the real neighbour (instance: this lesson's second plan).**
`events-grid.spec.ts` "holds the rows while not following, counts the new events, and reveals the newest on request" failed
twice in about six full runs (120052 against 120028), never in eight runs of the test alone. **Causes listed (ER-28), with what
each measurement showed:** (1) the leak detector: refuted, Debrief was report-only then and the failing run had no finding for it, and the
failure is the same with the detector's timer wrapper out of the picture (the 1 ms runs below); (2) a push moving the end between the
reveal and the read: partly confirmed. At `liveMs: 1` the test failed 3 of 3 (126124/126148, 124468/124492, 123508/123532: one row of 24 px
short, the same shape), at 25 ms 0 of 8 alone, so the rate follows the push rate; (3) a leaked mock-clock timer from an earlier test:
refuted for the 1 ms runs (a single test); not excluded for the full-run rate; (4) a render frame between reads: refuted, the
numbers stay put after another `flush` and `settle` (`top=126148 max=126172` at the assertion, after the flush and after the settle).
**Sequence measured (temporary logging, `liveMs: 1`):** one `liveRefresh('end')` for the reveal, `applyLive mode=end newTotal=5270`,
then the assertion reads `top` = the end of 5269 rows while `total` is 5270; events pushed after the reveal are held (`newEvents`
counts them), so nothing moves the top afterwards. **Fix (spec):** the test's backend runs its pushes on a `ManualClock` (`clock.advance`
makes the ticks), so no push can land in the reveal; 13 runs alone and 4 full Debrief runs were green. **Product cause, found and fixed in the same round (ER-44):** the reveal's refresh asks for several pages in one round (page 0, the
pages in view, the page of the old last row). The first answer triggers `applyLive` and the scroll to the end of its total; an event pushed
between two answers made a later page report a larger total, which `PagedRows` stored without scrolling, so the view stayed one row
(24 px) short of the newest event (a user-visible bug: "reveal newest" stopping one row above it). The manual-clock pumping did not
reproduce it because it never interleaved a push between two page answers; the deterministic test does that on purpose (the second page
read of the reveal is answered after a push) and failed with 119836 against 119860 before the fix. **Fix:** `PagedRows.arriveLive`
remembers the round and total of an arrival that scrolled to the end and, for a later answer of the same round with a larger total, calls
`onLive` again in end mode (bounded by the pages of the round, no loop). Class: **a test reads a moving value without holding the source still** (earlier: the axe theme read, Dyno's row-menu redraw).

**Rule (review of the first fix, same day):** a follow-up end arrival belongs to the reveal's own pages, not to the round. The first
version of the fix kept `endArrival` for the whole round; after a reveal with following off the grid holds new events, so no new round
starts, and pages fetched because the user scrolled later answered in the reveal's round: each larger total jumped the view back to the end
(scroll to 60000 ended at top=120028). `PagedRows` now records the pages `liveRefresh` asked for, follows only answers to those, clears the
follow-up when the last of them has arrived, and `endReveal()` (called by the grid for a scroll it did not write itself) ends it at the
user's first scroll. Specs: the forced-order reveal and "does not pull the view back to the end after the user scrolls away" (red on the
first fix at 120028 against under 70000). `flush` in `events-grid.spec.ts` no longer sleeps a fixed `SCROLL_IDLE_MS + 30`: it waits on the
store's `scrollIdlePending`. C71 had caught that helper (its sites were in the baseline); the baseline was shrunk by the three sites.

**Real browser (e2e, same day):** jsdom hid a second cause. `applyLive` wrote `scrollTop` before the spacer had grown to the new total; Chromium and
WebKit clamp such a write, and the scroll event that follows reports the clamped position, which the grid took for the user's scroll
(the reveal ended 21 rows short in Chromium, with or without a push between the pages). Fix: a live arrival writes the scroll after the render
(`setTop(..., afterRender)`, `afterNextRender`), the position signal is still set at once. Playwright e2e `live tail: reveal newest ends on the
newest row when events are pushed between the reveal pages` (new mock switch `mockPageStaggerMs` answers pages at different times, pinned
clock moves them and the 50 ms ticks): red in both engines without the deferred write, green 3 of 3 in Chromium and in WebKit with it.

2026-11-17, Debrief `helpers.spec.ts` leak (fixed, ER-28). Debrief runs the leak detector in fail mode, and the timeout spec of `settleUntil`
failed 7 of 300 runs (one process, the spec repeated) with "left a live setTimeout(0) started at helpers.ts:191" (`settle()`). Causes listed
before measuring: (1) `vi.waitFor` rejects at its timeout while an attempt is inside `settle()` waiting on its `setTimeout(0)`; (2) the test
ends between the first `await` and the timer; (3) fake timers left by another spec of the file; (4) the detector counting the previous test's
timer. (3) and (4) are out: the file installs no fake timers and the leak named this test's own call. (1) and (2) are the same race. Confirmed:
a spec that stretches every zero-delay timer to 20 ms and times out at 5 ms fails every time without the fix (the timer is pending when the
wait rejects) and passes with it; the repeated run went from 7 of 300 red to 0 of 300. Fix: `settleUntil` awaits its last attempt before it
returns or throws, so no timer of its own outlives it. No sleep and no longer timeout (ER-12).

2026-11-17, unit tests of live mock feeds. Only the Debrief events-grid live tail (run 8191) was a unit test asserting a value while a
feed kept changing it; the Dyno row menu under live updates was an e2e case (the e2e kit has `pauseLive` and `withLivePaused`). Common
practice for a feed on timers is a clock the test moves (`ManualClock` for the Debrief mock backends, `clock` option) or Vitest's fake
timers, never the page's real timers; a hold on top of a manual clock added nothing, so no helper was built. C136 (report-only) flags a
unit spec block that starts a ticking `MockBackend` or `synthetic` feed without one.
**WebKit and the ADR (same day):** at `7fa15ba95` the WebKit e2e failed 3 of 7 runs. Logs added in `setTop` (scheduled, written, dropped), `onScroll` (read,
written, pending, origin) and `applyLive` (ER-27) showed the sequence in a failing run: two `end` arrivals reached the end (read = max), then the
total grew once more (spacer 24 px taller) with no arrival: a page fetched outside the round answered with a larger total, `PagedRows` stored it and
nothing scrolled. That is a third shape of the same class (a total that grows without the view following), not the deferred write failing. Fix through
ADR 0072 (`onGrow`, one pending-write slot, `classifyScroll`, user take-over from input events, stored `pinned`); the e2e looked green 12 of 12 per engine at 69e230d15, but a 20-repeat run of the whole matrix with 4 workers (460 runs per engine) failed 4 to 5 times in WebKit and 3 in Chromium; see the next entry for what that found and the final counts.

**Review of 69e230d15, and the final measurements (2026-11-17).** The reviewer's probes found three real faults, each fixed with a failing test first:
(1) the deferred re-pin after the next render fought a user whose wheel scroll was applied after the render: removed; a wheel or key takes the view over only
when it moves **away** from the newest end (xterm's rule), re-pinning comes only from a position event (a scroll that is not ours, a key that lands there, the
release of the scrollbar, never while it is held); (2) `arriveLive` consumed the first answer of any page, so an unowned fill page could start the round and a
take-over did not stop a round that had not answered: the arrival now starts from an owned page, a take-over downgrades a pending `end` to `keep`, a failed first
owned page ends the hold; (3) the follow and hold rules were not in the reducer alone (`setTop` re-derived `pinned`, the holding effect and `applyGrowth` decided
themselves): the reducer now has `positioned{atNewest, byUser}`, `grew`, `released` and `followOn`, `setTop` is a pure writer, `liveMode` is gone. Re-measuring
the matrix at 20 repeats with 4 workers found three more browser behaviours, each seen in the grid log first: Chromium and WebKit reset or shift `scrollTop` with
no input while the viewport or the spacer changes size (a scroll event the grid took for the user: `classifyScroll` now returns `spurious` when the layout changed
since its last write, or it wrote within 300 ms, and no input came since; the grid writes its own position back); a key move to the end written before the render caught up with
the total was clamped (such a write now waits for the render too); a headless-WebKit wheel gesture is sometimes dropped (the e2e sends it again while the view has not
moved; that is a test-infrastructure miss, not a product behaviour). **Final counts, quoted from the runs:** the whole matrix plus the reveal case, 23 tests
x 20 repeats = 460 runs per engine, `--workers 4`: Chromium 460 of 460, WebKit 460 of 460; before the last two fixes the same run failed 3 (Chromium) and 4 to 5 (WebKit) times.
A WebKit scrollbar press sometimes misses the thumb (the overlay scrollbar is drawn under the pointer: the test hovers first); it is a test precondition, not measured by the grid.

**Second review of the same feature (2026-11-17).** Two more faults, both from the `spurious` classification added to cope with the browsers' own scroll adjustments:
(1) `spurious` reverted a real scrollbar drag: the input flag was set by the press but cleared by every grid write, and a drag sends only pointer moves, so
with newest first, following and not pinned each 250 ms shift write ended the gesture and the next scroll of the drag was written back (the reviewer measured drags
ending anywhere between 1,739 and 18,436 px; the new e2e fails on that code in both engines: red 1 of 2, green 12 of 12 on the fix). The input flag now holds for the
whole gesture (a write while the pointer is held does not clear it, and the held pointer counts as input), and `spurious` is narrowed to the two shapes seen (a far position
reset to 0; a shift by the size of the layout change, `layoutDelta`): any other position with no input stays the user's (find in page and assistive technology, also when the
total changes without a write in oldest-first keep mode: table row and e2e). (2) The 300 ms `SETTLE_MS` window had no recorded measurement (ER-12): it is removed; the layout key, the
layout delta and the gesture-scoped input flag decide, and no time constant remains. The e2e's `expectLeftNewest` now resends a wheel only when the grid's log shows no take-over
since the last send (at most twice), and fails at once when the grid took over and the view is nevertheless back on the newest end; the resends are attached to the test.

**Final counts after the second review (2026-11-17).** Matrix of 26 tests (both sort orders, the reveal case, the no-input scroll) x 20 repeats = 520 runs per engine, `--workers 4`:
on the idle machine (>= 60% idle at the start) Chromium 520 of 520 and WebKit 517 of 520 (before the resend tweak; the three: two dropped wheel gestures, one key move that did not
reach the grid); the last full run was made while other agents loaded the machine (load average 170, runs 60% slower): Chromium 519 of 520 and WebKit 511 of 520. Every WebKit
failure in these runs is of one kind: an input (a key, a wheel, a scrollbar press) that the page did not turn into a scroll within the poll's 5 s of real time ("the view left the
newest end"), or a top-row read taken while pages were still loading, not a view pulled back after a delivered gesture; the grid log of those runs shows no take-over, or a take-over
with no scroll. The drag-across-rounds row fails on the code of 576b47507 in both engines (red 1 of 2) and is green 12 of 12 on this code. Input delivery in headless WebKit under load
is the open measurement: it is not a claim of "460 of 460". The 300 ms window and its constant are gone.

**Deterministic ownership rows (2026-11-17, third round).** A real wheel, key or scrollbar press in headless WebKit is sometimes dropped by the browser (measured: 517 of 520 on the
idle machine, the reviewer's 259 of 260; the grid log shows the wheel received and the take-over logged, then no scroll event and no grid write), so the ownership claims cannot be
proved by real input alone. The common practice (TanStack Virtual's and the CDK's tests) is an untrusted event dispatched on the element plus a scroll position set by the test; the
ownership rows now do that and are 640 of 640 in Chromium and WebKit, and a small real-input smoke set keeps the real pipeline covered (WebKit 639 of 640, the failure a dropped wheel).
Making the rows deterministic found two things the real input had hidden: (1) the reveal request is handled when change detection runs, which can be after the user's wheel; the
late request pinned the view again (G10, unit spec first; closed, with the same race for Follow on and Resume (found in review), by one rule: requests and take-overs carry a sequence stamp taken when the user acts, a required argument so no effect takes one, and the reducer drops a request stamped before the last take-over, ADR 0072 amendment); (2) a dispatched key does not scroll natively and the grid handles keys itself, so the test presses
enough keys to take the focus out of the window and moves nothing. The classifier's second `spurious` rule also required no `layoutChanged`: in oldest-first keep mode the delta
grows without any write, so a no-input scroll (find in page, assistive technology) within the band was written back; it now needs the layout to be changing (the total or the viewport
changed and the frame that renders it has not passed, or the element's height differs from the last write's), table row first.

## Debrief: "resize while not pinned keeps the top row" was two rows off in about 1 in 75 WebKit runs: the browser's scroll anchoring (2026-11-17)

**Symptom.** The e2e row (oldest first) of `live-tail-follow.e2e.ts` failed 4, 6 and 3 of 300 WebKit runs, 0 of 340 in Chromium. The grid log showed its
own write of the top row, then a scroll event with no input, 59 px away (in this round's repeats: -132 and +164 px), classified `user` and kept.
**Causes listed (ER-28), with what each measurement showed:** (1) WebKit clamping a write against a stale `scrollHeight`: refuted, the reads
were far from the maximum (118632 against a maximum of about 121900) and in both directions; (2) a second ResizeObserver pass: refuted, no
second `Resize re-applied` line before the move; (3) the spacer height changing between write and paint: refuted, the shift equalled the change
of the row layer's height (`.layer`, bound to `viewportH`), not of the spacer (a probe that logged each scroll event's layer and spacer heights:
shift 132 = layer 278 - 146, spacer change unrelated); (4) `setViewportSize` itself: refuted by (5); (5) CSS scroll anchoring: confirmed. The
spacer is the scroller's only in-flow element, below the sticky row layer; when the layer's height changes on a resize, the browser moves
`scrollTop` by that change to keep the spacer still. A probe of 20 resizes per run, view not pinned, was red 11 of 40 runs with
`overflow-anchor: auto` and 0 of 80 with `none` (computed style checked in each run), the same build. A WKWebView probe (macOS 27.2.1, the
runtime Tauri uses) moved a scroller 1000 -> 1200 when an element above the anchor grew 200 px, and not with `overflow-anchor: none`: users
were exposed, not only the tests. Safari Technology Preview 241 lists "Added stable support for CSS scroll anchoring" (310113@main).
**Fix.** `.viewport { overflow-anchor: none }`, as the Angular CDK virtual scroll viewport does (`.cdk-virtual-scrollable`): the grid places
its rows and writes `scrollTop` itself. ADR 0072 decision 2 is unchanged. Regression test first: "the browser does not move the view when the
row layer changes height" grows the layer by 100 px and reads `scrollTop`; red in both engines (100 px), green after.
**Why the logs showed it only partly.** The `Scroll origin` line showed a no-input move, not its size against the layer change; WebKit's
console preview cut the line after five fields. **Class:** a browser default that moves the scroll position (clamping, bug 3 of ADR 0072;
anchoring here).

## A test kit that read the live environment saw other tests' changes (tests, 2026-11-16, ADR 0100)
Symptom: under the plain harness `env::hermetic_points_the_user_folders_and_tmp_into_the_scratch` failed 5 of 40 runs, as
"scratch folder: No such file or directory" or `Leftovers([".tmp…/"])`, and a test program started directly linked rustup's
cargo proxy (40 of 40 runs of the cargo-home test). First hypothesis: a slow file system; refuted by the paths in the failures,
which were inside another test's scratch `tmp`. Root cause: the plain harness runs a binary's tests on threads of one process,
and the kit read `TMPDIR`, `PATH` and `CARGO` live, while another thread's test had changed them through `EnvLock` (`hermetic`
points `TMPDIR` into its scratch and removes it at the end; process tests set `PATH` to stand-in toolchains), and a program
started directly has no `CARGO` at all. Contributing: two tests read `HOME` before taking the lock. Why the logs did not show
it: nothing said which temp folder a kit folder was made in. Fix: the kit captures its machine values once, under the lock,
before any test changes them (`paddock_testkit::ambient`); tests that need other values pass an `Ambient` to
`TestCommand::on`; the cargo comes from `rustc --print sysroot` when `CARGO` is unset; the tests read the environment only under
the lock. Measured after: 0 of 100 plain-harness runs failed. **Class: a process-wide value read live in a multi-threaded test
harness (the same shape as ADR 0103's leaked fake timer: shared mutable state across tests of one process).**

## Debrief's Explorer width jumped 920 to 934 px: a constant copied a number another module decides (layout, 2026-11-17)

Symptom: the Explorer pane changed width by 14 px (920 to 934) when the layout settled. An earlier 51 px placeholder had the
same cause.
**Cause.** The Explorer's width constant re-typed the layout plan's rule, and the placeholder re-typed the filter bar's height.
When the owner changed, or two reads disagreed, the copy did not follow.
**Fix, one mechanism.** A value that another module decides is read from that module, by a call or an exported constant. Rule 42
says so, and the pre-review checklist and the independent-review checks ask "does any number re-type a decision another module
owns?" (ER-22).
**Class: a copied value that drifts from its owner (earlier: the filter bar height placeholder; mechanism copies of rule 55 are the same shape).**

## paddock-ipc: a call whose connection closed before its caller looked it up waited for its whole timeout (CI run 8214, 2026-11-17)

**Symptom.** In ci.yml run 8214 (train 13, the crates job on Linux) paddock-plugin-host's
`faults::a_crashing_plugin_is_restarted_after_the_back_off_and_then_quarantined` ran until nextest stopped it at 60 s, with no
output, and passed in 0.7 s on its retry. It was the only such failure in the 78 crates jobs of the last 135 ci.yml runs (1.3 %).
**First hypothesis and what refuted it.** The brief named ETXTBSY (the "plugin-host ETXTBSY" of run 8174). Run 8174's failure was
another test (`a_program_changed_after_install...`, an ETXTBSY panic in under a second); a busy program file fails a start within
the 1.3 s bound of `paddock_env::text_busy::retry`, so it cannot hang a test for 60 s. Also listed and refuted: the plugin stuck in
`exit` or its socket held open by another process (the fixture's `crash` is `process::exit`, the host spawns with close-on-exec and
this binary starts no guardian); a lost wake-up in `FakeClock::wait` (its generation check orders the token's waker against the wait);
the memory watcher or the tree kill blocking (they wait on the real clock or a reaped child).
**Root cause.** `Peer::wait_event` installs a fresh wake-up token in the call's `Pending` entry, then waits on the peer's clock.
Since 381ca0d70 (2026-11-10, "a close is reported only once it has ended the calls"; in no tag) `mark_closed` takes the pending calls
out of the map under the lock and posts their end later, after the close is reported. A caller that looked its call up in that
window found nothing, so its new token was never cancelled; the close cancelled the old one. The caller then waited for the call's
timeout on the clock: for ever on the test's `FakeClock` (the plugin host runs its peers on the app's clock), and for the whole call
timeout in a real app, so a crashed plugin was noticed only when its call timed out. The crash test hits it because the plugin exits
as soon as it reads the call: the close races the caller's step from sending to waiting.
**Confirmation (ER-28).** With the window widened (a 20 ms pause after the send and a 50 ms pause between the drain and the post,
env-gated, not committed) the e2e test hung 6 of 6 runs on macOS; with the fix and the same widening it passed 20 of 20 on macOS and
60 of 60 in the Linux CI-image container. These are the numbers in 4cb546db5's message. One more run, made after that commit was
written and so not in its message: without the fix, the widened e2e test hung 10 of 10 in the Linux container (the plugin a zombie,
the guard's worker and timer threads parked on the fake clock). Natural rate before the fix: 0 of 1,900 runs in the Linux container (arm64, 4 CPUs, CPU-bound
neighbours) and 0 of 200 on macOS, 1 of 78 in CI.
**Fix.** A call no longer listed is not waited for on the clock: its end is already in the channel or is posted (or its sender
dropped) by the close right after, so the caller blocks on the channel. Regression test first, through three test seams (the caller
before and after its look-up, the closer between the drain and the post): red at its 30 s hang guard with one waiter left on the
fake clock, green after. Debrief's copy (`debrief-ipc`, rule 55) waits on the channel itself, so it does not have the window.
**Why the logs did not show it.** A hung test prints nothing; the end line of the connection (`pending=1`) was written, but nothing
said that a caller then waited on the clock. **Class:** a waiter registers its wake-up after the waker fired (a lost wake-up), here
created by moving a post out of the lock that orders it against the waiter's registration.
**Redesign (ADR 0106, same day).** The fourth connection-end race in `paddock-ipc` in nine days (runs 7761 and 8214, a
container run, a review finding): each call now ends through one completion slot that the caller and the closer share (one
completion, no lost wake-up, an end when the sender is dropped). It also closed a hole the patch above left: a close that
panicked after taking the calls out left a caller already waiting on the clock waiting for its whole timeout (red at
4cb546db5).

## Fix item: gallery data.spec:193 read the table's left edge while the columns still moved (2026-11-18, CI run 8215)
`table: loading and empty say no count, and the columns do not move when rows arrive` (ui/e2e/data.spec.ts:193) failed once on
Linux in ci.yml run 8215 (job 40623, reference apps shard 1/2): `box-loading` was at x 363 against 353 for the loaded table. It
passed in runs 8218 and 8225 on the same tree and 20 of 20 locally (Chromium, 4 workers). FLAKY in code train 13 reaches (pd-table);
recorded here and not blocking, by the coordinator's decision. Next: reproduce in the Linux CI container with a failing run first,
then wait for the layout to settle (two reads agree, ER-12) before the comparison. **Class: an e2e read before the layout settled
(the ready-read class; train 13's workbench palette, Dyno alarms, gallery menu, grid header, Pitwall tray and Manage-layouts rename).**

## The notices generator took a missing npm install for an empty one (2026-11-18, ER-33)

`bash scripts/notices.sh dyno --check` in a worktree set up with `npm ci --ignore-scripts -w conformance` said "out of date", and
a write run would have written a wrong file. `scripts/notices.sh` ran `npm ci` only when the root `node_modules` folder was missing,
and that recipe leaves one (only `yaml` and the conformance link). `npmPackages` then found no folder for any of Dyno's 76
production packages, warned `no licence file found` 76 times, rendered each as `[no text]` and dropped the 46 texts only npm
packages use: 288 distinct texts became 242 and every `[MIT-n]` label moved. Measured (Mac, cargo-about 0.9.2, train 15): with a
full root `npm ci` the output is byte-identical to the committed file and `--check` exits 0 for Dyno and Pitwall, so the OS was never
the cause; 1222 licence files of the user's cargo registry against a fresh one (0 differ), CRLF, the cargo-about version and the
targets were ruled out. It had cost false "stale" findings; no committed file ever carried the shape.
**Fix, one mechanism.** `npmPackages` throws when a production package the lock lists has no installed folder, an unparseable
package.json or another installed version (only a package npm marks optional may be absent: a warning, and it is left out of the
render; an installed package with no licence file stays a warning). `paddock-notices` exits 6 (not measured, which `./dev gen check`
reads as "could not look") and writes nothing. `--provision` (what `scripts/notices.sh` passes, now a 12-line wrapper) looks after
the tools in the CLI: with `--check` it installs nothing (no `npm ci`, no cargo-about install; the version probe runs with
`RUSTUP_AUTO_INSTALL=0`, ER-45) and exits 6 with the cause and the next step; a write run installs cargo-about and runs
`npm ci --ignore-scripts`. A caller of `--check` must therefore install the npm packages first (debrief.yml's licences job did not
and got the step added; the generator registry declares the install in `requires`).
Class: a generator or check that treats a missing input as an empty input; compare the input with what declares it (here the
lock) and stop when they differ.
**Recurring: input-precondition-not-verified** (ER-33 cases on Dyno 1.1.0, the 2026-11-17 Debrief dock spec, `every()` over nothing in ER-12).

## An abandoned kept-clone phase let its git outlive the step (2026-11-18)
`host_steps::kept_clone_the_create_phase_is_bounded_by_its_own_limit` failed once in 5.9 s and passed on its retry (5.5 s) in a full
nextest run at load 10 to 25: the reaper found a process "below a child" still running after the Rust step ended. Causes listed, with
what ruled each out: (1) the product ends only the git's group and not the tree: refuted by `paddock_proc::kill_tree` (SIGKILL of
the group, then each process by identity) and its test of a grandchild; (2) the reaper reads the table before the kill is reaped:
refuted for the git, whose own wait is reaped by the run before it returns, and the reaper skips zombies; (3) the phase is abandoned
at its bound while the git's kill is still in flight, and the step ends with the git running: confirmed (below).
The git ends itself at the phase's limit (1 s from its start); the phase is abandoned at limit + 1 s from the clock mark (the hanging
git's first line), so ending the git's tree must take less than a second. **Where the time goes** (temporary probes, not committed;
`host_steps::` at 16 threads, 6 iterations, load 17 to 26, 59 hung gits): the timer fired on time (at most 11 ms late); `kill_tree`
took up to 1.79 s (it reads the whole process table, with a start-time look per process, before it signals anything); the run
returned after at most 1.72 s of a 1 s limit. At that load the phase was abandoned while its git still ran in 40 of 59 hung gits (the
step said so: "ended 1 program(s) of the abandoned phase"), and the parity tests failed on the extra line, so the failure is common at
load 26 and rare at load 10. The earlier single-test loops at load 28 never reached it (one test at a time reads a table that is not
under 16 threads of its own).
Regression tests: `a_phase_abandoned_at_its_bound_leaves_no_program_of_its_own_running` (kept clone) and
`a_hung_git_of_an_abandoned_phase_is_ended_with_the_programs_below_it` (stable workspace), both red without the fix (the program below
the git was still running when the phase returned). **Fix:** `Held` (in `ci_host/phase.rs`) is owned by `bounded`, `bounded_hung` and
`bounded_marked`, handed to the phase's closure, and ended on `Phase::Timeout`: a captured `ToolCall` is `held_by` it (fed by
`paddock_proc::run_bounded_noting`, which tells the caller the program's identity at its start, `None` when its start time is
unreadable: counted and reported, never ended by guess), and `Held::end_all` ends every program still running by identity, once per
running program (the first version read the table twice per program: 6.7 s for three programs at that load), and waits until they
are gone. A program that starts after that is ended at its start. The step logs how many it ended and how long it took. The phase's
limits are unchanged. **`ENDED_WITHIN` (10 s, ER-12):** the deadline of the wait after the signals, not an expected time: measured
`end_all` at most 1.9 s (59 calls, one program each, load 17 to 26), all of it the table read; a process that outlasts 10 s is stuck
in the kernel and is reported by name, not waited for. The stand-in's `pid_file` now ends in a newline (`read_pid_file` waits for a
complete line and never completed on it: both new tests hung 40 s before that fix; test in `paddock-testkit`'s stand_in).
**Class: work a step abandons keeps running past the step** (earlier: the abandoned checkout phase that set the next phase's flag,
2026-11-15, above; found both times by ADR 0103's reaper and the parity tests under load). Open, not done here: `kill_tree` reads the
whole process table before it signals; signalling the group first and reading the table for escapees would cut the 1.8 s but changes
what a tree kill can find (a child whose parent was killed is no longer below it), so it needs its own design.

## Fix items from the bulk-API inventory: four possible bugs read from code (2026-11-18, ADR 0108)
The inventory for ADR 0108 read four defects from code. None is reproduced yet; each fix starts with a failing test (ER-11).
- **Pitcrew `job-outcomes.json` loses outcomes when runners refresh together.** `runner_activity`
  (`apps/pitcrew/app/src-tauri/src/commands.rs`, an async command on the blocking pool) calls `gitea::outcomes`
  (`apps/pitcrew/app/src-tauri/src/gitea.rs`), which loads the shared cache file, adds its runner's lookups and saves the whole
  file. The dashboard asks for every runner at once (`dashboard.store.ts`, `Promise.all`), so the last save wins and the other
  runners' new outcomes are looked up again on the next refresh. Fix with ADR 0108 WP-9 (one outcomes pass for all runners in
  one command), or a lock around load-modify-save. Test: two concurrent `outcomes` calls with disjoint new jobs keep both.
- **Spotter avatar writes share one temporary name.** `write_atomic` (`apps/spotter/crates/spotter-engine/src/avatar.rs`)
  writes `path.with_extension("tmp")`, so `<key>.img` and `<key>.json` both use `<key>.tmp`. Two `watch_avatar` calls (async
  commands) storing the same key at once can rename one's bytes into the other's file. Fix: a unique temporary name per write
  (the full file name plus `.tmp` and a random or process-unique suffix). Test: two concurrent stores of one key leave a
  readable image and meta.
- **Dyno's two smartctl readers are not registered.** The local `SmartctlWear::read`
  (`apps/dyno/crates/dyno-platform/src/hardware.rs`) and the helper's `smart_reader.rs` each run `smartctl` scan plus one run
  per drive; `docs/known-copies.toml` lists neither (rule 55). Fix with ADR 0108 WP-2 (DY-4: one shared reader, bounded
  parallel), or register the pair first with a delete-by tag.
- **`RepoGit::run` in paddock-dev starts git with no timeout.** `tools/paddock-dev/src/pr/mod.rs` runs
  `git_env::git()...output()`, so a hung git hangs `./dev pr` for ever. Fix: run it through the bounded runner
  (`paddock_proc::run_bounded`) with a limit, as the other git readers do; with ADR 0108 WP-11 (TD-10). Test: a stand-in git
  that never exits ends at the limit with an error naming the command.
Class (the first two): per-item calls (ADR 0108) bring their own races when they run in parallel against one shared file; the bulk form (one
pass, one write) removes the race with the calls.

## Fix item: job-isolation's orphan-days test failed once in the push gate (2026-11-18)
`disk: kept folders no job used for PADDOCK_CI_ORPHAN_DAYS ... go when their slot is idle; used ones stay`
(conformance/test/job-isolation.test.mjs:357, "an old-layout folder nobody took over: gone") failed once in the pre-push row
`conformance-tests` of `docs/adr-bulk-apis` (machine load 22 to 27, the row at concurrency 4 beside the other rows), and passed in
two runs right after on the same tree (`npm test` in conformance, then `./dev ci run conformance tests` at concurrency 4). The
change being pushed was docs only and does not reach `job-isolation.sh` or its test, so the push went ahead (ER-12: a FLAKY test
outside the change's plan is recorded, not blocking). Causes to rule out, with the observation that rules each out: (1) `release`
returns before the orphan sweep of unslotted folders has finished (rule out: the sweep runs inside `release`'s own process and
`release` waits on it); (2) the unslotted folder is swept only under a lock another concurrent test file can hold (rule out: the
lock path is under the test's own `host()` root); (3) the 20-day time stamp is renewed by something before the sweep (rule out: the
folder's mtime just before `release`). Next: reproduce under load with a failing run first (ER-28), then fix the cause.

## paddock-assistant: "Stop ends a slow Pitwall within 2 s" timed the turn's save, not Stop (CI run 8241, Windows, 2026-11-18)
**Symptom.** `pitwall::stop_ends_a_slow_pitwall_within_a_read_slice_in_every_phase` failed once on main's Windows crate job
(run 8241, job 41141; 2.8 s against its 2 s limit) and passed on its retry (FLAKY). It measured from Stop to the turn's end event.
**Causes listed (ER-28), with what each measurement showed** (probe on the real Windows runner, branch
`wip/assistant-stop-windows-probe`, run 8264: 40 turns x 3 conditions x 2 tries = 240, split at the moment the provider's chat
returned): (1) the cancel acting late (read slice, socket timeout, close): refuted, Stop to the generation's end was at most 65 ms
in every condition (idle, every core busy, four fsync writers); (2) the save before the end event (`ConversationStore::append`,
an atomic write with fsync): load-sensitive, median 16 ms idle and 75 ms with the fsync writers, at most 349 ms; in the failing run
a test that runs a real cargo build, pack and sign (`cargo-paddock-plugin::it flow`) ended next to it; (3) a connect retry (Windows
reports a refused loopback connect after about 2 s): refuted, nothing connects after Stop on this path (code read:
`PitwallProvider::chat`, `loopback::send_via`). The 2.8 s was not reproduced in isolation (0 of 240); a held save reproduces the
failure deterministically (the old assertion with the save taking 2.1 s: red, Stop itself 41 ms).
**Root cause.** The test asserted a wall-clock limit (rule 40, C30 flagged it) over an interval that included the disk.
**Fix (test).** The world's store and provider are wrapped (`HeldSaves`, `Watched`): the test holds the save, stops, waits for
the generation to end (the testkit's hang guard only), checks that no end event came before the save, then lets the save go. No wall-clock
assertion; the read slice's promptness stays with `paddock_pitwall`'s own `cancel_acts_while_the_server_is_silent`. The sibling
`stop_takes_effect_within_100_ms_and_keeps_what_was_said` (turns.rs, the same class, the other C30 finding in the crate) now asserts
that the blocked generation saw the stop, without its 100 ms limit. Positive control: with the provider's cancel not passed on,
the new test fails at its hang guard ("the generation never ended after Stop"). Stress: 300 of 300 (macOS, load 30).
**Why the logs did not show it.** The test printed only the total; the split needed the probe. **Class: a test asserts a wall-clock
limit** (rule 40; earlier: Debrief 1.1.1's three cancel tests, 2026-10-25, above).

## Debrief: two live-tail e2e timed out in WebKit because every live tick costs the page about a second (CI run 8243, 2026-11-18)
**Symptom.** `live-tail.e2e.ts:207` (the tail status panel and Settings ▸ Live tail) and `:247` (rolling files) failed both tries in
WebKit on main (debrief.yml run 8243, shard 3/4): the Settings form not visible after 5 s ("Loading settings…" in the snapshot), an
axe scan cut by the 60 s test timeout, a 60 s timeout, and `data-dock-planned` missing after 5 s. In the same job
`main-layout.e2e.ts:113` and in shard 4 `windowing.e2e.ts:93` were FLAKY, the first on the same `data-dock-planned` wait.
**Evidence.** The traces: every step slow while live tail ran (a click 1 to 3 s, `page.clock.runFor(2500)` 34 s); the green
main run 8229 already took 30.3 s and 24.5 s for these two tests (of 60 s), and its shard ran in 15.2 min against 26.9 min in 8243
(the runner 1.8 times slower). In the CI image (4 CPUs, WebKit, 2 workers) the two took 32 to 37 s; Chromium 5 to 6 s; macOS WebKit 7 s.
**Causes listed (ER-28), with what each measurement showed** (CI image, WebKit): (1) the mock live tail's ticks (every 250 ms)
saturate the page: confirmed, 3 to 18 frames in 3 s with ticks against 184 with live tail on and no ticks; JS in timer callbacks
was 12 % of the time, so most of it is the page's own update and rendering after each tick; (2) the pinned page clock (new between
the green and the red run): refuted, the page is saturated with and without it, and the tests were already at 30 s before it;
(3) CSS animations (two infinite pulses while live): refuted, pausing them changed nothing, and with no ticks the page ran 184 frames
with them; (4) forced synchronous layout: refuted, 11 k `scrollTop`/`scrollLeft` reads cost 9 ms in 5 s; (5) Intl formatting:
refuted, 2 ms; (6) the mock's settings load stuck: refuted, it loads within 1 to 2.3 s on an idle page; (7) the app load's dock
planning (`data-dock-planned`, a 5 s wait after the status bar): load-dependent, not stuck (no settle retries ran out): 0.05 to
1.9 s at 1 worker, 3.9 to 6.5 s at 4 workers (8 loads each), 3.8 to 5.2 s in run 8243, where it failed twice at 5 s.
**Fix (specs).** The status test opens a live tail whose first tick is two minutes away (`QUIET_LIVE`): it checks files, status and
set_live, not new lines (the options go to set_live only while live tail is on, so turning it off was no answer). The rolling-files
test ticks every 500 ms (5 ticks in its 2.5 s, not 10) and turns live tail off (`liveOff`) once the two rotations are in. Holding the
page clock does not work: axe and the live toggle wait on page timers (tried: both tests hung). The app load (`openApp`,
`openAppWith`) now gives its waits one budget, `APP_LOAD_TIMEOUT` (15 s, the status bar's already), each wait never less than the
5 s it had; the measurement is recorded next to the constant (ER-12). `expectDockPlanned` takes an optional `timeout`.
**Counts** (CI image, 4 CPUs, WebKit, `--workers=4 --repeat-each=20`, the two tests, 40 runs each way): before 29 of 40 failed
(9 at `data-dock-planned`, the others at the Settings form, the members count, an axe scan or a 60 s timeout); after 40 of 40 passed (median 30.7 s, at most 48.5 s with
four workers on four CPUs). macOS, both engines: green before and after (not the failing environment).
**Open (not done here).** Whether a live tick is this costly in the desktop app's WebKitGTK on Linux (GPU compositing, not the
headless software renderer) is not measured; the per-tick work (what re-renders) is a product question for Debrief's live tail.
**Class: a test runs a moving source it does not need while it reads** (earlier: the events-grid fix item, 2026-11-17, and the
live-tail toast clicks, 2026-11-12, above).

## Retiring the JavaScript twins of 12 checks: an oracle test cannot outlive the oracle (pair P3, 2026-11-19)

Every Rust check test that compared with `conformance/src/checks/C*.mjs` ran the live JS host, so deleting the modules would have
deleted the proof with them. Order that worked: (1) give each comparison a case name and record the JS answer through the one bless of
`golden.rs` (`tests/it/recorded.rs`, files in `tests/fixtures/check/recorded`), (2) harvest every input the 12 JS test files gave their
functions and `runChecks` (a loader hook wrapped them; 310 cases in `tests/fixtures/check/units`) and record the JS answer for each as
a one-file tree, (3) only then delete. Harvesting found nothing the Rust host answered differently except wording (a JSON parser's
error text, the regenerate command, "check broken" against "not measured" for a run that looks at nothing). Three things a recorded
answer must not hold: a run's timestamp (`at`), a commit in a message (`merge base 8a41...`) and a stale disk name that C125 would then
find in the recording itself. A whole-tree answer is not recorded at all (the first version was a ceiling per check and file, and it failed for an unrelated new shell script, review P1-1): the live tree is held only to what holds whatever it contains (it ran, its control passed, it looked at something, no error), and the comparison with the JS answers is made on a frozen copy of files (`tests/fixtures/check/real-tree.json`).
Also: `maskRust` and 14 other imports lived in the deleted C39 module; check what else imports a module before deleting it. The trigger
needed no new rule (the conformance job is in every plan), but a test now says so for every input these checks read.
**Class: a retired side takes its trigger, its helper exports or its oracle with it** (earlier: the P1 planner retirement, 2026-11-18, where
`CONTRACT_INPUT` had to be added for the tests the script's own tests had covered).

## Fix item: drop the `golden::` nextest waiver once the real legs are measured (2026-11-19)

The `golden::` waiver in `tools/paddock-dev/.config/nextest.toml` (4 x 60 s) was made for the node runs of `scripts/affected.mjs`, which are
gone since the planner retirement (2026-11-18). On the Mac the 20 golden tests now take 11 s in all, but ER-9 and ER-12 do not let one Mac
decide it. **To do:** read the `golden::` test durations from the next train CI run (the Windows and Mac legs), and if every one is inside the
60 s default, delete the override and the `golden::` entry of the default profile's override. Until then the old budget stays, with its old
cause noted in the file.

Measured 2026-11-19 (the conformance step's cost of compiling paddock-dev's tests): the step `cargo nextest run --manifest-path
tools/paddock-dev/Cargo.toml --profile ci --no-tests=fail -E 'test(/^contracts::/)'` in the local CI image (arm64 build of
paddock-ci, `tools/linux-container.sh --ci-image`, 4 CPUs, 2 jobs, a new empty build volume, host load 27): cold 71 s, warm 1 s (the 12
tests run in 1.2 s). Under the limits set for the choice (30 s warm, 3 min cold), so the step stays a nextest step. Not measured: a
real amd64 CI runner; read the step's timing from the next train run and revisit if the cold time passes 3 min.

## Fix item: sccache with `SCCACHE_BASEDIRS` once a release ships it (2026-11-19)

sccache 0.19.0 (2026-10-24, the pin in `tools/paddock-dev/src/ci_setup_rust.rs`) keys every Rust compile on the checkout path
(`CARGO_MANIFEST_DIR`, the working folder), so the workspace's own crates never hit across worktrees and 18% of third-party crates
(those with `OUT_DIR`) miss too (docs/research/2026-11-19-sccache.md). The fix, PRs #2918 and #3022 (`SCCACHE_BASEDIRS` for rustc and
`SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS`), was merged on 2026-11-02 and is in no release yet. **To do (user decision 2026-11-19, WP-6):**
when a release contains both, bump `SCCACHE_VERSION` and the four checksums, start the agents' server with
`SCCACHE_BASEDIRS=<worktree root>` and `SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS=CARGO_MANIFEST_DIR,CARGO_MANIFEST_PATH`
(`local_sccache.rs`), and measure workspace-crate hits across worktrees with incremental off in the push gates. Check
<https://github.com/mozilla/sccache/releases> at each train.

## A slow-to-start stand-in git met the phase limit: the mark now bounds every call of a hung-phase test (2026-11-19)
`ci_host::kept_clone::tests::a_phase_abandoned_at_its_bound_writes_nothing_into_the_next_phase` failed once at a 1-minute load of
about 31 (integrator-train16, m2-nextest.log): the stand-in git recorded no call, because the git's own 1 s limit, counted from its
spawn, ended it before it ran. Fourth sighting of "work before the hang point of a hung-phase test meets its limit under load"
(fe1bd861d, e0fdd0286, 2026-11-15 above). The failing line (integrator-train16 `m2-nextest.log`, line 463, after `FAIL [2.040s]`): `assertion left == right failed: the abandoned phase started no git after it was abandoned; left: []; right: ["-c core.hooksPath=.../bin/no-hooks fetch first"]`, at 2.040 s: the 1 s limit plus the 1 s bound, with the stand-in's one call never recorded. Causes listed: (1) the test's own setup calls: none, the first call is the hang; (2) the
phase bound (limit + 1 s from phase start, as no mark was set in that unit test): ruled out as the first cause, the git is ended
by its own limit first; (3) the git's limit counts from spawn and the stand-in's start-up under load exceeds it: confirmed by a
deterministic stand-in that sleeps 1.5 s (past the 1 s limit) before it writes its mark: red on the old code ("did not finish within
20 s", the git "timed out after 1 s", no mark), green on the new. The 2026-11-15 fix had moved the phase's limit to the mark but
left a git command's own limit on its spawn and said so (ADR 0103); with `Held::end_all` it could be closed. The review of the fix found a fifth test of the class, `a_phase_abandoned_at_its_bound_leaves_no_program_of_its_own_running` (no mark; a 2.5 s start delay turned it red); it is now on the mark (`hang_flow`) with a 2.5 s slow-start control (red with the mark taken out, 42 s). **Fix (the class):**
with a clock mark a git in a phase has the general limit and the phase's bound, counted from the mark, ends it; the unit test, the new
test and `stable_workspace`'s hung-git test (which had the same shape, its `run_with` now takes an optional `ClockMark`; red before:
no pid file after 40 s) use the one mechanism, `phase::bounded_marked`; the parity normaliser treats `signal 9` like `timed out`.
**Measured:** natural rate on the old code 0 of 200 runs (28 processes x 1 thread, 40 CPU burners, load 33), so the injected delay is
the proof, as the failure is rare; the heavy run was stopped at the coordinator's request (ER-24). Capped runs after the fix (coordinator's cap: 4 processes x 1 thread, no burners,
ambient load 35 to 40 from other agents): 200 runs of the 4 kept_clone unit tests plus stable_workspace's hung-git test, 1,000 test
executions, 0 failed. **The class is now checked, not reviewed for (ER-21):** `phase::tests::no_source_file_times_a_phase_test_from_its_start_without_a_clock_mark_or_a_reason` fails on any test in a file that uses `ci_host::phase` that calls `.phase(`, `bounded*` or `run_with(` without a `ClockMark` (or `hang_flow`/`hang_env`) or a `// conformance: no-clock-mark - <reason>` line (two exemptions, both fakes with no program); its positive control is `the_audit_of_unmarked_phase_tests_sees_one_and_accepts_a_mark_or_a_reason`. The 20 s general limit is one constant, `phase::HUNG_TEST_GENERAL_LIMIT`. **Rule:** a timed hung-phase test counts every limit it asserts from the stand-in's mark, setup calls included, and proves it
with a stand-in that is slow before its hang. **Class: work before the hang point of a hung-phase test meeting its limit under load
(fourth sighting; ADR 0103, second amendment).**

## Fix item: `./dev ci dispatch --trial --onto` and `--json` (2026-11-18, Windows slowdown trials 8251 to 8257)
- **`--onto` with a branch pushed after the trial clone was made** ("trial: git rev-parse failed: fatal: Needed a single revision").
  Already fixed on train 14 in ae662f1e9 (`trial::prepare` fetches an `origin/<branch>` target; test
  `onto_a_branch_that_is_neither_main_nor_a_train_fetches_it`); the failure came from a `./dev` built from main's tree, which lacks
  it. Still open: `--onto <sha>` of a commit no fetched ref holds fails with the same bare git message. Next: name the ref to fetch, or
  say "fetch a branch that holds it" in the error.
- **`dispatch --json` returns no run id** (`{"ok":true,"dispatched":true,"waitedMs":535}`): Gitea's dispatch endpoint answers with no
  body, so a script cannot tell its run from another agent's. A script that took the newest run took another agent's ci.yml trial
  (8256) as its own. Next: read the run list just before and after the dispatch (`client.rs` `runs_page`), return the new run whose
  `path` is `<workflow>@refs/heads/<ref>` and whose head is the dispatched commit, as `runId`; the dispatch tests' `Script` fake
  answers both lists. More than an hour with the fake's ordering, so not done in that series.
**Class: a tool's answer that a caller has to guess around (ER-27).**

## Fix item: the notices generator says STALE, not NOT MEASURED, in a worktree without the root `npm ci` (2026-11-18)
`./dev merge check HEAD` on perf/git-heavy-tests-2 (the merge of fix/lettre-0.11.24) reported `notices-debrief`, `notices-dyno` and
`notices-pitwall` STALE. The worktree had only conformance's npm packages (`./dev worktree add` installs those), so
`scripts/notices.sh` found no licence texts for the npm production packages: a regenerated Dyno file had 242 distinct licence texts
against the committed 288. After a root `npm ci --ignore-scripts` all three checks were current. Next: the generator's check (and
`scripts/notices.sh --check`) reports NOT MEASURED, naming `npm ci` as the next step, when `node_modules` lacks a package the
lockfile lists, instead of comparing an incomplete file. **Class: a check that compares output its missing input made (not
measured read as a finding; ADR 0089).**

## One test per fixture case, not a loop of cases inside one test (2026-11-19, train 14 run 8271)
`merge_check::the_remerge_subject_is_read_in_any_case_and_in_the_train_slash_spelling` built four fixture repositories and ran four
merge checks in one test, and `generated_files_are_skipped_by_every_detector` five. On the Windows runner under the suite's 8 test
threads they took 44 to 64 s against nextest's 60 s slow-timeout (ci.yml 8271 timed out on the first try; windows-host-probe 8277 on
train 14 and 8280 with the batched git reader: still 44.4 s FLAKY and 49.3 s). The batched reader did not move them: 116 of the remerge
test's 188 git starts were its own fixture. Split into one test per case with the same assertions (perf/merge-check-batched-reads
381137e33), the cases took 12.0 to 19.2 s each and nothing in merge_check went over 27.3 s (8281). **Rule:** a test that repeats an
expensive fixture for several inputs is one test per input (a shared helper, one `#[test]` each), so a test's time is one case's and
nextest spreads the cases; never raise the timeout for it (ER-12). **Class: one per-item cost summed into one bound** (the same class
as ADR 0108's one git start per item, here inside a test).

## A fixed total run budget cuts started rows that are healthy (2026-11-19, feat/gate-budget-per-workspace)

The push gate's 15-minute total ended rows that had started: 87 steps in five days, median 33 s of run, because earlier rows had used
the total (docs/research/2026-11-19-gate-hang-detection.md). A cold build costs minutes, a warm one seconds, and a total over all rows
scales with the workspace count. Do not replace one fixed number by another: the total now only stops new rows from starting, a
started row has its own generous cap (60 min) with the process list and stacks written before the kill, and a stall judgement on the
row's own progress follows. Recurring: a wall-clock bound judges liveness (ADR 0031's class; CI run 7096, the 2026-11-12 slot-acquire
wait, ER-12).

## A total over the rows of one push is the same defect as the 15-minute budget, with a new number (2026-11-21, fix/push-gate-queue-stall)

The 2026-11-19 fix kept a total run budget "only to stop new rows" and made the queue allowance a total over the push; on 2026-11-21
three pushes were refused with 5 to 15 rows "not measured" and no row over its cap. Time a step queued while a sibling row of its own push
held the heavy slot counted as run time, so one `clippy-windows` run (859 s run, 1,534 s queued) spent the 15 minutes; the allowance then
ran out over 15 rows. **Rule:** a row's bound is its own, on its own run time, from when it starts; a wait is bounded by the progress
of what it waits for, not by its length, and never by a total shared with other rows (ADR 0102, amendment 2026-11-21; ER-12: the number
is the longest measured gap, recorded in docs/benchmarks.md). Recurring: a wall-clock total judges liveness (2026-11-19 lesson above;
ADR 0031's class; CI run 7096; the 2026-11-12 slot-acquire wait).

## A FLAKY test blocks only when the change touches its feature (train 14, runs 8297 and 8300, 2026-11-19)

Debrief train 14 was held by rare WebKit flakes (1 in 145 runs) in the accessibility check and the home dashboard. The train changed
other Debrief source, so the plan reached both tests through the app package, but it touched neither feature. **Rule:** a FLAKY test
blocks a landing only when the change touches the test itself or the source it exercises (its feature or module: the files it drives,
not just any file in the same app package or crate); every other FLAKY is recorded with its log as a fix item for the next train or batch
(ER-12, user, 2026-11-19; rule 50 (k) and the train sequence (4) in `docs/rules.md`). **Open:** `./dev ci triage --members` still blames a
flaky failure on every member whose plan reaches its project (`tools/paddock-dev/src/queue/blame.rs:34-50`); that is a separate code change.

## Three gate-tool defects: an empty selection that passed, a wrapper that broke every Windows C compile, a build step that invalidated its own check (2026-11-19)

- `./dev check --pre-push --only "doctest crates"` matched no row (the selector is a row's first word) and printed "0 passed, safe to push". A selection that keeps no row is now a usage error naming the selector and listing the row names (ER-33). `gen check --only` and `check run --only` already refused unknown ids; conformance's `--only` too.
- `RUSTC_WRAPPER=<sccache>` (from `./dev sccache env`) made cc-rs wrap `clang-cl`, and sccache 0.19 cannot probe it, so every `cargo xwin` lint of ring failed (seen with `./dev heavy -- cargo xwin clippy -p paddock-net`). cc-rs 1.8 to 1.11 has no per-target switch and cargo gives build scripts `RUSTC_WRAPPER` whatever sets it. The fix is one place, common practice: the variable names a shim called `sccache` (so native C stays cached) that runs `clang-cl`, `cl` and `--target=...-windows-msvc` compilers directly and everything else through the real sccache. A first version used a differently named wrapper and lost all C caching; review found it. Verified for real with the same command, which passed; the lockfile range of cc is tested.
- `ts-dyno-shell` warned that its build step "does not build what it runs". Cause (measured, not guessed): `paddock-build` creates the sidecar placeholders during the build script's run and lists them in `rerun-if-changed`, so cargo saw them as newer than the run and rebuilt `dyno-app` once on the next call. Placeholders now get the oldest mtime (best effort, with a `cargo:warning` when it fails); the registry entry was right. A tool that copies a real sidecar over a placeholder with its old time (`cp -p`, `tar` x) will not make cargo rerun the build script: touch the file. The warning also names the first crates that compiled.

Recurring: a check or run that cannot see its subject reads as a pass (ER-33; also lessons of 2026-11-17 on C137 and `expectIdle`).

## Debrief: four WebKit e2e tests failed once each in debrief.yml run 8297 (train 14, 2026-11-19)
Each failed on its first try and passed on its retry; none had failed in the 281 earlier Debrief runs of 30 days (`./dev ci flakes`).
Flows read from the failing attempts' traces and screencasts (playwright-report-3 and -4 of run 8297).
- **narrow-window.e2e.ts:94, Ctrl+B at 900 px: the Explorer opened and closed again (a product race).** The trace shows one
  `User view.toggleSidebar via shortcut`, the grid resized twice within 600 ms (opened, then closed) and no second toggle. Causes listed:
  (1) the key was lost: refuted, the activity line and the first resize; (2) the start-up decision of train 14 (`decideAtStart`) closed it:
  refuted, it runs once after the first data and its effect is destroyed; (3) a key repeat toggled twice: refuted, one activity line;
  (4) a re-plan that was due from the service's own close ran after Ctrl+B set the open flag and before the render drew the Explorer, and
  the verdict added the Explorer's preferred width (300 px) to a dock that had not given it up: confirmed by a test that is red on the old
  code. The plan saw 1100 px for 800, said "keep", `evaluate` took that as "opened by hand while it fits" (`keptByUser` false), and the
  next re-plan closed it. The same error happened when dockview had not yet followed its container (its last size plus the drawn width).
  **Fix:** the verdict is asked with the room the Explorer takes now (`drawnWidth`, 0 before it is drawn) and the dock's container as laid
  out now, a sum that is the same before and after the render; `NarrowWindow` logs each decision (`ui.layout`). Not caused by train 14:
  the inference in `evaluate` and the 300 px fallback are on main (84b22df4e); the window is the 150 ms re-plan debounce after the close.
- **jobs.e2e.ts:62, the toast's View: the toast had timed out.** "Explanation ready" was posted when the explain job ended (2.4 s, before
  the export dialog had closed); the test then spent about 6.5 s on the export, the Jobs pane and its axe check, saw the toast, ran a
  second axe check, and the toast left 8 s after it came (TOAST_MS, as designed; VS Code also times out info toasts and holds them under
  the pointer). Refuted: the export result replaced it (the export still ran), the stack dropped it (two toasts). **Fix (spec):**
  `mockJobsHold=ai` holds the answer until both jobs were seen; the test hovers the toast (which holds its time) and clicks with the
  clock paused. Positive control: the old spec with 8.5 s of page time before the toast fails as in CI; the new one passes.
- **profiling-native.e2e.ts:6 and :29, File > Open performance capture ran nothing.** In both workers the File menu was drawn at the
  window's top-left for about 500 ms during the first load (CDK positions an overlay after Angular's next render), the click aimed at its
  item there, the menu moved under File and no command ran (no activity line). **Fix:** `openMenu`/`openMenuPath` wait for the menu to sit
  against its trigger (`expectMenuAnchoredTo`, a new kind in `ui/projects/paddock-e2e/src/ready.ts`, with `menuGap` tested on the run's boxes);
  positive control: a menu held at the top-left fails the wait.
**Why the logs did not show it:** the Explorer's service logged nothing (now it does); the toast and the menu were visible only in the
screencast. **Class:** the narrow-window race is the third "layout decision from a stale measurement" (dockview's last size: the
`watchResize` settle loop, train 14's same-frame fix, this one); the other two are the ready-read class (a spec acting on a control that
is in the DOM but not positioned or not lasting).

## The dock's room has one reader; menus are found through one helper; why the File menu sat at the top-left (Debrief, 2026-11-19)
Follow-ups of run 8297 that the user approved.
- **One reader of the dock's room.** `DockLayout.room(api)` is the dock's container as laid out now (null without a box); `plan`, grow-back
  (`growBackIds`, `restorable`) and the Explorer verdict read it, and a test makes dockview's `width`/`height` throw a record on every read
  (red before: ten reads in one plan, verdict and grow-back). `needsFit` compares the plan with the groups' own sizes and reads no room.
  With the plan reading the container, `watchResize` no longer waits up to 20 x 50 ms for dockview to agree (the settle loop of
  `RESIZE_SETTLE_TRIES`): it lays dockview out at the container (`followContainer`) and re-plans after the 150 ms debounce, and
  `fitToWindow` does the same first. Third instance of the class "a layout decision from dockview's last size" (the settle loop,
  f253014fa's same-frame fix, run 8297's Explorer race), so the class is closed by construction rather than by another wait.
- **Menus and toasts through one helper each.** All 59 `getByRole('menu')` sites of Debrief's specs moved onto `menu-support.ts`
  (`anchoredMenu`, `contextMenu`, `panelMenuButton`, `menuNamed` for closed or text checks), and all 29 toast sites onto `toast-support.ts`
  (`heldToast`, `pressInToast`, `runningToast`, `toastNamed`). The ESLint rule `paddock/e2e-transient-wait` reports a spec that finds a menu
  or a toast itself (by role, test id or a `locator()` selector); report-only (ER-41, `PADDOCK_E2E_TRANSIENT_LINT=warn`, reported in
  Debrief's lint job): 88 findings on the old specs (59 menus + 29 toasts), 0 after.
- **Why the menu waited (measured, not guessed).** Every Debrief spec runs on Playwright's installed clock (`pinnedClockTest`), which fakes
  `setTimeout` and `requestAnimationFrame`. Angular's zoneless scheduler renders in whichever of the two fires first
  (`scheduleCallbackWithRafRace`, @angular/core 22.2.3), and CDK positions an overlay in `afterNextRender` (`OverlayRef.attach`). With the real
  clock the rAF branch runs in the browser's rendering step, before paint, so the menu is placed before its first frame: sampled at the
  start of every frame, 0 of 6 WebKit opens and 6 of 6 Chromium opens showed one frame (the sampler's rAF runs before Angular's in the same
  frame, so that frame is read before CDK positions the menu, not necessarily painted). With the clock installed the fake rAF fires from
  the clock's own timer, outside the rendering step: up to 5 frames (41 ms) on Chromium locally, and in run 8297, under CI load, about
  500 ms. A CPU profile of the window (Chromium, the browser build) showed the mock backend's queries (`mock-backend.ts`, `mock-engine.ts`)
  and the clock's `_runTo` on the main thread, not a stall in Angular. **Users:** no jump to fix in the app; the menu is in place before
  it is painted. Hiding an overlay until it is positioned (opacity 0) would not help the specs either (Playwright counts opacity 0 as
  visible), so no product change; the specs wait for the place instead.
**Class:** the ready-read class (a spec acts on an element that is in the DOM but not yet placed); amplifier: the installed clock moves
the app's render out of the browser's frame.

## Fix item: live-tail-follow.e2e.ts:298 "a wheel at once after Follow on is not pulled back" (Debrief, 2026-11-19)
Failed once in a full local WebKit run of fix/debrief-webkit-flakes (689 passed; `onNewest` was true after `runFor(1_500)`); passed 40 of 40
right after (20 runs x 4 workers, WebKit, load 43); `ci flakes` lists it flaky on Chromium on 2026-11-19. The spec touches no menu,
toast or Explorer code of that branch. A local failure, not a CI result, so ER-12 does not apply yet (it judges CI runs); the Chromium flake
in CI is the CI evidence. Next: reproduce with the trace on
(`--trace on`, WebKit, under load) and read the grid's `Scroll written` lines around the wheel and the Follow request before fixing.

## Debrief: ai-chat.e2e.ts:130 red on both tries in debrief.yml run 8300; two more WebKit FLAKYs (train 14, 2026-11-19)
- **ai-chat.e2e.ts:130 "a long conversation scrolls in its list only".** `waitAnswered` (ready.ts `expectStreamDone`, 5 s) saw the third
  answer still streaming. Trace: answers took 3.2 s and 2.4 s from the turn to `aria-busy="false"`, the third more than 5.5 s. Causes listed:
  (1) the `mockJobsHold=ai` hold of jobs.e2e.ts (624910cb0) outlived its test: refuted, jobs.e2e.ts ran after ai-chat in that shard (tests 286
  and 27), every test has a fresh page and the hold lives in the page, and ai-chat after jobs in one WebKit worker passed with and without
  624910cb0 (12 of 12 each); (2) a dock or Explorer re-plan loop from the same member: refuted, no `ui.layout` and no grid resize lines after
  the first question; (3) the stream's cost on a loaded runner: the shard ran 1.7 to 1.9 times slower than in 8297 (ai-chat:101 21.9 s
  against 11.0 s, :116 19.9 s against 11.8 s) and this test already took 22.0 s in 8297, about 3.3 s per answer against a 5 s wait.
  **Fix:** the test needs six answers, not their streaming: `?mockAiChunkChars=100000` (new mock switch, unit-tested: one delta instead of
  one per 12 characters) sends each answer in one piece. Locally (WebKit, 4 workers) the test went from 4.7-5.4 s to 3.9-5.0 s; 20 of 20
  green at 4 workers, load 26-30. Streaming keeps its own tests (:12, :116).
- **Fix item, fixed: ux-home.e2e.ts:73 "can be turned off for later imports" (shard 4/4, 1 of 145).** `uncheck` failed with "Clicking
  the checkbox did not change its state": the screencast shows the health check's findings arriving while the click was made, which pushed
  the checkbox about 90 px down, so the click landed on a health card. The spec now waits for the summary (Refresh enabled), the first
  finding and a still checkbox (`expectNotMoving`) before pressing. The product moves a control as content arrives above it (the ADR 0061
  class: controls that do not move); left as is.
- **Fix item, open: a11y.e2e.ts:6 "main screen has no serious accessibility violations (dark)" (shard 3/4, 1 of 145).** axe saw
  `--faint` text in the Inspector at 4.36-4.38:1 (#797d86 on #15161a, #787d85 on #151619) where the calm dark tokens give #8d929c on
  #141518/#17181b: values that fit the Inspector drawn at about 85% opacity over `--bg-0`, i.e. mid-fade. The selection was logged at the
  moment the animation wait returned (7605 ms), so the Inspector's content may have started a fade after the wait; the screencast shows
  no Inspector on screen at all. Not reproduced yet. Next: run the spec with the trace on under load and read the Inspector's animations
  (`document.getAnimations()`) when axe starts; wait for the Inspector to show the clicked record before settling.

## Fix item: Debrief WebKit ai-chat "regex proposal as a highlight rule" saw the marker dialog still open (2026-11-19, local)
`AI chat › a regex proposal is applied as a highlight rule through the marker dialog` (apps/debrief/app/e2e/ai-chat.e2e.ts:77)
failed once in 170 local WebKit runs (ai-chat, ux-home and jobs at 10 repeats, 4 workers, retries off, on train 14 bfa9f914d): after
the rule was filled in and applied, `getByRole('dialog', { name: 'Highlight lines' })` still counted 1 for the whole 5 s. The
machine was at 17% CPU idle and the base was not measured on it, so this is a finding only (ER-9), not a CI result; train 14 does
not touch this test or the marker-dialog path (fix/debrief-webkit-flakes changes only `:130` and a mock switch whose default stays
12 characters). Not blocking, by the coordinator's reading of ER-12's feature-level scope (2026-11-19). Next: reproduce under load
with a failing run first, then wait for the dialog's close to finish before the count. **Class: an e2e read before the page settled
(the ready-read class); covered by the ready audit the user approved.**
- **windowing.e2e.ts:93 "background work lists running tasks and stops one", red on both tries in run 8301 (shard 4/4, 2 of 144 before).**
  The click on the status bar's Background work chip came 40 ms after the status bar showed "events"; the screencast shows the problems badge
  arriving to the chip's right right after, which moved the chip about 55 px left, and the popover never opened (no listitems). Not train 14:
  TestCommand, the jobs hold and the AI chunk switch do not reach the status bar or `?mock-tasks`. The audit branch had not touched this spec.
  **Fix:** the click is made with the clock paused (`clickWithTimePaused`, the status-bar case its doc names); the product fix is ADR 0061
  extended to the status bar (approved 2026-11-15).

## The ready-signal audit's sweep: 10 WebKit passes of the whole Debrief suite, 6,899 of 6,900 (2026-11-19)
Ten passes of the full Debrief e2e suite in WebKit at 4 workers (pass 1 on fc18530b9, passes 2-10 with ab099989b's windowing fix; load from
11 to 34 at the starts), cut from 20 to 10 so landings go first (ER-24): 6,899 of 6,900 green. Pass 1 of an earlier start had found one
defect of the audit's own helper (holding a "running in Jobs" toast its result replaces), fixed before these ten.
**Fix item, open: live-tail-follow.e2e.ts:240 "scrollbar drag during a reveal (real input) leaves the view where the user put it" (pass 10,
load 31).** The grid logged `User took over {Input: scrollbar, Pinned: true}` at the press and no scroll at all during the drag (no `Scroll
origin` from the user; the drag's moves took 13 ms), so `expectLeftNewest` ended with "taken over, the view never left" and, by design, sent
nothing again. This is the open measurement recorded above (headless WebKit under load: a take-over with no scroll; a scrollbar press can
miss the thumb), not a view pulled back after a delivered gesture; the deterministic ownership rows of the same spec (dispatched input)
passed in all ten passes. Next: when a take-over came with no scroll event, read the thumb's position at the press (it can move while the
reveal grows the total, `Total grew outside a round` 7 ms after the press here) before deciding whether a resend is safe.

## Main run 8315: a required choice dismissed before its frame existed; a tray height read during the entry slide (2026-11-19)
- **shell-demo.spec.ts:696 (reference apps shard 2/2): "How should the demo greet you?" not found after Escape.** The test opens
  `sd.choose-style` (a required choice) and presses Escape at once. `InPageDialogBackend` pushes the dialog on its stack synchronously,
  but the frame that applies the required-choice rule only exists after the host renders it (`ngAfterViewInit` sets `requestDismiss`).
  Until then `ref.dismiss()` was a plain dismissal, so an Escape in that window closed a dialog that must be answered. Chromium runs
  input before timers, so a loaded runner widens the window. Causes listed: (1) the pre-view dismissal: confirmed, the shell-demo page
  with the main thread slowed 20x (CDP CPU throttling) and Escape right after the open fails 17 of 20 with the CI message, 20 of 20
  green with the fix; (2) the lazy content chunk stalled over 5 s on the dev server (the run 7854 class): refuted by (1), the same
  slowed page shows the dialog in time once the dismissal is refused; (3) the click did not open it: refuted by the same runs. Not
  caused by a train-14 member: the code dates from 2026-10-31; local unthrottled runs are 40 of 40 green before the fix. **Fix:** before
  its view exists a dialog follows the frame's loading rule (a required choice ignores a dismissal); unit test in `ui.spec.ts`.
- **tray.e2e.ts:112 (Pitwall e2e-webkit, Linux): Activity 80 while loading against 81 ready.** Activity is 80.5 px tall (its `.big`
  line box is 22.5 px). The popover slides in (`pd-tray-in`, translateY 4 px); a read during the slide sits at a fractional offset and
  carries the transform's float error (80.49999237 or 80.50000763), and `Math.round` turned that into 1 px. Run 8212's fix (two reads
  agree) did not hold: both reads agreed on rounded values while the popover still moved (y from 49.0 to 53.0 over the reads). Measured
  in the arm64 CI image (Linux WebKit, 4 workers): raw heights read before the fix 3 of 20 red; with the wait for the entry animation's
  end, 60 of 60 green at load 17-35; the whole file 19 of 19 there and 38 of 38 on macOS (both engines). Not caused by a train-14
  member (same symptom in run 8212, before the train). **Fix:** `openTray` waits until the popover's finite animations have finished,
  and the test compares unrounded heights. Class: a layout read during an entry animation (the "two reads agree" rule does not catch a
  box that moves while its rounded size stays).

## A click that closes its own window: Debrief's popout.e2e.ts:62 Dock back (2026-11-20, train 16)
- **Failure:** `locator.click: Target page, context or browser has been closed` at popout.e2e.ts:65, the click on the pop-out's own
  "Dock back into the main window" button (debrief.yml run 8338 job 42172, Chromium, FLAKY: passed on its retry; 1 of 151 runs). The
  call log stops at "performing click action": the click had been delivered. Dock back asks the host (BroadcastChannel) and the host
  closes the pop-out (`win.close()` in popout-backend.ts), so the window can go away while Playwright is still finishing the click.
- **Causes weighed:** (1) the window closed for another reason before the click: refuted by the call log (actionability passed, the
  action was being performed) and by the retry docking normally; (2) a slow host: no, the close is the expected effect of the click;
  (3) Playwright treating every such close as an error: refuted, a click after which the host closes the window passes 80 of 80
  (synchronous, deferred, mousedown, pointerdown, BroadcastChannel; local probes in Playwright's pinned Chromium 1248). The race is
  only the close landing inside Playwright's own completion of the click, which cannot be scheduled from outside.
- **Reproduction (deterministic):** the kit's selftest `close-click.spec.ts` holds the click open on a disabled button and closes the
  window from its host meanwhile: a plain click rejects with the same error every time (red with the old click-then-wait-for-close),
  green with the fix; 20 repeats at 4 workers in Chromium and in WebKit.
- **Fix:** `@decryp7/paddock-e2e` `untilItsPageCloses(page, step)` and `clickThatClosesItsPage(locator)`: a step that closes its own page
  counts as done when the page closed; a step that fails while the page stays open, or that leaves it open, still fails. The spec's own
  next assertion (the panel is back in the host) checks that the step did its job. Both such steps in popout.e2e.ts use it (Dock back,
  and the dispatched pointer-up that docks a dragged pop-out). **Rule:** a test step whose action ends the page it acts on goes through
  `untilItsPageCloses`, never a plain `await` on that page.

## Fix items from train 16's full run (ci.yml 8346, debrief.yml 8347), not this train's features (2026-11-20)
- **debrief-plugin-host `elevated_output::standard_error_is_forwarded_cut_and_capped`** (debrief.yml run 8347, job 42279, "rust tests";
  FLAKY: try 1 failed after 40.064 s at `tests/it/elevated_output.rs:189` with the collected messages empty, try 2 passed in 0.107 s;
  history 2 of 151 runs). The test waits with `eventually(...)` for the plugin's forwarded standard error ("m1 line 2", "m1 long ...")
  to reach the log; the first try saw nothing for its whole wait. Train 16 changes nothing in debrief-plugin-host (ER-12: not this
  train's feature). Next: a failing reproduction first (the forwarding path under a held log writer or a slow plugin start), then the
  cause, never a longer wait.
- **spotter-render `real_browser closing_quits_the_browser_by_itself_before_any_kill`** (ci.yml run 8346, job 42302, macOS; failed,
  flake history 8 of 138 runs). Spotter untouched by train 16. Next: a reproduction first, as above.
- **ui gallery `open()` waits a fixed 600 ms** (`ui/e2e/gallery.spec.ts`: "Let the chart draw ... and fonts settle"). Train 16 fixed the
  one test that failed on it (the path box, which now waits for its fitted text; ci.yml 8346 job 42258); every other test that reads a
  measured or animated value after `open()` still leans on that wait. Next: wait on each test's own condition and drop the fixed wait.
- **paddock-dev `queue::tests::a_batch_is_at_most_six`** (ci.yml run 8359, job 42403, windows-latest; FLAKY: try 1 timed out at
  60 s, try 2 passed in 50.1 s; 19.2 s in run 8335, job 42150). The same run's git-heavy `ci_runs::trial` tests went from 6 to 9 s to
  43 to 60 s while the whole job went from 603 s to 753 s: three jobs shared the Windows runner with 8 test threads each on 16 cores
  (the ER-48 job share, fixed on its own branch in Train A). Next: confirm the test is back near 19 s once the Windows share lands; if
  not, a failing reproduction first (its git starts on Windows), never a longer limit.
- **./dev's kept paddock-dev build folder under /sccache is read while another lane prunes or writes it** (debrief.yml run 8373,
  job 42677, "licences, notices + commits", d-gtr9; `error[E0463]: can't find crate for 'foldhash'` while ./dev built paddock-dev
  in `/sccache/paddock-dev/rustc-0ef1bc386059`; the same commit built it everywhere else, and the retry 8377 did not hit it). Same
  class as the faithful stand-ins taking the real build-folder lock. Next: one owner per kept build folder (a lock held for the
  build and the prune), a failing reproduction first.
- **debrief webkit `e2e/a11y.e2e.ts:6` main screen has no serious accessibility violations (dark)** (debrief.yml run 8373, job
  shard 3/4; FLAKY, history 2 of 150). Train A does not touch the feature. Next: a failing reproduction first.
- **debrief webkit `e2e/ux-shortcuts-editor.e2e.ts:21` shortcut editor rebinds a command ...** (debrief.yml run 8373, shard 4/4;
  FLAKY, history 1 of 150). Train A does not touch the feature. Next: a failing reproduction first.

## Chrome took over 5 s to quit: it reads the user's temp folder, which leaked test folders had filled (Spotter, 2026-11-20, train 16)
- **Failure:** spotter-render `real_browser::closing_quits_the_browser_by_itself_before_any_kill` red on both tries in ci.yml runs
  8334, 8341 and 8346 (app spotter, macos-latest): "needed a kill: 5.55 s", the whole `CLOSE_GRACE` after `Browser.close`. It had
  passed in 0.4 to 1.5 s in every run back to 7957 and got slower as the days went on.
- **First hypotheses and what refuted them:** paddock-proc's Session change ee3aeb503 (spotter-render does not use `Session`; the same
  commit e0fd0f511 passed in 8335 and failed in 8341); a new Chrome for Testing (unchanged 156.0.8078.47 since 2026-11-09); a slow
  `Browser.close` reply (5.55 s = under 0.6 s of reply plus the 5 s wait); CPU starvation (the stuck browser was waiting on disk
  reads, not runnable).
- **Root cause:** when Chrome quits on macOS, a thread-pool task reads through its temp folder. Chromium takes that folder from
  `MAC_CHROMIUM_TMPDIR`, or else from `NSTemporaryDirectory`, which ignores `TMPDIR` (base/files/file_util_apple.mm). That is the
  user's `/private/var/folders/.../T`, shared by local gates and the three CI runners (one user). It held 903,000 entries, 730,000 of
  them `x9-fixture-*` folders that conformance's `makeApp` made with mkdtemp and never removed. Under I/O load the read outlasted the
  grace. Evidence: `sample` of a stuck browser showed one ThreadPoolForegroundWorker in readdir/`__getdirentries64` plus fnmatch for 802
  of 806 samples, and lsof showed the T folder open. With 4 workers: unset, 55 of 80 closes took over 5 s; `TMPDIR` set to a small
  folder, 16 of 16 (not honoured); `MAC_CHROMIUM_TMPDIR` set to a small folder, 0 of 32.
- **Fix:** every browser launch (spotter-render, and under rule 55 the canonical paddock-chromium and Debrief's chromium-driver)
  makes an empty `browser-temp` inside its profile and passes it as `MAC_CHROMIUM_TMPDIR`. `makeApp` uses one folder per process and
  removes it at exit. A close that needs the kill now logs a warning (canonical, Debrief).
- **Why the logs did not show it:** the close path logged nothing, and the test said only "needed a kill". The cause took a stack sample.
- **Class: tests leave folders in the system temp folder (recorded for the foundation queue, not fixed here).** The repo has 147
  `tmpdir()` sites in 97 JS/TS files, 60 `std::env::temp_dir()` sites and 24 `paddock_testkit::tempdir` sites in Rust (the 2,005
  `tempfile::tempdir` sites remove their folder on drop and leak only when a test is killed). One conformance run still leaves about
  120 other folders there (`paddock-isolation-*`, `paddock-tauri-tools-*`, `paddock-sccache-*`, `paddock-rustup-*`,
  `paddock-job-tmpdir-*` and more). The planned redesign is a shared per-run scratch root for tests, plus a report-only conformance
  check with a positive control (ER-21, ER-41).

## cargo-llvm-cov asks to install into ~/.rustup and waits on stdin (sccache work, 2026-11-20)

- **What happened:** a local `cargo llvm-cov nextest -p paddock-net` stopped at "I will run `rustup component add llvm-tools-preview` ... Proceed? [Y/n]" and sat there for 4 hours, because a background run has no stdin to answer it. Answering yes would have installed a component into the user's `~/.rustup` (ER-14, ER-45).
- **Rule of thumb:** never run `cargo llvm-cov` locally. Coverage is measured by CI's llvm-cov gate (`./dev ci run crates coverage`, `scripts/coverage-gate.mjs`), whose image has the component. A shared crate's coverage is "not measured" in a branch report and read from the first CI run.
- **Recurring class:** a tool that starts an installer or a prompt when a probe runs (ER-45; earlier instance: rustup 1.28.1 installing a missing pinned toolchain, run 8060). Any local tool step that can prompt gets stdin from /dev/null so it ends with an error instead of waiting.

## A streamed answer that was still arriving failed its wait: the wait bounded the whole stream, not a stall (Debrief, 2026-11-20, train A)
- **Failure:** Debrief WebKit `ai-chat.e2e.ts:116` "the conversation is busy from the moment its turn is drawn until the answer is
  done" red on both tries in debrief.yml run 8377 job 42784 (shard 3/4, Linux), at `expectStreamDone` (paddock-e2e `ready.ts:85`):
  `aria-busy` stayed "true" for the 5 s expect timeout. It had 0 failures in 535 runs and passed on the same commit in 8373.
- **First hypotheses and what refuted them:** a stuck turn (a lost `done` event): the trace's snapshot right after the failed wait
  shows `aria-busy="false"` and the whole answer, and its screencast shows the answer still arriving near the end of the wait (at
  "System.Ou" with Stop shown, then complete); the paddock-proc change d68f912aa and the ER-48 job share cd867ef09: `apps/debrief`,
  `ui` and `package-lock.json` are byte-identical at 9ec43be3c and 5cd9602da, and the e2e page runs the TypeScript mock with no
  process; order dependence in the shard: the retry ran in a new worker and failed the same way, finishing just after its wait.
- **Root cause:** `expectStreamDone` was one `toHaveAttribute('aria-busy', 'false')`, so the whole stream had to finish inside one
  expect timeout. The spec streams 20 pieces 150 ms apart: 3.4 s from busy to done on an idle Mac (8 of 8 probes, 4 WebKit workers),
  about 4.8 to 5.4 s on the loaded runner, past the 5 s wait while still arriving. This breaks ADR 0103's rule that a step is bounded
  on progress, not total time.
- **Fix:** `expectStreamDone` waits one expect timeout at a time and goes on while something new arrived (text or elements). It fails
  only after a wait in which nothing arrived, saying so, and records each extra wait as a `stream-wait` annotation. A paddock-e2e
  self-test streams for 6.5 s (it fails the old helper with the reported error) and has two stalled streams as positive controls.
- **Why the logs did not show it:** the call log printed `…` for the log's content, so "slow" and "stuck" looked the same. Telling
  them apart took the trace. The new failure message names the stall and what the stream showed; a slow stream leaves annotations.
- **Class: an end-to-end wait bounds a streamed result by one expect timeout.** Earlier: debrief.yml run 8300, where six streamed
  answers ran past the wait (64f8165ae, fixed for that spec only by sending answers in one piece). It is fixed for every caller of
  `waitAnswered` here.

## A folder shared between concurrent CI lanes (2026-11-21, ADR 0111)

- **Run 8373 (debrief.yml, job 42677): E0463 "can't find crate for `foldhash`".** Linux CI had set `SCCACHE_DIR` to the whole `/sccache`
  mount, where `./dev` kept its build folder. sccache's local cache is one LRU over every file below its folder, so an insert that took
  it over `SCCACHE_CACHE_SIZE` deleted cargo's rlibs. Six lanes also meant six sccache servers on one folder, which sccache documents
  as unsupported. Reproduction: `tools/paddock-dev/tests/it/sccache_eviction.rs` (the file survives beside the cache folder, not in it).
- **Fix:** a lane folder per job (`<mount>/sccache-<lane>`, the slot holder the hosts use), `./dev` in the job's own home with its binary
  kept through `actions/cache` and verified for its hash, npm and Angular through `actions/cache`, every `actions/cache` use pinned to a digest.
- **Class: concurrent jobs write one folder.** Earlier: run 8027 (E0514, one build folder for two compilers), the heavy binary vanishing
  (2026-11-14), the Windows running-tool replace (`dev`, the `mv -f` fallback). The rule is drafted in ADR 0111: no writable folder is
  shared between concurrent CI lanes; shared state goes through a keyed cache or an artifact.

## A NAS restart and a tool prompt each held work for hours: the tools now hold on a down service and never wait on stdin (2026-11-20)
- **Failure:** a NAS restart took Gitea (HTTP 502) and RustFS down in the middle of a release: a hand-typed push through the keychain
  helper got a 401 that erased the user's git login (now prevented by the repository's helper, which reads `GITEA_TOKEN` and ignores
  erase), and a cache trial failed with nothing to say why. Separately `cargo llvm-cov` waited 4 h on "install llvm-tools? [Y/n]", and
  rustup auto-installed a pinned toolchain in run 8060.
- **Cause:** nothing asked whether the servers answered before a dispatch, a wait or a push, and a child started by a check inherited the
  caller's standard input and rustup's default.
- **Fix:** `paddock-dev`'s health gate (`health.rs`) holds `ci dispatch` and `wait --ci-run` (and `ci dispatch --trial`'s push) while
  Gitea is down, bounded by the command's own `--max`; a command holds only on the services it needs (a wait on a run never on
  RustFS, review of aa64cb4c3), and a wrong `--server` fails at once; `paddock_env::command::no_prompts` closes standard input and pins
  `RUSTUP_AUTO_INSTALL=0` for `heavy`, `ci run` lines and the generators; `.claude/hooks/no-local-installs.sh` refuses an agent's
  `cargo llvm-cov`, rustup installs and updates, and `cargo install cargo-llvm-cov`. Each has a test that fails without it.
- **Class: a tool that waits on something it cannot see.** Earlier: run 8060 (rustup auto-install), `pgrep -f` loops (ER-32).

## Fix items from Train A's full rounds (ci.yml 8389, 8425; debrief.yml 8426; dyno-release.yml 8446), not its features (2026-11-21)
- **paddock-ipc `endings::ipc_link_server_ends_every_connection_right_or_is_in_the_ratchet` on Windows** (ci.yml run 8425, job 43164,
  crate (windows-latest); FLAKY: try 1 "ipc-link-server busy: RIG FAILURE (write)", try 2 passed). Second Windows instance after run
  8174. Next: a failing reproduction first, then the cause.
- **debrief webkit `e2e/a11y.e2e.ts:6` main screen has no serious accessibility violations (dark)** (debrief.yml run 8426, job 43195,
  shard 3/4; FLAKY, third sighting after 8373). Next: a failing reproduction first.
- **dyno webkit `e2e/layout.spec.ts:224` settings pages at 900x600 with the advanced fields** (dyno-release.yml run 8446, job 43252;
  FLAKY: `span.dy-pw-muted "Memory 24.8 of 48 GB": text needs 135px, has 132px, no accepted ellipsis`). A product bug, not only a test:
  a live value can overflow the popup-widget preview at 900x600. Next: a failing reproduction that pins the longest value, then an
  accepted ellipsis or a width that fits it.
- **paddock-watch `native::a_real_folder_change_reaches_the_action_and_a_recursive_watch_sees_sub_folders` on macOS** (ci.yml run 8389,
  job 42837, d7-mac-mini; FLAKY: try 1 timed out at 30 s "the watcher did not report the change", try 2 passed in 4.5 s). First run
  with three concurrent macOS jobs on one runner. Next: a failing reproduction first; check job interference on the shared runner.
- **The pre-push `conformance-tests` row failed once under load without naming the failing test** (2026-11-21, reported by the
  coordinator). Two items: the hook output does not say which conformance test failed (ER-27: a failing step names its cause), and
  an unknown conformance test is flaky under load. Next: make the row print the failing test's name and its last lines, then a
  failing reproduction under load.

## Two real-folder watcher tests timed out on the Mac: fseventsd was saturated, and the tests assumed a prompt, lossless watch (2026-11-21)
- **Failure:** ci.yml run 8466 job 43338 (`crate (macos-latest)`, d7-mac-mini): `paddock-packs::it native::the_real_watcher_applies_an_added_a_broken_and_a_deleted_pack`
  and `paddock-watch::it native::a_real_folder_change_reaches_the_action_and_a_recursive_watch_sees_sub_folders` timed out at 30 s
  on try 1 and passed on try 2 (3.6 s, 2.3 s); run 8389 job 42837 had the same paddock-watch failure. Both waited on one FSEvents
  event after `notify` started a stream.
- **Hypotheses and what refuted them:** the watched folder on the external volume, or TCC Removable Volumes: both jobs logged
  `job-tmpdir: temp folder /Users/***/gitea-runner/tmp/<job> on the internal disk`, and the TCC log had no request from cargo,
  nextest or a test binary in the failure window (the two Removable Volumes requests that day, at 00:03Z and 00:27Z, came
  through sandboxd and were answered in about 2 ms; the user saw the prompt from `chrome-headless-shell`, whose copy lives in
  `/Volumes/vault-dev/caches/ms-playwright`). The macOS job share (8 test threads per job at capacity 3) and load: not ruled out
  at load 60 or more, but with fseventsd healthy at load 15 to 20 every event took about 10 ms.
- **Root cause (host):** fseventsd had run 6.5 days and sat at 100% to 130% CPU with 12.7 GB resident. A probe on the same Mac
  (a new watch, a write at once, a second write after the first event or 30 s) measured, with fseventsd saturated: the first event
  in a median 912 ms (up to 4.1 s), the second in a median 119 ms, and 5 of 50 waits saw nothing for 30 s. fseventsd restarted at
  02:37:54Z (not by the agent); after that, 0 of 100 waits were lost and each took about 10 ms, whether the test wrote at once or
  after a readiness probe. Not reproduced: putting the saturation back (it cannot be made on purpose without loading the user's Mac).
- **Contributing (tests):** the tests wrote the change right after the watch started and waited for exactly one event, so a late
  start or one dropped event failed them, and a late event from an earlier write (seen: one arrived 0 ms after the drain) could
  have passed a wait for a later change. Apple documents dropped events (`kFSEventStreamEventFlagKernelDropped`/`UserDropped`,
  with `MustScanSubDirs`: "you must do a full scan"), so a test of a real watch cannot assume none.
- **Fix:** both tests prove the watch live first: a probe file (one the pack loader ignores) is rewritten every 250 ms until the
  action has run once (`paddock_testkit::wait::retry_within`, 30 s guard as before). The paddock-watch action reports which of the
  test's paths existed when it ran, so a late event cannot stand in for the change; the packs waits already checked the store's
  content. A timeout names the likely host state (a saturated fseventsd) and the command to see it. A change the watch still drops
  fails, as it should on a sick host. Host: restart fseventsd or reboot when it sits at 100% CPU.
- **Why the logs did not show it:** the failure said only "Timeout", nothing about the host. fseventsd's CPU and memory were found
  with `ps`; the probe needed the event times.
- **Open (not verified):** whether `notify` 8.2's FSEvents callback drops a dropped-events notice whose path is outside the watched
  folder (it filters by path before it turns flags into a `Rescan` event, `fsevent.rs` callback); if so, a product watch never
  learns it lost events. What grew fseventsd to 12.7 GB (a client that stopped reading, or the 690k leaked test folders and their
  paused delete) is not known.
- **Class:** a test of an OS notification assumes it is prompt and lossless. Earlier instances: none recorded.

## Train 18's two FLAKYs: a stand-in timed against a 1 s limit, and a wrapper that started a program per folder level (2026-11-22, run 8546)
**macOS, `generators::tests::real::a_wait_for_the_slot_past_the_hang_guard_is_not_measured_and_names_the_holder` (job 44050).**
Try 1 panicked with `TimedOut` after 1.042 s; try 2 passed. Causes listed: (1) `run_bounded` dropped output written just before
the kill: ruled out by reading it, the loop waits for both pipes to drain after the kill (`PIPE_GRACE`); (2) train 18's change to
`base_command` (`no_prompts`: stdin null, `RUSTUP_AUTO_INSTALL=0`): ruled out, `run_bounded` already closed stdin,
and the variable does not reach the stand-in's first write; (3) the 1 s limit counts from the spawn, and the stand-in's start-up plus its first write took longer on a loaded
runner: confirmed, a stand-in that waits 1.5 s before its line fails the same way at the same line every time; the natural rate on a
quiet Mac was 0 of 25 runs at 4 threads. **Fix:** what `Real::run` makes of a finished run is `run::ran_from`, a function of the
output alone; the test gives it a timed-out output and no clock is involved (30 stress runs of the group at 4 threads, green).
Fifth sighting of "work before the hang point of a timed test meets its limit under load" (2026-11-15, 2026-11-19 `kept_clone`
above): the phase audit sees only files that use `ci_host::phase`, so this one was not checked.
**Windows, `paddock-dev::it wrapper::a_config_that_only_names_the_build_folder_does_not_change_the_hash` (job 44099).** Try 1 hit
nextest's 60 s; try 2 took 33.9 s; on earlier runs it took 27 to 44 s (runs 8335, 8346, 8359, 8495, 8509), so it sat near the limit
before train 18, whose `dev` changes are not on the `--wrapper-hash` path. The test runs `./dev --wrapper-hash` eight times, and each
run started 42 programs in the macOS test fixture (a `dirname` per folder level, and grep, sed, cut and tr for each parsed line), at
about 45 ms a start on that runner plus a fork per command substitution. **Fix:** the walks go up with parameter expansion and the
`-vV` lines, the flag filter and the explain line are read by the shell or one awk: 13 starts, guarded by
`a_wrapper_hash_costs_a_bounded_number_of_starts_whatever_the_folder_depth` (red at 42 before). On a bare drive the walk now ends at
`C:` as Rust's `Path::ancestors` does, where dirname went on to `.`. Same class as the 2026-11-18 Windows git starts (ER-20).

## The lessons file lost its blank lines to git's union merge four times (2026-11-21)
- **Lessons file format (C145).** The file is merged with git's union driver, which works on lines: twice (9ec43be3c, b9bf5fc4b) a merge
  dropped the blank line between two sections and left a `## ` heading directly under the last line of the section above, so the heading
  rendered as text; 35 such headings were already in the file. New check C145 (report-only, ER-41) reads docs/lessons.md for the
  format it relies on: a blank line before every `## ` heading, one `# ` title on the first line, no repeated heading, a single final
  newline (headings in fenced code are not headings). It runs in the conformance job, so every member's gate runs it on the combined tree
  at merge time; `./dev merge check` has no separate detector, because its detectors judge a merge three-way against its parents and
  this defect is visible in the merged text alone.
- **Cause of the drop, and the fix (the 4th time: 9ec43be3c, b9bf5fc4b, 75ce0d2c8, 5c241d93f).** `git merge-file --union` on two branches that each append
  `\n## X\n- x\n` gives `... - a\n## B\n- b`: the blank line is a line both sides added identically, so the diff takes it as one shared
  line and the second branch's heading is glued to the first branch's last line. A trailing blank line at the end of each entry drops the same
  way (measured), so no file layout avoids it. Fix: `.gitattributes` says `merge=lessons`; `./dev merge lessons-driver` runs the union merge and
  puts the blank line back before every `## ` heading outside a fence. `./dev hooks install` sets `merge.lessons.driver` (a git setting, per
  clone, shared by its worktrees: run it once in the main checkout and the integrator's worktrees; a clone without it falls back to git's plain
  merge, which conflicts and is loud, never a silent drop). Tested with a real three-way merge (`conformance/test/lessons-merge.test.mjs`,
  with the plain union as the control that reproduces the drop); C145 stays as the net.
- **Class: a merge driver that works on lines damages a file whose meaning is in its blank lines** (9ec43be3c, b9bf5fc4b, 75ce0d2c8, 5c241d93f).

## Fix items from train 18's rounds (ci.yml 8495, 8509, 8546, 8554; debrief.yml 8511, 8547; dyno-release.yml 8549), not its features (2026-11-22)
- **paddock-dev `golden::workflow_action_and_script_edits_narrow_the_plan` on Windows** (ci.yml run 8509, job 43734; FLAKY: try 1
  killed at 240 s, try 2 passed in 110 s; again in run 8546, job 44099: try 1 killed at 240 s, try 2 passed in 231 s, on
  fa3397043; it passed in 179 s in the green run 8554, still close to its 240 s limit). The train does not touch the planner. The time is git starts on the Windows runner (about
  45 ms each, ER-20). Next: count the test's git starts and batch them (the shared batched reader), with a spawn budget that a test
  guards; a failing reproduction first.
- **dyno-agent `disk::centre::tests::the_cached_volumes_never_list_on_the_request` on macOS** (ci.yml run 8495, job 43528; FLAKY: "a
  cached read within the freshness window never asks the system again" failed once, passed on retry). A time-window assertion under
  load, likely reading the wall clock. Next: inject the clock (ER-12), a failing reproduction first.
- **debrief webkit `e2e/ai-chat.e2e.ts:12` streams an answer with a tool chip, and a cited event jumps to the grid** (debrief.yml run
  8511, job 43736, shard 3/4; FLAKY: 16.5 s, then passed in 22.2 s). Train 18 does not touch Debrief's app. Next: a failing
  reproduction first; check whether the progress-based stream wait (95c7e4722) also covers this test's citation step.
- **debrief webkit `e2e/home-narrow.e2e.ts:70` nothing overflows at 720 px** (debrief.yml run 8511, job 43736; FLAKY). A layout check:
  check it reads no live value (the class of the Dyno 900x600 item). Next: a failing reproduction first.
- **Debrief WebKit e2e FLAKYs on train 18** (debrief.yml run 8547 on fa3397043; each failed try 1 and passed its retry, WebKit only):
  `e2e/a11y.e2e.ts:6` "main screen has no serious accessibility violations (dark)" (shard 3/4, job 44075; the fourth sighting, after 8373, 8426);
  `e2e/resilience-context-live.e2e.ts:88` "the status bar Live toggle keeps the selection, passes axe while on ..." and
  `e2e/ux-shortcuts-editor.e2e.ts:21` "rebinds a command, warns about a conflict ..." (shard 4/4, job 44076). Train 18 changes no
  file under apps/debrief or ui, so none blocks (ER-12). Next: a failing reproduction per spec under the pinned clock, then the fix.
- **Dyno WebKit e2e FLAKY** `e2e/flows.spec.ts:743` "startup filter › hides the system and signed entries, shows the counts and
  remembers the choice" (dyno-release.yml run 8549 on fa3397043, job 44142, app dyno ubuntu e2e-webkit; try 1 failed, retry passed).
  Train 18 changes no file under apps/dyno or ui. Next: a failing reproduction first.
- **Debrief WebKit e2e FLAKYs in the scheduled nightly on main 67ac6d6ec** (debrief.yml run 8559; each failed try 1 and passed its
  retry): `e2e/ai-pitwall.e2e.ts:68` "with no Pitwall at all, every AI entry point lands on Settings ▸ AI and none is a dead button"
  (shard 3/4, job 44324) and `e2e/live-tail-follow.e2e.ts:255` "a scrollbar drag across refresh rounds ends where the user dropped
  it" (shard 4/4, job 44325). Train 18 changed no file under apps/debrief or ui. Next: a failing reproduction first.
- **From train 19's trial ci.yml run 8576 (ebe7ddd17), in code train 19 does not touch:** paddock-dev
  `sccache_eviction::control_the_same_file_beside_the_sccache_folder_survives` on Windows (job 44486; FLAKY: try 1 failed at 2.8 s,
  try 2 passed at 3.8 s; train 18's test, started through paddock-testkit); dyno-agent `disk::centre::tests::the_cached_volumes_never_list_on_the_request`
  on macOS again (job 44462; FLAKY, after 8495); Dyno vitest `settings-page.spec.ts:41` "renders the sections once loaded and shows
  no save bar while clean" timed out at 5000 ms (job 44477, TZ=Pacific/Auckland; it also failed in main's push run 8555 and passed in
  8554 on the same commit). Vitest has no retry here, so it cannot show as FLAKY: give it flaky-as-failure like nextest and
  Playwright (ER-12). Next: a failing reproduction for each first.
- **From train 19's trial ci.yml run 8601 (7d3f5a6c6), in Dyno code train 19 does not touch (it changes only Dyno's lockfiles and
  notices):** Dyno vitest, single attempts with no retry (job 44651, app dyno ubuntu-latest): `network-page.spec.ts` "Try again on
  the owners source line asks the helper now, once, outside the poll" and `processes-page-actions.spec.ts` "summarises the machine"
  timed out at 5000 ms (network-page also failed in main's push run 8555); `tray.spec.ts` "draws the engine layout in order: alarms,
  disk space, CPU and memory, with their values" (`expected 'Active alarms2 active' to contain 'node started listening on TCP port
  30...'`) and "lists every alarm and volume the engine sends, scrolling inside the popover ..." (`Cannot read properties of null
  (reading 'getAttribute')`). Dyno e2e `[chromium] e2e/layout.spec.ts:224` "settings pages at 1024x800 with the advanced fields"
  (job 44649; FLAKY: failed once, passed its retry; the class of the 900x600 item). Different Dyno specs time out at 5 s in each run
  (8576, 8601): the class the vitest retry and worker budget of feat/fixed-wait-checks address (6 lanes x 29 vitest workers on 30
  cores before, 6 x 5 after). Next: a failing reproduction for each first.
- **From train 19's full ci.yml run 8610 (b3c254a2d), in code train 19 does not touch (it changes only Spotter's lockfile):**
  `paddock-chromium::it real_browser::a_browser_ended_by_force_leaves_nothing_behind` on Windows (job 44761, crate windows-latest;
  FLAKY: try 1 panicked at real_browser.rs:539 after 7.0 s, try 2 passed in 5.0 s) and `spotter-render::real_browser
  a_download_address_is_reported_as_a_download` on macOS (job 44768, app spotter macos-latest; FLAKY: try 1 panicked at
  real_browser.rs:318 after 120.3 s, try 2 passed in 2.0 s). Next: a failing reproduction for each first.
- **A push gate row not measured, cause likely but not proven (2026-11-22):** the first push of fix/member-gate-self-dispatch
  (0b1482f0b) was refused with `clippy-linux tools/paddock-dev` NOT MEASURED (10 passed, 1 not measured, 2154 s). The row's reason
  line was lost: the integrator filtered the push output to row summaries, and the row left no log under paddock-pre-push/logs.
  On the same commit with main merged in, under the gate that counts heavy queue time apart from run time (fix/push-gate-queue-stall),
  the row passed alone in 15.6 s after 223 s in the queue. Likely cause: the old gate's 15-minute budget spent on the heavy queue
  while other agents held the slot; not proven. Since then the integrator keeps the full output of every gate row.

## Debrief WebKit e2e on loaded runners: a navigation echo, the dock-planned budget and the Inspector slide (train 20, 2026-11-23)
- **Fixed: navigation-history.e2e.ts:83** (debrief.yml run 8636, job 45079, both tries; passed in 8643). After a Back or Forward, the
  range watcher saw the unchanged range again on any query change and recorded it as a new place, which dropped the places ahead; only
  the 600 ms echo window (`NAV_ECHO_MS`) hid it, so on a slow runner Alt+Right went nowhere. The watcher now reads the range alone, by
  value (63e95174b; unit test first). Users who filtered after going back lost Forward the same way. The echo window is still a wall-clock
  rule: recognising an echo by its cause instead is the class fix, not done.
- **Fixed: a11y.e2e.ts:6 (dark)** (runs 8297, 8300, 8373, 8426, 8547, 8556, 8643): axe read the Inspector's 140 ms slide-in half-way.
  Every axe scan now holds the page still (`withPageStill`, c51c1b39a; deterministic test that restarts the slide inside the scan).
- **Fix item, open: the dock-planned wait** (ai-vision.e2e.ts:96 in 8636 job 45078, both tries; ai-pitwall.e2e.ts:68 in 8559 job 44324).
  Both ran out on `[data-dock-planned]` with 6.2 to 7.1 s of the 15 s load budget left, in shards whose tests averaged 15 s. Not
  reproduced on the Mac (0 of 80 WebKit runs, two of them with 12 CPU burners). The wait is now bounded on the dock's progress
  (`data-dock-progress`, first wait = what is left of the load budget), and the dock logs `ui.layout` "Plan ran" / "Planned". Whether the
  dock was slow, stuck or looping in those runs is NOT confirmed (ER-28): the next failure's log, with those lines and the wait's
  "stuck" / "keeps re-planning" message, settles it. Close this item then.

## A dependency of a shared crate moved the workspace hack, serde_json's features and every lockfile, twice (2026-11-23)
- `toml`, then `oxc` and `syn`, added to `crates/paddock-repo-graph`: `cargo hakari generate --diff` went non-empty, `oxc_resolver` forced
  `serde_json/preserve_order`, every app's `Cargo.lock` changed and Clippy broke in paddock-mcp under `--all-features`. CI saw each one
  after the push; the push gate had no hakari row (`NOT_RUN`) and no check of resolved features. Recurring class: a shared crate's
  dependency graph is an input of every app, and nothing read it. `./dev locks guard` reads it (the hack, the pinned features of the
  watched crates, the lockfiles) in the push gate and CI. Next: flip it to blocking after its first green CI run (`locks_guard::MODE`).

## 2026-11-23: the heavy queue admitted one step whatever it cost (feat/heavy-queue-admission)
Pre-push rows that ran in about 1 s on a warm compile cache waited 270 to 948 s behind long steps, and several pushes waited 15 to 85 min. The slot
rule followed the 15-minute load only, and that load has been 16 to 48 on 14 cores for days, so it gave 1. Fix, in `tools/heavy`: memory is a limit
beside the load (`MEMORY_RESERVE_MB`, `MEMORY_PER_SLOT_MB`), a row the history says takes under 10 s (`HEAVY_ROW`, `heavy-history.tsv`, bounded) takes a
place in a quick lane beside the full slots, each step gets `CARGO_BUILD_JOBS` from cores / most steps at once (ER-48), and each admission is
logged with what the rule saw (ER-27). Lesson: an admission rule needs a cost, not only a count; a row's key for history is the row, never the run's name.

## The CI night of 2026-11-22/23: the planner marked everything, and nine more causes (2026-11-23)
- **The planner marked everything for plumbing changes** (runs 8610, 8635, 8647; about 700 to 730 job-minutes a train round).
  Train 20 spent six full rounds, each rerunning every app's suite. Two causes: `.gitea/actions/README.md` was on the planner's
  global list, and a train dispatch has no base, so `plan_ci::choose_base` planned the whole tree (ADR 0113's whole-tree landing).
  The user stopped all CI (runs 8654 to 8661 cancelled), paused the nightly and chose graph first. **Covered by:** ADR 0117 (one
  repo graph with declared inputs per node, an unowned or unresolvable reference selects everything, and a landing run that tests
  the train's diff from main's last complete commit). Class "default owner or reach fails open" (2nd instance).
- **The verdict reader found no complete base, so every push to main planned everything** (push run 8664 cancelled). `last_full`
  needed a successful push run, and there was none since 8045 on f7bbe2b86 (2026-11-12); landings accept a FLAKY-only red (ER-12)
  while the reader needs success, so two definitions of green existed (ER-15). **Covered by:** ADR 0117 WP-4, one typed outcome
  shared by the landing and the reader; tags still need Green.
- **Local sccache was off for every agent from train 19 to 2026-11-23.** `local_sccache.rs` `level()` took the first "L0 (disk)"
  line, which is a counter line ("hits 0"), so the tiered stats never matched. CI was fine (run 8647: sccache-backend s3, 293 of 293
  hits). **Fix:** d04af7b46 (the reader takes a level's description line). **Covered by:** ADR 0090 revived, so a parser of another
  program's output is tested against captured real output (register `docs/captured-output.toml`, check C104).
- **An expired review date blocked every push.** The C50 `review_by` (Dyno shell start, ADR 0029) passed on 2026-11-22, and
  RUSTSEC-2026-0592 had done the same the day before. Each moved by a `Gates-date:` trailer to 2026-12-07 by the user's approval.
  **Covered by:** the approved time-based gates item (ER-12 amendment): a date warns on push, blocks only a push that touches that
  entry, tags and releases, and is reminded a week ahead.
- **The heavy queue held 1 s rows behind 15 to 60 minute waits** (push gates stalled 30 to 85 minutes, three times that night).
  **Covered by:** heavy admission by load and memory, with a quick lane for rows under 10 s (feat/heavy-queue-admission e6e5bfaed;
  it needs a `heavy-v*` tag and `./dev heavy-install` to take effect).
- **Long test hangs came from plain `cargo test`.** `worktree::list_clean` hung 3.5 h at 0% CPU, `ci_isolation` 18 min, and test-spawned
  `ci slot hold` processes were orphaned four times. Agents had run `cargo test --test it`, which has no nextest timeout and no
  progress watchdog (both exist). **Covered by:** rule 89, every test run goes through the bounded runner, with a hook that steers
  agents off plain `cargo test`.
- **Generated files went stale after text merges** (3 instances: train 20 ci-run golden, runs 8633 and 8634 cancelled; train 19
  `tools/paddock-dev/Cargo.lock` conflict 73f151535; train 19 notices after the notify 9 bump, 96ee947c5). **Covered by:** merge-time
  regeneration (feat/merge-regenerates) and, in ADR 0117 WP-7, generators as graph nodes with an input hash.
- **Members based on an old main conflicted** (4 times). **Covered by:** `./dev worktree add -b` bases on the open train head by
  default (feat/worktree-base-open-train 8ce4c9327).
- **d04af7b46 landed on scoped run 8663 by the user's exception.** No complete verdict exists for it and nothing is tagged from it;
  the next complete run covers it. This is a recorded exception, not a precedent.
- **Class: a default that fails open** (the global list, the base-less dispatch, the unowned file; 2nd instance) and **a gate that
  waits on a date nobody moved** (RUSTSEC-2026-0592, C50; 2nd instance).

## A test cleanup ended other jobs' processes on the shared Windows host (run 8674, 2026-11-23)

- **What happened.** Within 8.5 s on the Windows host (parallel slots s1 to s3), a Dyno test process (13:11:40.48), a Pitcrew test
  process (13:11:41.06), the crate job's cargo/nextest (13:11:44.39) and Pitcrew's own nextest (about 13:11:49) ended with exit code 1
  and no output: what `taskkill /F` gives (`TerminateProcess(h, 1)`).
- **Cause (identified, not reproduced on Windows, ER-28).** Pitcrew's supervision harness (`tests/supervision.rs`,
  `kill_leftover_processes`, run on every test's drop since 69a0c7cc7) took one look at the process table, added every process whose
  parent pid was in its set (no start-time check), and then ended each pid through sysinfo's `Process::kill`, which on Windows runs
  `taskkill.exe /PID <pid> /F` once per pid, serially. The set was mostly the fake runner's job processes, which `release_job()` had just
  ended, and Windows hands a pid on at once; the other slots' nextests start a process per test. Each victim fell within about a second
  before a supervision test ended (40.03, 41.08, 45.33, and the four tests still running at 49.0). The file's blanket
  `#![allow(clippy::disallowed_methods)]` hid the ADR 0033 ban on `sysinfo::Process::kill`. Not seen before because the Windows jobs
  only now run in parallel slots: a stale pid used to land on the same job's processes.
- **Same class in shipped code.** Pitcrew's `launch::kill_tree` and its tool-tree fallback ran `taskkill /PID <pid> /T /F` (`/T` takes
  every process whose parent pid matches, strangers that inherited a reused parent pid included), and an adopted runner was ended by a
  bare pid recorded earlier. In paddock-proc, the table joined sysinfo's parent pid with a start time read later, so a pid handed on in
  between paired a stranger's identity with the old parent link, and `kill_tree` would end it (the age check in `descendants` cannot see
  that).
- **Fix (fix/proc-tree-pid-reuse).** paddock-proc: a table entry's parent and identity describe one process (Linux: one `/proc/<pid>/stat`
  read; Windows and macOS: a start time counts only if earlier than the list, read on Windows' coarse clock); every end logs pid, start
  time, program and parent chain (ER-27); `processes_inside` finds a harness's processes by identity. Pitcrew: the harness, the runner and
  tool trees and adopted runners end by identity through paddock-proc; `taskkill` is gone from pitcrew-service.
- **Recurring:** ADR 0033's class (ending a bare pid read earlier, following parent pids without age): Chromium driver stale-snapshot
  kill (ADR 0033 regression test), Windows stale-pid kills (rule 53). Open: blanket `disallowed_methods` allows in test files hide the
  ban; a check that refuses the ADR 0033 entries under a file-wide allow is the follow-up.

## A runner counted another holder's orphans as its job (run 8694, 2026-11-23)

- **What happened.** In `app pitcrew (windows-latest)` (job 45770), `usage_follows_the_runner_and_the_processes_it_started` timed out
  on try 1 waiting for "the exited child is no longer counted" (40 s of real time, 51.1 s in all) and passed on try 2 (11.6 s). The
  status it printed showed the runner `Busy` with `usage.processes: 178` and `mem_bytes: 4211302400`: not the fake runner's one
  exited child, but a large tree that was never the runner's.
- **Hypotheses and what ruled them out (ER-28).** (1) The usage table keeps an exited pid until the next snapshot: the table is read
  again every probe tick, and one stale child gives 3 processes, not 178. (2) Windows keeps an exited process visible while a handle is
  open: the same count rules it out, and `Busy` needs a job-named child. (3) The refresh never re-ran: `until` pumps 20 s steps over
  40 s of real time, several probe ticks; the job stayed `Busy`, which is a fresh `has_job_children` answer each tick. (4) The train's
  paddock-proc snapshot change: Pitcrew's usage tree did not use paddock-proc's table at all. (5) Kept: the probe's `tree_members` and
  `has_job_children` followed sysinfo's parent pid with no start-time check (ADR 0033 WP-6, open since 2026-10-30, waived in C53). On
  Windows a parent pid outlives its process; the orphans of an earlier holder of the runner's pid (a detached server and everything it
  runs, other slots compile on the same host) named the runner as their parent. Try 2's runner got another pid.
- **Fix (fix/usage-exited-child-race).** paddock-proc gains `parent_of` (a link only from an older to a newer process, the rule
  `descendants` and `tree_fold` use, now one function `follows`), and `listing_started` plus `table_of_listing`, so a caller that reads
  its own list pairs each parent with an identity as `snapshot` does. Pitcrew's probe builds that table on every refresh; the usage
  tree, `has_job_children` and the process metas take the parent only through `parent_of`. The two WP-6 waivers and three gone C53
  baseline sites are removed. Regression tests fail on the old link: `a_parent_link_counts_only_when_the_child_started_after_its_parent`
  and `a_runner_whose_pid_was_handed_on_does_not_count_the_old_holders_orphans` ("the exited child is no longer counted", left
  `[10, 20, 21, 22]`). Identified from the job's own status line, not reproduced on Windows: Unix reparents orphans, so the link cannot
  go stale there.
- **Recurring:** ADR 0033's class, third time in two days (run 8674's harness kills, the paddock-proc pairing in 682208725, this). The
  waived WP markers are the known sites; nothing finds a new parent-pid walk. A Clippy `disallowed-methods` entry for
  `sysinfo::Process::parent` outside paddock-proc would make the class checked.

## The ownership test timed out on Windows a second time: the work was parsing in a debug build, not process starts (run 8694, 2026-11-24)

- **What happened.** `repo_model::tests::every_tracked_file_of_the_tree_has_an_owner_...` was FLAKY on the Windows runner (run 8694,
  job 45791): try 1 stopped at 60 s, try 2 passed in 18.2 s. The first fix (e348851aa) had already removed the OS placement from it.
- **First hypothesis, and what refuted it.** Process starts at about 45 ms each on Windows. Counted with `paddock_env::starts`: 29 starts
  (25 `cargo metadata`, one per workspace, and 4 git), about 1.3-2.4 s on Windows, the same for any number of files. Not the bulk.
- **Root cause (measured, docs/benchmarks.md 2026-11-24).** CPU work in the debug test build: oxc parsing about 2,200 JS/TS files without
  optimisation (0.83 s), syn parsing 428 Rust sources with each macro's tokens parsed twice, owner lookups that visited all 4,628 model
  nodes per changed file three times (0.30 s), and 137 dynamic script words matched against all 14,291 paths twice each (0.33 s).
- **Contributing cause.** The test reads the repository graph (25 `cargo metadata` runs) but was not in the ci profile's `repo-graph`
  group, so under load it ran beside the other tests that do the same.
- **Fix (fix/owner-test-windows-cost).** oxc crates optimised in the dev profile as syn already was; a macro's tokens parsed once; syn
  parses in 4 threads; a file index for the model's owners (shared crate, for every plan); glob matching over a sorted set by literal
  prefix and literal run (shared crate). CPU 7.35 s to 5.10 s, wall 22.1 s to 10.9 s median on the loaded Mac. The test now pins the
  start budget (workspaces + 4) and joins the `repo-graph` group. No limit was raised.
- **Why the logs did not show it.** nextest gives one duration per test; the phases are not logged. The measurement needed temporary
  timers. Not changed here: a whole-tree test has no phase log.
- **Class.** A whole-repository test whose cost is per file times per item, or a parser in an unoptimised build: C102's syn parse
  (2026-11-09, the syn `opt-level` in tools/paddock-dev/Cargo.toml), check_c122's quadratic walk (2026-11-18, .config/nextest.toml), this
  test's OS placement (e348851aa, 2026-11-23) and now its parsing and lookups.

## Two Debrief vitest specs timed out: the frontend job ran 29 Vitest workers outside the lane budget (run 8753, 2026-11-24)

- **What happened.** debrief.yml run 8753 (train/2026-11-23-graph, 27a7dd840), job 46355: `OutliersView > says so when nothing stands out,
  and shows engine errors with Retry` (20.8 s) and `TracesView > reports a trace without spans and failures` (21.9 s) passed the 20 s
  test timeout; the leak detector named `settle()`'s `setTimeout(0)` (helpers.ts:191) as live. Run 8695 on 727810fe9 was green, with no
  change under `ui/` or `apps/debrief` between the two commits (`git diff --quiet`).
- **Hypotheses and what ruled them out (ER-28).** (1) A wait that never re-checks (train 20's `until()`): neither spec uses `until` or
  `settleUntil`, and `until.ts` was already in 8695's tree. (2) `settle()` looping: it runs a fixed number of rounds; the live timer is
  that of a test still running, not of a hang. (3) Fake timers leaking from an earlier spec: the tests before and after each failing test
  in the same file and worker passed through the same `settle()` (4 to 11 s); two shuffled full runs (seeds 8753 and 4242, 5 workers) had
  neither test fail and no timeout. (4) A signal the stub never emits: both pass locally every run and took 7.8 s in 8695. (5) Kept:
  CPU starvation from the job's own worker count. The frontend job has no build-jobs step, so `VITEST_MAX_WORKERS` was unset and Vitest
  used `availableParallelism() - 1` = 29 workers on d-gtr9 (30 cores, 6 lanes; the other five ran rust tests and four e2e shards in both
  runs). The whole suite slowed (median 1.15x, p99 4.4x, eight tests over 15 s, a third at 20.75 s); these two mount the analysis
  workspace two and three times, so they sit closest to the bound.
- **Measured (docs/benchmarks.md, 2026-11-24 under "Worker budget").** Full suite with coverage on the dev Mac: at 26 workers the two
  tests took 4 to 6 times as long as at 5 (medians 4.96 and 5.49 s against 0.86 and 1.28 s), while the suite's wall time hardly changed.
  20 runs at 5 workers: no failure, slowest test 3.5 s. Not reproduced at the real 20 s bound on the Mac (it would take about four times
  the oversubscription of a shared machine); confirmed only at CI's headroom, scaled.
- **Fix (fix/debrief-vitest-timeouts-8753).** build-jobs in debrief.yml `frontend` and `e2e` and nightly.yml `dialog-commands`, with the
  tag rule for ./dev in their env. A conformance test (ci-actions.test.mjs, "every job that starts a Vitest or Playwright worker pool
  takes the job's core share first") reads every workflow with the yaml package and fails on a pool step (`ng test`, `vitest`, `playwright
  test`, `npm test`, `./dev ci run`, app-gates) with no unconditional build-jobs or setup-rust before it; it failed on exactly these three
  jobs first. No timeout was raised and no spec changed.
- **Why the logs did not show it.** The job prints neither its worker count nor the host's cores: build-jobs is the step that logs both.
- **Class.** ER-48, a worker pool outside the lane budget: cargo per lane (runs 8335 against 8346, 2026-11-20), Windows 3 x 8 threads
  (8335 against 8359), Vitest and Playwright defaults added to build-jobs (a080f848a, 2026-11-22) without a check that every pool job runs
  it. The new test is that check, beside the cargo one ("every job that runs cargo sets its build jobs").
- **Seen on the way (fix item, not this change).** Shuffled order (`sequence.shuffle`, seeds 8753 and 4242) fails 8 and 11 other Debrief
  tests, all order-dependent (native-feel.spec.ts, app.spec.ts, palette.spec.ts, timeline-panel.spec.ts, testing/startup.spec.ts), none a
  timeout. ER-29's nightly shuffled run would show them.

## The graph train's landing runs (ci.yml 8830, debrief.yml 8831 on 47a27bf1f): a graph reader outside the repo-graph group again, and fix items (2026-11-24)

- **What happened.** `repo_model::code::tests::on_the_real_tree_every_read_candidate_parses_with_syn` was FLAKY on the Windows runner
  (job 47457): try 1 stopped at 60 s while the ownership test, which reads the same graph, was running beside it; try 2 passed in 23.4 s.
  It blocks: repo_model is new in the train (ER-12).
- **Cause.** It reads the repository graph (`plan::read_graph`: 25 `cargo metadata` runs, one per workspace) but was not in the ci
  profile's `repo-graph` group, so under load it queued on cargo's lock beside the other readers. The ownership test's identical failure
  in run 8694 was fixed by joining the group, one test by name; nothing found the others.
- **Fix (fix/graph-train-r5-repo-graph-group).** Every test that runs `cargo metadata` joins the group, found by measurement, not by
  reading call chains (`read_graph` is reached through `plan_for`, the generators, the gates and the `./dev` binary): a full ci-profile run
  under a logging `cargo` found 15 tests outside the group, 7 of them over the real tree (docs/benchmarks.md, 2026-11-24, "which
  paddock-dev tests run cargo metadata"). After the change, 0 of 1028 calls come from outside the group. No limit was raised.
- **Class (ER-21, second instance).** A test that reads the repository graph but is outside the `repo-graph` group: the ownership test
  (run 8694, fix/owner-test-windows-cost) and this one (job 47457). Proposed check, not built here: nextest exports `NEXTEST_TEST_GROUP`
  and `NEXTEST_PROFILE` to every test (and a `./dev` the test starts inherits them, ER-47), so the one place that reads the graph
  (`read_graph_placing`) can refuse to run under the ci or member-audits profile of paddock-dev unless the group is `repo-graph`. A new
  reader then fails on its first run on every OS, with the next step named, instead of flaking under load. About half a day with its
  tests and positive control. Open design points for the coordinator: it puts a test-profile condition into the tool's code; and it
  must start report-only (ER-41), but nextest does not show a passing test's output, so a report-only warning would go unseen.
- **The macOS timeout, blocking (fix/graph-train-conformance-line-test-cost).** `ci_run::tests::adding_a_check_changes_no_golden_row_
  but_the_rest_of_the_conformance_line_does` was stopped at 60 s on both tries in job 47411 (H') and again in the full run 8843 (job 47644,
  fe331ca55), each time beside two heavy jobs on the same Mac (47415 and 47396; then 8843's WebKit reference apps and 8844's macOS rust
  tests). Alone on the same runner it took 12.6 s (run 8838). C167 ruled out. Two parts: the host layout (8 test threads on a 4-core share,
  heavy jobs beside each other; feat/test-cost-classes, ADR 0121) and the test's own work. Its work, measured by CPU time (docs/benchmarks.md,
  2026-11-24): 91% building the CI model, four whole builds, though it reads ci.yml's rows only; the train made each build 11% dearer
  (the steps it added to the workflows, times the matrix legs: real work, nothing repeated). The cut builds from ci.yml alone, with one full
  build kept as the control that no row is lost (positive control: the control fails when the narrowing keeps no workflow): CPU 8.7 s to
  4.9 s median, below main's 10.5 s. No limit raised.
- **Fix items from the same runs, not blocking:**
  - Debrief `[chromium] e2e/annotations.e2e.ts:92` "plugin marker kinds are listed read-only with their owner, can be switched off and
    restyled", FLAKY (job 47419): `toHaveClass(/off/)` on the `net.decryptology.windows/app-crash` row of the "Plugin marker kinds" table
    found no element (line 102), 27.7 s; the retry passed in 23.0 s. Markers manager and MarkerProfile, which the train does not touch.
  - Debrief WebKit FLAKYs (job 47430, 1 of 133 runs each in the flake history): `e2e/about.e2e.ts:43` "Help menu opens it too" failed in
    `openApp` (e2e/app.ts:45, the timeline lane not visible within the 5000 ms floor of the load budget; 20.8 s, retry 15.0 s), and
    `e2e/home-narrow.e2e.ts:70` "nothing overflows at 320 px" failed in `openHome` (no `.finding` within 5 s; 13.3 s, retry 10.3 s). The
    train changes `openApp` only after line 46 (the dock wait), not the wait that failed.
  - Debrief `bundle (macos-latest)` failed in `hdiutil` (job 47424) after the Mac runner's desktop session crashed at 02:33Z; rerun on the
    final head.
  - The demo release workflows (`shell-demo-release.yml`, `workbench-demo-release.yml`) have no runner-wait first job, so a train's
    dispatch of them waits in `./dev ci dispatch` instead of in the run.
  - The train's worktree lacked `apps/debrief/.cargo/libduckdb.toml`, so the gen check needed `fetch-libduckdb.sh` by hand first.

## Whole-repository tests on a loaded host: a cost class per test instead of names in lists (ADR 0121, 2026-11-24)

- **Class (ER-21, seven instances).** A paddock-dev test that runs `cargo metadata` or reads the whole real tree fails a landing run on a
  loaded shared host with no product bug: runs 7787, 8138, 8694 and 8830 (two jobs), and the live-test waivers of 2026-11-11 and
  2026-11-18. Each was fixed by naming one test (a waiver, one more name in the `repo-graph` filter), and nothing found the next.
- **Two causes.** Cargo's package-cache lock (graph readers queue on it) and CPU: on the shared Mac a job has 8 test threads on a 4-core
  share (the ER-48 exception, measured for tests that mostly wait); whole-tree tests compute, so eight at once over-subscribe it.
- **Mechanism.** The test function's name declares its class (`graph_`, `tree_`); one class table in `.config/nextest.toml` gives each
  class its group, two threads per test and its budget; every `cargo metadata` start charges the graph class at run time (reported under
  every profile; it refuses only from the step that records its first green CI run, ADR 0121 D3); `./dev ci test-cost` reports an unclassed test over a quarter of
  its stop from nextest's own output.
- **Lesson.** A list of test names is a record of the last failure, not a rule: declare the property on the test, put the policy in one
  place, and check the declaration by what makes it true (the lock's group at run time, the cost from the run's own timings).

## Active apps: train and branch CI run the foundation only (feat/app-ci-pause, 2026-11-24)

The user chose to run the foundation first and bring the apps back one at a time (docs/ci-active-apps.toml, docs/ci-schedules.md
"Active apps"). The first design paused apps per tier in each `paddock.ci.json`. On the same day it was replaced by one active set
that the nightly ignores: one file the coordinator edits, and no tier `if:` lines in five workflows (KISS). **Lesson:** a switch that
many readers need lives in one file with one accessor; a per-app field would have been read by every reader through every contract.

**Open fix item: the nightly does not count as an app's full green run for its tag.** `ci verdict` reads only the named workflow's runs
(ci.yml for a leg app), and the nightly's ci.yml call runs inside nightly.yml. Until the reader also reads the nightly's called
ci.yml jobs (or the nightly writes a verdict of its own), a tag of a ci.yml-leg app needs the app active and a green ci.yml run on the
commit (`ci verdict --tag --app`). debrief.yml's own nightly run already counts for Debrief.

**Open fix item: the train dispatch guard does not read the nightly yet.** `active_apps::classify_nightly` sorts a red nightly into
train-stopping failures (foundation, active apps) and inactive apps' fix items; the green-main guard that refuses a dispatch on the
first and records the second (`ci fix-items`) is part of the queued green-main work.

## The long paddock-dev test hangs had one cause: a test moved its process into a temporary folder (2026-11-23, fix/bounded-test-runs)

- **Cause.** `check_c129::a_control_holds_in_a_repository_whose_lists_differ_from_its_own` called `std::env::set_current_dir` into a
  temporary repository (since 91db3611f, 2026-11-16), relying on nextest's process per test. Under a plain `cargo test --test it`
  every test shares one process, so once that folder was removed every later child (git, node, bash, the tool itself) started in a
  deleted folder. git's start-up calls getcwd, and macOS's libc then falls back to reading the parent folder entry by entry and
  `fstatat`-ing each one, looking for an inode that no longer exists. The parent is the user temp folder, which held about 992,000
  entries (about 71% of a 400,000-entry sample are `x9-fixture-*` folders from before the conformance helper fix, dated 2026-11-20).
  Each git start took 20 s to over 2 min of disk work, 14 at once (disk0 at about 32,000 operations a second, 125 MB/s of 4 KB
  reads). That was `worktree::list_clean` at 3.5 h (the test process waits at 0% CPU while its git children are in disk wait),
  `ci_isolation` at 18 min, and the live-tree checks, `build_jobs` and C130's git-repository tests failing together.
- **Reproduced** (plain libtest harness, 14 threads, the it binary run directly): the full suite stalled at 81 and at 142 finished
  tests, with 14 `git rev-parse` children in state U and `sample` showing `init_git -> strbuf_getcwd -> __private_getcwd -> fstatat`.
  A/B with `--test-threads 1`: `worktree::list_clean::clean_dry_run` alone took 1.1 s; after the C129 test it was stopped at 300 s;
  with the fixed test it took 1.0 s. The full plain run with the fix finished in 161 s (723 passed; the 8 `pre_push` failures are the
  missing `CARGO` variable of a binary started without cargo, not a hang).
- **Fix.** The test runs the built binary with `current_dir` set to the repository and reads the control from its JSON report.
  `std::env::set_current_dir` is now a disallowed method in `lint/clippy.toml` and every copy (positive control: a planted call fails
  clippy). Rule 89's hook (`no-plain-cargo-test`) refuses plain `cargo test` (except `--doc` and `--no-run`), so a test's process-wide
  change cannot reach another test.
- **Orphaned slot holders.** `ci slot hold` processes started by a test lived on after nextest stopped the test, because their job
  folder (a temporary folder) was never removed and their cap is 3.5 h. Every TestCommand now sets `PADDOCK_LIFELINE_PID` to its test
  process (`paddock_env::lifeline`), and the holder ends at its next look when that process has ended (`HoldEnd::LifelineGone`). The new
  test was red first (60 s, "slot s1 never became free") and is green in 0.6 s.
- **Two tests that were too expensive under load.** `os_scoped::a_new_skip_or_only_entry...` ran C155 five times over a copy of
  `crates`, `apps` and `tools/paddock-dev`. It now copies only `crates`, as do its three siblings that only touch `crates`: 31.5 s to
  9.4 s at load about 25. `ci_run::tests::adding_a_check_changes_no_golden_row...` built the whole CI model four times (2.2 s each in
  a debug build). It now builds the edits from ci.yml alone, after checking that this gives the full model's rows: 8.7 s to 5.0 s.
- **Not done.** The temp folder still holds about a million stale entries. Removing old `x9-fixture-*` and other `paddock-*` folders
  is the user's housekeeping decision. Other suites still make temp folders that a killed run leaves behind (`paddock-isolation-*`
  about 17,000 in the sample, from the shell tests of job-isolation.sh).
- **Open fix item: `worktree::concurrent::two_adds_of_one_path_make_it_once` lost a round with exit 1 instead of 4.** In 1 of 5 full
  nextest runs at 4 workers under load (15-minute load 21 to 30, 4 CPU burners, sweep run 1 of fix/bounded-test-runs), round 2 ended
  `[0, 1]`: one add won and the other failed instead of being refused. Stressed alone it passed 60 of 60 (4 workers, load about 17) and
  120 of 120 (8 workers, 6 burners, load 31 to 37), 720 rounds in all, so the cause is not confirmed (ER-28). The test discarded both
  programs' output, so the loser's error was lost; it now prints both outputs in the failure. A further stress with that output in place
  (fix/bounded-test-runs-2, 125 iterations x 4 rounds = 500 rounds, 8 workers, 6 CPU burners, load 15 to 31) passed 500 of 500, so 1,220
  rounds alone have not caught it. Next step: read the loser's message from the next failure.
- Sweep after the fixes (fix/bounded-test-runs, Dev Mac, 4 workers, 4 CPU burners, the run starting only at a 15-minute load under 30):
  6 full nextest runs of paddock-dev. 5 passed in full (2580 or 2587 tests, 277 to 544 s); 1 had the failure above. No slot holder
  of ours was left after any run (3 `ci slot hold` processes after run 3, while the train's CI shared the Mac, are not attributed).
- Class "process-wide state changed by one test reaches the others under the plain harness": 2nd instance (1st: `set_var`, ADR 0100
  amendment, 9ac185c24). Covered by the clippy bans on both and by rule 89's process per test.

## The graph train's landing (ci.yml 8869 on cdd4c3d10, push run 8888 reused it): landing notes and fix items (2026-11-24)

- **Landed.** ci.yml run 8869 on cdd4c3d10 was full green, and the push to main (run 8888) reused it (ER-49). Train 2026-11-25-a records
  8869 as ci.yml's first green scope run (`./dev ci scope-record --workflow ci.yml --run 8869`, docs/ci-scope-first-runs.toml) and deletes
  the plan job's old `plan verdict:` block in the same commit, so `./dev ci verdict` now decides ci.yml runs on the declared scope (ADR
  0113, ER-41). The first `scope-record` at 15:15 was run outside the train's tree and was lost; it was run again in the train.
- **A since_run round never reruns a job whose `needs` passed in the original run, nor anything that depends on it (class; fix item).**
  The plan job of debrief.yml 8839 (on fe331ca55) selected `bundle (macos-latest)` ("the whole job reruns"), but `bundle` needs
  `rust-lint`, which a round skips because it passed in 8831, and a job with a skipped need is skipped, so the bundle never ran (all three
  legs skipped). A survey with the YAML parser of every workflow that takes `since_run` (ci.yml, debrief.yml): ci.yml has no job that
  needs more than `plan`; in debrief.yml `bundle` is the one selectable job with that shape (`tauri-smoke` has the same kind of needs, but
  its `if` already tolerates a skipped need, the model for the fix); `release` and `nightly-mark` run only on tags and schedules. The
  full run 8844 gave the bundle verdict instead.
- **A since_run filter that selects nothing must fail closed, not warn (class; fix item).** The round ci.yml 8868 (since_run=8843, on
  cdd4c3d10) reran `paddock-dev (macos-latest)` with `test(=ci_run::tests::adding_a_check_...)`, but ADR 0121 (9a4d16f9b, in the same
  round) had renamed it `tree_adding_a_check_...`. The workflow warns on a filter that matches nothing, so the leg would have passed
  without running the test. 8868 was cancelled and the full run used instead. Fix: map failed tests through renames, or refuse the round
  with an `::error::` naming the missing test; the queued failed-only dispatch guard checks that the round ran each failed test.
- **Fix items, not blocking:**
  - `repo_model::code::tests::on_the_real_tree_every_read_candidate_parses_with_syn` took 29.8 s alone on the Windows runner (ci.yml 8838,
    job 47580) against its 60 s budget: half the budget with no neighbour. For the test-cost work (ADR 0121).
  - `./dev ci dispatch` prints no run id, with `--json` either; a caller finds its run again by head sha and workflow, against ER-51.
  - App failures in the landing runs, non-blocking by the user's decision of 2026-11-24 (app CI off until the foundation is done):
    `app spotter (windows-latest)` FLAKY `spotter-render::real_browser a_missing_element_times_out_as_failed` (8843, job 47674: panic at
    real_browser.rs:179 after 20.3 s, the retry passed in 17.4 s; the train does not touch spotter-render); Debrief WebKit
    `e2e/context-actions.e2e.ts:76` "mask dialog previews the replacement and saves" FLAKY (debrief.yml 8844, job 47663: the first row
    still unmasked within 5 s at line 90, 38.2 s; the retry passed in 26.5 s; the train touches only the dock and navigation services).
    debrief.yml 8844 was cancelled once it was no longer required; no installer runs.
  - The landing was judged on foundation jobs only: `./dev ci dispatch` and the plan had no switch to leave app legs out, so the normal
    ci.yml run was dispatched and its `app *` and WebKit reference-app jobs were not judged. feat/app-ci-pause (train 2026-11-25-a) adds
    that switch (docs/ci-active-apps.toml).
- **Seen while assembling train 2026-11-25-a (fix items):**
  - The lessons merge driver (`./dev merge lessons-driver`) puts back the blank line that union merge drops only before a `## ` heading.
    A section that starts with a plain paragraph heading (docs/rules-timing-and-report-visibility's "Measuring and reporting
    (2026-11-24)") lost its blank line in its merge into train 2026-11-25-a (the member was then taken out for C100: its rule text names
    `./dev bench`, which does not exist yet). Third instance of the class "the lessons file loses blank lines in a union merge" (2026-11-21
    section above; the driver fixed the `## ` shape only). Fix: restore the blank line before any block the union appended, or require
    every appended block to start with `## ` (conformance).
  - `./dev wait --pid <p> --max <d>` counts `--max` from the process's start, not from the wait's start: a wait with `--max 2h` on a gate
    that had run 1 h 51 min ended after about 8 minutes with "gave up after 2h".
  - Member 3's gate took about 4.5 hours, most of it queue time in the heavy queue (doctest rows queued up to 1624 s each, behind another
    agent's pre-push), and its gen row `notices-dyno` was NOT MEASURED because the train worktree had no `node_modules`: `./dev worktree
    add` does not run `npm ci`, so an integrator must run it before the first gate.

## Assembling train 2026-11-25-b: fix items (2026-11-24)

- **A fresh worktree lacks what the gates need (third instance, class).** Train 2026-11-25-a's gen row `notices-dyno` was NOT MEASURED
  for want of `node_modules`; train 2026-11-25-b's `ts-debrief-wire` failed to link (`ld: library 'duckdb' not found`: no
  `apps/debrief/.cargo/libduckdb.toml`, written by `cargo xtask libduckdb`), and `publish-dry-run` was NOT MEASURED for want of
  `ui/dist/paddock-ui` and then `ui/dist/paddock-e2e`; the graph train had the libduckdb miss too (section above). Fix: `./dev worktree
  add --profile gates` prepares them itself (`npm ci`, `cargo xtask libduckdb`, `npm run build -w ui`, `npm run build:e2e -w ui`), or each
  gen row names the exact setup command in its NOT MEASURED line.
- **A gate row that runs nextest keeps only its exit code.** Both `member-audits` failures on this train (`exit 100`) named no test in
  the gate's log or JSON, so the profile had to run again to learn which test failed. Fix: the row keeps nextest's failed test names and
  their panic lines in its output and JSON.
- **A build folder inside the checkout.** A gate built tools/wiki into `tools/wiki/target` inside train 2026-11-25-b's worktree (319 MB);
  it showed as untracked once chore/cut-ci-and-gates deleted tools/wiki with its `.gitignore`. Builds never write inside the checkout
  (ER-51): the row that builds a workspace passes it a build folder outside.
- **Members fixed on the train (by the coordinator's call, small and deterministic):** feat/gates-active-apps failed C122 rule B in its
  new `tests/it/doctests_off.rs` (a `.replace('\\', "/")` path fold, both tries), fixed with `paddock_testkit::path::text`;
  chore/conformance-flip-and-drop's new C123 sample named the binary's env name as text, so the dev_bin audit failed, fixed by joining
  the name as C123's own samples do. Both were the members' own code, which their branch pre-push did not run (member-audits runs on
  the merged tree only).
- **Skipped:** feat/check-result-reuse (f67b7f6a1, based before the push gate's rework) conflicts in `pre_push/exec.rs`: its reuse hook
  calls the BudgetClock model that the train's push gate no longer has. It goes to train c after its branch merges main.
- **Train 2026-11-25-a landed on a narrowed run (ER-15; the coordinator's call, option A).** Its full ci.yml run 8983 on 627b28afc was
  green, but its plan declared `"ran":{...,"whole":true,"os_full":false,"smoke":["icons"],"affected":[]}` against
  `"need":{...,"whole":true,"os_full":true,"smoke":[],"affected":[]}` (inactive: every app): the train changed shared steps of ci.yml's
  `icons` job (`step:ci.yml#icons/1,2,6`), so the graph planner ran icons in smoke mode (phase `installed`). Main was fast-forwarded
  to 627b28afc on 8983 and the push run (8984) plans icons whole; `ci reuse` does not reuse 8983, and `ci fix-items` recorded nothing
  (8983 had no FLAKY test). Two fix items, queued for train c with the dispatch guards:
  1. `./dev ci verdict` prints an empty reason for a smoke-narrowed run: "only partial: 8983 ()".
  2. The landing run's scope and a push to main's need disagree for a job reached only through its steps (smoke in the run, whole in
     the need), so a landing run that ran exactly what its diff planned is never complete and the push to main runs again. That breaks
     ER-49's one definition of "complete" shared by the landing and the reader.
  - Train 2026-11-25-a's landing run ci.yml 8976 (plan job 48889) could not save the escape monitor's selection record:
    `::warning::the selection record of this plan was not saved (git push origin refs/paddock/plan-records/*:refs/paddock/plan-records/*:
    fatal: could not read Username for 'http://172.16.1.220:7400': No such device or address); the next nightly counts this commit as
    not measured`. The plan job pushes the refs with no credentials over the http remote; likely fix: push with the job token, the way
    checkout authenticates (to be fixed with the dispatch guards, train c).
  - The same plan printed its `::notice::inactive:` glued to the end of that warning: `eprintln!` writes a line's text and its newline
    as two writes, and the runner merged the notice from standard output between them. Fixed in this train (`log_text::write_line`,
    one write per line, for the plan and the eleven `|l| eprintln!("{l}")` log closures). Still open: the other `eprintln!` calls with
    inline formatting have the same two-write shape; a clippy `disallowed-macros` ban of `eprintln!` in paddock-dev's sources, with
    `log_text::eline` as the replacement, would cover them.
