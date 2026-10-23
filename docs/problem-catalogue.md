# Paddock problem catalogue

Every class of problem the Paddock apps have hit, what caused it, which rule or lesson covers it, and, above all, how it is
**enforced automatically** so that it is not left to memory. Written 2026-10-24 on the user's request ("ensure all problems we
faced are addressed across all apps"); rule 40 applies: a class that recurs gets a mechanism, not another patch.

Sources: [lessons.md](lessons.md), [rules.md](rules.md) (40 rules), each app's `docs/suite-lessons.md` and `AGENTS.md`, the
owner's notes (memory and briefs), and `git log main` of Debrief (213 fix/ci commits), Pitcrew (35), Pitwall (23), Spotter (23)
and Dyno (28). The per-app results are in [audits/](audits/); the shared check that closes most gaps is proposed in
[conformance-proposal.md](conformance-proposal.md) (check ids `C01` to `C27` below refer to it).

## How to read the tables

- **Enforcement class**: **S** = shared mechanism (a `paddock-ui` part, a Paddock crate, or a shared CI script that every app runs),
  **A** = per-app check (exists in some apps as a copy; the proposal moves it to S), **R** = review only (a person or agent has
  to remember; kept to the minimum and each one has a note on what would remove it).
- **Status**: `exists` (running in CI of the apps named), `partial` (some apps), `proposed` (described in the conformance
  proposal, not built), `ADR` (covered by a Paddock ADR in review).
- Classes marked **ADR** are the three UI classes of the Debrief 1.0.x round and are covered by Paddock ADRs 0009 (dialog kinds
  and declared contracts), 0010 (notifications and service health) and 0011 (layout by declared constraints), drafted on branch
  `docs/adr-ui-contracts`. They are listed here for completeness and not described again.

## 1. Build, bundle and release

