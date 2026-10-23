# Audit: Debrief against the problem catalogue

Read-only, 2026-10-24. Source: `main` @ 85dfdc9e (version 1.0.2, not tagged), tag v1.0.1 (released). Class ids are from
[problem-catalogue.md](../problem-catalogue.md). Severity: **C** = a released or about-to-be-released build is broken for users,
**H** = a rule is unmet and a known incident class can recur, **M** = rule unmet, low immediate risk, **L** = polish.

## Gaps, by severity

| # | Sev | Class | Finding | Fix |
|---|---|---|---|---|
| D1 | C | B04, B11 | v1.0.1 (released) ships a store worker that aborts on macOS (libduckdb keeps its vendor Team ID; hardened runtime on). The fix (`6f72e163` re-sign + Team-ID verify + `--self-test`, `dd1b00c8` `hardenedRuntime:false`) exists only on `fix/macos-store-and-dialogs`; main, already versioned 1.0.2, has neither (`tauri.macos.conf.json` has only `signingIdentity`). Tagging main would re-ship the crash. | Merge the branch (86 commits, incl. the dialog work) before the 1.0.2 tag; run the macOS smoke (D3) on the merged tree; add the 1.0.2 changelog entry "macOS: the log store starts". |
| D2 | C | U01, U02 | Dialog windows (ADR 0050), the "calm store-down" state and the provider/command fixes are on the same unmerged branch. Main still has overlay dialogs (rule 17 unmet) and 51 hand-configured dialogs are what ADR 0009 replaces. | Merge as D1, then adopt Paddock ADRs 0009 and 0010 through Debrief ADRs 0054 and 0055; the registry check `C10` is the exit criterion. |
| D3 | H | B04, B16, R09 | `tauri-smoke` runs on Linux and Windows only (`os: [ubuntu-latest, windows-latest]`); macOS has only the bundle verify, which does not start the sidecars on main. This is why D1 reached users. | Add `macos-latest` to `tauri-smoke`; start every sidecar (`debrief-store-worker --self-test`, plugin hosts) from the built .app; assert Quit exits within 10 s. `C04`, `C20`. |
| D4 | H | T01 | 159 `std::env::temp_dir()` uses and 323 manual `remove_dir_all` in Rust tests (25 crates use `tempfile`); the temp-leak check lived in a merged-away branch and is not in `build.yml`. 98k folders (55 GB) is the history. | `C13` `disallowed-methods` for `std::env::temp_dir` in tests, convert to `tempfile::TempDir`; `C14` leak check after the test step. |
| D5 | H | T02 | `chromium-driver` has `find_for_tests` (only `CHROMIUM_TEST_PATH`) but tests skip silently when it is unset, CI installs no pinned browser and has no `*_REQUIRE_TEST_BROWSER`; `discover.rs` still scans `/Applications/Google Chrome.app` for the product (correct) so the guard is the only protection. | Use the Spotter scripts (`pin-chromium`, `install-test-browser`) via `C15`; set `DEBRIEF_REQUIRE_TEST_BROWSER=1` in the Linux test job; close with CDP `Browser.close` before kill (verify). |
| D6 | H | B02, R01, U10 | No `check-index` in CI or `beforeBuildCommand` (`npm run build` only), and `index.html` has no splash. `inlineCritical:false` is set but nothing proves it. | Port `check-index.mjs` (Pitwall's) and call it in `beforeBuildCommand`; add the splash (`C02`). |
| D7 | M | S02 | No `unwrap_used` lint in `Cargo.toml` (the other four apps warn); AGENTS forbids it. `clippy.toml` only has `disallowed-methods` for `Command::new`. | Adopt the `C13` preset; fix findings by crate, test code exempt. |
| D8 | M | A01 to A06 | Angular rules unmet: 1 of 263 components lacks explicit OnPush; 168 `effect(` (rule 28); 35 files with `loading:` flags and no `withRequestStatus` (rule 30); 310 `providedIn: 'root'` (rule 32); 2 `patchState` calls outside `*.store.ts` (rule 31); 1 `@defer` (rule 35); no Signal Forms. Zoneless and no `ngClass`/`*ngIf` already hold. | ESLint preset (`C09`) with a waiver baseline equal to today's counts, ratchet down per touched file; build `withRequestStatus` in `paddock-ui` first. |
| D9 | M | T06, T07, T10 | Playwright `retries: 2` in CI with no report of retried tests; port default 4300 with `reuseExistingServer: !CI` (a local run reuses whatever listens). | `C17`: retried-test list in the job summary; env port required, `reuseExistingServer:false`. |
| D10 | M | P05, P06 | `commit-lint.mjs` does not reject `Co-Authored-By`/AI attribution; `AGENTS.md` carries the rules (rules 1-40, good) but no hash of the Paddock revision. | `C23`. |
| D11 | M | L01 to L06 | `paddock-jobs` is not used; Debrief has its own phase/progress model (1.0.2: phases, monotonic via `fetch_max` in `dumps.rs`). Not a defect, but the two implementations will diverge; audit 2026-10-23 rows 1 to 8 (import tail, reload, save project, redacted copy, Chromium install, export, store maintenance, dumps) are open on main. | Adopt `paddock-jobs` for the open rows when the Jobs batch lands; keep the audit table as the checklist. |
| D12 | M | T04 | Hard-coded versions are gone from the e2e (reads `APP_VERSION`, `CHANGELOG`); `app.spec.ts:89` uses `'0.1.0'` as mock lifecycle data (harmless, but `C16` would flag it). | Use a fixture constant from the version source. |
| D13 | M | L10 | Benchmarks exist (`bench.yml`), but budgets are not tied to the release gate. | `C26` nightly with stored baselines; failing budget blocks the tag preflight. |
| D14 | L | T11 | `docs/testing/quarantine.md` lists environment-bound tests (good) with no expiry. | Waiver format with `expires` (`C17`). |
| D15 | L | S04 | No fixture scan for confidential log material. | `C25`. |

## Addressed (with evidence)

- B03 ad-hoc seal: `signingIdentity "-"`, `scripts/ci/macos-verify-bundle.sh` (codesign deep strict on app and DMG, per-Mach-O verify).
- B08 icons (`scripts/ci/check-icons.mjs`), B09 key strip (`build.yml:767`), B13 stale installer guard, B15 First launch (build.yml), B10 release `needs` (7 jobs).
- R09 exit watchdog (`lifecycle::arm_exit_watchdog`); L04 every command async (364, test in `tests/window_commands.rs`); R06 `disallowed-methods`; R07 `thread_kind` in `debrief-monitor`.
- L05/L06 fixed in 1.0.2 (hash while writing, live counts); U04 Home reflow and middle truncation, 6 files with `@container`, overflow e2e.
- T04 version read from the changelog; T07 301 files with `data-testid`; U09 axe in about 100 e2e files; T08 partially (no `TZ` step in CI).
- S03 guard/fuzz/mutants workflows; S05 `cargo deny` + `npm audit`; B12 nightly schedule.
