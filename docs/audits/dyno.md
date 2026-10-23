# Audit: Dyno against the problem catalogue

Read-only, 2026-10-24. Source: `main` @ 258c815 (workspace version 0.1.0). **No tag exists: Dyno is not released** and its v1
scope is not frozen, so these gaps close before the first tag. Severity as in [debrief.md](debrief.md). Class ids from
[problem-catalogue.md](../problem-catalogue.md).

## Gaps, by severity

| # | Sev | Class | Finding | Fix |
|---|---|---|---|---|
| DY1 | H | B08 | No icon check in CI (Pitwall 1.0.0 shipped a placeholder that showed blank). | Copy `check-icons.mjs` (`C03`). |
| DY2 | H | T04 | `settings-page.spec.ts:43` asserts the About text contains `'0.1.0'`, and `core/mock-backend.ts`/`mock/data.ts` hard-code `0.1.0`: exactly the Debrief class that broke the 1.0.0 bump. Rule 6. | Read the version from `Cargo.toml`/`package.json` through the mock and assert its shape (`C16`). |
| DY3 | H | R03 | Service survival is split: the Tauri app registers the agent through `SMAppService` (`login.rs`) while the agent crate has its own launchd install with `ensure()` (re-registers when stale or not running on macOS). There is no per-version proactive re-register and no wait-until-the-job-is-gone step; `ensure()` calls `install()` which `bootout`s once. This is the pattern that failed in Pitcrew/Pitwall (re-registering over a live job does nothing). | Port the Pitwall flow (unregister, `launchctl bootout`, wait, register with retries, `service_registered_version`) and add `C21`; ideally use the shared `paddock-service` crate. |
| DY4 | H | L03, L01, L02 | Every `dyno` request has a default timeout ("Dyno did not answer in time.", `tauri-backend.ts:688`) and the long ones use a fixed `LOG_READ_TIMEOUT_MS`; there is no progress channel or cancel (audit 2026-10-23). | Adopt `paddock-jobs` for scans, cleanup, benchmark and backup; drop the timeouts for job commands (`C19`). |
| DY5 | H | B03, B04, B16 | `check-bundle.sh` verifies the seal and rpaths (good, the most complete of the five) but not Team IDs/hardened runtime, and no check starts the agent/helper from the built .app (only a CI `agent once` run from the build tree). | Add `hardenedRuntime:false` while ad-hoc, a one-TeamIdentifier assertion and a start of `dyno-agent --version` and `dyno-helper --version` from the .app (`C04`). |
| DY6 | M | R07 | Per-application memory is the sum of the members' resident memory (`dyno-core/src/grouping.rs:194`); thread entries are skipped (`processes.rs`) but there is no PSS, so shared pages of a multi-process app are counted once per process. | Use PSS on Linux and the shared helper (`C22`); show "shared memory counted once" in the UI. |
| DY7 | M | R01, U10 | No splash in `index.html`; the main window is `"create": false` (created in code), so a failure there leaves no window at all. | Splash (`C02`); a smoke test that the main window exists within N seconds of launch. |
| DY8 | M | R09 | No exit watchdog; the agent and helper are separate processes that must also end. | `C20`. |
| DY9 | M | T03 | The `macos` job runs the platform tests on the Mac host ("launch agent, ... codesign"); the service tests use a fake `runner`, but nothing proves no test registers a real launchd job or Login Item on a developer's Mac. | `C14` (process and launchd census) on the macOS job. |
| DY10 | M | T01 | 16 `env::temp_dir()` uses next to 9 `tempfile` crates; the job removes `dyno-*` in `$TMPDIR` with `rm -rf` (hides leaks instead of failing). | Convert, then fail on leftovers (`C14`). |
| DY11 | M | U01 | In-page dialogs (`rule-dialog`, `fix-dialog`, `process-dialogs`, `web-search-dialog`) and popover/extra windows (`window.rs`, `popover.rs`). | ADR 0009 kinds; popovers stay in-app. |
| DY12 | M | P06 | `AGENTS.md` (205 lines) lacks the Paddock rules; `suite-lessons.md` lacks the newest lessons. | `C23`. |
| DY13 | M | T07, U09 | 0 `data-testid`, 0 axe tests. | `C17`. |
| DY14 | M | T10 | `reuseExistingServer: true` always; port 4352 (`PW_PORT`, chosen to avoid clashes). | `C17`. |
| DY15 | M | A02, A03, A04 | 91 of 91 OnPush, zoneless; 10 effects; 7 files with `loading:` flags; 38 root providers across 30 stores. | `C09` baseline. |
| DY16 | L | R11 | Volume free-space polling at most once a minute (Finder stall) is a code comment and a test, not a rule. | Add to lessons (done in this change). |

## Addressed (with evidence)

- B03 partly: `signingIdentity "-"`, codesign verify in `check-bundle.sh` (CI line "bundle check"); B07 rpath check for Linux and macOS; B09 key strip (both secrets); B13 stale-installer clear; B14 publish scripts; B15 First launch; B12 `cargo deny`.
- B06 + S01 `scripts/check-capabilities.mjs` (the only capability linter; the template for `C12`); B02 `check-index.mjs` in CI and `beforeBuildCommand`.
- R10 postcard enum fix (`391181b`); R11 polling cap (`a068435`); R07 thread skip; R06 `disallowed-methods`; T08 `TZ=Pacific/Auckland` for ng tests; `--no-fail-fast` everywhere; Windows lints on the Windows runner (`3ba788e`).
- R03 partly: `ensure()` repairs a stale or stopped macOS service (see DY3).
