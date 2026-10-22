# Roadmap and known gaps

Updated at the end of each round. Items are listed with their state.

## Next (paused 2026-10-23 so Debrief can ship; resume in this order)

Done this round: Tauri shell (async commands, `dto.rs` mirroring `api.types.ts`, tray + popup, `Notifier`/`OsToast`, keyring
secrets, least-privilege capability and CSP with tests, window lifecycle, single instance, start at login, updater),
renderer on `spotter-core`'s `PageRenderer`, pinned Chrome for Testing manifest with SHA-256 (ADR 0004, verified
against the real macOS archive), version typography, coverage (core 99.5%, engine 96.8%, sources 92%, Angular 100% lines),
architecture diagrams and the overview SVG (`docs/diagrams`).

1. **CI green on three OSes.** Fixed locally, not yet re-run: the DNS-dependent `guard` test (injected resolver) and the
   slow-runner `real_browser` missing-element assertion. Last run (7a92461) failed on Ubuntu (guard test) and Windows
   (`real_browser`: "the page did not finish loading in time" in 3 tests; the pages load slowly on the Windows runner:
   raise the render timeouts of those tests or find the cause). Push, re-run, read logs through
   `GET /api/v1/repos/decryp7/spotter/actions/runs/<run>/jobs` and `actions/jobs/<id>/logs` with GITEA_TOKEN. Dispatch only
   when no Debrief run is queued or running.
2. **Regex extraction, products, artifacts, "Latest by", folder scaling.** Work in progress on branch
   `feat/regex-extraction` (worktree `../spotter-wt-regex`): `id`/`channel`/`variant`/text groups, version text with
   letters and leading zeros, Rust preview and pattern helper commands, per-product state, artifacts per release, settle
   time, snapshots and caps for big shares, Latest by (version / modified / created / name / updated in place). Merge into
   wip/redevelop, then finish what the branch report lists as open.
3. **Visual redesign and UI review 2** (briefs `ui-review-2-spotter.md`, `ui-review-2-patterns.md`, `design-review-spotter.md`):
   avatars (owner avatar / favicon / monogram), summary strip, release feed grouped by day with one primary action,
   cadence timeline, watch detail hero with upgrade line, no-wrap releases table, dense watch list, wizard step indicator
   and preset tiles, tray popover, empty states, motion; every P1 and P2 fixed and marked at the bottom of the review file.
4. **Screenshots** at 1440x900, DPR 2, both themes, realistic invented data, every screen viewed and iterated; update the
   README.
5. Tray extras: popup height fitted to content, rounded corners on Windows 11 (DWMWCP_ROUND), toast action buttons and
   click-through (Open, Mark as seen), e-mail channel delivery.
6. Re-run the coverage guards after the merges (core >= 99%, UI >= 80%, stores/services near 100%).

## Renderer

- Pinned Chromium: done (ADR 0004). The manifest pins Chrome for Testing 156.0.8078.47 with SHA-256 for five platforms;
  `node scripts/pin-chromium.mjs` re-pins; the download only happens on the user's request. Windows and Linux archives were
  not extracted and run on those OSes yet (only macOS arm64 was, with the `#[ignore]` test).
- OS web view renderer: only the status kind and the `Unavailable` hint exist; no renderer is built on the OS web view
  (it cannot be supervised or isolated). Not started.
- Windows type-check of `spotter-render` and the shell relies on the Windows CI runner (compiled there in run 4).
- The CDP driver is still inside `spotter-render` (modules `cdp`, `launch`, `page`, `process`, `discover`, `ready`,
  `challenge`, `install`, `manifest`); extraction to the shared paddock repo is pending.

## UI

Done: shell (splash, sidebar, banner, toasts, native feel, theme via paddock), Tauri and mock backends, stores (watches,
dashboard, settings, toasts, editor, detail), pages (dashboard, watches, detail, wizard, settings, tray), 18 Playwright
flows (Chromium and WebKit, port 4370), `ng build` and `ng lint` clean.

Left:
- Unit tests: specs exist for core services, format/labels/chart-options and copied utilities (59 tests); still missing for
  mock-backend, watches/dashboard/settings/editor stores, draft-utils, panel/ago/confirm/clipboard/chart, and every page
  component. Overall Angular coverage is therefore below the 80% target until these land (run `ng test --coverage`).
- `npm audit --omit=dev --audit-level=high` not run yet; run a second `ng test` under another TZ.
- Screenshots in docs/screenshots are from the first pass; no design critique iteration yet (mock data shows "newer than
  installed" on almost every watch).
- Reliability chart and downloads chart (only releases-over-time exists); "copyable upgrade command" needs an engine field.
- Check scripts/check-index.mjs against dist/app/browser/index.html.

- Check schedules (ADR 0005): the wizard's Schedule section has Every / At times / Cron (`sp-schedule-editor`) with the engine's live preview; add a screenshot of the cron mode and let the watches table show the schedule text next to Next.

## Engine
- Done: SQLite state/history (migrations, corrupt-DB recovery), atomic config, polite fetcher (per-host gap, ETag/Last-Modified, body cap, manual redirects with loop detection, SSRF guard), pipeline, scheduler, Engine facade, webhook/ntfy delivery.
- Open: more failure-path tests (oversize body, redirect loop, timeout, bad webhook, digest flush, soak through the engine, rendered pages with a fake PageRenderer, page change mode end-to-end); coverage run for spotter-engine; E-mail channel delivery (shell/SMTP); local UTC offset must be set by the shell via SystemClock::set_utc_offset_minutes; DNS-rebinding pinning for the SSRF guard; Engine::download is the shell's job (use spotter-sources::download, then Engine::record_download); a `Db::in_memory` failure calls process::exit (SQLite unusable) - consider returning an error; Cargo.lock holds shared changes, commit it with the integration.
- Check schedules (ADR 0005): done in `spotter-core` (`schedule.rs`, `calendar.rs`) and the scheduler: every / at times / cron, per-source minimum interval, jitter, DST rules, one catch-up check after sleep, `preview_schedule`. Open: a cron description in other languages; calendar schedules do not yet follow the machine's zone change until the hourly refresh.
