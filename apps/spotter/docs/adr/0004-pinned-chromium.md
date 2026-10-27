# 4. The pinned Chromium: Chrome for Testing, verified, on request only

- Status: accepted (2026-10-23)

## Context
ADR 0002 prefers an installed Edge, Chrome or Chromium and allows a pinned download when none exists. The download needs
real archives, checksums and a source that stays available.

## Decision
- **Source: Chrome for Testing** (https://googlechromelabs.github.io/chrome-for-testing/), Google's own builds of Chrome
  made for automation, with versioned, permanent URLs on `storage.googleapis.com/chrome-for-testing-public/<version>/`
  for win64, mac-arm64, mac-x64, linux64 and linux-arm64 (no Windows on ARM build exists). It is maintained by the Chrome
  team, ships signed macOS builds and does not auto-update itself, which suits a pinned, supervised browser.
- **Pinned, not floating.** `crates/spotter-render/src/chromium-manifest.toml` names one version, one URL and one SHA-256
  per platform. The JSON endpoints publish no hashes, so `node scripts/pin-chromium.mjs` downloads each archive, hashes it
  and rewrites the manifest; a human reviews the diff. A test checks that every entry is an https URL on that host, names
  the pinned version and has its own hash.
- **Verified twice.** The download is hashed as it streams and again before extraction; a mismatch deletes the file and
  installs nothing. Extraction refuses paths and links that leave the target folder, caps the expanded size, recreates the
  macOS framework links that stay inside, locates the executable and runs `--version` as a self-test before moving the
  tree into place. An `#[ignore]` test runs this against the real archive of the current platform.
- **Only on request.** Nothing downloads at build time or start-up. The `install_renderer` command runs when the user
  presses the button in Settings; the status says an installed Edge, Chrome or Chromium wins over a download.

## Consequences
Bumping Chromium is a deliberate commit (run the script, read the diff). A platform without an entry shows "install Edge,
Chrome or Chromium instead". The OS web view stays a status only (no renderer is built on it).
