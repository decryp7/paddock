# Roadmap and known gaps

Updated at the end of each round. Items are listed with their state.

## Renderer

- Pinned Chromium build not configured: `crates/spotter-render/src/chromium-manifest.toml` holds placeholder entries
  (URL and SHA-256 per platform), so `install` returns "the pinned Chromium build is not configured yet" and
  `RendererStatus.canDownload` is false. The download, checksum, safe extraction and self-test mechanism is built and
  tested against a local server; filling the manifest (for example Chrome for Testing zips with hashes computed from
  the downloaded files) is the only missing step. Not started.
- OS web view renderer: only the status kind and the `Unavailable` hint exist; no renderer is built on the OS web view
  (it cannot be supervised or isolated). Not started.
- Windows type-check of `spotter-render` could not run on the Mac (the TLS dependency needs the Windows SDK); it
  relies on the Windows CI runner.
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
