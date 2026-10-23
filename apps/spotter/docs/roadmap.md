# Roadmap and known gaps

Updated at the end of each round. Items are listed with their state.

## Next (round 3 finished 2026-10-23)

Done in round 3: regex extraction branch merged and verified (products, artifacts, Latest by, match on name/folder/path, live
Rust preview, pattern helper, scale limits, settle time); updater `BASE_URL` is the public https URL; releases are published
through `scripts/release/publish.mjs` (no Go action); visual redesign and every UI review 2 P1/P2 (identity avatars, day-grouped
feed with one action per row, dimmed version suffix, cadence cards, detail hero with upgrade line and release side panel,
calm tables, wizard tiles, tray, empty states, both themes); check schedules (every / at times / cron, ADR 0005); Windows
`real_browser` fix (page budget starts after the browser is up, stability flags, lighter process snapshots); engine and sources
coverage (98.6% / 98.2% lines, see Engine); screenshots at 1440x900 @2x in `docs/screenshots` (dark and light).

1. **CI on three OSes (paused 2026-10-23 for Debrief, run 6125 cancelled):** push adbd36d is not yet verified. Run 6115 (2f0c152): all green except Windows `real_browser`.
   Run 0053a21 showed the 3 slow-load tests fixed and 2 left: the watchdog test (its 4 s limit counts from spawn, so a slow start killed the browser
   before launch returned; test now allows for START_LIMIT) and `challenges_become_needs_you` (a DevTools call timed out because 8 browsers started at once
   on the runner; the file's tests now take turns via `serial()`). Next: dispatch/push once Debrief has shipped, read logs with /tmp-style
   `actions/runs/<run>/jobs` then `actions/jobs/<id>/logs`. Also added: stale-bundle guards in the bundle job (not yet run in CI).
2. Avatars are monograms only: fetch and cache favicons / org avatars in the engine, bundle marks for the catalogue entries.
3. Right-click context menus and Up/Down row navigation in the feed and tables; sticky headers on the detail tables.
4. Tray extras: popup height fitted to content, rounded corners on Windows 11 (DWMWCP_ROUND), toast action buttons and
   click-through (Open, Mark as seen), e-mail channel delivery.
5. Coverage: spotter-engine and spotter-sources are below 99% lines; the rest are `?`/`map_err` regions on SQLite and file
   errors that need fault-injection seams (see the list in the commit 5af6ee0).

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
- Reliability chart and downloads chart (only releases-over-time exists); "copyable upgrade command" needs an engine field.
- Check scripts/check-index.mjs against dist/app/browser/index.html.

- Check schedules (ADR 0005): the wizard's Schedule section has Every / At times / Cron (`sp-schedule-editor`) with the engine's live preview; add a screenshot of the cron mode and let the watches table show the schedule text next to Next.

## Engine
- Done: SQLite state/history (migrations, corrupt-DB recovery), atomic config, polite fetcher (per-host gap, ETag/Last-Modified, body cap, manual redirects with loop detection, SSRF guard), pipeline, scheduler, Engine facade, webhook/ntfy delivery.
- Open: more failure-path tests (oversize body, redirect loop, timeout, bad webhook, digest flush, soak through the engine, rendered pages with a fake PageRenderer, page change mode end-to-end); coverage run for spotter-engine; E-mail channel delivery (shell/SMTP); local UTC offset must be set by the shell via SystemClock::set_utc_offset_minutes; DNS-rebinding pinning for the SSRF guard; Engine::download is the shell's job (use spotter-sources::download, then Engine::record_download); a `Db::in_memory` failure calls process::exit (SQLite unusable) - consider returning an error; Cargo.lock holds shared changes, commit it with the integration.
- Check schedules (ADR 0005): done in `spotter-core` (`schedule.rs`, `calendar.rs`) and the scheduler: every / at times / cron, per-source minimum interval, jitter, DST rules, one catch-up check after sleep, `preview_schedule`. Open: a cron description in other languages; calendar schedules do not yet follow the machine's zone change until the hourly refresh.