| ID | Symptom | Root cause | Rule / lesson | Enforcement |
|---|---|---|---|---|
| B01 | A feature directory silently missing from git (`app/src/app/features/logs/`) | Unanchored ignore rule (`logs/`) | lessons: Build and bundle | **S** `C01` every directory under `app/src` and `crates/*/src` is not ignored (`git check-ignore`). proposed |
| B02 | Bundled app has no component styles (CSP nonce makes the webview ignore `unsafe-inline`) | Angular `inlineCritical` inlines `<style>` | lessons: Build and bundle | **A** `check-index.mjs` on the built `index.html` (Pitcrew, Pitwall, Spotter, Dyno; Debrief has none). `C02` makes it shared. partial |
| B03 | "Debrief.app is damaged" after download | Linker-signed bundle, no sealed resources | rule 23 | **A** `signingIdentity "-"` + `codesign --verify --deep --strict` on .app and DMG (Debrief, Pitwall; Dyno's `check-bundle.sh`; none in Pitcrew and Spotter). `C04`. partial |
| B04 | Store worker SIGABRT on macOS (Debrief 1.0.1): "different Team IDs" | Hardened-runtime ad-hoc sidecar refuses a bundled dylib that keeps its vendor's Developer ID (libduckdb); even an ad-hoc re-sign fails with the runtime on | lesson "A valid signature is not enough"; decision 2026-10-23 (runtime off while ad-hoc) | **S** `C04`: one TeamIdentifier across Mach-Os, `hardenedRuntime:false` while `signingIdentity` is `-`, every sidecar started from the built .app (`--self-test`), macOS real-app smoke job. **Exists nowhere on main**; Debrief has it on `fix/macos-store-and-dialogs`. proposed |
| B05 | Tauri build fails per target (`macos-private-api`) | Feature list and `macOSPrivateApi` must agree | lessons | **S** `C12` (config consistency). proposed |
| B06 | Frameless window cannot be moved | Missing `core:window:allow-start-dragging` | lessons | **A** unit test on the capability (Pitcrew); Dyno `check-capabilities.mjs`. `C12`. partial |
| B07 | Sidecar cannot find its native library | No rpath (`@executable_path/../Resources/lib`, `$ORIGIN`) or a build-dir rpath | lessons | **A** Dyno `check-bundle.sh` (reads every Mach-O/ELF rpath). `C05`. partial |
| B08 | Blank app icon (Pitwall 1.0.0: art covered 10% of the canvas) | Placeholder icon shipped | rule 25; lessons: Releasing | **A** `check-icons.mjs` (Debrief, Pitwall, Spotter; missing in Pitcrew and Dyno). `C03`. partial |
| B09 | Bundle fails with base64 "Invalid symbol 10" | Trailing newline in `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_UPDATER_PUBKEY` | rule 25 | **A** strip step in Debrief, Pitwall (`tauri-bundle.sh`), Spotter, Dyno; **missing in Pitcrew**. `C06`. partial |
| B10 | A release built from a `[skip ci]` commit, never verified | Gitea skips the tag run for such commits; docs-only skips hide broken code | rule 9; lessons: Releasing | **S** `C07` tag preflight refuses a tag whose commit is `[skip ci]` or has no green full run. Today only `needs:` in the release job (all apps except Pitwall's separate `release.yml`). partial |
| B11 | A fix exists but the next release ships without it (Debrief: 86 commits incl. the macOS fix only on `fix/macos-store-and-dialogs`, main is already "1.0.2") | Fix lives on a side branch; the version bump lands on main first | new lesson (below) | **S** `C07`: preflight fails when a `fix/*` or `release/*` branch has commits newer than the last tag and not in main, unless listed in `docs/release-waivers.md`. proposed |
| B12 | Release blocked overnight by a RUSTSEC advisory (Spotter) | Advisory database moves | rule 25 | **A** `cargo deny` in every app's CI and nightly `schedule`; `C07` re-runs `cargo deny check advisories` at tag time. partial |
| B13 | A stale installer from an earlier build collected into the release; wrong version attached | Persistent target folder on host runners | lessons | **A** clear `bundle/` and refuse another version's installer (Debrief, Pitcrew, Pitwall, Spotter, Dyno). `C08`. exists |
| B14 | Release publish fails (Go action on an image without Go; updater URL uses the runner's internal `server_url`) | Third-party release action; wrong base URL | lessons | **A** `scripts/release/publish.mjs` + `latest-json.mjs`, each with node tests, copied into every app. `C08` + move to one shared script. partial (copies drift) |
| B15 | Gatekeeper / SmartScreen warning surprises users | No Developer ID or certificate (deferred decision) | rule 25 | **A** "First launch" section appended by the release job (all apps; Pitwall only in `release.yml`). `C08` checks the notes. exists |
| B16 | Installer looks fine in `latest.json`, is broken on install | Only metadata was checked | lessons | **S** `C04` + `C05` + macOS smoke inspect the actual artifact. partial |
| B17 | Engine binary empty or not executable (Pitwall): `tauri-build` placeholder sidecar overwrote the real one; `copy_file_range` produced an empty file | Build-order and filesystem quirks on CI | Pitwall commits 9ca11b1, 2c95025, 6e3efa4 | **S** `C05`: sidecars exist, are executable, are real Mach-O/ELF/PE and larger than a floor. proposed |
| B18 | Merge noise: `messages.xlf` hand-merged, ADR numbers collide | Parallel branches | lessons: More findings | **A** `npm run i18n` check in Debrief CI; **S** `C24` unique ADR numbers. partial |
| B19 | Worktree builds fail (relative paths in a git-ignored local config) | `.cargo/libduckdb.toml` | lessons | **R** (Debrief only): use env vars; `scripts` helper below (P02). |

## 2. Runtime, windows, services and platform

| ID | Symptom | Root cause | Rule / lesson | Enforcement |
|---|---|---|---|---|
| R01 | Window opens blank and stays blank (Pitcrew reopen, hidden WKWebView) | Initializer awaited IPC with no timeout; window created hidden; no visible first paint | lessons: Platforms | **A** `PLATFORM_WAIT_MS` race (Pitcrew); splash in `index.html` (Pitwall, Spotter only). **S** `C02` requires the splash; a smoke test asserts first paint within N s. partial |
| R02 | Closing/reopening tray apps breaks (Dock icon does nothing) | Close must destroy the window; Reopen must rebuild | lessons | **A** per-app shell code and tests (Pitcrew, Pitwall, Spotter, Dyno). **R** for new apps until `paddock-window` exists (ADR 0006 lists it). partial |
| R03 | After an update the background service fails with EX_CONFIG (78) in a loop (macOS) | SMAppService keeps the old build's launch constraint while the launchd job lives; re-register over a live job does nothing | rule 24; lessons: More findings | **S** target `paddock-service` crate (unregister, `launchctl bootout`, wait until gone, register, once per version, `service_registered_version`); **S** `C21` and a macOS smoke that installs build N then N+1. Implemented per app in Pitcrew and Pitwall (main, unreleased); Dyno partial. partial |
| R04 | "Server not running" with no way to start it (Pitwall Settings > Service rendered only after the server loaded); installable engine listed as "planned" | A repair control that depends on the thing it repairs | rule 19 | **R** today. **S** `ADR 0010` (service health model with a Repair action that is part of the contract); `C10` tests that every health state offers Repair. ADR |
| R05 | Service cannot find tools (minimal launchd/systemd PATH) | No login-shell PATH | lessons | **A** per-app (Pitcrew, Pitwall, Debrief). `paddock-proc` target. partial |
| R06 | Windows: tool starts in the wrong session over RDP; extra `conhost.exe` counted as job leftovers; console windows flash | Session and process-tree rules differ | lessons | **A** clippy `disallowed-methods` for `Command::new` in all five apps (exists), session code in Pitcrew. exists/partial |
| R07 | Process memory wildly wrong on Linux (idle Chrome 17 GB) | sysinfo lists threads as processes; RSS summed | rule 26 | **S** `C22` + shared helper in `paddock-proc` (skip `thread_kind()`, PSS from `smaps_rollup`). Fixed in Spotter and Debrief monitor; Pitcrew/Dyno skip threads; Dyno sums per-app memory (check PSS). partial |
| R08 | Paths wrong when a pure function targets another OS | Used the host's `Path` | lessons | **A** unit tests with each OS's rules (Pitcrew, Dyno). **R** otherwise. partial |
| R09 | App does not quit / hangs on Quit (Debrief session-0 hang, WebView2 close stall) | Window close or a worker never returns after ExitRequested | owner notes (Debrief), new lesson | **S** `C20`: exit watchdog present (`arm_exit_watchdog`) and a real-app smoke asserts the process exits within 10 s. **Only Debrief has the watchdog.** proposed |
| R10 | Page fails against the real agent but passes against the mock (Dyno Network: internally tagged enums cannot be read by postcard) | Mock/contract divergence; serde feature limits on the IPC format | new lesson | **A** `generated_types` test (Debrief). **S** `C19b`: a round-trip test of every IPC DTO through the real codec; contract test that the mock returns values decoded from the real DTO fixtures. proposed |
| R11 | macOS Finder stalls when volume free space is polled often (Dyno) | Aggressive polling of an OS API | Dyno a068435 | **R**: note in lessons; budgets in the nightly benchmark (rule 16). |
| R12 | Tray popup looks like a web page in a box | Opaque square window | lessons: Platforms | **S** `paddock-ui` `tray-popover` component (Spotter uses it; Pitcrew/Pitwall/Dyno have their own). partial |
| R13 | Native feel wrong (selection, context menu, browser keys, font) | Defaults of the webview | lessons (Debrief ADR 0028) | **R**. Candidate: `paddock-window` crate/`paddock-ui` directive. |
| R14 | Window opens too small/large or off-screen; remembered position wrong | Hand-set sizes | rule 17 | ADR 0009 (window geometry in the dialog contract). ADR |

## 3. Long operations and performance

| ID | Symptom | Root cause | Rule / lesson | Enforcement |
|---|---|---|---|---|
| L01 | Progress stuck at 0 during discovery; one bar restarting (checksum looks like a second download) | Total unknown; phases not modelled | rule 11 | **S** `paddock-jobs` (phase-labelled, growing count, monotonic reporter). Not yet consumed by any app on main. `C19`. partial |
| L02 | No Cancel, or Cancel leaves half-written state | No cancel token; delete-then-build | rule 11 | **S** `paddock-jobs` `cancel`; **A** cancel-from-the-progress-callback tests (Spotter). partial |
| L03 | UI shows "did not answer" while the work carries on | UI call timeouts shorter than the engine's work | rule 12 | **S** `C19`: command names in a UI timeout table must not be job-starting commands. Pitcrew, Pitwall, Dyno still time out jobs (up to 600 s); Spotter fixed. partial |
| L04 | UI freezes during a command | Sync Tauri command runs on the main thread, or work on an IPC handler | rule 10 | **A** Debrief test that every command is async. **S** `C12` (Pitcrew 8 sync commands, Pitwall 6). partial |
| L05 | "found" count ticks backwards on Windows; flaky counts | Parallel workers report out of order | lessons | **S** `paddock-jobs` reporter (`fetch_max`); Spotter and Debrief have their own tests. partial |
| L06 | Downloaded file read again for its hash | Hash after write | rule 15 | **S** `paddock-jobs::hash` (hash while writing). Debrief fixed, Pitcrew `HashingWriter` discards the digest (audit). partial |
| L07 | IDs differ between runs/moved files get new IDs | Parallel commit order | rule 14 | **S** `paddock-jobs` `pipeline_ordered` property tests. exists (crate), adopted: none. |
| L08 | Machine overloaded / OOM (Mac load 240, 98k temp folders) | Unbounded parallelism | rule 13 | **S** pool sizing in `paddock-jobs::workers`; **R** agent concurrency (P03). partial |
| L09 | "Fits this computer" wrong; model spills to CPU | Flat size+1-2 GB heuristic | lessons: UX findings | **A** Debrief fit check from weights+KV+compute buffer; Pitwall has own. Candidate shared crate `paddock-fit`. partial |
| L10 | Slow start, slow scroll, memory growth unnoticed | No budgets | rule 16 | **A** Debrief `bench.yml`; others none. `C26` nightly budgets. partial |

## 4. User interface

| ID | Symptom | Root cause | Rule / lesson | Enforcement |
|---|---|---|---|---|
| U01 | Blank dialog windows, dialog denied its command, dialogs too tall/short/tiny, missing Close, inconsistent modality | 51 dialogs hand-configured | rules 17, 34, 40 | **ADR 0009** (kinds + declared contracts + a test that boots every dialog). ADR |
| U02 | Three notifications for one failure; "X not running" pointing at the page it is shown on | No single notification/health model | rules 19, 40 | **ADR 0010**. ADR |
| U03 | Each layout fix squeezes the next pane (AI pane 180 px, one-letter titles) | No declared min/preferred sizes | rules 18, 40 | **ADR 0011**. ADR |
| U04 | Overflow/clipping at narrow widths; paths cut at the end | Viewport media queries; no truncation rule | rule 18 | **A** Debrief overflow e2e; container queries: Debrief 6 files, Dyno 4, Pitwall 2, Pitcrew/Spotter 0. **S** `paddock-ui` truncation component + `C11`. partial |
| U05 | Suite UI mistakes repeated in all apps (filled button per row, cards for tables, mono headings, native selects) | Standard lived in a brief | ui-review-2 patterns (20 rules) | **A** Debrief `lint-styles`, Spotter `style-guard`, units/count guard. **S** `paddock-ui` components + stylelint/ESLint rules (proposed, `C09`); the rest **R** (design review) |
| U06 | Search cannot find an identifier written differently | Tokenisation | rule 20 | **A** Debrief tests; **R** elsewhere (Pitwall/Spotter/Dyno search boxes). |
| U07 | Skeleton waits forever; failed load shows "No watches yet" | No request status union | rules 30, 38 | **S** `withRequestStatus` in `paddock-ui` (not yet built; 35 `loading:` flags in Debrief). `C09`. proposed |
| U08 | Mock backend hides a feature that does not work (pasted sources, version `0.1.0`) | Mock drifts from the engine | AGENTS "The mock backend" | **A** Debrief mock derives version from the changelog; others hard-code. `C16`, `C19b`. partial |
| U09 | Accessibility regressions | No automated check | design guide | **A** axe in Debrief (100 e2e files); Pitwall 1; others none. **S** `paddock-ui` gallery runs axe; apps' e2e should too. `C17`. partial |
| U10 | Empty window while loading; no feedback for 400 ms | No splash | lessons: Platforms | `C02` (see R01). partial |

## 5. Tests and CI

| ID | Symptom | Root cause | Rule / lesson | Enforcement |
|---|---|---|---|---|
| T01 | 98k `debrief-*` folders (55 GB) in `$TMPDIR` | `env::temp_dir()` + manual `remove_dir_all`, skipped on panic | rule 7 | **S** `C14` run tests with `TMPDIR` redirected and fail on leftovers; `tempfile`/guard lint (`disallowed-methods` for `std::env::temp_dir`). Counts of raw `temp_dir()`: Debrief 159, Pitcrew 30, Dyno 16; Pitwall/Spotter use `tempfile`. proposed |
| T02 | 441 GB of Chrome code-sign clones filled the disk | Tests launched the installed Google Chrome and killed it | rule 7 | **S** `C15` + the pinned-browser scripts from Spotter (`pin-chromium`, `install-test-browser`, `*_REQUIRE_TEST_BROWSER`) moved to Paddock. Debrief: guard in `find_for_tests`, but skips silently and CI has no pinned browser. partial |
| T03 | Tests touch user data, launchd, real pipes/ports/keychain names | Production names derived without a test folder | rule 7; lessons | **S** `C14` (HOME/XDG/APPDATA redirected; fail if anything appears outside the scratch dir); **A** name derivation tests (Pitcrew). partial |
| T04 | A hard-coded `0.1.0` broke the What's new e2e; "Debrief 0.1.0" shipped in the status bar | Version typed into tests and mocks | rule 6 | **S** `C16`: no literal equal to the current version in specs; mocks import it. Debrief fixed; Dyno spec and Pitcrew/Spotter/Dyno mocks still hard-code. partial |
| T05 | Race tests (background write before read; cancel from a poller the work outruns) | Timing assumptions | rule 5; lessons | **S** `paddock-jobs` test helpers (cancel from the callback); **S** `C17` bans `waitForTimeout`/sleep in specs; repeat fixed flakes 20-50 times (**R**). partial |
| T06 | Retries hide flakes; reds ignored | Playwright `retries` 1-2 with no report | test-architecture-after-v1 | **S** `C17`: JSON report of retried tests into the job summary, count must not grow. proposed |
| T07 | 234 e2e failures from one locator change | Role/CSS chains, no testids | test-architecture-after-v1 | **A** Debrief 301 files with `data-testid`; others 0-18. `C17` (page objects + testids). partial |
| T08 | Tests pass only in one timezone | Local time in fixtures | lessons | **A** `TZ=Pacific/Auckland` run (Pitwall Rust, Dyno ng). `C18`. partial |
| T09 | Test binary picked with `ls -t` opened a blank window on the desktop; processes left behind | Wrong binary; no Drop kill | lessons | **A** MockRuntime for windows, harness Drop (Pitcrew, Debrief). **S** `C14` (process census before/after). partial |
| T10 | A build overwrote another checkout's artefacts; Playwright tested another agent's app | Shared `CARGO_TARGET_DIR`; `reuseExistingServer` | rule 2 | **S** `C17`: own port from env, `reuseExistingServer:false` in CI; **S** `paddock-worktree` helper (P02). Pitcrew, Spotter, Dyno reuse always. partial |
| T11 | Environment-bound tests silently skipped or stale | Skips without a record | rule 5 | **A** Debrief `docs/testing/quarantine.md`. **S** `C17` every `skip` needs a waiver entry with an expiry. partial |
| T12 | Browser tests fail in CI containers (root, no display) | Chrome sandbox, headful | lessons | **A** Spotter (`NO_SANDBOX` test switch, xvfb). `C15`. exists (Spotter) |
| T13 | Windows tests too tight (ping -n N, slow runners) | Deadlines | lessons | **R** + generous-deadline helper in `paddock-jobs` tests. |
| T14 | Coverage or unit-format guards break main (raw numbers in templates broke it 3x) | Not run before push | lessons | **A** Debrief units guard, Pitwall `ui-coverage-guard`. `C27`. partial |
| T15 | CI fixed push by push (40-50 min rounds); a quoting typo cost a run | No local pre-push check | rule 4 | **S** `paddock-prepush` (the lint/type-check part of the conformance script, runnable locally). **R** otherwise. proposed |
| T16 | Time-dependent tests (live tail, throttles, watchers) flaky | Real clocks | test-architecture-after-v1 | **S** `paddock-jobs::clock`; Angular fake clock (Debrief 43 files). partial |

## 6. Angular and SignalStore (rules 27-39)

| ID | Symptom | Root cause | Rule | Enforcement |
|---|---|---|---|---|
| A01 | Fields assigned in `subscribe` stop updating the view; zone.js creeping back | Zoneless without signals | 27 | **S** `C09` no `zone.js`/`provideZoneChangeDetection`, OnPush explicit. All five apps are zoneless today; Debrief has 1 component without explicit OnPush. exists by convention, unchecked |
| A02 | 165+ effects re-implementing derivations | `effect` for derived state | 28 | **S** ESLint preset (`@paddock/eslint-config`): effect that calls `.set`/`patchState` needs a justification comment. Debrief 168 effects, Pitcrew 18, Dyno 10, Pitwall 9, Spotter 9. proposed |
| A03 | `loading` + `error` flags disagree | Separate flags | 30 | **S** `withRequestStatus` in `paddock-ui` + lint banning `loading:` in `*.store.ts`. 0 users today. proposed |
| A04 | Stores global by default (310 `providedIn: 'root'` in Debrief) | Every window pays for every store | 31, 32, 34 | **S** lint: `patchState` only in `*.store.ts` (Debrief 2 violations), root allowlist, max two injected stores. proposed |
| A05 | Large first load; heavy panels eager | No lazy boundary | 35 | **S** initial-chunk budget in `angular.json` plus a release record; `@defer` use: Debrief 1. proposed |
| A06 | Copied components drift between apps | Not shared | 39 | **R** + `C09` flags duplicate file names against `paddock-ui` exports. Only Spotter depends on `@decryp7/paddock-ui`. proposed |

## 7. Security and robustness

| ID | Symptom | Root cause | Rule / lesson | Enforcement |
|---|---|---|---|---|
| S01 | Over-broad IPC permissions, `unsafe-eval` in CSP | Convenience | AGENTS: Security | **A** Dyno `check-capabilities.mjs`; **S** `C12` (wildcards, remote origins, `unsafe-eval`, `innerHTML`). partial |
| S02 | A panic or a poisoned lock takes down the app | `.lock().unwrap()`, parsers that panic | AGENTS: Robustness | **S** `C13` workspace lint set (`unwrap_used` warn: Pitcrew, Pitwall, Spotter, Dyno have it, **Debrief does not**), forbid `lock().unwrap()` via `disallowed-methods`. partial |
| S03 | Parser/plugin crash affects other work | No containment | AGENTS | **A** Debrief `guard`, fuzz and mutants workflows; Pacenotes/Sighter inherit the plugin model. exists (Debrief) |
| S04 | Private logs end up in fixtures, docs or commits | Confidential material pasted | owner's note | **S** `C25` fixture scan against a local denylist; **R**: invent same-shape data. partial |
| S05 | Dependency or licence regression | Drift | rule 25 | **A** `cargo deny`, `npm audit` in all CI; Renovate. exists |

## 8. Process and documentation

| ID | Symptom | Root cause | Rule / lesson | Enforcement |
|---|---|---|---|---|
| P01 | Six apps at once overloaded Mac and CI, releases half-done | Parallelism across apps | rule 1 | **R** (unavoidable: a human scheduling decision). Mitigation: the audits make "what is left" visible. |
| P02 | Two agents edited one checkout; worktrees overwrote each other's targets and ports | Shared checkout, target and port | rule 2 | **S** `paddock-worktree` script: creates the worktree, exports `CARGO_TARGET_DIR`, `PW_E2E_PORT`, refuses the main checkout. proposed |
| P03 | Load 240 from 7-8 agents | Too many heavy agents | lessons | **R**; the worktree script prints `uptime` and refuses above a limit. proposed |
| P04 | A blanket `docker stop` destroyed a user container | No ownership label | rule 7 | **S** label every container with `paddock.owner=<task>`; wrapper `paddock-docker`. proposed |
| P05 | Co-Authored-By / AI attribution appears | Commit templates | rule 8 | **S** `C23`: `commit-lint` rejects it (none of the five `commit-lint.mjs` does today). proposed |
| P06 | App `AGENTS.md` and `suite-lessons.md` lag behind the rules (only Debrief carries the rules; Pitcrew's lessons file is a 4-line stub) | Copy-by-hand | rules 8, 39 | **S** `C23`: the rules block and lessons copy must equal the Paddock revision pinned in the app (hash compare). proposed |
| P07 | Drawings and screenshots shipped unreviewed or stale | No gate | rule 22 | **S** `C24`: `docs/images/REVIEWED.md` holds the hash of every reviewed image; a changed hash fails. proposed |
| P08 | Release from a workstation | Convenience | rule 9 | **S** `C07` (only the tag workflow holds the secrets); **R** otherwise. partial |
| P09 | Tunables hard-coded; knowledge lists in code | Not Settings/TOML packs | rule 21 | **R** + `C09`-style grep for magic thresholds is impractical: reviewed in the audit; Settings schema test per app. |
| P10 | Stale model/engine catalogues | Catalogue not dated | lessons | **A** Pitwall/Debrief update scripts (`update-llama-runtimes.py`); nightly check that catalogue entries resolve. proposed |

## Summary

90 classes: 19 build and release, 14 runtime, 10 long operations, 10 UI (3 covered by ADRs), 16 tests and CI, 6 Angular, 5
security, 10 process. No automated check runs in all five apps today; the per-app copies (check-index, check-icons, codesign
verify, key strip, release scripts) exist in two to four apps each. The proposal moves them into one shared check (see
[conformance-proposal.md](conformance-proposal.md)). Review-only classes are P01 (unavoidable), R11, R13, B19, T13, P09 and the
unmeasured parts of U05 and U06; each names what would remove it.

The macOS Team-ID class (B04) and services-after-update (R03) are the two that shipped to users in a release; both are fixed
only on branches or on unreleased main commits (see the audits).
