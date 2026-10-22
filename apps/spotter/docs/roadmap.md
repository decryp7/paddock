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
