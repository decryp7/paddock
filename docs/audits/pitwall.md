# Audit: Pitwall against the problem catalogue

Read-only, 2026-10-24. Source: `main` @ 477652b (version 1.0.4, not tagged), tag v1.0.3 (released). Severity as in
[debrief.md](debrief.md). Class ids from [problem-catalogue.md](../problem-catalogue.md).

## Gaps, by severity

| # | Sev | Class | Finding | Fix |
|---|---|---|---|---|
| PW1 | C | R03 | Released v1.0.3 lacks `f17f230` ("replace the launchd job the update left behind, and keep the service controls available when the server is down"); main has it (1.0.4 chore commit, untagged). 1.0.3 changelog promised the repair; on macOS an updated install can still loop EX_CONFIG with the server down. | Tag 1.0.4 from CI after preflight; add `C21` (install N then N+1) to the macOS leg. |
| PW2 | H | B10 | Two workflows build releases: `build.yml` (checks) and `release.yml` (bundles on `v*` tags, `workflow_dispatch`) and the release workflow has no `needs`/gate on the checks and still carries the comment "The Tauri shell is built by another change; bundle steps skip while it is missing". A tag can publish from a commit whose lint/tests were red. | One workflow with `release needs [...]` like the other four, or `workflow_run` gating; `C07` preflight (green run for the commit) at the start of the release job. |
| PW3 | H | L04 | 6 non-async Tauri commands (`take_quit_ask`, `decline_service`, `get_autostart`, `set_autostart`, `app_update_status`, `chat_cancel`). `chat_cancel` must be reachable while a chat holds the runtime. | `async`, `spawn_blocking`; `C12`. |
| PW4 | H | L03 | UI call timeouts on jobs: `loadModel` 180 s, `runBenchmark` 300 s, `installService`/`uninstallService` 150 s, `installAppUpdate` 600 s (`timeout-backend.ts`). Rule 12: the UI never times out a running job. | Make these start-and-subscribe commands (job events from `paddock-jobs`), delete the SLOW table entries (`C19`). |
| PW5 | H | B04 | `hardenedRuntime` not disabled and no Team-ID check; Pitwall downloads vendor binaries (llama.cpp builds) at run time and runs them as child processes, which hardened runtime allows, but any bundled or dlopen'ed vendor library would repeat Debrief 1.0.1. `macos-verify-bundle.sh` verifies the seal only. | `hardenedRuntime:false` while ad-hoc; extend the verify script with one-TeamIdentifier and a sidecar self-test (`C04`). |
| PW6 | M | R09 | No exit watchdog; quit asks about serving (`quit-prompt`) but a stuck engine stop can hang Quit. | `C20` helper. |
| PW7 | M | P06 | `AGENTS.md` has none of the Paddock rules; `suite-lessons.md` (63 lines) lacks the Team-ID, Chrome, temp-leak, progress, UI and Angular lessons. | `C23`. |
| PW8 | M | T01 | `tempfile` used (5 crates) and no raw `temp_dir()`; no post-run leak check. The macOS runner cleanup script `macos-cleanup.sh` exists but does not fail on leftovers. | `C14`. |
| PW9 | M | U01 | About 6 in-page dialog components (`create-app-dialog`, `key-secret-dialog`, `quit-prompt`, `service-consent`, shared `confirm`): all rule-17 work; credentials and consent are modal kinds in ADR 0009. | Convert through the dialog registry once ADR 0009 is accepted. |
| PW10 | M | A02, A03 | 51 of 51 OnPush, zoneless; 9 effects; 13 files with `loading:` flags; no `withRequestStatus`; 11 root providers. | `C09` baseline. |
| PW11 | M | T07, T10 | `workers: 1` and port from `PW_E2E_PORT`, `reuseExistingServer: !CI` (good); 6 files with `data-testid`; flaky "dead-engine reload count" class fixed in tests but no retry report. | `C17`. |
| PW12 | M | U04, U09 | 2 `@container` files, 6 media-query files, 1 axe file. | `C11`, axe in e2e. |
| PW13 | L | R07 | Engine memory is read for one pid via sysinfo `memory()` (RSS) in `system.rs`; no thread filter or PSS (single pid, so no overcount, but shared pages of the engine's GPU mappings may be). | Use the shared helper (`C22`). |
| PW14 | L | B15 | "First launch" is added by `release.yml` only; fine once PW2 merges the workflows. | Single workflow. |

## Addressed (with evidence)

- B02 `scripts/ci/check-index.mjs` (CI line 277); B08 `check-icons.mjs`; B09 key strip in `tauri-bundle.sh`; B13 stale-bundle clear and installer version guard (`166f632`); B14 `publish.mjs`/`latest-json.mjs`; B17 sidecar overwrite found and fixed (`9ca11b1`).
- R03 fixed on main (service_install/service_repair, `service_registered_version`); R04 service controls independent of the server (`f17f230`, `f8a25d6` engines installable, no "planned"); R01 splash in `index.html`; R05 login-shell PATH; B03 `macos-verify-bundle.sh` in both workflows.
- T08 `TZ: Pacific/Auckland` for Rust tests; T14 `ui-coverage-guard.mjs --min 80`; S01 privacy-check script; commit-lint with tests; clippy `unwrap_used` warn.
