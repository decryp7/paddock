# Audit: Pitcrew against the problem catalogue

Read-only, 2026-10-24. Source: `main` @ 1346f84 (version 1.0.2, not tagged), tag v1.0.1 (released). Severity as in
[debrief.md](debrief.md). Class ids from [problem-catalogue.md](../problem-catalogue.md).

## Gaps, by severity

| # | Sev | Class | Finding | Fix |
|---|---|---|---|---|
| PC1 | C | R03 | Released v1.0.1 has the Repair UI and stale-registration retry (`7db5934`) but not the fix that replaces the launchd job an update leaves behind (`768fd53`, "unregister, bootout, wait, register"); main has it and the 1.0.2 chore commit, untagged. Users who update 1.0.0 to 1.0.1 on macOS can still end with the agent failing EX_CONFIG. | Tag 1.0.2 from CI after the preflight; add the update-simulation smoke (`C21`) so the next regression fails before release. |
| PC2 | H | B03, B04, B16 | The `bundle` job has no `codesign --verify` and no bundle layout check; `scripts/macos-verify.sh` is a local post-install script only. Rule 23 unmet. `hardenedRuntime` is not disabled (low risk today: the sidecar is Pitcrew's own, but nothing proves it). | Port Pitwall's `macos-verify-bundle.sh` (+ Team-ID check) into the macOS bundle leg; start `pitcrew-service --version` from the built .app (`C04`, `C05`). |
| PC3 | H | B09 | `build.yml` (lines 358-377) exports `TAURI_SIGNING_PRIVATE_KEY`/`TAURI_UPDATER_PUBKEY` unstripped; the other four apps strip. A pasted secret with a newline breaks the release job ("Invalid symbol 10"). | Strip as in Spotter/Dyno (`tr -d ' \t\r\n'`), or use the shared script (`C06`). |
| PC4 | H | B08 | No icon check in CI. | Copy Pitwall's `check-icons.mjs` (`C03`). |
| PC5 | H | L01 to L03 | No progress channel and no cancel anywhere (audit 2026-10-23: `ServiceEvent` has only Status and Notification); UI timeouts shorter than the work (`clean_disk`, `register_runner`, `install_app_update` 120 s, `engine_prune`; call-timeout.ts); runner binary download discards the digest (`work.rs:67-76`). | Adopt `paddock-jobs` in `pitcrew-service`; add a `Progress` event to protocol v3; remove job commands from the timeout table (`C19`). Audit rows 1 to 9 are the work list. |
| PC6 | H | L04 | 8 non-async Tauri commands (`platform_name`, `take_pending_navigation`, `connection_state`, `app_update_status`, `get_autostart`, `set_autostart`, `open_url`, `reveal_logs`); the autostart pair and `open_url` call OS APIs on the main thread. | Make them `async` / `spawn_blocking`; add the `C12` rule. |
| PC7 | H | T01 | 30 `env::temp_dir()` uses, no `tempfile` dependency anywhere; leftovers are cleaned by hand. | `tempfile::TempDir` + `C13`/`C14`. |
| PC8 | M | R01, U10 | No splash in `index.html` (the app that had the permanent-blank-window bug). The initializer awaits `backend.init()` but bounds it (`PLATFORM_WAIT_MS`), so the blank-forever case is fixed; the empty window during load is not. | Add the splash (`C02`). |
| PC9 | M | R09 | No exit watchdog; the shell ends with `std::process::exit(1)` only on startup failure. A stuck runner stop on Quit can hang the app. | Shared `arm_exit_watchdog` (`C20`) with a bounded runner stop. |
| PC10 | M | P06 | `AGENTS.md` has none of the Paddock rules; `docs/suite-lessons.md` is a 4-line stub (misses every lesson since 2026-10-22: Team ID, Chrome, temp leaks, progress, UI findings, Angular). | Copy `rules.md` and `lessons.md` (`C23`). |
| PC11 | M | T04, U08 | Mock backend hard-codes `currentVersion: '0.1.0'` and `serviceVersion: '0.1.0'` (app is 1.0.2); `mock-backend.spec.ts` asserts mock `5.3.0`-type data (fixture, fine). | Mock reads the version from the single source (`C16`). |
| PC12 | M | T10 | `playwright.config.ts`: `reuseExistingServer: true` always; default port 4310. | Env port, `false` in CI (`C17`). |
| PC13 | M | A02, A03, A04 | Angular: 56 of 56 OnPush, zoneless, no `ngClass`; 18 effects, 6 files with `loading:` flags, 12 `providedIn: 'root'` (fine as singletons, needs the allow-list), no `withRequestStatus`. | `C09` baseline. |
| PC14 | M | U01, R04 | No dialogs today (confirmations inline, register wizard in a page). Rule 17 unmet for the wizard and confirmations; the service Repair control exists (`service_repair.rs`) and renders without the service (rule 19 met). | Adopt ADR 0009 kinds when the wizard becomes a window; keep the Repair contract in ADR 0010. |
| PC15 | M | U04, U09 | 0 `@container` queries (2 media queries), no overflow e2e, no axe, 18 files with `data-testid`. | Adopt `C11` overflow test, add axe to the e2e. |
| PC16 | L | T08 | No `TZ` step for UI tests. | `C18`. |

## Addressed (with evidence)

- B03 partly: `signingIdentity "-"` in `tauri.macos.conf.json`; B02 `check-index.mjs` in `beforeBuildCommand` and its node test in CI; B05 `macOSPrivateApi`; B06 capability; B13 stale installer guard (`rm -rf bundle`, version check); B14 `publish.mjs` + `latest-json.mjs` with tests; B15 First launch; B12 `cargo deny`; B10 release `needs` (6 jobs).
- R03 fixed on main (`service_install.rs` `service_registered_version`, `service_repair.rs` bootout flow); R04 Repair always renders; R05/R06 session and PATH code; R07 `thread_kind` in `probe.rs`; R08 target-OS paths (`8631a69`); R01 bounded init.
- T09 harness Drop kills runners (`72ccba9`); commit-lint with tests; clippy `disallowed-methods` and `unwrap_used` warn; `CREATE_NO_WINDOW` in 3 places.
