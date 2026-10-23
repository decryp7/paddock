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
2. Avatars: engine side done (round 4, ADR 0006): owner avatars and favicons are fetched from the watched site, checked, cached on disk with expiry, and fall back to a bundled mark per source kind, then to the monogram. Commands `watch_avatar`, `preset_avatar`, `kind_avatar`. Open: the UI still draws monograms until it calls them (feat/ui-gaps); bundled marks exist per source kind, not per preset (brand logos are not licensed for bundling).
3. Right-click context menus and Up/Down row navigation in the feed and tables; sticky headers on the detail tables.
4. Tray extras: popup height fitted to content, rounded corners on Windows 11 (DWMWCP_ROUND), toast action buttons and
   click-through (Open, Mark as seen). E-mail channel delivery is done in the engine (SMTP through lettre, TLS, timeouts,
   password in the credential store, typed errors with a next step; commands `mail_status`, `set_mail_password`,
   `send_test_email`); the UI has no mail form yet (feat/ui-gaps), and no test against a real TLS server has been run (the tests use a local fake SMTP server; STARTTLS and TLS failure paths are covered, a successful TLS session is not).
5. Coverage (done 2026-10-23, release prep): `spotter-sources` 99.0% and `spotter-engine` 98.8% lines (`cargo llvm-cov`, summary
   method); the rest are `?` regions on SQLite and file operations (seek, flush, write, pragma, transactions) that need a
   fault-injecting database or file layer. Added seams: the in-memory database returns an error (not an exit) and takes an injected opener, a size limit
   parameter for downloads, platform parameters for folder error messages, `Gathered::into_candidates`, `feeds_in`.
6. App icons are guarded in CI (`scripts/ci/check-icons.mjs`, copied from Pitwall): the artwork must fill the canvas.

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

Unit tests (done 2026-10-23): 50 spec files, 402 tests (Vitest + TestBed) covering mock-backend, every store, draft-utils,
panel/ago/confirm/clipboard/chart and all page components; coverage 99.4% lines, 98.5% statements, 93.1% branches, 96.7%
functions. `npm audit --omit=dev --audit-level=high` reports 0 vulnerabilities; the suite also passes under
`TZ=Pacific/Auckland`.

Left:
- Reliability chart and downloads chart (only releases-over-time exists); "copyable upgrade command": the engine field exists (`Release.upgradeCommand`, container tags and registry packages) and the UI does not show it yet.
- Check scripts/check-index.mjs against dist/app/browser/index.html.

- Check schedules (ADR 0005): the wizard's Schedule section has Every / At times / Cron (`sp-schedule-editor`) with the engine's live preview; add a screenshot of the cron mode and let the watches table show the schedule text next to Next.

## Engine
- Done: SQLite state/history (migrations, corrupt-DB recovery), atomic config, polite fetcher (per-host gap, ETag/Last-Modified, body cap, manual redirects with loop detection, SSRF guard), pipeline, scheduler, Engine facade, webhook/ntfy delivery.
- Done in round 4 (ADR 0006): failure-path tests through the engine (slow server, oversize body, redirect loop, bad webhooks, soak of 400 checks, rendered pages and page change mode with a fake renderer, digest flush); local UTC offset set by the shell (`lifecycle::refresh_local_time`, hourly); DNS-rebinding pinning (`GuardedResolver`: the client resolves through the guard, and the pre-check uses the same resolver); `Db::open` and `Db::in_memory` return an error instead of exiting the process; `Engine::download` stays the shell's job (the shell calls `spotter-sources::download`, then `Engine::record_download`); e-mail delivery; avatars; install commands.
- Open: a successful TLS session against a real mail server (no test server speaks TLS); `Cargo.lock` holds shared changes, commit it with the integration.
- Check schedules (ADR 0005): done in `spotter-core` (`schedule.rs`, `calendar.rs`) and the scheduler: every / at times / cron, per-source minimum interval, jitter, DST rules, one catch-up check after sleep, `preview_schedule`. Open: a cron description in other languages; calendar schedules do not yet follow the machine's zone change until the hourly refresh.
