# Audit: Spotter against the problem catalogue

Read-only, 2026-10-24. Source: `main` @ 8af2e42 (version 1.0.0). **No tag exists: Spotter is not released**, so every gap
below is closed before the first tag. Severity as in [debrief.md](debrief.md). Class ids from
[problem-catalogue.md](../problem-catalogue.md). Spotter is the reference for the pinned-browser mechanism (T02) and is the only
app that depends on `@decryp7/paddock-ui`.

## Gaps, by severity

| # | Sev | Class | Finding | Fix |
|---|---|---|---|---|
| SP1 | H | B03, B04, B16 | The `bundle` job has no `codesign --verify`, no bundle layout check and no real-app smoke on any OS; `macos` is clippy + tests only. The first tag would ship an unverified .app (the Debrief 1.0.0 "damaged" case, then the 1.0.1 sidecar case). `hardenedRuntime` not disabled; Spotter's render crate launches an external Chrome (child, not a loaded library), so the Team-ID risk is low, but nothing checks it. | Add Pitwall's `macos-verify-bundle.sh` plus Team-ID and sidecar start to the macOS bundle leg; add a launch smoke (`C04`, `C05`). |
| SP2 | H | B10 | Release `needs` is complete (bundle, rust-lint, rust-test, macos, licences, frontend, e2e) but there is no tag preflight and there are no tags yet to prove the flow; `docs/first-launch` and `release/1.0` branches exist. | `C07` at the first tag; confirm `release/1.0` and `docs/first-launch` are merged (B11). |
| SP3 | M | R09 | No exit watchdog. Spotter runs a tray and background checks; a stuck browser render at Quit can hang. | `C20` (the render crate already asks every browser to quit before kill: `b3679b7`). |
| SP4 | M | T01 | `tempfile` used (5 crates), no raw `temp_dir()`; no post-run leak or process census (the browser tests are the risk: `real_browser.rs` cleans its profile dir). | `C14`. |
| SP5 | M | T04, U08 | Mock backend hard-codes `version: '1.0.0'` and `currentVersion: '1.0.0'` (`mock-backend.ts:353`, `:792`); will go stale at 1.0.1. Dashboard fixtures with versions are data, fine. | Read the version from the single source (`C16`). |
| SP6 | M | T10 | `reuseExistingServer: true` always; port default 4370 (`PW_PORT`). | `C17`. |
| SP7 | M | P06 | `AGENTS.md` (87 lines) lacks the Paddock rules; `suite-lessons.md` (61 lines) lacks Team ID, Chrome, temp leaks, progress, UI, Angular lessons. | `C23`. |
| SP8 | M | U01 | One `confirm-dialog.ts` component and the tray window; rule 17 asks for a window for confirmations. | ADR 0009 modal "confirm" kind when adopted. |
| SP9 | M | A02, A03, A04 | 32 of 32 OnPush, zoneless; 9 effects; 8 files with `loading:` flags; no `withRequestStatus` (it should come from `paddock-ui`, which Spotter already consumes). | `C09` baseline; first adopter of `withRequestStatus`. |
| SP10 | M | U09, U04 | 0 axe tests, 0 `@container` queries (6 media-query files), 6 files with `data-testid`. | `C11` overflow test, axe in e2e. |
| SP11 | L | L01 to L05 | Long operations: phase-labelled progress, Cancel and "no UI time limit" landed in `a50510d`/`f2712be`; `paddock-jobs` not used, and the found-count monotonic test is local to `folder.rs`. | Move to `paddock-jobs` when convenient; the local test remains the contract. |
| SP12 | L | T08 | No `TZ` step. | `C18`. |

## Addressed (with evidence)

- T02 pinned browser: `scripts/pin-chromium.mjs`, `install-test-browser.mjs` (SHA-256), `SPOTTER_REQUIRE_TEST_BROWSER` set in the Rust jobs (`ed4331a`), `NO_SANDBOX` test switch for root CI (`6bb5cbd`), graceful quit before kill (`b3679b7`), xvfb for headful tests. This is the template for `C15`.
- R07 PSS and thread skip (`cfac2be`); L05 found count only grows (`folder.rs:1233` test, cancel from the callback `8af2e42`); L03 no UI limit for long jobs.
- B02 `check-index.mjs` in `beforeBuildCommand` and test; B08 `check-icons.mjs` in CI; B09 key strip in the bundle step; B13 stale-bundle clear; B12 `cargo deny`; B15 First launch; R01 splash in `index.html`; B14 publish scripts with tests.
- B03 partly: `signingIdentity "-"`. U07 failed first load shows the error with Retry (`cbde49b`). S02 `unwrap_used` warn; R06 `disallowed-methods`; commit-lint; style-guard test.
